use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::broadcast;

/// Event bus for publishing and subscribing to system events
#[derive(Clone)]
pub struct EventBus {
    sender: broadcast::Sender<Event>,
}

impl EventBus {
    /// Create a new event bus with specified channel capacity
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    /// Publish an event to all subscribers
    pub fn publish(&self, event: Event) {
        // Ignore send errors (happens when no subscribers)
        let _ = self.sender.send(event);
    }

    /// Subscribe to all events
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.sender.subscribe()
    }

}

impl Default for EventBus {
    fn default() -> Self {
        Self::new(1000)
    }
}

/// System-wide event types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum Event {
    // System events
    SystemStartup,
    SystemShutdown,
    SystemError { error: String },

    // State machine events
    StateChanged {
        from: String,
        to: String,
        timestamp: DateTime<Utc>,
    },

    // Arena events
    SessionStarted { timestamp: DateTime<Utc> },
    SessionEnded { timestamp: DateTime<Utc> },
    PresenceDetected { detected: bool },

    // Energy bar events (WARMING state)
    EnergyChanged { energy: f32, max_energy: f32 },
    EnergyFull,

    // Fight events
    FightStarted { timestamp: DateTime<Utc> },
    FightEnded {
        reason: FightEndReason,
        timestamp: DateTime<Utc>,
    },
    ActivityTimeout,

    // Audio events
    AudioDeviceConnected { device_name: String },
    AudioDeviceDisconnected,
    AudioError { error: String },
    ShoutDetected {
        intensity: f32,
        duration_sec: f32,
        level_db: f32,
    },
    AudioLevelChanged { level_db: f32 },
    /// Continuous, low-latency shout level for the LED shout meter (~30 Hz while active, one
    /// final 0 when it falls silent). `level` 0..1 (fast attack, ~200 ms release),
    /// `db_above_floor` the band level over the adaptive noise floor. Not a detection:
    /// `ShoutDetected` stays the event that counts shouts.
    ShoutLevel { level: f32, db_above_floor: f32 },
    /// Which input the brain captures from. `pulse_source` is the PipeWire/Pulse source
    /// node behind an alias device ("default"), `external` = the station's stream.
    AudioInputInfo { device_name: String, sample_rate: u32, pulse_source: Option<String>, external: bool },
    /// No usable microphone (only null/monitor devices, or none at all). The capture
    /// is retried; this is published when the reason changes.
    AudioInputMissing { reason: String },
    /// The default PipeWire/Pulse output sink (services/audio_output.rs), on change.
    AudioOutputSink {
        sink: Option<String>,
        description: Option<String>,
        bluetooth: bool,
        volume_pct: Option<u32>,
        muted: Option<bool>,
    },

    // Sensor station (services/station_manager.rs). Sensor values only: the brain
    // decides what they mean. Radar, level and direction are throttled (about 2 / 2 / 5 Hz).
    /// First frame after a (re)connect, and again when the station's Hello arrives.
    StationConnected { station_id: Option<u32>, fw_version: Option<String> },
    StationDisconnected { reason: String },
    StationError { error: String },
    /// `present` is the radar's own bool (targets 1..=3); distance is of the nearest
    /// reported target (moving if the moving bit is set, else static), cm.
    StationRadar { present: bool, distance_cm: Option<u16>, moving_energy: u8, static_energy: u8 },
    StationLevel { rms_dbfs_l: f32, rms_dbfs_r: f32, peak_dbfs: f32 },
    /// GCC-PHAT angle, positive = toward mic B (right). Not used for game decisions.
    StationDirection { angle_deg: f32, confidence: f32, rms_dbfs: f32 },
    /// BLE provisioning progress of one station (services/station_provisioner.rs).
    /// `state`: found | provisioning | up | failed. `detail`: ip for up, reason for failed.
    StationProvision { station: String, state: String, detail: Option<String> },
    StationLinkStats { frames: u64, seq_gaps: u64, frames_lost: u64, bad_frames: u64, audio_gap_frames: u64 },

    // BLE events
    BleDeviceConnected {
        device_name: String,
        address: String,
    },
    BleDeviceDisconnected { reason: String },
    /// Link probe succeeded while idle (no punches); refreshes health `last_seen`.
    BleAlive,
    PunchDetected {
        power: f32,
        acceleration: AccelerationData,
    },
    BleConnectionError { error: String },

    // Bluetooth speaker (services/speaker_manager.rs)
    SpeakerConnected { name: String },
    SpeakerDisconnected,
    /// Link check passed; refreshes health `last_seen`.
    SpeakerAlive,
    SpeakerError { error: String },
    /// Reconnect attempts failed `attempts` times in a row; next try in about `retry_in_sec`.
    SpeakerBackoff { attempts: u32, retry_in_sec: u64 },

    // Music events
    MusicStarted { track_name: String },
    MusicStopped,
    MusicError { error: String },

    // WLED events
    WledConnected { ip: String },
    WledDisconnected { ip: String },
    WledError { ip: String, error: String },

    // Schedule events
    ScheduleChanged { active: bool },

    // Tasmota events
    TasmotaStateChanged { name: String, powered: bool },
    TasmotaError { name: String, error: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FightEndReason {
    TimeElapsed,
    InactivityTimeout,
    ManualStop,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccelerationData {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub magnitude: f32,
    pub timestamp: DateTime<Utc>,
}

impl Event {
    /// Create a state changed event
    pub fn state_changed(from: impl Into<String>, to: impl Into<String>) -> Self {
        Event::StateChanged {
            from: from.into(),
            to: to.into(),
            timestamp: Utc::now(),
        }
    }

    /// Create a shout detected event
    pub fn shout_detected(intensity: f32, duration_sec: f32, level_db: f32) -> Self {
        Event::ShoutDetected {
            intensity,
            duration_sec,
            level_db,
        }
    }

    /// Create a punch detected event
    pub fn punch_detected(power: f32, x: f32, y: f32, z: f32) -> Self {
        let magnitude = (x * x + y * y + z * z).sqrt();
        Event::PunchDetected {
            power,
            acceleration: AccelerationData {
                x,
                y,
                z,
                magnitude,
                timestamp: Utc::now(),
            },
        }
    }

    /// Create an energy changed event
    pub fn energy_changed(energy: f32, max_energy: f32) -> Self {
        Event::EnergyChanged { energy, max_energy }
    }

    /// Get event type as string for logging/display
    pub fn event_type(&self) -> &str {
        match self {
            Event::SystemStartup => "SystemStartup",
            Event::SystemShutdown => "SystemShutdown",
            Event::SystemError { .. } => "SystemError",
            Event::StateChanged { .. } => "StateChanged",
            Event::SessionStarted { .. } => "SessionStarted",
            Event::SessionEnded { .. } => "SessionEnded",
            Event::PresenceDetected { .. } => "PresenceDetected",
            Event::EnergyChanged { .. } => "EnergyChanged",
            Event::EnergyFull => "EnergyFull",
            Event::FightStarted { .. } => "FightStarted",
            Event::FightEnded { .. } => "FightEnded",
            Event::ActivityTimeout => "ActivityTimeout",
            Event::AudioDeviceConnected { .. } => "AudioDeviceConnected",
            Event::AudioDeviceDisconnected => "AudioDeviceDisconnected",
            Event::AudioError { .. } => "AudioError",
            Event::AudioInputInfo { .. } => "AudioInputInfo",
            Event::AudioInputMissing { .. } => "AudioInputMissing",
            Event::AudioOutputSink { .. } => "AudioOutputSink",
            Event::ShoutDetected { .. } => "ShoutDetected",
            Event::AudioLevelChanged { .. } => "AudioLevelChanged",
            Event::ShoutLevel { .. } => "ShoutLevel",
            Event::StationConnected { .. } => "StationConnected",
            Event::StationDisconnected { .. } => "StationDisconnected",
            Event::StationError { .. } => "StationError",
            Event::StationRadar { .. } => "StationRadar",
            Event::StationLevel { .. } => "StationLevel",
            Event::StationDirection { .. } => "StationDirection",
            Event::StationProvision { .. } => "StationProvision",
            Event::StationLinkStats { .. } => "StationLinkStats",
            Event::BleDeviceConnected { .. } => "BleDeviceConnected",
            Event::BleDeviceDisconnected { .. } => "BleDeviceDisconnected",
            Event::BleAlive => "BleAlive",
            Event::PunchDetected { .. } => "PunchDetected",
            Event::BleConnectionError { .. } => "BleConnectionError",
            Event::SpeakerConnected { .. } => "SpeakerConnected",
            Event::SpeakerDisconnected => "SpeakerDisconnected",
            Event::SpeakerAlive => "SpeakerAlive",
            Event::SpeakerError { .. } => "SpeakerError",
            Event::SpeakerBackoff { .. } => "SpeakerBackoff",
            Event::MusicStarted { .. } => "MusicStarted",
            Event::MusicStopped => "MusicStopped",
            Event::MusicError { .. } => "MusicError",
            Event::WledConnected { .. } => "WledConnected",
            Event::WledDisconnected { .. } => "WledDisconnected",
            Event::WledError { .. } => "WledError",
            Event::ScheduleChanged { .. } => "ScheduleChanged",
            Event::TasmotaStateChanged { .. } => "TasmotaStateChanged",
            Event::TasmotaError { .. } => "TasmotaError",
        }
    }
}

/// Shared event bus wrapped in Arc for easy cloning
pub type SharedEventBus = Arc<EventBus>;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_event_bus_publish_subscribe() {
        let bus = EventBus::new(10);
        let mut rx = bus.subscribe();

        bus.publish(Event::SystemStartup);

        let event = rx.recv().await.unwrap();
        assert!(matches!(event, Event::SystemStartup));
    }

    #[test]
    fn test_event_type() {
        let event = Event::SystemStartup;
        assert_eq!(event.event_type(), "SystemStartup");

        let event = Event::state_changed("IDLE", "WARMING");
        assert_eq!(event.event_type(), "StateChanged");
    }
}
