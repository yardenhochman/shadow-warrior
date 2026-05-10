use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::core::ArenaState;

/// Complete brain state snapshot for API responses
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrainState {
    pub arena: ArenaStateInfo,
    pub hardware: HardwareStatus,
    pub statistics: Statistics,
}

/// Arena state information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArenaStateInfo {
    pub current_state: ArenaState,
    pub state_entered_at: DateTime<Utc>,
    pub time_in_state_sec: f64,

    // Energy bar (for WARMING state)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub energy: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_energy: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub energy_percentage: Option<f32>,

    // Fight timers (for FIGHT state)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fight_elapsed_sec: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fight_remaining_sec: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_since_last_activity_sec: Option<f64>,
}

/// Hardware connection status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareStatus {
    pub ble: ConnectionStatus,
    pub audio: ConnectionStatus,
    pub wled: Vec<WledStatus>,
    pub tasmota: Vec<TasmotaStatus>,
    pub music: MusicStatus,
}

/// Generic connection status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionStatus {
    pub connected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connected_at: Option<DateTime<Utc>>,
}

/// WLED controller status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WledStatus {
    pub ip: String,
    pub connected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_effect: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

/// Tasmota plug status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TasmotaStatus {
    pub name: String,
    pub ip: String,
    pub connected: bool,
    pub powered: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

/// Music player status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MusicStatus {
    pub playing: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_track: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub playlist_size: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

/// Statistics and metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Statistics {
    // Session stats
    pub total_sessions: u64,
    pub current_session_punches: u32,
    pub current_session_max_power: f32,

    // Sensor data
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_acceleration: Option<AccelerationSample>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_audio_level_db: Option<f32>,

    // System uptime
    pub uptime_sec: f64,
}

/// Acceleration data sample
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccelerationSample {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub magnitude: f32,
    pub timestamp: DateTime<Utc>,
}

/// API request/response types

#[derive(Debug, Deserialize)]
pub struct PresenceRequest {
    pub detected: bool,
}

#[derive(Debug, Deserialize)]
pub struct ShoutRequest {
    pub intensity: f32,
}

#[derive(Debug, Deserialize)]
pub struct PunchRequest {
    pub power: f32,
}

#[derive(Debug, Deserialize)]
pub struct StateTransitionRequest {
    pub state: ArenaState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameTunables {
    // Arena/Energy
    pub warming_energy_threshold: f32,
    pub shout_energy_multiplier: f32,
    pub energy_decay_rate: f32,
    pub fight_duration_sec: u64,
    pub fight_inactivity_timeout_sec: u64,
    pub cooldown_duration_sec: u64,
    pub shout_threshold_db: f32,
    pub shout_sensitivity: f32,
    pub vad_threshold: f32,

    // BLE/Punch
    pub punch_threshold: f32,
    pub punch_alpha: f32,
}

#[derive(Debug, Serialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl<T> ApiResponse<T> {
    pub fn success(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
        }
    }

    pub fn error(error: impl Into<String>) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(error.into()),
        }
    }
}

impl ApiResponse<()> {
    pub fn ok() -> Self {
        Self {
            success: true,
            data: None,
            error: None,
        }
    }
}

impl Default for ConnectionStatus {
    fn default() -> Self {
        Self {
            connected: false,
            device_name: None,
            last_error: None,
            connected_at: None,
        }
    }
}

impl Default for MusicStatus {
    fn default() -> Self {
        Self {
            playing: false,
            current_track: None,
            playlist_size: None,
            last_error: None,
        }
    }
}
