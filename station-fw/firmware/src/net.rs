//! Wi-Fi station link: credentials, connection supervisor, UDP sockets.
//!
//! Settings come from flash (NVS namespace `station`: keys `ssid`, `pass`, `brain` = "ip:port"),
//! falling back to build-time env `STATION_WIFI_SSID`, `STATION_WIFI_PASS`, `STATION_BRAIN`, and can
//! be (re)written at run time over BLE (`ble.rs`) through [`Update`]. With no SSID the supervisor
//! idles (link `Unconfigured`) and the station runs over USB only until it is provisioned.
//!
//! Transport: one `station-proto` frame (`0x00 COBS 0x00`) per UDP datagram to the brain, and
//! command frames from the brain on UDP port [`CMD_PORT`]. Power save is off (modem sleep adds
//! 100+ ms of jitter). USB serial keeps working at the same time.

use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU8, Ordering};
use std::sync::mpsc::Receiver;
use std::thread;
use std::time::{Duration, Instant};

use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::modem::Modem;
use esp_idf_svc::nvs::{EspDefaultNvsPartition, EspNvs};
use esp_idf_svc::sys;
use esp_idf_svc::wifi::{AuthMethod, ClientConfiguration, Configuration, EspWifi};
use log::{info, warn};

/// Port on the station that receives command frames from the brain.
pub const CMD_PORT: u16 = 4211;
/// Default port on the brain when `brain` has no port.
pub const DEFAULT_BRAIN_PORT: u16 = 4210;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
const BACKOFF_MAX: Duration = Duration::from_secs(30);
const NVS_NS: &str = "station";

pub static UP: AtomicBool = AtomicBool::new(false);
pub static CONFIGURED: AtomicBool = AtomicBool::new(false);
/// `station_proto::provision::Link` as u8.
pub static LINK: AtomicU8 = AtomicU8::new(0);
/// IPv4 octets, big endian.
pub static IP: AtomicU32 = AtomicU32::new(0);
pub static RSSI: AtomicI32 = AtomicI32::new(0);
pub static UDP_SENT: AtomicU32 = AtomicU32::new(0);
pub static UDP_DROPPED: AtomicU32 = AtomicU32::new(0);
pub static CONNECTS: AtomicU32 = AtomicU32::new(0);
/// Milliseconds timestamp of the last time the link was up (0 = never since boot).
pub static LAST_UP_MS: AtomicU32 = AtomicU32::new(0);

const LINK_UNCONFIGURED: u8 = 0;
const LINK_CONNECTING: u8 = 1;
const LINK_UP: u8 = 2;
const LINK_FAILED: u8 = 3;

#[derive(Clone)]
pub struct NetCfg {
    pub ssid: String,
    pub pass: String,
    /// `None`: Wi-Fi joins but there is nobody to send frames to yet.
    pub brain: Option<SocketAddr>,
}

/// A settings write from BLE. An empty `brain` keeps the previous one.
pub struct Update {
    pub ssid: String,
    pub pass: String,
    pub brain: String,
}

fn parse_brain(s: &str) -> Option<SocketAddr> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    s.parse().ok().or_else(|| format!("{s}:{DEFAULT_BRAIN_PORT}").parse().ok())
}

/// NVS first, then build-time env. `None` when there is no SSID.
pub fn load_cfg(nvs: &EspDefaultNvsPartition) -> Option<NetCfg> {
    let mut ssid = option_env!("STATION_WIFI_SSID").unwrap_or("").to_string();
    let mut pass = option_env!("STATION_WIFI_PASS").unwrap_or("").to_string();
    let mut brain = option_env!("STATION_BRAIN").unwrap_or("").to_string();
    if let Ok(h) = EspNvs::new(nvs.clone(), NVS_NS, false) {
        let mut buf = [0u8; 96];
        for (key, dst) in [("ssid", &mut ssid), ("pass", &mut pass), ("brain", &mut brain)] {
            if let Ok(Some(v)) = h.get_str(key, &mut buf) {
                *dst = v.to_string();
            }
        }
    }
    if ssid.is_empty() {
        info!("net: no Wi-Fi SSID stored (NVS station/ssid or STATION_WIFI_SSID); waiting for BLE provisioning");
        return None;
    }
    let b = parse_brain(&brain);
    if b.is_none() {
        warn!("net: no valid brain address ({brain:?}); Wi-Fi will join but no frames are sent until one is set");
    }
    Some(NetCfg { ssid, pass, brain: b })
}

fn save_cfg(nvs: &EspDefaultNvsPartition, c: &NetCfg) {
    let r = EspNvs::new(nvs.clone(), NVS_NS, true).and_then(|h| {
        h.set_str("ssid", &c.ssid)?;
        h.set_str("pass", &c.pass)?;
        h.set_str("brain", &c.brain.map(|b| b.to_string()).unwrap_or_default())
    });
    match r {
        Ok(()) => info!("net: settings saved to flash (ssid {:?})", c.ssid),
        Err(e) => warn!("net: could not save settings: {e}"),
    }
}

/// Erase the stored settings (used by the BLE `ClearConfig` command, followed by a restart).
pub fn clear_cfg(nvs: &EspDefaultNvsPartition) {
    if let Ok(h) = EspNvs::new(nvs.clone(), NVS_NS, true) {
        for k in ["ssid", "pass", "brain"] {
            let _ = h.remove(k);
        }
    }
}

fn merge(cur: Option<&NetCfg>, u: Update) -> NetCfg {
    NetCfg { ssid: u.ssid, pass: u.pass, brain: parse_brain(&u.brain).or(cur.and_then(|c| c.brain)) }
}

fn set_link(l: u8) {
    LINK.store(l, Ordering::Relaxed);
}

/// Start the Wi-Fi supervisor. `publish(Some(sock))` hands the connected UDP socket to the frame
/// sender when an IP address is obtained, `publish(None)` when the link drops. New settings arrive
/// on `updates`; they are saved to flash and applied without a restart.
pub fn spawn(
    modem: Modem<'static>,
    sysloop: EspSystemEventLoop,
    nvs: EspDefaultNvsPartition,
    initial: Option<NetCfg>,
    updates: Receiver<Update>,
    publish: impl Fn(Option<UdpSocket>) + Send + 'static,
) -> anyhow::Result<()> {
    let mut wifi = EspWifi::new(modem, sysloop, Some(nvs.clone()))?;
    wifi.set_configuration(&Configuration::Client(ClientConfiguration::default()))?;
    wifi.start()?;
    // SAFETY: Wi-Fi is started; plain setter.
    unsafe { sys::esp_wifi_set_ps(sys::wifi_ps_type_t_WIFI_PS_NONE) };
    CONFIGURED.store(initial.is_some(), Ordering::Relaxed);

    thread::Builder::new().name("wifi".into()).stack_size(8192).spawn(move || {
        let mut cur = initial;
        let mut backoff = Duration::from_secs(1);
        // Apply a pending settings write; true when settings changed.
        let take_update = |cur: &mut Option<NetCfg>| -> bool {
            let mut changed = false;
            while let Ok(u) = updates.try_recv() {
                let c = merge(cur.as_ref(), u);
                save_cfg(&nvs, &c);
                *cur = Some(c);
                CONFIGURED.store(true, Ordering::Relaxed);
                changed = true;
            }
            changed
        };
        loop {
            take_update(&mut cur);
            let Some(cfg) = cur.clone() else {
                set_link(LINK_UNCONFIGURED);
                thread::sleep(Duration::from_millis(300));
                continue;
            };
            let mut client = ClientConfiguration {
                auth_method: if cfg.pass.is_empty() { AuthMethod::None } else { AuthMethod::WPA2Personal },
                ..Default::default()
            };
            let ok = client.ssid.push_str(&cfg.ssid).is_ok() && client.password.push_str(&cfg.pass).is_ok();
            if !ok {
                warn!("net: SSID (max 32) or password (max 64) too long; ignoring settings");
                cur = None;
                CONFIGURED.store(false, Ordering::Relaxed);
                continue;
            }
            let _ = wifi.disconnect();
            if let Err(e) = wifi.set_configuration(&Configuration::Client(client)) {
                warn!("net: set_configuration failed: {e}");
                thread::sleep(backoff);
                continue;
            }
            set_link(LINK_CONNECTING);
            info!("net: joining {:?}, brain {:?}", cfg.ssid, cfg.brain);
            if let Err(e) = wifi.connect() {
                warn!("net: connect call failed: {e}");
            }
            let t0 = Instant::now();
            let mut changed = false;
            while t0.elapsed() < CONNECT_TIMEOUT && !wifi.is_up().unwrap_or(false) {
                if take_update(&mut cur) {
                    changed = true;
                    break;
                }
                thread::sleep(Duration::from_millis(200));
            }
            if changed {
                continue;
            }
            if !wifi.is_up().unwrap_or(false) {
                set_link(LINK_FAILED);
                warn!("net: no connection after {CONNECT_TIMEOUT:?}, retry in {backoff:?}");
                let t = Instant::now();
                while t.elapsed() < backoff && !take_update(&mut cur) {
                    thread::sleep(Duration::from_millis(200));
                }
                backoff = (backoff * 2).min(BACKOFF_MAX);
                continue;
            }
            backoff = Duration::from_secs(1);
            let ip = wifi.sta_netif().get_ip_info().map(|i| i.ip).ok();
            if let Some(ip) = ip {
                IP.store(u32::from(ip), Ordering::Relaxed);
            }
            let sock = cfg.brain.and_then(|b| {
                UdpSocket::bind("0.0.0.0:0")
                    .and_then(|s| {
                        s.connect(b)?;
                        s.set_nonblocking(true)?;
                        Ok(s)
                    })
                    .map_err(|e| warn!("net: udp socket failed: {e}"))
                    .ok()
            });
            CONNECTS.fetch_add(1, Ordering::Relaxed);
            UP.store(true, Ordering::Relaxed);
            set_link(LINK_UP);
            info!("net: up, ip {:?}, frames to {:?}", ip, cfg.brain);
            publish(sock);
            // Supervise: refresh RSSI, notice a lost link or new settings.
            while wifi.is_up().unwrap_or(false) {
                let mut ap: sys::wifi_ap_record_t = Default::default();
                // SAFETY: valid out pointer; fails harmlessly when not associated.
                if unsafe { sys::esp_wifi_sta_get_ap_info(&mut ap) } == sys::ESP_OK {
                    RSSI.store(ap.rssi as i32, Ordering::Relaxed);
                }
                LAST_UP_MS.store(now_ms(), Ordering::Relaxed);
                if take_update(&mut cur) {
                    break;
                }
                thread::sleep(Duration::from_millis(500));
            }
            UP.store(false, Ordering::Relaxed);
            publish(None);
            warn!("net: link down or settings changed, rejoining");
        }
    })?;
    Ok(())
}

fn now_ms() -> u32 {
    // SAFETY: esp_timer_get_time has no preconditions.
    ((unsafe { sys::esp_timer_get_time() }) / 1000) as u32
}

/// Receive command frames from the brain on UDP `CMD_PORT`; each datagram is one frame
/// (`0x00 COBS 0x00`). `on_frame` gets the bytes between the delimiters.
pub fn spawn_cmd_rx(mut on_frame: impl FnMut(&[u8]) + Send + 'static) -> anyhow::Result<()> {
    thread::Builder::new().name("udp-cmd".into()).stack_size(4096).spawn(move || {
        let mut buf = [0u8; 128];
        let sock = loop {
            // Binding needs an IP stack; retry until Wi-Fi is up.
            match UdpSocket::bind(("0.0.0.0", CMD_PORT)) {
                Ok(s) => break s,
                Err(_) => thread::sleep(Duration::from_secs(2)),
            }
        };
        let _ = sock.set_read_timeout(Some(Duration::from_millis(500)));
        info!("net: command port UDP {CMD_PORT}");
        loop {
            let Ok((n, _from)) = sock.recv_from(&mut buf) else { continue };
            for f in buf[..n].split(|&b| b == 0).filter(|f| !f.is_empty()) {
                on_frame(f);
            }
        }
    })?;
    Ok(())
}
