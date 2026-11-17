//! Keeps a paired Bluetooth speaker connected, audio profile included.
//!
//! Measured on an Orange Pi with a UE BOOM 4 (see docs/STATION_INSTALL.md): when the
//! speaker drops, PipeWire silently moves the playing stream to the board's unused
//! analog output, and when the speaker links back up on its own the A2DP audio
//! profile often does *not* come back, so music keeps "playing" into nothing. An
//! explicit connect fixes it and PipeWire moves the stream back by itself, so all
//! that is missing is (1) re-establishing the link and the audio profile and
//! (2) telling the rest of the brain about it (`Speaker*` events -> health registry).
//!
//! The policy (`SpeakerWatcher`) is platform independent and unit tested against a fake
//! link; only the BlueZ glue in [`bluez_link`] is Linux specific.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use anyhow::{Context, Result};
use async_trait::async_trait;
use tracing::{info, warn};

use crate::config::SpeakerConfig;
use crate::core::{Event, SharedEventBus};

/// What the watcher needs from the Bluetooth stack.
#[async_trait]
pub trait SpeakerLink: Send + Sync {
    /// Is the device connected at the link level (any profile)?
    async fn is_connected(&self) -> Result<bool>;
    /// Connect the device (link level, plus whatever profiles BlueZ picks).
    async fn connect(&self) -> Result<()>;
    /// Idempotently make sure the A2DP sink profile is connected (BlueZ answers success whether
    /// or not it was already up, so there is nothing to learn from the result).
    async fn ensure_audio(&self) -> Result<()>;
}

/// Link/profile supervision with exponential backoff and de-duplicated events.
pub struct SpeakerWatcher {
    name: String,
    bus: SharedEventBus,
    /// Longest wait (in ticks) between attempts while failing.
    max_backoff_ticks: u32,
    healthy: bool,
    /// The last check found the link itself down (as opposed to audio failing on a live link).
    link_down: bool,
    fails: u32,
    /// Ticks still to skip before the next attempt.
    wait: u32,
    last_error: Option<String>,
    /// Pause between audio-profile attempts within one check.
    settle: std::time::Duration,
}

/// `connect()` returns before the profile finishes coming up, so the first audio request can
/// hit "operation in progress"; retry a few times before calling it a failure.
const AUDIO_ATTEMPTS: u32 = 4;

impl SpeakerWatcher {
    pub fn new(name: String, bus: SharedEventBus, max_backoff_ticks: u32) -> Self {
        Self {
            name,
            bus,
            max_backoff_ticks: max_backoff_ticks.max(1),
            healthy: false,
            link_down: false,
            fails: 0,
            wait: 0,
            last_error: None,
            settle: std::time::Duration::from_secs(2),
        }
    }

    #[cfg(test)]
    fn with_settle(mut self, d: std::time::Duration) -> Self {
        self.settle = d;
        self
    }

    /// One supervision step; call every check interval.
    pub async fn tick(&mut self, link: &dyn SpeakerLink) {
        if self.wait > 0 {
            // Backoff throttles reconnect *attempts*, not this cheap passive check: a speaker
            // that was down and is switched back on reconnects itself and should be picked up
            // right away (measured: waiting out the backoff delayed the audio by ~18 s). A
            // live link whose audio keeps failing stays throttled.
            if self.link_down && matches!(link.is_connected().await, Ok(true)) {
                self.wait = 0;
            } else {
                self.wait -= 1;
                return;
            }
        }
        match self.check(link).await {
            Ok(()) => {
                self.fails = 0;
                self.last_error = None;
                if !self.healthy {
                    self.healthy = true;
                    info!("Speaker {} connected", self.name);
                    self.bus.publish(Event::SpeakerConnected { name: self.name.clone() });
                }
                self.bus.publish(Event::SpeakerAlive);
            }
            Err(e) => self.fail(format!("{e:#}")),
        }
    }

    async fn check(&mut self, link: &dyn SpeakerLink) -> Result<()> {
        if !link.is_connected().await.context("checking link")? {
            self.link_down = true;
            self.mark_down();
            link.connect().await.context("connect")?;
        }
        self.link_down = false;
        let mut result = Ok(());
        for attempt in 1..=AUDIO_ATTEMPTS {
            result = link.ensure_audio().await;
            if result.is_ok() {
                break;
            }
            if attempt < AUDIO_ATTEMPTS {
                tokio::time::sleep(self.settle).await;
            }
        }
        result.context("audio profile")
    }

    /// Announce a healthy -> unhealthy transition exactly once.
    fn mark_down(&mut self) {
        if self.healthy {
            self.healthy = false;
            warn!("Speaker {} lost", self.name);
            self.bus.publish(Event::SpeakerDisconnected);
        }
    }

    fn fail(&mut self, msg: String) {
        self.mark_down();
        self.fails += 1;
        self.wait = (1u32 << self.fails.min(16)).min(self.max_backoff_ticks);
        if self.last_error.as_deref() != Some(msg.as_str()) {
            warn!("Speaker {}: {msg}", self.name);
            self.bus.publish(Event::SpeakerError { error: msg.clone() });
            self.last_error = Some(msg);
        }
    }
}

/// Start supervising `cfg.address`. `None` when no speaker is configured.
pub fn spawn(cfg: SpeakerConfig, bus: SharedEventBus) -> Option<tokio::task::JoinHandle<()>> {
    cfg.address.as_deref().filter(|a| !a.trim().is_empty())?;
    Some(tokio::spawn(run(cfg, bus)))
}

#[cfg(target_os = "linux")]
async fn run(cfg: SpeakerConfig, bus: SharedEventBus) {
    use std::time::Duration;
    let interval = Duration::from_secs(cfg.check_interval_sec.max(1));
    let max_ticks = (cfg.max_backoff_sec / cfg.check_interval_sec.max(1)).max(1) as u32;
    let mut reported: Option<String> = None;
    loop {
        // BlueZ may not be up yet at boot, or the adapter may be missing: keep retrying.
        match bluez_link::open(&cfg).await {
            Ok((link, name)) => {
                info!("Supervising Bluetooth speaker {name}");
                let mut watcher = SpeakerWatcher::new(name, bus.clone(), max_ticks);
                let mut ticker = tokio::time::interval(interval);
                loop {
                    ticker.tick().await;
                    watcher.tick(&link).await;
                }
            }
            Err(e) => {
                let msg = format!("{e:#}");
                if reported.as_deref() != Some(msg.as_str()) {
                    warn!("Speaker manager waiting for Bluetooth: {msg}");
                    bus.publish(Event::SpeakerError { error: msg.clone() });
                    reported = Some(msg);
                }
                tokio::time::sleep(interval).await;
            }
        }
    }
}

#[cfg(not(target_os = "linux"))]
async fn run(_cfg: SpeakerConfig, _bus: SharedEventBus) {
    warn!("speaker.address is set but speaker supervision is Linux/BlueZ only; skipping");
}

#[cfg(target_os = "linux")]
mod bluez_link {
    use super::*;
    use std::time::Duration;

    /// A2DP sink service class (what a phone/PC connects to on a speaker).
    const A2DP_SINK: &str = "0000110b-0000-1000-8000-00805f9b34fb";
    /// BlueZ connect calls can hang on an unreachable device.
    const CALL_TIMEOUT: Duration = Duration::from_secs(20);

    pub struct BluerLink {
        device: bluer::Device,
    }

    pub async fn open(cfg: &SpeakerConfig) -> Result<(BluerLink, String)> {
        let address: bluer::Address = cfg
            .address
            .as_deref()
            .unwrap_or_default()
            .trim()
            .parse()
            .context("speaker.address is not a Bluetooth address")?;
        let session = bluer::Session::new().await.context("cannot reach bluetoothd")?;
        let names = session.adapter_names().await.context("listing Bluetooth adapters")?;
        // Not inherited from ble.adapter: the bag and the speaker normally use different radios.
        let idx = crate::services::ble_manager::select_adapter(&names, cfg.adapter.as_deref())
            .map_err(anyhow::Error::msg)?;
        let adapter = session.adapter(&names[idx]).context("opening adapter")?;
        adapter.set_powered(true).await.context("powering adapter")?;
        let device = adapter.device(address).context("opening speaker")?;
        let name = device.alias().await.unwrap_or_else(|_| address.to_string());
        Ok((BluerLink { device }, name))
    }

    fn already_connected(e: &bluer::Error) -> bool {
        matches!(e.kind, bluer::ErrorKind::AlreadyConnected)
    }

    #[async_trait]
    impl SpeakerLink for BluerLink {
        async fn is_connected(&self) -> Result<bool> {
            Ok(self.device.is_connected().await?)
        }

        async fn connect(&self) -> Result<()> {
            match tokio::time::timeout(CALL_TIMEOUT, self.device.connect()).await {
                Ok(Ok(())) => Ok(()),
                Ok(Err(e)) if already_connected(&e) => Ok(()),
                Ok(Err(e)) => Err(e.into()),
                Err(_) => anyhow::bail!("timed out after {}s", CALL_TIMEOUT.as_secs()),
            }
        }

        async fn ensure_audio(&self) -> Result<()> {
            let uuid: bluer::Uuid = A2DP_SINK.parse().expect("valid uuid");
            match tokio::time::timeout(CALL_TIMEOUT, self.device.connect_profile(&uuid)).await {
                Ok(Ok(())) => Ok(()),
                Ok(Err(e)) if already_connected(&e) => Ok(()),
                Ok(Err(e)) => Err(e.into()),
                Err(_) => anyhow::bail!("timed out after {}s", CALL_TIMEOUT.as_secs()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::EventBus;
    use std::sync::{Arc, Mutex};

    /// Scriptable speaker: link state, call results and a log of the calls made.
    struct Fake {
        connected: Mutex<bool>,
        connect: Mutex<Result<(), String>>,
        /// Error to return for the next N audio requests, then success.
        audio_fails: Mutex<(u32, String)>,
        calls: Mutex<Vec<&'static str>>,
    }

    impl Fake {
        fn new(connected: bool) -> Self {
            Self {
                connected: Mutex::new(connected),
                connect: Mutex::new(Ok(())),
                audio_fails: Mutex::new((0, String::new())),
                calls: Mutex::new(vec![]),
            }
        }
        fn count(&self, what: &str) -> usize {
            self.calls.lock().unwrap().iter().filter(|c| **c == what).count()
        }
    }

    #[async_trait]
    impl SpeakerLink for Fake {
        async fn is_connected(&self) -> Result<bool> {
            Ok(*self.connected.lock().unwrap())
        }
        async fn connect(&self) -> Result<()> {
            self.calls.lock().unwrap().push("connect");
            let r = self.connect.lock().unwrap().clone();
            if r.is_ok() {
                *self.connected.lock().unwrap() = true;
            }
            r.map_err(anyhow::Error::msg)
        }
        async fn ensure_audio(&self) -> Result<()> {
            self.calls.lock().unwrap().push("audio");
            let mut f = self.audio_fails.lock().unwrap();
            if f.0 > 0 {
                f.0 -= 1;
                return Err(anyhow::Error::msg(f.1.clone()));
            }
            Ok(())
        }
    }

    fn setup(max_ticks: u32) -> (SpeakerWatcher, tokio::sync::broadcast::Receiver<Event>) {
        let bus: SharedEventBus = Arc::new(EventBus::new(256));
        let rx = bus.subscribe();
        (SpeakerWatcher::new("BOOM 4".into(), bus, max_ticks).with_settle(std::time::Duration::ZERO), rx)
    }

    fn drain(rx: &mut tokio::sync::broadcast::Receiver<Event>) -> Vec<String> {
        let mut v = vec![];
        while let Ok(e) = rx.try_recv() {
            v.push(match &e {
                Event::SpeakerError { error } => format!("SpeakerError({error})"),
                other => other.event_type().to_string(),
            });
        }
        v
    }

    #[tokio::test]
    async fn healthy_speaker_is_announced_once_then_kept_alive() {
        let (mut w, mut rx) = setup(8);
        let link = Fake::new(true);
        for _ in 0..3 {
            w.tick(&link).await;
        }
        assert_eq!(drain(&mut rx), ["SpeakerConnected", "SpeakerAlive", "SpeakerAlive", "SpeakerAlive"]);
        assert_eq!(link.count("connect"), 0, "a connected speaker is not reconnected");
    }

    #[tokio::test]
    async fn dropped_link_is_announced_and_reconnected() {
        let (mut w, mut rx) = setup(8);
        let link = Fake::new(true);
        w.tick(&link).await;
        drain(&mut rx);

        *link.connected.lock().unwrap() = false; // speaker powered off / went out of range
        w.tick(&link).await;
        assert_eq!(drain(&mut rx), ["SpeakerDisconnected", "SpeakerConnected", "SpeakerAlive"]);
        assert_eq!(link.count("connect"), 1);
    }

    #[tokio::test]
    async fn transient_audio_error_is_retried_within_the_check() {
        // Measured: right after connect, BlueZ answers "operation in progress" for a moment.
        let (mut w, mut rx) = setup(8);
        let link = Fake::new(true);
        *link.audio_fails.lock().unwrap() = (2, "Bluetooth operation in progress".into());
        w.tick(&link).await;
        assert_eq!(drain(&mut rx), ["SpeakerConnected", "SpeakerAlive"], "no false failure");
        assert_eq!(link.count("audio"), 3);
    }

    #[tokio::test]
    async fn persistent_audio_failure_is_reported_and_backed_off() {
        // e.g. no audio server on the station: "Protocol not available".
        let (mut w, mut rx) = setup(100);
        let link = Fake::new(true);
        *link.audio_fails.lock().unwrap() = (u32::MAX, "Protocol not available".into());
        w.tick(&link).await;
        assert_eq!(drain(&mut rx), ["SpeakerError(audio profile: Protocol not available)"]);
        assert_eq!(link.count("audio"), 4, "AUDIO_ATTEMPTS tries, then give up");
        w.tick(&link).await; // backing off: no new attempt
        assert_eq!(link.count("audio"), 4);
    }

    #[tokio::test]
    async fn failures_back_off_and_each_distinct_error_is_reported_once() {
        let (mut w, mut rx) = setup(100);
        let link = Fake::new(false);
        *link.connect.lock().unwrap() = Err("device not available".into());
        for _ in 0..9 {
            w.tick(&link).await;
        }
        // attempts on ticks 1, 4 and 9 (waits of 2 then 4 ticks in between)
        assert_eq!(link.count("connect"), 3);
        assert_eq!(drain(&mut rx), ["SpeakerError(connect: device not available)"]);

        // The third failure queued a wait of 8 ticks, so the next attempt is the 9th tick.
        *link.connect.lock().unwrap() = Err("timed out".into());
        for _ in 0..9 {
            w.tick(&link).await;
        }
        assert_eq!(link.count("connect"), 4);
        assert_eq!(drain(&mut rx), ["SpeakerError(connect: timed out)"]);
    }

    #[tokio::test]
    async fn backoff_is_capped() {
        let (mut w, _rx) = setup(3);
        let link = Fake::new(false);
        *link.connect.lock().unwrap() = Err("nope".into());
        for _ in 0..40 {
            w.tick(&link).await;
        }
        // waits: 2, 3, 3, 3... ticks => an attempt every 3-4 ticks, never starved
        assert!(link.count("connect") >= 10, "attempts: {}", link.count("connect"));
    }

    #[tokio::test]
    async fn recovery_resets_backoff_and_announces_connected() {
        let (mut w, mut rx) = setup(100);
        let link = Fake::new(false);
        *link.connect.lock().unwrap() = Err("off".into());
        for _ in 0..4 {
            w.tick(&link).await;
        }
        drain(&mut rx);

        *link.connect.lock().unwrap() = Ok(()); // speaker switched on
        for _ in 0..6 {
            w.tick(&link).await;
        }
        let ev = drain(&mut rx);
        assert_eq!(ev.iter().filter(|e| *e == "SpeakerConnected").count(), 1, "{ev:?}");
        assert!(ev.contains(&"SpeakerAlive".to_string()));
        // after recovery there is no waiting: the next tick checks immediately
        let before = link.count("audio");
        w.tick(&link).await;
        assert_eq!(link.count("audio"), before + 1);
    }

    #[test]
    fn spawn_requires_an_address() {
        let bus: SharedEventBus = Arc::new(EventBus::new(4));
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let _g = rt.enter();
        assert!(spawn(SpeakerConfig::default(), bus.clone()).is_none());
        assert!(spawn(SpeakerConfig { address: Some("  ".into()), ..Default::default() }, bus).is_none());
    }

    #[tokio::test]
    async fn speaker_that_reconnects_itself_is_picked_up_during_backoff() {
        let (mut w, mut rx) = setup(100);
        let link = Fake::new(false);
        *link.connect.lock().unwrap() = Err("device not available".into());
        w.tick(&link).await; // attempt fails, long wait queued
        drain(&mut rx);

        *link.connected.lock().unwrap() = true; // speaker switched back on, links by itself
        w.tick(&link).await;
        assert_eq!(drain(&mut rx), ["SpeakerConnected", "SpeakerAlive"]);
        assert_eq!(link.count("connect"), 1, "no extra connect attempt was needed");
    }
}
