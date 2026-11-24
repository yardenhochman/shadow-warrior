//! Wi-Fi station link: credentials, connection supervisor, UDP sockets.
//!
//! Settings come from flash (NVS namespace `station`: keys `ssid`, `pass`, `brain` = "ip:port"),
//! falling back to build-time env `STATION_WIFI_SSID`, `STATION_WIFI_PASS`, `STATION_BRAIN`.
//! Without an SSID Wi-Fi stays off and the station runs over USB only.
//!
//! Transport: one `station-proto` frame (`0x00 COBS 0x00`) per UDP datagram to the brain, and
//! command frames from the brain on UDP port [`CMD_PORT`]. Power save is off (modem sleep adds
//! 100+ ms of jitter). USB serial keeps working at the same time.

use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};
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

pub static UP: AtomicBool = AtomicBool::new(false);
pub static RSSI: AtomicI32 = AtomicI32::new(0);
pub static UDP_SENT: AtomicU32 = AtomicU32::new(0);
pub static UDP_DROPPED: AtomicU32 = AtomicU32::new(0);
pub static CONNECTS: AtomicU32 = AtomicU32::new(0);

#[derive(Clone)]
pub struct NetCfg {
    pub ssid: String,
    pub pass: String,
    pub brain: SocketAddr,
}

fn parse_brain(s: &str) -> Option<SocketAddr> {
    let s = s.trim();
    s.parse().ok().or_else(|| format!("{s}:{DEFAULT_BRAIN_PORT}").parse().ok())
}

/// NVS first, then build-time env. `None` when there is no SSID or no valid brain address.
pub fn load_cfg(nvs: &EspDefaultNvsPartition) -> Option<NetCfg> {
    let mut ssid = option_env!("STATION_WIFI_SSID").unwrap_or("").to_string();
    let mut pass = option_env!("STATION_WIFI_PASS").unwrap_or("").to_string();
    let mut brain = option_env!("STATION_BRAIN").unwrap_or("").to_string();
    match EspNvs::new(nvs.clone(), "station", false) {
        Ok(h) => {
            let mut buf = [0u8; 96];
            for (key, dst) in [("ssid", &mut ssid), ("pass", &mut pass), ("brain", &mut brain)] {
                if let Ok(Some(v)) = h.get_str(key, &mut buf) {
                    *dst = v.to_string();
                }
            }
        }
        Err(e) => info!("net: no NVS settings ({e}); using build-time settings"),
    }
    if ssid.is_empty() {
        info!("net: no Wi-Fi SSID configured (NVS station/ssid or STATION_WIFI_SSID); USB only");
        return None;
    }
    let Some(brain) = parse_brain(&brain) else {
        warn!("net: invalid brain address {brain:?} (want ip[:port]); Wi-Fi off");
        return None;
    };
    Some(NetCfg { ssid, pass, brain })
}

/// Start the Wi-Fi supervisor thread. `publish(Some(sock))` hands the connected UDP socket to the
/// frame sender when an IP address is obtained, `publish(None)` when the link drops.
pub fn spawn(
    modem: Modem<'static>,
    sysloop: EspSystemEventLoop,
    nvs: EspDefaultNvsPartition,
    cfg: NetCfg,
    publish: impl Fn(Option<UdpSocket>) + Send + 'static,
) -> anyhow::Result<()> {
    let mut wifi = EspWifi::new(modem, sysloop, Some(nvs))?;
    let mut client = ClientConfiguration {
        auth_method: if cfg.pass.is_empty() { AuthMethod::None } else { AuthMethod::WPA2Personal },
        ..Default::default()
    };
    client.ssid = cfg.ssid.as_str().try_into().map_err(|_| anyhow::anyhow!("SSID longer than 32 bytes"))?;
    client.password = cfg.pass.as_str().try_into().map_err(|_| anyhow::anyhow!("password longer than 64 bytes"))?;
    wifi.set_configuration(&Configuration::Client(client))?;
    wifi.start()?;
    // SAFETY: Wi-Fi is started; plain setter.
    unsafe { sys::esp_wifi_set_ps(sys::wifi_ps_type_t_WIFI_PS_NONE) };
    info!("net: Wi-Fi started, ssid {:?}, brain {}", cfg.ssid, cfg.brain);

    thread::Builder::new().name("wifi".into()).stack_size(8192).spawn(move || {
        let mut backoff = Duration::from_secs(1);
        loop {
            if let Err(e) = wifi.connect() {
                warn!("net: connect call failed: {e}");
            }
            let t0 = Instant::now();
            while t0.elapsed() < CONNECT_TIMEOUT && !wifi.is_up().unwrap_or(false) {
                thread::sleep(Duration::from_millis(200));
            }
            if !wifi.is_up().unwrap_or(false) {
                warn!("net: no connection after {CONNECT_TIMEOUT:?}, retry in {backoff:?}");
                let _ = wifi.disconnect();
                thread::sleep(backoff);
                backoff = (backoff * 2).min(BACKOFF_MAX);
                continue;
            }
            backoff = Duration::from_secs(1);
            let ip = wifi.sta_netif().get_ip_info().map(|i| i.ip.to_string()).unwrap_or_default();
            match UdpSocket::bind("0.0.0.0:0").and_then(|s| {
                s.connect(cfg.brain)?;
                s.set_nonblocking(true)?;
                Ok(s)
            }) {
                Ok(s) => {
                    CONNECTS.fetch_add(1, Ordering::Relaxed);
                    UP.store(true, Ordering::Relaxed);
                    info!("net: up, ip {ip}, sending frames to {}", cfg.brain);
                    publish(Some(s));
                }
                Err(e) => warn!("net: udp socket failed: {e}"),
            }
            // Supervise: refresh RSSI, notice a lost link.
            while wifi.is_up().unwrap_or(false) {
                let mut ap: sys::wifi_ap_record_t = Default::default();
                // SAFETY: valid out pointer; fails harmlessly when not associated.
                if unsafe { sys::esp_wifi_sta_get_ap_info(&mut ap) } == sys::ESP_OK {
                    RSSI.store(ap.rssi as i32, Ordering::Relaxed);
                }
                thread::sleep(Duration::from_secs(1));
            }
            UP.store(false, Ordering::Relaxed);
            publish(None);
            warn!("net: link lost, reconnecting");
        }
    })?;
    Ok(())
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
