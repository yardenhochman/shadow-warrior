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
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub log_level: String,
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

    // Punching bag parameters
    pub alpha: f32,
    pub threshold: f32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AudioConfig {
    pub sample_rate: u32,
    pub channels: u16,
    pub buffer_size: usize,
    pub shout_threshold_db: f32,
    pub shout_min_duration_sec: f32,
    pub shout_sensitivity: f32,
    pub vad_threshold: f32,
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

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WledEffect {
    pub effect: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub palette: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed: Option<u8>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TasmotaConfig {
    pub plugs: Vec<TasmotaPlug>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TasmotaPlug {
    pub ip: String,
    pub name: String,
}

impl Config {
    /// Load configuration from config.yaml and environment variables
    pub fn load() -> Result<Self, figment::Error> {
        Figment::new()
            .merge(Serialized::defaults(Self::default()))
            .merge(Yaml::file("config.yaml"))
            .merge(Env::prefixed("BRAIN_"))
            .extract()
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server: ServerConfig {
                host: "0.0.0.0".to_string(),
                port: 8000,
                log_level: "info".to_string(),
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
                alpha: 0.8,
                threshold: 2.0,
            },
            audio: AudioConfig {
                sample_rate: 44100,
                channels: 1,
                buffer_size: 1024,
                shout_threshold_db: -15.0,
                shout_min_duration_sec: 0.5,
                shout_sensitivity: 1.0,
                vad_threshold: 0.5,
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
                    },
                    warming: WledEffect {
                        effect: "rainbow".to_string(),
                        palette: Some("rainbow".to_string()),
                        speed: Some(200),
                    },
                    fight: WledEffect {
                        effect: "realtime".to_string(),
                        palette: None,
                        speed: None,
                    },
                    cooldown: WledEffect {
                        effect: "fire".to_string(),
                        palette: Some("lava".to_string()),
                        speed: Some(150),
                    },
                },
            },
            tasmota: TasmotaConfig { plugs: vec![] },
        }
    }
}
