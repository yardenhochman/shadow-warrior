//! BLE provisioning of sensor stations that have no working Wi-Fi (docs/STATION.md, station-fw/docs/BLE.md).
//!
//! A station without Wi-Fi advertises the provisioning GATT service. This service scans for it,
//! connects, reads info/status/challenge, writes `seal(derive_key(passphrase), challenge, Config)`
//! and watches the status notifications until the station reports `Up` (or `Failed` / timeout).
//! The station then sends UDP frames and `station_manager` takes over.
//!
//! [`Provisioner`] is the state machine, generic over [`Scanner`]/[`Link`] so it is tested without
//! hardware; the btleplug implementation is at the bottom. Secrets (passphrase, Wi-Fi password)
//! are never logged or put into events.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use station_proto::provision::{self, Config as WireConfig, Link as LinkState, Status};
use tracing::{info, warn};

use crate::config::{ProvisionConfig, StationConfig};
use crate::core::{Event, SharedEventBus};

/// A station seen while scanning.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    /// Stable key: advertised name (`sw-station-xxxx`) or the address.
    pub key: String,
    pub address: String,
}

/// Why the config write was refused.
#[derive(Debug)]
pub enum WriteError {
    /// ATT error from the station: wrong passphrase, stale challenge or bad payload.
    Rejected(String),
    /// Link problem.
    #[allow(dead_code)]
    Io(String),
}

/// One connection to a station's provisioning service.
#[async_trait]
pub trait Link: Send {
    async fn info(&mut self) -> Result<String>;
    async fn status(&mut self) -> Result<Status>;
    async fn challenge(&mut self) -> Result<[u8; provision::CHALLENGE_LEN]>;
    /// Write with response.
    async fn write_config(&mut self, sealed: &[u8]) -> std::result::Result<(), WriteError>;
    /// Next status notification, `Ok(None)` when `timeout` elapsed without one.
    async fn next_status(&mut self, timeout: Duration) -> Result<Option<Status>>;
    async fn disconnect(&mut self);
}

#[async_trait]
pub trait Scanner: Send {
    async fn scan(&mut self, listen: Duration) -> Result<Vec<Found>>;
    /// Connect and subscribe to status notifications.
    async fn connect(&mut self, found: &Found) -> Result<Box<dyn Link>>;
}

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// Station already joined Wi-Fi (or is joining): left alone.
    Skipped(String),
    Up(String),
    Failed(String),
}

#[derive(Default)]
struct Backoff {
    failures: u32,
    next_try: Option<Instant>,
}

pub struct Provisioner<S: Scanner> {
    cfg: ProvisionConfig,
    /// `ip:port` written to the station.
    brain_addr: String,
    key: [u8; 32],
    scanner: S,
    bus: SharedEventBus,
    backoff: HashMap<String, Backoff>,
}

fn ip_str(ip: [u8; 4]) -> String {
    format!("{}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3])
}

impl<S: Scanner> Provisioner<S> {
    pub fn new(cfg: ProvisionConfig, brain_addr: String, scanner: S, bus: SharedEventBus) -> Result<Self> {
        if cfg.provision_key.is_empty() || cfg.wifi_ssid.is_empty() {
            return Err(anyhow!("station.provision needs provision_key and wifi_ssid"));
        }
        build_config(&cfg, &brain_addr)?; // validates field lengths
        let key = provision::derive_key(&cfg.provision_key);
        Ok(Self { cfg, brain_addr, key, scanner, bus, backoff: HashMap::new() })
    }

    fn publish(&self, station: &str, state: &str, detail: Option<String>) {
        self.bus.publish(Event::StationProvision { station: station.into(), state: state.into(), detail });
    }

    fn is_due(&self, key: &str, now: Instant) -> bool {
        self.backoff.get(key).and_then(|b| b.next_try).is_none_or(|t| now >= t)
    }

    fn record(&mut self, key: &str, now: Instant, ok: bool) {
        let b = self.backoff.entry(key.to_string()).or_default();
        if ok {
            b.failures = 0;
            b.next_try = Some(now + Duration::from_secs_f32(self.cfg.retry_backoff_max_sec));
        } else {
            b.failures += 1;
            let secs = (self.cfg.retry_backoff_sec * 2f32.powi(b.failures as i32 - 1)).min(self.cfg.retry_backoff_max_sec);
            b.next_try = Some(now + Duration::from_secs_f32(secs));
        }
    }

    /// One scan and provisioning of every due station found.
    pub async fn cycle(&mut self, now: Instant) -> Result<Vec<(String, Outcome)>> {
        let found = self.scanner.scan(Duration::from_secs_f32(self.cfg.scan_timeout_sec)).await?;
        let mut out = Vec::new();
        for f in found {
            if !self.is_due(&f.key, now) {
                continue;
            }
            self.publish(&f.key, "found", None);
            let outcome = self.provision_one(&f).await;
            match &outcome {
                Outcome::Up(ip) => {
                    info!("station {} provisioned, ip {ip}", f.key);
                    self.publish(&f.key, "up", Some(ip.clone()));
                    self.record(&f.key, now, true);
                }
                Outcome::Skipped(why) => {
                    info!("station {} left alone: {why}", f.key);
                    self.record(&f.key, now, true);
                }
                Outcome::Failed(why) => {
                    warn!("station {} provisioning failed: {why}", f.key);
                    self.publish(&f.key, "failed", Some(why.clone()));
                    self.record(&f.key, now, false);
                }
            }
            out.push((f.key, outcome));
        }
        Ok(out)
    }

    async fn provision_one(&mut self, f: &Found) -> Outcome {
        let mut link = match self.scanner.connect(f).await {
            Ok(l) => l,
            Err(e) => return Outcome::Failed(format!("connect: {e:#}")),
        };
        let outcome = self.run_session(&f.key, link.as_mut()).await;
        link.disconnect().await;
        outcome
    }

    async fn run_session(&self, key: &str, link: &mut dyn Link) -> Outcome {
        macro_rules! try_io {
            ($what:expr, $e:expr) => {
                match $e {
                    Ok(v) => v,
                    Err(e) => return Outcome::Failed(format!("{}: {e:#}", $what)),
                }
            };
        }
        let info = try_io!("read info", link.info().await);
        let st = try_io!("read status", link.status().await);
        info!("station {key}: {info}, link {:?}", st.link);
        match st.link {
            LinkState::Up => return Outcome::Skipped(format!("already up at {}", ip_str(st.ip))),
            LinkState::Connecting => return Outcome::Skipped("already connecting".into()),
            LinkState::Unconfigured | LinkState::Failed => {}
        }
        // A fresh challenge per attempt; the station rotates it after every write.
        let challenge = try_io!("read challenge", link.challenge().await);
        let cfg = match build_config(&self.cfg, &self.brain_addr) {
            Ok(c) => c,
            Err(e) => return Outcome::Failed(format!("{e:#}")),
        };
        let sealed = match provision::seal(&self.key, &challenge, &cfg) {
            Ok(s) => s,
            Err(e) => return Outcome::Failed(format!("seal: {e:?}")),
        };
        self.publish(key, "provisioning", None);
        match link.write_config(&sealed).await {
            Ok(()) => {}
            Err(WriteError::Rejected(m)) => {
                return Outcome::Failed(format!("config rejected (wrong provision_key?): {m}"))
            }
            Err(WriteError::Io(m)) => return Outcome::Failed(format!("write: {m}")),
        }
        let deadline = Instant::now() + Duration::from_secs_f32(self.cfg.up_timeout_sec);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match try_io!("status", link.next_status(left).await) {
                None => return Outcome::Failed("timeout waiting for Wi-Fi".into()),
                Some(s) => match s.link {
                    LinkState::Up => return Outcome::Up(ip_str(s.ip)),
                    LinkState::Failed => {
                        return Outcome::Failed("station could not join Wi-Fi (wrong ssid/password or out of range)".into())
                    }
                    _ => {}
                },
            }
        }
    }
}

/// The wire config for `cfg`; errors name the field that is too long.
pub fn build_config(cfg: &ProvisionConfig, brain_addr: &str) -> Result<WireConfig> {
    Ok(WireConfig {
        ssid: cfg.wifi_ssid.as_str().try_into().map_err(|_| anyhow!("wifi_ssid longer than 32 bytes"))?,
        pass: cfg.wifi_pass.as_str().try_into().map_err(|_| anyhow!("wifi_pass longer than 64 bytes"))?,
        brain: brain_addr.try_into().map_err(|_| anyhow!("brain_addr longer than 48 bytes"))?,
    })
}

/// LAN address of this host toward the default route (no packet is sent), with `udp_bind`'s port.
pub fn derive_brain_addr(udp_bind: Option<&str>) -> Result<String> {
    let port = udp_bind.and_then(|b| b.rsplit(':').next()).and_then(|p| p.parse::<u16>().ok()).unwrap_or(4210);
    let sock = std::net::UdpSocket::bind("0.0.0.0:0")?;
    sock.connect("192.0.2.1:9")?;
    let ip = sock.local_addr()?.ip();
    if ip.is_unspecified() {
        return Err(anyhow!("cannot determine the brain's LAN address; set station.provision.brain_addr"));
    }
    Ok(format!("{ip}:{port}"))
}

pub fn resolve_brain_addr(st: &StationConfig) -> Result<String> {
    let a = st.provision.brain_addr.trim();
    if a.is_empty() { derive_brain_addr(st.udp_bind.as_deref()) } else { Ok(a.to_string()) }
}

/// Passphrase: `provision_key` if set, else the shared `SYSTEM_SECRET` environment value.
pub fn effective_key(cfg: &ProvisionConfig, env: Option<String>) -> Option<String> {
    Some(cfg.provision_key.clone()).filter(|k| !k.is_empty()).or(env.filter(|k| !k.is_empty()))
}

/// Start the provisioning loop. Returns an error only for unusable configuration.
pub fn spawn(
    st: &StationConfig,
    ble_adapter: Option<String>,
    bus: SharedEventBus,
) -> Result<tokio::task::JoinHandle<()>> {
    let mut st = st.clone();
    st.provision.provision_key = effective_key(&st.provision, std::env::var("SYSTEM_SECRET").ok())
        .ok_or_else(|| anyhow!("station provisioning disabled: set SYSTEM_SECRET (or station.provision.provision_key)"))?;
    let st = &st;
    let brain = resolve_brain_addr(st)?;
    info!("Station provisioning enabled, stations will send to {brain}");
    let adapter = st.provision.adapter.clone().or(ble_adapter);
    let interval = Duration::from_secs_f32(st.provision.scan_interval_sec.max(1.0));
    let mut p = Provisioner::new(st.provision.clone(), brain, bt::BtScanner::new(adapter), bus)?;
    Ok(tokio::spawn(async move {
        loop {
            if let Err(e) = p.cycle(Instant::now()).await {
                warn!("station provisioning scan: {e:#}");
            }
            tokio::time::sleep(interval).await;
        }
    }))
}

/// btleplug implementation. Hardware only; covered by the fake in the tests and by bench testing.
mod bt {
    use super::*;
    use btleplug::api::{Central, Characteristic, Manager as _, Peripheral as _, ScanFilter, WriteType};
    use btleplug::platform::{Adapter, Manager, Peripheral};
    use futures::{Stream, StreamExt};
    use std::pin::Pin;
    use uuid::Uuid;

    pub struct BtScanner {
        adapter_sel: Option<String>,
        keep: Option<(Manager, Adapter)>,
        seen: HashMap<String, Peripheral>,
    }

    impl BtScanner {
        pub fn new(adapter_sel: Option<String>) -> Self {
            Self { adapter_sel, keep: None, seen: HashMap::new() }
        }
    }

    fn uuid(s: &str) -> Uuid {
        Uuid::parse_str(s).expect("valid uuid constant")
    }

    #[async_trait]
    impl Scanner for BtScanner {
        async fn scan(&mut self, listen: Duration) -> Result<Vec<Found>> {
            let _g = crate::services::ble_manager::ADAPTER_LOCK.lock().await;
            let manager = Manager::new().await?;
            let adapters = manager.adapters().await?;
            let mut names = Vec::new();
            for a in &adapters {
                names.push(a.adapter_info().await.unwrap_or_else(|_| "unknown".into()));
            }
            let idx = crate::services::ble_manager::select_adapter(&names, self.adapter_sel.as_deref())
                .map_err(anyhow::Error::msg)?;
            let adapter = adapters.into_iter().nth(idx).expect("index validated");
            let svc = uuid(provision::SERVICE_UUID);
            adapter.start_scan(ScanFilter { services: vec![svc] }).await?;
            tokio::time::sleep(listen).await;
            let _ = adapter.stop_scan().await;
            self.seen.clear();
            let mut out = Vec::new();
            for p in adapter.peripherals().await? {
                let Ok(Some(props)) = p.properties().await else { continue };
                // BlueZ caches devices seen earlier, so check the service again.
                if !props.services.contains(&svc) {
                    continue;
                }
                let address = props.address.to_string();
                let key = props.local_name.clone().unwrap_or_else(|| address.clone());
                self.seen.insert(key.clone(), p);
                out.push(Found { key, address });
            }
            self.keep = Some((manager, adapter));
            Ok(out)
        }

        async fn connect(&mut self, f: &Found) -> Result<Box<dyn Link>> {
            let _g = crate::services::ble_manager::ADAPTER_LOCK.lock().await;
            let p = self.seen.get(&f.key).ok_or_else(|| anyhow!("station vanished"))?.clone();
            tokio::time::timeout(Duration::from_secs(15), p.connect())
                .await
                .map_err(|_| anyhow!("connect timed out"))??;
            p.discover_services().await?;
            let chars = p.characteristics();
            let find = |u: &str| {
                chars.iter().find(|c| c.uuid == uuid(u)).cloned().ok_or_else(|| anyhow!("characteristic {u} missing"))
            };
            let status = find(provision::CHR_STATUS)?;
            let link = BtLink {
                info: find(provision::CHR_INFO)?,
                challenge: find(provision::CHR_CHALLENGE)?,
                config: find(provision::CHR_CONFIG)?,
                notifications: p.notifications().await?,
                status,
                p,
                _keep: self.keep.clone(),
            };
            link.p.subscribe(&link.status).await?;
            Ok(Box::new(link))
        }
    }

    struct BtLink {
        p: Peripheral,
        info: Characteristic,
        status: Characteristic,
        challenge: Characteristic,
        config: Characteristic,
        notifications: Pin<Box<dyn Stream<Item = btleplug::api::ValueNotification> + Send>>,
        _keep: Option<(Manager, Adapter)>,
    }

    #[async_trait]
    impl Link for BtLink {
        async fn info(&mut self) -> Result<String> {
            Ok(String::from_utf8_lossy(&self.p.read(&self.info).await?).into_owned())
        }
        async fn status(&mut self) -> Result<Status> {
            let b = self.p.read(&self.status).await?;
            Status::from_bytes(&b).ok_or_else(|| anyhow!("bad status ({} bytes)", b.len()))
        }
        async fn challenge(&mut self) -> Result<[u8; provision::CHALLENGE_LEN]> {
            let b = self.p.read(&self.challenge).await?;
            b.as_slice().try_into().map_err(|_| anyhow!("challenge is {} bytes", b.len()))
        }
        async fn write_config(&mut self, sealed: &[u8]) -> std::result::Result<(), WriteError> {
            self.p
                .write(&self.config, sealed, WriteType::WithResponse)
                .await
                .map_err(|e| WriteError::Rejected(e.to_string()))
        }
        async fn next_status(&mut self, timeout: Duration) -> Result<Option<Status>> {
            let deadline = tokio::time::Instant::now() + timeout;
            loop {
                match tokio::time::timeout_at(deadline, self.notifications.next()).await {
                    Err(_) => return Ok(None),
                    Ok(None) => return Err(anyhow!("link lost")),
                    Ok(Some(n)) if n.uuid == self.status.uuid => {
                        if let Some(s) = Status::from_bytes(&n.value) {
                            return Ok(Some(s));
                        }
                    }
                    Ok(Some(_)) => {}
                }
            }
        }
        async fn disconnect(&mut self) {
            let _ = self.p.disconnect().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::EventBus;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    fn cfg() -> ProvisionConfig {
        ProvisionConfig {
            enabled: true,
            provision_key: "sw-test".into(),
            wifi_ssid: "arena-wifi".into(),
            wifi_pass: "hunter2 hunter2".into(),
            retry_backoff_sec: 10.0,
            retry_backoff_max_sec: 40.0,
            ..Default::default()
        }
    }

    fn st(link: LinkState, ip: [u8; 4]) -> Status {
        Status { link, rssi_dbm: -50, ip, selftest: 0, flags: 0 }
    }

    /// Fake firmware: enforces the passphrase and single-use challenges like the real one.
    struct FakeStation {
        key: [u8; 32],
        challenge: [u8; 12],
        counter: u8,
        initial: Status,
        /// Notifications after a successful open().
        script: VecDeque<Status>,
        accepted: Option<WireConfig>,
        writes: u32,
        challenges_read: Vec<[u8; 12]>,
        pending: VecDeque<Status>,
    }

    type Shared = Arc<Mutex<FakeStation>>;

    fn fake(pass: &str, initial: Status, script: Vec<Status>) -> Shared {
        Arc::new(Mutex::new(FakeStation {
            key: provision::derive_key(pass),
            challenge: [1; 12],
            counter: 1,
            initial,
            script: script.into(),
            accepted: None,
            writes: 0,
            challenges_read: vec![],
            pending: VecDeque::new(),
        }))
    }

    struct FakeLink(Shared);

    #[async_trait]
    impl Link for FakeLink {
        async fn info(&mut self) -> Result<String> {
            Ok("station-fw test id=0042".into())
        }
        async fn status(&mut self) -> Result<Status> {
            Ok(self.0.lock().unwrap().initial)
        }
        async fn challenge(&mut self) -> Result<[u8; 12]> {
            let mut s = self.0.lock().unwrap();
            let c = s.challenge;
            s.challenges_read.push(c);
            Ok(c)
        }
        async fn write_config(&mut self, sealed: &[u8]) -> std::result::Result<(), WriteError> {
            let mut s = self.0.lock().unwrap();
            s.writes += 1;
            let res = provision::open(&s.key, &s.challenge, sealed);
            // Rotate after every attempt, like the firmware.
            s.counter += 1;
            s.challenge = [s.counter; 12];
            match res {
                Ok(c) => {
                    s.accepted = Some(c);
                    s.pending = std::mem::take(&mut s.script);
                    Ok(())
                }
                Err(e) => Err(WriteError::Rejected(format!("{e:?}"))),
            }
        }
        async fn next_status(&mut self, _t: Duration) -> Result<Option<Status>> {
            Ok(self.0.lock().unwrap().pending.pop_front())
        }
        async fn disconnect(&mut self) {}
    }

    struct FakeScanner {
        found: Vec<Found>,
        station: Shared,
        scans: u32,
    }

    #[async_trait]
    impl Scanner for FakeScanner {
        async fn scan(&mut self, _l: Duration) -> Result<Vec<Found>> {
            self.scans += 1;
            Ok(self.found.clone())
        }
        async fn connect(&mut self, _f: &Found) -> Result<Box<dyn Link>> {
            Ok(Box::new(FakeLink(self.station.clone())))
        }
    }

    fn prov(pass_on_station: &str, initial: Status, script: Vec<Status>) -> (Provisioner<FakeScanner>, Shared) {
        let station = fake(pass_on_station, initial, script);
        let sc = FakeScanner {
            found: vec![Found { key: "sw-station-0042".into(), address: "AA:BB".into() }],
            station: station.clone(),
            scans: 0,
        };
        let bus = Arc::new(EventBus::new(64));
        (Provisioner::new(cfg(), "192.168.1.20:4210".into(), sc, bus).unwrap(), station)
    }

    fn up_script() -> Vec<Status> {
        vec![st(LinkState::Connecting, [0; 4]), st(LinkState::Up, [192, 168, 1, 50])]
    }

    #[tokio::test]
    async fn provisions_and_reports_ip() {
        let (mut p, station) = prov("sw-test", st(LinkState::Unconfigured, [0; 4]), up_script());
        let r = p.cycle(Instant::now()).await.unwrap();
        assert_eq!(r[0].1, Outcome::Up("192.168.1.50".into()));
        let s = station.lock().unwrap();
        let c = s.accepted.as_ref().unwrap();
        assert_eq!(c.ssid.as_str(), "arena-wifi");
        assert_eq!(c.pass.as_str(), "hunter2 hunter2");
        assert_eq!(c.brain.as_str(), "192.168.1.20:4210");
    }

    #[tokio::test]
    async fn wrong_passphrase_rejected_then_backoff_doubles() {
        let (mut p, station) = prov("other", st(LinkState::Unconfigured, [0; 4]), up_script());
        let t0 = Instant::now();
        let r = p.cycle(t0).await.unwrap();
        assert!(matches!(&r[0].1, Outcome::Failed(m) if m.contains("rejected")), "{r:?}");
        // Within the first backoff (10 s): not retried.
        assert!(p.cycle(t0 + Duration::from_secs(5)).await.unwrap().is_empty());
        assert_eq!(station.lock().unwrap().writes, 1);
        // After 10 s: retried, fails again, next delay is 20 s.
        assert_eq!(p.cycle(t0 + Duration::from_secs(10)).await.unwrap().len(), 1);
        assert!(p.cycle(t0 + Duration::from_secs(29)).await.unwrap().is_empty());
        assert_eq!(p.cycle(t0 + Duration::from_secs(30)).await.unwrap().len(), 1);
        // Then 40 s, capped at retry_backoff_max_sec.
        assert!(p.cycle(t0 + Duration::from_secs(69)).await.unwrap().is_empty());
        assert_eq!(p.cycle(t0 + Duration::from_secs(70)).await.unwrap().len(), 1);
        assert!(p.cycle(t0 + Duration::from_secs(109)).await.unwrap().is_empty());
        assert_eq!(p.cycle(t0 + Duration::from_secs(110)).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn fresh_challenge_every_attempt() {
        let (mut p, station) = prov("other", st(LinkState::Unconfigured, [0; 4]), up_script());
        let t0 = Instant::now();
        p.cycle(t0).await.unwrap();
        p.cycle(t0 + Duration::from_secs(10)).await.unwrap();
        let s = station.lock().unwrap();
        assert_eq!(s.challenges_read.len(), 2);
        assert_ne!(s.challenges_read[0], s.challenges_read[1]);
    }

    #[tokio::test]
    async fn replayed_write_with_stale_challenge_is_rejected() {
        // What a sniffer would do: reuse a sealed blob after the station rotated its challenge.
        let key = provision::derive_key("sw-test");
        let wire = build_config(&cfg(), "192.168.1.20:4210").unwrap();
        let sealed = provision::seal(&key, &[1; 12], &wire).unwrap();
        let station = fake("sw-test", st(LinkState::Unconfigured, [0; 4]), vec![]);
        let mut link = FakeLink(station);
        link.write_config(&sealed).await.unwrap();
        assert!(matches!(link.write_config(&sealed).await, Err(WriteError::Rejected(_))));
    }

    #[tokio::test]
    async fn status_failed_is_reported() {
        let script = vec![st(LinkState::Connecting, [0; 4]), st(LinkState::Failed, [0; 4])];
        let (mut p, _) = prov("sw-test", st(LinkState::Unconfigured, [0; 4]), script);
        let r = p.cycle(Instant::now()).await.unwrap();
        assert!(matches!(&r[0].1, Outcome::Failed(m) if m.contains("Wi-Fi")), "{r:?}");
    }

    #[tokio::test]
    async fn timeout_is_a_failure() {
        let (mut p, _) = prov("sw-test", st(LinkState::Failed, [0; 4]), vec![st(LinkState::Connecting, [0; 4])]);
        let r = p.cycle(Instant::now()).await.unwrap();
        assert_eq!(r[0].1, Outcome::Failed("timeout waiting for Wi-Fi".into()));
    }

    #[tokio::test]
    async fn already_up_station_is_not_touched() {
        let (mut p, station) = prov("sw-test", st(LinkState::Up, [192, 168, 1, 7]), up_script());
        let r = p.cycle(Instant::now()).await.unwrap();
        assert!(matches!(&r[0].1, Outcome::Skipped(m) if m.contains("192.168.1.7")));
        let s = station.lock().unwrap();
        assert_eq!(s.writes, 0);
        assert!(s.challenges_read.is_empty());
    }

    #[tokio::test]
    async fn publishes_state_events() {
        let station = fake("sw-test", st(LinkState::Unconfigured, [0; 4]), up_script());
        let sc = FakeScanner { found: vec![Found { key: "sw-station-0042".into(), address: "A".into() }], station, scans: 0 };
        let bus = Arc::new(EventBus::new(64));
        let mut rx = bus.subscribe();
        let mut p = Provisioner::new(cfg(), "10.0.0.2:4210".into(), sc, bus).unwrap();
        p.cycle(Instant::now()).await.unwrap();
        let mut states = vec![];
        while let Ok(Event::StationProvision { state, detail, .. }) = rx.try_recv() {
            states.push((state, detail));
        }
        assert_eq!(
            states,
            vec![
                ("found".into(), None),
                ("provisioning".into(), None),
                ("up".into(), Some("192.168.1.50".into()))
            ]
        );
    }

    /// Known-answer vector for firmware cross-checks: the sealed bytes are fixed by the key,
    /// challenge and config, so the firmware must open exactly this blob.
    #[test]
    fn known_answer_vector() {
        let key = provision::derive_key("sw-test");
        let ch = *b"0123456789ab";
        let c = WireConfig {
            ssid: "arena-wifi".try_into().unwrap(),
            pass: "hunter2 hunter2".try_into().unwrap(),
            brain: "192.168.1.20:4210".try_into().unwrap(),
        };
        let sealed = provision::seal(&key, &ch, &c).unwrap();
        let hex: String = sealed.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(provision::open(&key, &ch, &sealed).unwrap(), c);
        assert_eq!(hex, KNOWN_SEALED_HEX, "sealed vector changed; firmware must match");
    }
    const KNOWN_SEALED_HEX: &str = "7a7f431aac89c29a4efc77e3da5e58686ba13285a98e8e6b06258fbce6f79e5c946726d2e053f2c3292dc0da8c234ae54434c0a12f34b592f002c261ae";

    #[test]
    fn config_validation_and_redaction() {
        let mut c = cfg();
        c.wifi_ssid = "x".repeat(33);
        assert!(build_config(&c, "1.2.3.4").is_err());
        let dbg = format!("{:?}", cfg());
        assert!(!dbg.contains("hunter2") && !dbg.contains("sw-test"), "{dbg}");
        let json = serde_json::to_string(&cfg()).unwrap();
        assert!(!json.contains("hunter2") && !json.contains("sw-test"), "{json}");
        let mut empty = cfg();
        empty.provision_key.clear();
        let bus = Arc::new(EventBus::new(64));
        let sc = FakeScanner { found: vec![], station: fake("a", st(LinkState::Up, [0; 4]), vec![]), scans: 0 };
        assert!(Provisioner::new(empty, "1.2.3.4".into(), sc, bus).is_err());
    }

    #[test]
    fn key_defaults_to_system_secret() {
        let mut c = cfg();
        assert_eq!(effective_key(&c, Some("env".into())).as_deref(), Some("sw-test"));
        c.provision_key.clear();
        assert_eq!(effective_key(&c, Some("env".into())).as_deref(), Some("env"));
        assert_eq!(effective_key(&c, Some(String::new())), None);
        assert_eq!(effective_key(&c, None), None);
    }

    #[test]
    fn derived_brain_addr_uses_udp_bind_port() {
        if let Ok(a) = derive_brain_addr(Some("0.0.0.0:4999")) {
            assert!(a.ends_with(":4999"), "{a}");
        }
    }
}
