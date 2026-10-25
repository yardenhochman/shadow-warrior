//! Hardware health registry: folds device events from the event bus into
//! per-device connection state, last-seen / last-error timestamps and error text.
//! Managers only publish events; they never talk to the registry directly.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast::error::RecvError;

use super::events::{Event, SharedEventBus};

/// Detailed health record for a single device.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DeviceHealth {
    pub connected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connected_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disconnected_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disconnect_reason: Option<String>,
    /// Last time any event from this device was observed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_seen: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error_at: Option<DateTime<Utc>>,
    pub error_count: u64,
    /// Device specific: tasmota power state, music playing, ...
    #[serde(skip_serializing_if = "Option::is_none")]
    pub powered: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl DeviceHealth {
    fn seen(&mut self, now: DateTime<Utc>) {
        self.last_seen = Some(now);
    }
    fn up(&mut self, now: DateTime<Utc>) {
        if !self.connected {
            self.connected_at = Some(now);
        }
        self.connected = true;
        self.disconnect_reason = None;
        self.seen(now);
    }
    fn down(&mut self, now: DateTime<Utc>, reason: Option<String>) {
        if self.connected || self.disconnected_at.is_none() {
            self.disconnected_at = Some(now);
        }
        self.connected = false;
        self.disconnect_reason = reason;
    }
    fn fail(&mut self, now: DateTime<Utc>, err: String) {
        self.last_error = Some(err);
        self.last_error_at = Some(now);
        self.error_count += 1;
    }
}

/// Snapshot of every device, serialized in the status API.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HardwareHealth {
    pub ble: DeviceHealth,
    pub audio: DeviceHealth,
    pub music: DeviceHealth,
    /// keyed by controller IP
    pub wled: BTreeMap<String, DeviceHealth>,
    /// keyed by plug name
    pub tasmota: BTreeMap<String, DeviceHealth>,
}

#[derive(Default)]
pub struct StatusRegistry {
    inner: Mutex<HardwareHealth>,
}

impl StatusRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn snapshot(&self) -> HardwareHealth {
        self.inner.lock().unwrap().clone()
    }

    /// Fold one event into the registry (pure w.r.t. the supplied clock).
    pub fn apply(&self, event: &Event, now: DateTime<Utc>) {
        let mut h = self.inner.lock().unwrap();
        match event {
            Event::AudioDeviceConnected { device_name } => {
                h.audio.name = Some(device_name.clone());
                h.audio.up(now);
            }
            Event::AudioDeviceDisconnected => h.audio.down(now, None),
            Event::AudioLevelChanged { .. } | Event::ShoutDetected { .. } => h.audio.seen(now),
            Event::BleDeviceConnected { device_name, address } => {
                h.ble.name = Some(device_name.clone());
                h.ble.address = Some(address.clone());
                h.ble.up(now);
            }
            Event::BleDeviceDisconnected { reason } => h.ble.down(now, Some(reason.clone())),
            Event::PunchDetected { .. } => h.ble.seen(now),
            Event::BleConnectionError { error } => h.ble.fail(now, error.clone()),
            Event::MusicStarted { track_name } => {
                h.music.powered = Some(true);
                h.music.detail = Some(track_name.clone());
                h.music.up(now);
            }
            Event::MusicStopped => {
                h.music.powered = Some(false);
                h.music.seen(now);
            }
            Event::MusicError { error } => h.music.fail(now, error.clone()),
            Event::WledConnected { ip } => {
                let d = h.wled.entry(ip.clone()).or_default();
                d.address = Some(ip.clone());
                d.up(now);
            }
            Event::WledDisconnected { ip } => {
                let d = h.wled.entry(ip.clone()).or_default();
                d.address = Some(ip.clone());
                d.down(now, None);
            }
            Event::WledError { ip, error } => {
                let d = h.wled.entry(ip.clone()).or_default();
                d.address = Some(ip.clone());
                d.fail(now, error.clone());
            }
            Event::TasmotaStateChanged { name, powered } => {
                let d = h.tasmota.entry(name.clone()).or_default();
                d.name = Some(name.clone());
                d.powered = Some(*powered);
                d.up(now);
            }
            Event::TasmotaError { name, error } => {
                let d = h.tasmota.entry(name.clone()).or_default();
                d.name = Some(name.clone());
                d.fail(now, error.clone());
                d.down(now, Some(error.clone()));
            }
            _ => {}
        }
    }

    /// Subscribe to the bus and keep the registry up to date. Call before
    /// starting managers so initial connect events are not missed.
    pub fn spawn(self: &Arc<Self>, bus: &SharedEventBus) {
        let reg = self.clone();
        let mut rx = bus.subscribe();
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(ev) => reg.apply(&ev, Utc::now()),
                    Err(RecvError::Lagged(_)) => continue,
                    Err(RecvError::Closed) => break,
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn ble_lifecycle() {
        let r = StatusRegistry::new();
        let t0 = Utc::now();
        r.apply(
            &Event::BleDeviceConnected { device_name: "bag".into(), address: "AA".into() },
            t0,
        );
        let s = r.snapshot().ble;
        assert!(s.connected);
        assert_eq!(s.connected_at, Some(t0));
        assert_eq!(s.name.as_deref(), Some("bag"));

        let t1 = t0 + Duration::seconds(5);
        r.apply(&Event::punch_detected(1.0, 0.0, 0.0, 1.0), t1);
        assert_eq!(r.snapshot().ble.last_seen, Some(t1));
        assert_eq!(r.snapshot().ble.connected_at, Some(t0));

        let t2 = t1 + Duration::seconds(5);
        r.apply(&Event::BleDeviceDisconnected { reason: "gone".into() }, t2);
        r.apply(&Event::BleConnectionError { error: "timeout".into() }, t2);
        let s = r.snapshot().ble;
        assert!(!s.connected);
        assert_eq!(s.disconnected_at, Some(t2));
        assert_eq!(s.disconnect_reason.as_deref(), Some("gone"));
        assert_eq!(s.last_error.as_deref(), Some("timeout"));
        assert_eq!(s.error_count, 1);
    }

    #[test]
    fn reconnect_clears_reason_keeps_error_history() {
        let r = StatusRegistry::new();
        let t0 = Utc::now();
        r.apply(&Event::WledDisconnected { ip: "1.2.3.4".into() }, t0);
        r.apply(&Event::WledError { ip: "1.2.3.4".into(), error: "boom".into() }, t0);
        let t1 = t0 + Duration::seconds(1);
        r.apply(&Event::WledConnected { ip: "1.2.3.4".into() }, t1);
        let w = &r.snapshot().wled["1.2.3.4"];
        assert!(w.connected);
        assert_eq!(w.connected_at, Some(t1));
        assert_eq!(w.last_error.as_deref(), Some("boom"));
        assert_eq!(w.error_count, 1);
    }

    #[test]
    fn tasmota_per_device_and_error_marks_down() {
        let r = StatusRegistry::new();
        let t = Utc::now();
        r.apply(&Event::TasmotaStateChanged { name: "uv".into(), powered: true }, t);
        r.apply(&Event::TasmotaError { name: "led".into(), error: "http 500".into() }, t);
        let s = r.snapshot();
        assert!(s.tasmota["uv"].connected);
        assert_eq!(s.tasmota["uv"].powered, Some(true));
        assert!(!s.tasmota["led"].connected);
        assert_eq!(s.tasmota["led"].last_error.as_deref(), Some("http 500"));
    }

    #[test]
    fn audio_and_music() {
        let r = StatusRegistry::new();
        let t = Utc::now();
        r.apply(&Event::AudioDeviceConnected { device_name: "usb".into() }, t);
        r.apply(&Event::AudioLevelChanged { level_db: -20.0 }, t + Duration::seconds(1));
        r.apply(&Event::MusicStarted { track_name: "x".into() }, t);
        r.apply(&Event::MusicError { error: "no sink".into() }, t);
        let s = r.snapshot();
        assert!(s.audio.connected);
        assert_eq!(s.audio.last_seen, Some(t + Duration::seconds(1)));
        assert_eq!(s.music.detail.as_deref(), Some("x"));
        assert_eq!(s.music.last_error.as_deref(), Some("no sink"));
    }

    #[tokio::test]
    async fn spawn_consumes_bus() {
        let bus: SharedEventBus = Arc::new(super::super::EventBus::new(16));
        let r = Arc::new(StatusRegistry::new());
        r.spawn(&bus);
        tokio::task::yield_now().await;
        bus.publish(Event::AudioDeviceConnected { device_name: "m".into() });
        for _ in 0..50 {
            if r.snapshot().audio.connected {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("registry did not observe event");
    }
}
