use figment::{
    providers::{Env, Format, Yaml, Serialized},
    Figment,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    pub server: ServerConfig,
    pub arena: ArenaConfig,
    pub ble: BleConfig,
    pub audio: AudioConfig,
    pub music: MusicConfig,
    pub wled: WledConfig,
    pub tasmota: TasmotaConfig,
    #[serde(default)]
    pub schedule: ScheduleConfig,
    #[serde(default)]
    pub presence: PresenceConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub log_level: String,
    /// A connected device with no event for this long is reported `stale` in
    /// `hardware.health`.
    #[serde(default = "default_health_stale_sec")]
    pub health_stale_sec: u64,
}

fn default_health_stale_sec() -> u64 {
    300
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ArenaConfig {
    // Timing configuration
    pub warming_energy_threshold: f32,
    pub fight_duration_sec: u64,
    pub fight_inactivity_timeout_sec: u64,
    pub cooldown_duration_sec: u64,

    // Energy bar settings
    pub shout_energy_multiplier: f32,
    pub energy_decay_rate: f32, // Points per second
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BleConfig {
    pub auto_connect: bool,
    pub scan_timeout_sec: u64,
    pub retry_interval_sec: u64,
    pub device_name_patterns: Vec<String>,
    /// BlueZ adapter to use: `hci1`, a numeric index (`1`) or a name substring.
    /// Unset = first adapter.
    #[serde(default)]
    pub adapter: Option<String>,

    // Punching bag parameters
    pub alpha: f32,
    pub threshold: f32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AudioConfig {
    pub sample_rate: u32,
    pub channels: u16,
    pub buffer_size: usize,
    // Legacy `shout_threshold_db` (absolute dB) was replaced by `shout_margin_db`
    // above the adaptive noise floor. Old config files may still contain the key;
    // serde ignores unknown fields, so it is accepted and dropped.
    /// Shout must be sustained (allowing short gaps) this long before it fires.
    pub shout_min_duration_sec: f32,
    pub shout_sensitivity: f32,
    pub vad_threshold: f32,
    /// Required band level above the adaptive noise floor, dB.
    #[serde(default = "d_margin")]
    pub shout_margin_db: f32,
    /// Extra margin added while our own speakers are playing, dB.
    #[serde(default = "d_playback_margin")]
    pub shout_playback_margin_db: f32,
    /// VAD gate stays open this many frames (16ms each) after the last VAD-positive frame.
    #[serde(default = "d_hangover")]
    pub vad_hangover_frames: u32,
    /// Voiced F0 range for a shout, Hz.
    #[serde(default = "d_f0_min")]
    pub shout_f0_min_hz: f32,
    #[serde(default = "d_f0_max")]
    pub shout_f0_max_hz: f32,
    /// Minimum autocorrelation pitch confidence (0..1) to count as voiced.
    #[serde(default = "d_pitch_conf")]
    pub shout_pitch_conf_min: f32,
    /// Minimum spectral centroid, Hz (shouts are bright).
    #[serde(default = "d_centroid")]
    pub shout_centroid_min_hz: f32,
    /// Minimum spectral tilt, dB/kHz (shouts are less steeply falling than speech/bass).
    #[serde(default = "d_tilt")]
    pub shout_tilt_min_db_per_khz: f32,
    /// Maximum spectral flatness (0..1); high values are noise (wind, crowd).
    #[serde(default = "d_flatness")]
    pub shout_flatness_max: f32,
    /// How many of the 4 spectral/pitch checks (voiced, centroid, tilt, flatness) must pass.
    #[serde(default = "d_min_features")]
    pub shout_min_features: u32,
    /// Score at which ShoutDetected fires (each shouty frame adds 1-2 x sensitivity).
    #[serde(default = "d_score_threshold")]
    pub shout_score_threshold: f32,
    /// Per-frame multiplicative score decay on non-shouty frames.
    #[serde(default = "d_score_decay")]
    pub shout_score_decay: f32,
    /// Non-shouty frames tolerated inside a sustained shout.
    #[serde(default = "d_gap_frames")]
    pub shout_gap_frames: u32,
    /// Directory where audio clips (wav + feature json) are written.
    #[serde(default = "default_clip_dir")]
    pub clip_dir: PathBuf,
    /// Length of recorded clips in seconds.
    #[serde(default = "default_clip_seconds")]
    pub clip_seconds: f32,
    /// Automatically record a clip whenever a shout is detected.
    #[serde(default = "default_clip_on_shout")]
    pub clip_on_shout: bool,
    /// Minimum seconds between automatic shout clips.
    #[serde(default = "default_clip_cooldown_sec")]
    pub clip_cooldown_sec: f32,
    /// Also write a raw (pre band-pass, device-rate) WAV next to each clip by default.
    #[serde(default = "d_true")]
    pub clip_raw: bool,
    /// Long-session recorder: seconds of raw audio per WAV chunk.
    #[serde(default = "d_record_chunk")]
    pub record_chunk_sec: f32,
    /// Long-session recorder: stop when free disk space drops below this (MiB).
    #[serde(default = "d_record_min_free")]
    pub record_min_free_mb: u64,
    /// Long-session recorder: hard cap on session length, seconds.
    #[serde(default = "d_record_max")]
    pub record_max_session_sec: f32,
    /// Long-session recorder: bounded queue (audio callbacks) between the audio
    /// thread and the writer; overflow drops frames and counts them.
    #[serde(default = "d_record_queue")]
    pub record_queue_len: usize,
}

fn d_true() -> bool { true }
fn d_record_chunk() -> f32 { 60.0 }
fn d_record_min_free() -> u64 { 500 }
fn d_record_max() -> f32 { 4.0 * 3600.0 }
fn d_record_queue() -> usize { 512 }

fn d_margin() -> f32 { 15.0 }
fn d_playback_margin() -> f32 { 8.0 }
fn d_hangover() -> u32 { 6 }
fn d_f0_min() -> f32 { 150.0 }
fn d_f0_max() -> f32 { 700.0 }
fn d_pitch_conf() -> f32 { 0.4 }
fn d_centroid() -> f32 { 900.0 }
fn d_tilt() -> f32 { -25.0 }
fn d_flatness() -> f32 { 0.5 }
fn d_min_features() -> u32 { 3 }
fn d_score_threshold() -> f32 { 8.0 }
fn d_score_decay() -> f32 { 0.9 }
fn d_gap_frames() -> u32 { 3 }

fn default_clip_dir() -> PathBuf {
    PathBuf::from("./clips")
}
fn default_clip_seconds() -> f32 {
    5.0
}
fn default_clip_on_shout() -> bool {
    true
}
fn default_clip_cooldown_sec() -> f32 {
    10.0
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MusicConfig {
    pub playlist_dir: PathBuf,
    pub formats: Vec<String>,
    pub shuffle: bool,
    pub volume: f32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WledConfig {
    pub controllers: Vec<WledController>,
    pub udp_fps: u32,
    pub effects: WledEffects,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WledController {
    pub ip: String,
    pub num_leds: usize,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WledEffects {
    pub idle: WledEffect,
    pub warming: WledEffect,
    pub fight: WledEffect,
    pub cooldown: WledEffect,
}

/// A WLED effect description. `effect` and `palette` may be a name (resolved
/// against the device's own effect/palette list) or a numeric WLED id.
/// Everything except `effect` is optional and falls back to WLED defaults.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct WledEffect {
    pub effect: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub palette: Option<String>,
    /// Effect speed (WLED `sx`)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed: Option<u8>,
    /// Effect intensity (WLED `ix`)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intensity: Option<u8>,
    /// Master brightness (WLED `bri`)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub brightness: Option<u8>,
    /// Transition time in 100ms units (WLED `transition`)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transition: Option<u16>,
    /// Segment colors (WLED `col`), up to 3 RGB triples
    #[serde(skip_serializing_if = "Option::is_none")]
    pub colors: Option<Vec<[u8; 3]>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TasmotaConfig {
    pub plugs: Vec<TasmotaPlug>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TasmotaPlug {
    pub ip: String,
    pub name: String,
    /// Arena states (case-insensitive: suspended, idle, warming, fight,
    /// cooldown) during which this plug is ON; it is OFF in every other
    /// state. Empty (default) = never turned on by the game; the plug is only
    /// switched off when the arena goes IDLE/SUSPENDED.
    #[serde(default)]
    pub on_states: Vec<String>,
}

/// Fused presence detection (services/presence.rs). Every key is optional.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct PresenceConfig {
    /// Master switch for the automatic presence service (API-driven presence still works).
    pub enabled: bool,
    /// Mic: required dB above the adaptive noise floor (near-field gate).
    pub mic_margin_db: f32,
    /// Mic: extra margin while our own speakers play.
    pub mic_playback_margin_db: f32,
    /// Mic: VAD probability required for a frame to count.
    pub mic_vad_threshold: f32,
    /// Punch: evidence units one punch is worth (before the source weight).
    pub punch_units: f32,
    /// Per-source multiplier on evidence. Unknown sources (radar, motion...) use
    /// `default_weight`.
    pub source_weights: std::collections::HashMap<String, f32>,
    pub default_weight: f32,
    /// Accumulated evidence (weighted evidence-seconds) needed to enter.
    pub enter_threshold: f32,
    /// Accumulator leak, evidence-seconds per second. Must exceed the rate at
    /// which background noise leaks past the gate; this is what rejects bursts.
    pub decay_per_sec: f32,
    /// Accumulator cap (bounds how long a burst of evidence lingers).
    pub max_score: f32,
    /// Seconds without any evidence before leaving.
    pub leave_timeout_sec: f32,
}

impl Default for PresenceConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            mic_margin_db: 10.0,
            mic_playback_margin_db: 8.0,
            mic_vad_threshold: 0.5,
            punch_units: 1.0,
            source_weights: [("mic".to_string(), 1.0), ("punch".to_string(), 1.0)]
                .into_iter()
                .collect(),
            default_weight: 1.0,
            enter_threshold: 1.0,
            decay_per_sec: 0.2,
            max_score: 2.0,
            leave_timeout_sec: 60.0,
        }
    }
}

/// Daily schedule: the game is active only inside the configured windows.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Default)]
pub struct ScheduleConfig {
    /// When false the schedule is ignored and the game is always active.
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub windows: Vec<ScheduleWindow>,
}

/// One on-period. Times are local "HH:MM". If `end` <= `start` the window
/// wraps past midnight. Empty `days` means every day (day of the start time).
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct ScheduleWindow {
    #[serde(default)]
    pub days: Vec<String>,
    pub start: String,
    pub end: String,
}

impl Config {
    /// Load configuration from config.yaml and environment variables
    pub fn load() -> Result<Self, Box<figment::Error>> {
        Figment::new()
            .merge(Serialized::defaults(Self::default()))
            .merge(Yaml::file("config.yaml"))
            .merge(Env::prefixed("BRAIN_"))
            .extract()
            .map_err(Box::new)
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server: ServerConfig {
                host: "0.0.0.0".to_string(),
                port: 8000,
                log_level: "info".to_string(),
                health_stale_sec: default_health_stale_sec(),
            },
            arena: ArenaConfig {
                warming_energy_threshold: 100.0,
                fight_duration_sec: 180,
                fight_inactivity_timeout_sec: 30,
                cooldown_duration_sec: 60,
                shout_energy_multiplier: 10.0,
                energy_decay_rate: 0.5,
            },
            ble: BleConfig {
                auto_connect: true,
                scan_timeout_sec: 5,
                retry_interval_sec: 10,
                device_name_patterns: vec![
                    "shadow".to_string(),
                    "warrior".to_string(),
                    "punch".to_string(),
                    "bag".to_string(),
                ],
                adapter: None,
                alpha: 0.8,
                threshold: 2.0,
            },
            audio: AudioConfig {
                sample_rate: 44100,
                channels: 1,
                buffer_size: 1024,
                shout_min_duration_sec: 0.2,
                shout_sensitivity: 1.0,
                vad_threshold: 0.5,
                shout_margin_db: d_margin(),
                shout_playback_margin_db: d_playback_margin(),
                vad_hangover_frames: d_hangover(),
                shout_f0_min_hz: d_f0_min(),
                shout_f0_max_hz: d_f0_max(),
                shout_pitch_conf_min: d_pitch_conf(),
                shout_centroid_min_hz: d_centroid(),
                shout_tilt_min_db_per_khz: d_tilt(),
                shout_flatness_max: d_flatness(),
                shout_min_features: d_min_features(),
                shout_score_threshold: d_score_threshold(),
                shout_score_decay: d_score_decay(),
                shout_gap_frames: d_gap_frames(),
                clip_dir: default_clip_dir(),
                clip_seconds: default_clip_seconds(),
                clip_on_shout: default_clip_on_shout(),
                clip_cooldown_sec: default_clip_cooldown_sec(),
                clip_raw: d_true(),
                record_chunk_sec: d_record_chunk(),
                record_min_free_mb: d_record_min_free(),
                record_max_session_sec: d_record_max(),
                record_queue_len: d_record_queue(),
            },
            music: MusicConfig {
                playlist_dir: PathBuf::from("./music"),
                formats: vec!["mp3".to_string(), "flac".to_string(), "wav".to_string()],
                shuffle: true,
                volume: 0.8,
            },
            wled: WledConfig {
                controllers: vec![],
                udp_fps: 30,
                effects: WledEffects {
                    idle: WledEffect {
                        effect: "breathe".to_string(),
                        palette: Some("ocean".to_string()),
                        speed: Some(128),
                        ..Default::default()
                    },
                    warming: WledEffect {
                        effect: "rainbow".to_string(),
                        palette: Some("rainbow".to_string()),
                        speed: Some(200),
                        ..Default::default()
                    },
                    fight: WledEffect {
                        effect: "realtime".to_string(),
                        palette: None,
                        speed: None,
                        ..Default::default()
                    },
                    cooldown: WledEffect {
                        effect: "fire".to_string(),
                        palette: Some("lava".to_string()),
                        speed: Some(150),
                        ..Default::default()
                    },
                },
            },
            tasmota: TasmotaConfig { plugs: vec![] },
            schedule: ScheduleConfig::default(),
            presence: PresenceConfig::default(),
        }
    }
}
