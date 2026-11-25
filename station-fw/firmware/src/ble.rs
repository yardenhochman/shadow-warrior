//! BLE provisioning service (NimBLE, peripheral only). See `docs/BLE.md`.
//!
//! The brain reads a one-time challenge, writes a sealed `Config` (Wi-Fi SSID/password and the brain's
//! address) and the Wi-Fi supervisor joins without a restart. The sealing key is the shared system
//! secret (`STATION_PROVISION_KEY` at build time, from `SYSTEM_SECRET`). The station advertises only
//! when `station_core::ble_policy::should_advertise` says so.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use esp32_nimble::utilities::mutex::Mutex as NimMutex;
use esp32_nimble::{uuid128, BLEAdvertisementData, BLECharacteristic, BLEDevice, NimbleProperties};
use esp_idf_svc::sys;
use log::{info, warn};
use station_core::ble_policy::{should_advertise, AdvInput};
use station_proto::provision::{self, Command, Link, Status, CHALLENGE_LEN};

use crate::net;

/// Self-test result for the status characteristic: 0 pass, 1 warn, 2 fail, 255 not run.
pub static SELFTEST: AtomicU8 = AtomicU8::new(255);

const DEV_KEY: &str = "shadow-warrior-dev-key";

/// The provisioning passphrase from the build; falls back to an insecure development key.
pub fn passphrase() -> (&'static str, bool) {
    match option_env!("STATION_PROVISION_KEY") {
        Some(k) if !k.is_empty() => (k, true),
        _ => (DEV_KEY, false),
    }
}

fn fresh_challenge() -> [u8; CHALLENGE_LEN] {
    let mut c = [0u8; CHALLENGE_LEN];
    // SAFETY: writes exactly c.len() bytes; hardware RNG (true random while the radio is on).
    unsafe { sys::esp_fill_random(c.as_mut_ptr().cast(), c.len()) };
    c
}

fn status(now_ms: u32) -> Status {
    let link = match net::LINK.load(Ordering::Relaxed) {
        1 => Link::Connecting,
        2 => Link::Up,
        3 => Link::Failed,
        _ => Link::Unconfigured,
    };
    let _ = now_ms;
    Status {
        link,
        rssi_dbm: net::RSSI.load(Ordering::Relaxed).clamp(-127, 0) as i8,
        ip: net::IP.load(Ordering::Relaxed).to_be_bytes(),
        selftest: SELFTEST.load(Ordering::Relaxed),
        flags: net::CONFIGURED.load(Ordering::Relaxed) as u8,
    }
}

/// Start the GATT server and the advertising supervisor.
pub fn spawn(
    station_id: u32,
    version: &str,
    updates: Sender<net::Update>,
    on_command: impl Fn(Command) + Send + Sync + 'static,
    button_override: bool,
) -> anyhow::Result<()> {
    let (pass, custom) = passphrase();
    if !custom {
        warn!("ble: no STATION_PROVISION_KEY at build time (SYSTEM_SECRET); using the INSECURE development key");
    }
    let key = provision::derive_key(pass);

    let dev = BLEDevice::take();
    dev.set_preferred_mtu(247).ok();
    let adv = dev.get_advertising();
    let server = dev.get_server();
    server.advertise_on_disconnect(false);
    server.on_connect(|_s, d| info!("ble: client connected {:?}", d.address()));
    server.on_disconnect(|_d, r| info!("ble: client disconnected ({r:?})"));

    let svc = server.create_service(uuid128!(provision::SERVICE_UUID));

    let info_c = svc.lock().create_characteristic(uuid128!(provision::CHR_INFO), NimbleProperties::READ);
    info_c.lock().set_value(format!("station-fw {version} id={station_id:08x}").as_bytes());

    let status_c: Arc<NimMutex<BLECharacteristic>> =
        svc.lock().create_characteristic(uuid128!(provision::CHR_STATUS), NimbleProperties::READ | NimbleProperties::NOTIFY);
    status_c.lock().set_value(&status(0).to_bytes());

    let challenge = Arc::new(Mutex::new(fresh_challenge()));
    let chal_c = svc.lock().create_characteristic(uuid128!(provision::CHR_CHALLENGE), NimbleProperties::READ);
    chal_c.lock().set_value(&challenge.lock().unwrap_or_else(|e| e.into_inner())[..]);

    let cfg_c = svc.lock().create_characteristic(uuid128!(provision::CHR_CONFIG), NimbleProperties::WRITE);
    {
        let challenge = challenge.clone();
        let chal_c = chal_c.clone();
        cfg_c.lock().on_write(move |args| {
            let mut ch = challenge.lock().unwrap_or_else(|e| e.into_inner());
            let res = provision::open(&key, &ch, args.recv_data());
            // One attempt per challenge, success or not.
            *ch = fresh_challenge();
            chal_c.lock().set_value(&ch[..]);
            match res {
                Ok(c) => {
                    info!("ble: provisioning received for ssid {:?}", c.ssid.as_str());
                    let _ = updates.send(net::Update { ssid: c.ssid.as_str().into(), pass: c.pass.as_str().into(), brain: c.brain.as_str().into() });
                }
                Err(e) => {
                    warn!("ble: provisioning write rejected: {e:?}");
                    args.reject();
                }
            }
        });
    }

    let cmd_c = svc.lock().create_characteristic(uuid128!(provision::CHR_COMMAND), NimbleProperties::WRITE);
    cmd_c.lock().on_write(move |args| match args.recv_data().first().copied().and_then(Command::from_u8) {
        Some(c) => on_command(c),
        None => args.reject(),
    });

    let name = format!("sw-station-{:04x}", station_id & 0xffff);
    adv.lock().set_data(BLEAdvertisementData::new().name(&name).add_service_uuid(uuid128!(provision::SERVICE_UUID)))?;
    info!("ble: GATT service up, advertising name {name} when unconfigured or Wi-Fi is down");

    thread::Builder::new().name("ble".into()).stack_size(4096).spawn(move || {
        let mut advertising = false;
        let mut last = [0u8; Status::LEN];
        loop {
            let now = (unsafe { sys::esp_timer_get_time() } / 1000) as u32;
            let last_up = net::LAST_UP_MS.load(Ordering::Relaxed);
            let want = should_advertise(AdvInput {
                configured: net::CONFIGURED.load(Ordering::Relaxed),
                link_up: net::UP.load(Ordering::Relaxed),
                down_ms: now.wrapping_sub(last_up),
                uptime_ms: now,
                button_override,
            });
            if want != advertising {
                let r = if want { adv.lock().start() } else { adv.lock().stop() };
                match r {
                    Ok(()) => {
                        advertising = want;
                        info!("ble: advertising {}", if want { "on" } else { "off" });
                    }
                    Err(e) => warn!("ble: advertising {} failed: {e:?}", if want { "start" } else { "stop" }),
                }
            }
            let st = status(now).to_bytes();
            if st != last {
                last = st;
                status_c.lock().set_value(&st).notify();
            }
            thread::sleep(Duration::from_millis(500));
        }
    })?;
    Ok(())
}
