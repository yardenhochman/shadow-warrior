//! Hardware health registry: folds device events from the event bus into
//! per-device connection state, last-seen / last-error timestamps and error text.
//! Managers only publish events; they never talk to the registry directly.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast::error::RecvError;

use super::audio_health::{AudioInHealth, AudioOutHealth, FeatureFlags};
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
    /// Derived at read time: connected but nothing heard for longer than the
    /// stale timeout. Never set by events.
    #[serde(default)]
    pub stale: bool,
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

/// Sensor station: link health plus the latest sensor values for the dashboard.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct StationHealth {
    #[serde(flatten)]
    pub link: DeviceHealth,
    pub radar_present: Option<bool>,
    pub radar_distance_cm: Option<u16>,
    pub rms_dbfs_l: Option<f32>,
    pub rms_dbfs_r: Option<f32>,
    pub direction_deg: Option<f32>,
    pub direction_confidence: Option<f32>,
    pub frames: u64,
    pub frames_lost: u64,
    pub bad_frames: u64,
    pub audio_gap_frames: u64,
}

/// Provisioning state of one station: found | provisioning | up | failed.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ProvisionEntry {
    pub state: String,
    pub detail: Option<String>,
    pub updated: Option<DateTime<Utc>>,
}

/// Snapshot of every device, serialized in the status API.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HardwareHealth {
    pub ble: DeviceHealth,
    pub audio: DeviceHealth,
    pub music: DeviceHealth,
    /// The paired Bluetooth speaker (only reported once `speaker.address` is configured).
    #[serde(default)]
    pub speaker: DeviceHealth,
    /// The sensor station (only meaningful once `station.enabled`).
    #[serde(default)]
    pub station: StationHealth,
    /// BLE provisioning state per station (key: advertised name or address).
    #[serde(default)]
    pub station_provision: BTreeMap<String, ProvisionEntry>,
    /// Which optional features are configured (so "off" is not shown as an error).
    #[serde(default)]
    pub features: FeatureFlags,
    /// Microphone / station audio source and its live level.
    #[serde(default)]
    pub audio_in: AudioInHealth,
    /// Speaker supervision and the output sink in use.
    #[serde(default)]
    pub audio_out: AudioOutHealth,
    /// keyed by controller IP
    pub wled: BTreeMap<String, DeviceHealth>,
    /// keyed by plug name
    pub tasmota: BTreeMap<String, DeviceHealth>,
}

impl HardwareHealth {
    fn devices_mut(&mut self) -> impl Iterator<Item = &mut DeviceHealth> {
        [&mut self.ble, &mut self.audio, &mut self.music, &mut self.speaker, &mut self.station.link]
            .into_iter()
            .chain(self.wled.values_mut())
            .chain(self.tasmota.values_mut())
    }
}

/// Default for [`StatusRegistry::new`]; the daemon passes `server.health_stale_sec`.
const DEFAULT_STALE_AFTER: std::time::Duration = std::time::Duration::from_secs(300);

pub struct StatusRegistry {
    inner: Mutex<HardwareHealth>,
    stale_after: chrono::Duration,
}

impl Default for StatusRegistry {
    fn default() -> Self {
        Self::with_stale_after(DEFAULT_STALE_AFTER)
    }
}

impl StatusRegistry {
    #[cfg(test)]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_stale_after(d: std::time::Duration) -> Self {
        Self {
            inner: Mutex::new(HardwareHealth::default()),
            stale_after: chrono::Duration::from_std(d).unwrap_or(chrono::Duration::MAX),
        }
    }

    /// Record which features are configured (called once at startup).
    pub fn set_features(&self, f: FeatureFlags) {
        self.inner.lock().unwrap().features = f;
    }

    pub fn snapshot(&self) -> HardwareHealth {
        self.snapshot_at(Utc::now())
    }

    /// Snapshot with `stale` derived against the supplied clock: a connected
    /// device whose `last_seen` is older than the timeout is stale.
    pub fn snapshot_at(&self, now: DateTime<Utc>) -> HardwareHealth {
        let mut h = self.inner.lock().unwrap().clone();
        for d in h.devices_mut() {
            d.stale = d.connected
                && d.last_seen
                    .is_some_and(|t| now.signed_duration_since(t) > self.stale_after);
        }
        h.audio_in.derive(&h.features, h.audio.connected, now);
        h.audio_out.derive(&h.features, h.speaker.connected, h.speaker.last_error.is_some());
        h
    }

    /// Fold one event into the registry (pure w.r.t. the supplied clock).
    pub fn apply(&self, event: &Event, now: DateTime<Utc>) {
        let mut h = self.inner.lock().unwrap();
        match event {
            Event::AudioDeviceConnected { device_name } => {
                h.audio.name = Some(device_name.clone());
                h.audio.up(now);
                h.audio_in.reason = None;
                h.audio_in.device.get_or_insert_with(|| device_name.clone());
            }
            Event::AudioInputInfo { device_name, sample_rate, pulse_source, external } => {
                let a = &mut h.audio_in;
                a.source = Some(if *external { "station" } else { "microphone" }.into());
                a.device = Some(device_name.clone());
                a.pulse_source = pulse_source.clone();
                a.sample_rate = Some(*sample_rate);
                a.reason = None;
            }
            Event::AudioInputMissing { reason } => {
                h.audio_in.reason = Some(reason.clone());
                h.audio_in.source = None;
                h.audio.down(now, Some(reason.clone()));
                h.audio.fail(now, reason.clone());
            }
            Event::AudioOutputSink { sink, description, bluetooth, volume_pct, muted } => {
                let o = &mut h.audio_out;
                o.sink = sink.clone();
                o.sink_description = description.clone();
                o.sink_bluetooth = *bluetooth;
                o.volume_pct = *volume_pct;
                o.muted = *muted;
                o.checked_at = Some(now);
            }
            Event::SpeakerBackoff { attempts, retry_in_sec } => {
                h.audio_out.speaker_attempts = *attempts;
                h.audio_out.speaker_retry_in_sec = Some(*retry_in_sec);
            }
            Event::AudioDeviceDisconnected => {
                let reason = h.audio.last_error.clone();
                h.audio.down(now, reason);
            }
            Event::AudioError { error } => h.audio.fail(now, error.clone()),
            Event::AudioLevelChanged { level_db } => {
                h.audio.seen(now);
                h.audio_in.level(*level_db, now);
            }
            Event::AdaptiveSensitivity { ambient_db, adaptive_offset_db } => {
                h.audio_in.ambient_db = Some(*ambient_db);
                h.audio_in.adaptive_offset_db = Some(*adaptive_offset_db);
            }
            Event::ShoutDetected { .. } => h.audio.seen(now),
            Event::StationConnected { station_id, fw_version } => {
                let l = &mut h.station.link;
                if let Some(id) = station_id {
                    l.name = Some(format!("station-{id:08x}"));
                }
                l.detail = fw_version.as_ref().map(|v| format!("fw {v}")).or(l.detail.take());
                l.up(now);
            }
            Event::StationDisconnected { reason } => h.station.link.down(now, Some(reason.clone())),
            Event::StationError { error } => h.station.link.fail(now, error.clone()),
            Event::StationRadar { present, distance_cm, .. } => {
                h.station.radar_present = Some(*present);
                h.station.radar_distance_cm = *distance_cm;
                h.station.link.seen(now);
            }
            Event::StationLevel { rms_dbfs_l, rms_dbfs_r, .. } => {
                h.station.rms_dbfs_l = Some(*rms_dbfs_l);
                h.station.rms_dbfs_r = Some(*rms_dbfs_r);
                h.station.link.seen(now);
            }
            Event::StationDirection { angle_deg, confidence, .. } => {
                h.station.direction_deg = Some(*angle_deg);
                h.station.direction_confidence = Some(*confidence);
                h.station.link.seen(now);
            }
            Event::StationProvision { station, state, detail } => {
                h.station_provision.insert(
                    station.clone(),
                    ProvisionEntry { state: state.clone(), detail: detail.clone(), updated: Some(now) },
                );
            }
            Event::StationLinkStats { frames, frames_lost, bad_frames, audio_gap_frames, .. } => {
                let s = &mut h.station;
                s.frames = *frames;
                s.frames_lost = *frames_lost;
                s.bad_frames = *bad_frames;
                s.audio_gap_frames = *audio_gap_frames;
            }
            Event::BleDeviceConnected { device_name, address } => {
                h.ble.name = Some(device_name.clone());
                h.ble.address = Some(address.clone());
                h.ble.up(now);
            }
            Event::BleDeviceDisconnected { reason } => h.ble.down(now, Some(reason.clone())),
            Event::PunchDetected { .. } | Event::BleAlive => h.ble.seen(now),
            Event::BleConnectionError { error } => h.ble.fail(now, error.clone()),
            Event::SpeakerConnected { name } => {
                h.speaker.name = Some(name.clone());
                h.speaker.up(now);
                h.audio_out.speaker_attempts = 0;
                h.audio_out.speaker_retry_in_sec = None;
            }
            Event::SpeakerDisconnected => {
                let reason = h.speaker.last_error.clone();
                h.speaker.down(now, reason);
            }
            Event::SpeakerAlive => h.speaker.seen(now),
            Event::SpeakerError { error } => h.speaker.fail(now, error.clone()),
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
                let reason = d.last_error.clone();
                d.down(now, reason);
            }
            Event::WledMoved { old, new, .. } => {
                // Same device, new address: carry its history over, drop the dead key.
                let mut d = h.wled.remove(old).unwrap_or_default();
                d.address = Some(new.clone());
                h.wled.insert(new.clone(), d);
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

    #[test]
    fn stale_is_derived_at_read_time() {
        let r = StatusRegistry::with_stale_after(std::time::Duration::from_secs(60));
        let t = Utc::now();
        r.apply(&Event::AudioDeviceConnected { device_name: "usb".into() }, t);
        r.apply(&Event::WledConnected { ip: "1.2.3.4".into() }, t);
        r.apply(&Event::WledConnected { ip: "5.6.7.8".into() }, t + Duration::seconds(50));
        let s = r.snapshot_at(t + Duration::seconds(60));
        assert!(!s.audio.stale, "exactly at the timeout is not stale yet");
        let s = r.snapshot_at(t + Duration::seconds(61));
        assert!(s.audio.stale);
        assert!(s.wled["1.2.3.4"].stale);
        assert!(!s.wled["5.6.7.8"].stale, "recent event keeps it fresh");
        // a fresh event clears staleness without any timer
        r.apply(&Event::AudioLevelChanged { level_db: -30.0 }, t + Duration::seconds(100));
        assert!(!r.snapshot_at(t + Duration::seconds(120)).audio.stale);
        // stored state is untouched by reads
        assert!(!r.inner.lock().unwrap().audio.stale);
    }

    #[test]
    fn disconnected_or_never_seen_is_not_stale() {
        let r = StatusRegistry::with_stale_after(std::time::Duration::from_secs(1));
        let t = Utc::now();
        r.apply(&Event::BleDeviceConnected { device_name: "bag".into(), address: "aa".into() }, t);
        r.apply(&Event::BleDeviceDisconnected { reason: "gone".into() }, t);
        let s = r.snapshot_at(t + Duration::hours(1));
        assert!(!s.ble.connected && !s.ble.stale);
        assert!(!s.music.stale && !s.audio.stale);
    }

    #[test]
    fn audio_error_then_disconnect_records_reason_and_reconnect_clears_it() {
        let r = StatusRegistry::new();
        let t = Utc::now();
        r.apply(&Event::AudioDeviceConnected { device_name: "usb".into() }, t);
        r.apply(&Event::AudioError { error: "unplugged".into() }, t + Duration::seconds(1));
        r.apply(&Event::AudioDeviceDisconnected, t + Duration::seconds(1));
        let a = r.snapshot().audio;
        assert!(!a.connected);
        assert_eq!(a.disconnect_reason.as_deref(), Some("unplugged"));
        assert_eq!(a.error_count, 1);
        r.apply(&Event::AudioDeviceConnected { device_name: "usb".into() }, t + Duration::seconds(5));
        let a = r.snapshot().audio;
        assert!(a.connected && a.disconnect_reason.is_none());
        assert_eq!(a.error_count, 1);
    }

    #[test]
    fn wled_disconnect_after_error_carries_reason() {
        let r = StatusRegistry::new();
        let t = Utc::now();
        r.apply(&Event::WledError { ip: "9.9.9.9".into(), error: "timed out".into() }, t);
        r.apply(&Event::WledDisconnected { ip: "9.9.9.9".into() }, t);
        let w = &r.snapshot().wled["9.9.9.9"];
        assert!(!w.connected);
        assert_eq!(w.disconnect_reason.as_deref(), Some("timed out"));
    }

    #[test]
    fn ble_connection_error_is_counted() {
        let r = StatusRegistry::new();
        let t = Utc::now();
        r.apply(&Event::BleConnectionError { error: "no adapter".into() }, t);
        r.apply(&Event::BleConnectionError { error: "no device".into() }, t);
        let b = r.snapshot().ble;
        assert_eq!(b.error_count, 2);
        assert_eq!(b.last_error.as_deref(), Some("no device"));
        assert!(!b.connected);
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

    #[test]
    fn ble_alive_keeps_idle_device_fresh() {
        let r = StatusRegistry::with_stale_after(std::time::Duration::from_secs(300));
        let t0 = Utc::now();
        r.apply(&Event::BleDeviceConnected { device_name: "bag".into(), address: "a".into() }, t0);
        let later = t0 + chrono::Duration::seconds(400);
        assert!(r.snapshot_at(later).ble.stale, "no events -> stale");
        r.apply(&Event::BleAlive, later - chrono::Duration::seconds(10));
        assert!(!r.snapshot_at(later).ble.stale, "heartbeat refreshes last_seen");
    }

    #[test]
    fn speaker_drop_and_recovery_are_tracked() {
        let r = StatusRegistry::with_stale_after(std::time::Duration::from_secs(300));
        let t = Utc::now();
        r.apply(&Event::SpeakerConnected { name: "BOOM 4".into() }, t);
        assert!(r.snapshot_at(t).speaker.connected);
        r.apply(&Event::SpeakerError { error: "no a2dp".into() }, t);
        r.apply(&Event::SpeakerDisconnected, t);
        let d = r.snapshot_at(t).speaker;
        assert!(!d.connected && d.disconnect_reason.as_deref() == Some("no a2dp") && d.error_count == 1);
        r.apply(&Event::SpeakerConnected { name: "BOOM 4".into() }, t);
        let d = r.snapshot_at(t).speaker;
        assert!(d.connected && d.disconnect_reason.is_none());
        r.apply(&Event::SpeakerAlive, t + chrono::Duration::seconds(400));
        assert!(!r.snapshot_at(t + chrono::Duration::seconds(401)).speaker.stale);
    }

    fn flags() -> FeatureFlags {
        FeatureFlags { audio_input: true, speaker_configured: true, ..Default::default() }
    }

    #[test]
    fn missing_microphone_is_reported_with_reason_and_recovers() {
        let r = StatusRegistry::new();
        r.set_features(flags());
        let t = Utc::now();
        let reason = "no microphone: only null/monitor devices present (Discard all samples)";
        r.apply(&Event::AudioInputMissing { reason: reason.into() }, t);
        let h = r.snapshot_at(t);
        assert!(!h.audio.connected);
        assert_eq!(h.audio.last_error.as_deref(), Some(reason));
        assert_eq!(h.audio_in.status, "no_microphone");
        assert_eq!(h.audio_in.reason.as_deref(), Some(reason));

        r.apply(
            &Event::AudioInputInfo { device_name: "BOMGE".into(), sample_rate: 16000, pulse_source: Some("alsa_input.usb".into()), external: false },
            t,
        );
        r.apply(&Event::AudioDeviceConnected { device_name: "BOMGE".into() }, t);
        let h = r.snapshot_at(t);
        assert!(h.audio.connected);
        assert!(h.audio_in.reason.is_none());
        assert_eq!(h.audio_in.source.as_deref(), Some("microphone"));
        assert_eq!(h.audio_in.pulse_source.as_deref(), Some("alsa_input.usb"));
        assert_eq!(h.audio_in.status, "no_data", "connected but no level yet");
    }

    #[test]
    fn input_level_gives_signal_or_silent_and_goes_stale() {
        let r = StatusRegistry::new();
        r.set_features(flags());
        let t = Utc::now();
        r.apply(&Event::AudioDeviceConnected { device_name: "m".into() }, t);
        r.apply(&Event::AudioLevelChanged { level_db: -90.0 }, t);
        assert_eq!(r.snapshot_at(t).audio_in.status, "silent");
        r.apply(&Event::AudioLevelChanged { level_db: -20.0 }, t);
        r.apply(&Event::AudioLevelChanged { level_db: -40.0 }, t);
        let a = r.snapshot_at(t).audio_in;
        assert_eq!((a.status.as_str(), a.level_db, a.peak_db), ("signal", Some(-40.0), Some(-20.0)));
        // peak is held for a few seconds, then follows the level
        let later = t + chrono::Duration::seconds(5);
        r.apply(&Event::AudioLevelChanged { level_db: -50.0 }, later);
        assert_eq!(r.snapshot_at(later).audio_in.peak_db, Some(-50.0));
        assert_eq!(r.snapshot_at(later + chrono::Duration::seconds(10)).audio_in.status, "no_data");
    }

    #[test]
    fn station_stream_is_the_audio_source_and_disabled_input_is_not_an_error() {
        let r = StatusRegistry::new();
        assert_eq!(r.snapshot().audio_in.status, "disabled");
        r.set_features(flags());
        let t = Utc::now();
        r.apply(&Event::AudioInputInfo { device_name: "sensor station".into(), sample_rate: 16000, pulse_source: None, external: true }, t);
        assert_eq!(r.snapshot_at(t).audio_in.source.as_deref(), Some("station"));
    }

    #[test]
    fn output_sink_and_speaker_state() {
        let r = StatusRegistry::new();
        let t = Utc::now();
        assert_eq!(r.snapshot_at(t).audio_out.speaker_state, "not_configured");
        r.set_features(flags());
        assert_eq!(r.snapshot_at(t).audio_out.speaker_state, "connecting");
        assert_eq!(r.snapshot_at(t).audio_out.route, "unknown");
        r.apply(&Event::SpeakerError { error: "page timeout".into() }, t);
        r.apply(&Event::SpeakerBackoff { attempts: 2, retry_in_sec: 12 }, t);
        let o = r.snapshot_at(t).audio_out;
        assert_eq!((o.speaker_state.as_str(), o.speaker_attempts, o.speaker_retry_in_sec), ("failed", 2, Some(12)));

        r.apply(
            &Event::AudioOutputSink { sink: Some("alsa_output.builtin".into()), description: Some("Built-in Audio".into()), bluetooth: false, volume_pct: Some(80), muted: Some(false) },
            t,
        );
        assert_eq!(r.snapshot_at(t).audio_out.route, "builtin");
        r.apply(&Event::SpeakerConnected { name: "BOOM 4".into() }, t);
        r.apply(
            &Event::AudioOutputSink { sink: Some("bluez_output.x".into()), description: Some("UE BOOM 4".into()), bluetooth: true, volume_pct: Some(65), muted: Some(false) },
            t,
        );
        let o = r.snapshot_at(t).audio_out;
        assert_eq!((o.speaker_state.as_str(), o.route.as_str(), o.speaker_attempts), ("connected", "bluetooth", 0));
    }
}
