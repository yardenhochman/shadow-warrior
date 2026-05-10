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

    /// Get the number of active subscribers
    #[allow(dead_code)]
    pub fn subscriber_count(&self) -> usize {
        self.sender.receiver_count()
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
    ShoutDetected {
        intensity: f32,
        duration_sec: f32,
        level_db: f32,
    },
    AudioLevelChanged { level_db: f32 },

    // BLE events
    BleDeviceConnected {
        device_name: String,
        address: String,
    },
    BleDeviceDisconnected { reason: String },
    PunchDetected {
        power: f32,
        acceleration: AccelerationData,
    },
    BleConnectionError { error: String },

    // Music events
    MusicStarted { track_name: String },
    MusicStopped,
    MusicError { error: String },

    // WLED events
    WledConnected { ip: String },
    WledDisconnected { ip: String },
    WledError { ip: String, error: String },

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
            Event::ShoutDetected { .. } => "ShoutDetected",
            Event::AudioLevelChanged { .. } => "AudioLevelChanged",
            Event::BleDeviceConnected { .. } => "BleDeviceConnected",
            Event::BleDeviceDisconnected { .. } => "BleDeviceDisconnected",
            Event::PunchDetected { .. } => "PunchDetected",
            Event::BleConnectionError { .. } => "BleConnectionError",
            Event::MusicStarted { .. } => "MusicStarted",
            Event::MusicStopped => "MusicStopped",
            Event::MusicError { .. } => "MusicError",
            Event::WledConnected { .. } => "WledConnected",
            Event::WledDisconnected { .. } => "WledDisconnected",
            Event::WledError { .. } => "WledError",
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
