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
    #[serde(default)]
    pub ble_status: BleStatusConfig,
    #[serde(default)]
    pub speaker: SpeakerConfig,
    #[serde(default)]
    pub sound: SoundConfig,
    #[serde(default)]
    pub station: StationConfig,
    #[serde(default)]
    pub led: LedConfig,
}

/// Look of the brain-rendered LED scenes (warming, fight; see docs/LEDS.md).
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct LedConfig {
    /// Shout meter colour, RGB 0..255 (the energy bar is red to amber: keep it different).
    pub shout_meter_color: [u8; 3],
    /// Meter intensity in the fight scene relative to warming (0 = no meter in the fight).
    pub shout_meter_fight_scale: f32,
}

impl Default for LedConfig {
    fn default() -> Self {
        Self { shout_meter_color: [0, 200, 255], shout_meter_fight_scale: 0.5 }
    }
}

/// Sensor station link (services/station_manager.rs). Off unless `enabled`.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct StationConfig {
    pub enabled: bool,
    /// UDP address the brain listens on for station frames (one frame per datagram).
    /// Unset (null) = no UDP transport.
    pub udp_bind: Option<String>,
    /// Where command frames go (`ip:4211`). Unset = the source address of the last datagram, port 4211.
    pub station_cmd_addr: Option<String>,
    /// USB serial device of the station (`/dev/ttyUSB0`, `/dev/cu.usbserial-0001`). Unset = no serial.
    /// Reopened every few seconds after unplugging.
    pub serial_port: Option<String>,
    /// 115200 for the default firmware, 921600 for the raw-audio build.
    pub serial_baud: u32,
    /// Ask the station for raw 16 kHz stereo audio and feed it to the shout detector
    /// (replaces the local microphone: `audio.input` capture is skipped).
    pub stream_audio: bool,
    /// Seconds without any frame before the station is reported disconnected.
    pub link_timeout_sec: f32,
    /// Seconds between link statistics events.
    pub stats_interval_sec: f32,
    /// BLE provisioning of stations without working Wi-Fi (services/station_provisioner.rs).
    pub provision: ProvisionConfig,
}

/// BLE provisioning of sensor stations (docs/STATION.md). Off unless `enabled`.
/// The three secrets never serialize and never appear in `Debug` output.
#[derive(Clone, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct ProvisionConfig {
    pub enabled: bool,
    /// Shared passphrase; the firmware has it as `STATION_PROVISION_KEY`.
    #[serde(skip_serializing)]
    pub provision_key: String,
    pub wifi_ssid: String,
    #[serde(skip_serializing)]
    pub wifi_pass: String,
    /// `ip[:port]` the station sends frames to. Empty = this host's LAN address (toward the
    /// default route) with the port of `station.udp_bind`.
    pub brain_addr: String,
    /// BlueZ adapter selector (same syntax as `ble.adapter`). Unset = `ble.adapter`.
    pub adapter: Option<String>,
    /// Seconds between scans.
    pub scan_interval_sec: f32,
    /// How long each scan listens for advertisements.
    pub scan_timeout_sec: f32,
    /// Seconds to wait for the station's UDP frames after the config write (it reboots into Wi-Fi mode).
    pub up_timeout_sec: f32,
    /// First retry delay for a station that failed; doubles per failure up to `retry_backoff_max_sec`.
    pub retry_backoff_sec: f32,
    pub retry_backoff_max_sec: f32,
}

impl Default for ProvisionConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            provision_key: String::new(),
            wifi_ssid: String::new(),
            wifi_pass: String::new(),
            brain_addr: String::new(),
            adapter: None,
            scan_interval_sec: 30.0,
            scan_timeout_sec: 8.0,
            up_timeout_sec: 60.0,
            retry_backoff_sec: 60.0,
            retry_backoff_max_sec: 900.0,
        }
    }
}

impl std::fmt::Debug for ProvisionConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProvisionConfig")
            .field("enabled", &self.enabled)
            .field("provision_key", &"<redacted>")
            .field("wifi_ssid", &self.wifi_ssid)
            .field("wifi_pass", &"<redacted>")
            .field("brain_addr", &self.brain_addr)
            .field("adapter", &self.adapter)
            .finish_non_exhaustive()
    }
}

impl Default for StationConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            udp_bind: Some("0.0.0.0:4210".into()),
            station_cmd_addr: None,
            serial_port: None,
            serial_baud: 115_200,
            stream_audio: false,
            link_timeout_sec: 3.0,
            stats_interval_sec: 5.0,
            provision: ProvisionConfig::default(),
        }
    }
}

/// Keeps one paired+trusted Bluetooth speaker connected (services/speaker_manager.rs).
/// Off unless `address` is set. Linux/BlueZ only.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct SpeakerConfig {
    /// Bluetooth address of the speaker (`10:94:97:40:6F:95`). Unset = disabled.
    pub address: Option<String>,
    /// Adapter the speaker is paired on: `hciN`, an index, `usb`, `usb:VVVV:PPPP` or
    /// `builtin` (same selectors as `ble.adapter`). Unset = first adapter.
    pub adapter: Option<String>,
    /// Seconds between link checks.
    pub check_interval_sec: u64,
    /// Longest wait between reconnect attempts while the speaker keeps failing.
    pub max_backoff_sec: u64,
}

impl Default for SpeakerConfig {
    fn default() -> Self {
        Self { address: None, adapter: None, check_interval_sec: 10, max_backoff_sec: 60 }
    }
}

/// Event-triggered sound effects and voice lines (services/sound_manager.rs).
/// Off unless `enabled`; see docs/SOUND.md.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct SoundConfig {
    pub enabled: bool,
    /// Cue files are looked up under this directory.
    pub dir: PathBuf,
    /// Master gain for all cues (linear).
    pub volume: f32,
    /// Music level while a cue plays, in dB (negative = quieter).
    pub music_duck_db: f32,
    /// Music stays ducked this long after the last cue ends.
    pub duck_ms: u64,
    /// Cues playing at once; further triggers are dropped.
    pub max_concurrent: usize,
    /// Punch power below this is a jab; from `heavy_from` up is a heavy; between is a hook.
    pub hook_from: f32,
    pub heavy_from: f32,
    /// Cue name -> files. Names: `session_started`, `energy_full`, `fight_started`,
    /// `fight_ended` (fallback) / `fight_ended_time_elapsed` / `fight_ended_inactivity` /
    /// `fight_ended_manual`, `punch_jab`, `punch_hook`, `punch_heavy`.
    pub cues: std::collections::BTreeMap<String, CueConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct CueConfig {
    /// Files under `sound.dir`; one is picked at random, never the same one twice in a row.
    pub files: Vec<String>,
    /// Linear gain on top of `sound.volume`.
    pub gain: f32,
    /// Triggers closer than this to the last play of the same cue are dropped.
    pub min_interval_ms: u64,
}

impl Default for CueConfig {
    fn default() -> Self {
        Self { files: Vec::new(), gain: 1.0, min_interval_ms: 0 }
    }
}

impl Default for SoundConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            dir: PathBuf::from("./sounds"),
            volume: 1.0,
            music_duck_db: -9.0,
            duck_ms: 300,
            max_concurrent: 4,
            hook_from: 45.0,
            heavy_from: 75.0,
            cues: Default::default(),
        }
    }
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
    /// Advertise the brain as `<mdns_name>._shadow-brain._tcp` (host `<mdns_name>.local`); empty disables.
    #[serde(default = "default_mdns_name")]
    pub mdns_name: String,
}

fn default_mdns_name() -> String {
    crate::services::mdns_advert::DEFAULT_NAME.to_string()
}

pub(crate) fn default_warming_timeout_sec() -> u64 {
    120
}

fn default_health_stale_sec() -> u64 {
    300
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ArenaConfig {
    // Timing configuration
    /// WARMING -> IDLE after this many seconds without a shout (0 = never).
    #[serde(default = "default_warming_timeout_sec")]
    pub warming_timeout_sec: u64,
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
    /// BlueZ adapter to use: `hci1`, a numeric index (`1`), a name substring, or a
    /// hardware selector that survives `hciN` renumbering across boots: `usb`,
    /// `usb:VVVV:PPPP` (e.g. `usb:2357:0604`) or `builtin`. Unset = first adapter.
    #[serde(default)]
    pub adapter: Option<String>,

    // Punching bag parameters
    pub alpha: f32,
    pub threshold: f32,
    /// After a punch, further above-threshold samples within this window are the
    /// same hit (the bag keeps swinging) and are not counted again.
    #[serde(default = "default_punch_refractory_ms")]
    pub punch_refractory_ms: u64,
}

fn default_punch_refractory_ms() -> u64 {
    350
}

/// Read-only BLE GATT status service (BlueZ peripheral role, Linux only).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct BleStatusConfig {
    pub enabled: bool,
    /// Advertised name is `<name_prefix>-XXXX` (XXXX = last 2 bytes of the adapter address).
    pub name_prefix: String,
    /// BlueZ adapter for the peripheral role (`hci1`, index, name substring,
    /// `usb`, `usb:VVVV:PPPP` or `builtin`).
    /// Unset = same selection as `ble.adapter`.
    pub adapter: Option<String>,
    /// How often the brain state is sampled for change detection / notify.
    pub poll_ms: u64,
    /// Require an encrypted (paired/bonded) link to read characteristics. `false` is a
    /// debugging opt-out that lets any phone in range read the status; a warning is logged.
    pub require_encryption: bool,
    /// Also publish occupancy/activity data (presence, uptime, energy, fight stats).
    /// Off: only coarse state, schedule flag and per-device connected/stale.
    pub expose_activity: bool,
}

impl Default for BleStatusConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            name_prefix: "ShadowWarrior".into(),
            adapter: None,
            poll_ms: 1000,
            require_encryption: true,
            expose_activity: false,
        }
    }
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
    /// Shout meter (`ShoutLevel` events, services/shout_meter.rs): the meter reads 0 at
    /// `floor + shout_meter_lo_db` and 1 at `floor + shout_meter_hi_db` above the adaptive
    /// noise floor.
    #[serde(default = "d_meter_lo")]
    pub shout_meter_lo_db: f32,
    #[serde(default = "d_meter_hi")]
    pub shout_meter_hi_db: f32,
    /// Dim the meter for frames that do not look like a shout (voiced / centroid / tilt /
    /// flatness). `false`: level only.
    #[serde(default = "d_true")]
    pub shout_meter_gate: bool,
    /// Meter release time constant, milliseconds (attack is immediate).
    #[serde(default = "d_meter_release")]
    pub shout_meter_release_ms: f32,
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
    /// Capture the default microphone. `false` (env `BRAIN_AUDIO='{input=false}'`): no
    /// capture at all, e.g. on a dev machine whose default source is a headset; shouts then
    /// come only from `POST /api/arena/shout`.
    #[serde(default = "d_true")]
    pub input: bool,
    /// Refuse a sink-monitor / loopback capture (the brain would hear its own playback):
    /// audio health becomes an error ("no microphone: only null/monitor devices present") and shout detection stays off.
    #[serde(default = "d_true")]
    pub reject_monitor: bool,
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
fn d_score_threshold() -> f32 { 12.0 }
fn d_score_decay() -> f32 { 0.9 }
fn d_gap_frames() -> u32 { 3 }
fn d_meter_lo() -> f32 { 6.0 }
fn d_meter_hi() -> f32 { 35.0 }
fn d_meter_release() -> f32 { 200.0 }

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
    /// Number of columns in the arena (4-5). Default: highest configured column + 1.
    #[serde(default)]
    pub columns: Option<usize>,
    pub udp_fps: u32,
    pub effects: WledEffects,
}

/// One WLED controller. Without `strips` it drives one strip of `num_leds` that is column
/// number "this controller's index". With `strips` (a controller hosting several columns)
/// the strips are laid out consecutively in the controller's LED order and `num_leds` may be
/// omitted (it is then the sum).
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct WledController {
    pub ip: String,
    #[serde(default)]
    pub num_leds: usize,
    #[serde(default)]
    pub strips: Vec<StripConfig>,
    /// Build the strips from the controller's LED outputs (`GET /json/cfg`, once per
    /// connect): one strip per output, the first on this controller's column, further
    /// ones on the next free columns. `num_leds` may then be omitted (180 until probed).
    /// Set for controllers found by discovery; opt in per controller in config.
    #[serde(default)]
    pub auto_layout: bool,
}

/// LEDs on a typical column: one 3 m strip at 60 LEDs/m. Used when nothing else is known
/// (a controller found by discovery, before it reports its own count).
pub const DEFAULT_STRIP_LEDS: usize = 180;

/// One column's strip on a controller.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct StripConfig {
    /// Column number, 0-based, left to right.
    pub column: usize,
    pub leds: usize,
    /// The strip is wired top-down (serpentine): LED 0 is at the top of the column.
    #[serde(default)]
    pub reverse: bool,
}

impl WledController {
    /// The strips this controller drives, in LED order. `index` is the controller's position
    /// in the list, the column of the implicit single strip.
    pub fn strip_specs(&self, index: usize) -> Vec<StripConfig> {
        if self.strips.is_empty() {
            vec![StripConfig { column: index, leds: self.num_leds, reverse: false }]
        } else {
            self.strips.clone()
        }
    }

    /// Make the implicit column explicit: a controller without `strips` gets one strip on
    /// column `index`. After this its column no longer depends on its list position, so
    /// removing an earlier controller cannot renumber it.
    pub fn pin_column(&mut self, index: usize) {
        if self.strips.is_empty() {
            self.strips = self.strip_specs(index);
        }
    }
}

/// Pin the implicit columns of every controller (see [`WledController::pin_column`]).
pub fn pin_columns(controllers: &mut [WledController]) {
    for (i, c) in controllers.iter_mut().enumerate() {
        c.pin_column(i);
    }
}

impl WledConfig {
    /// Fill in `num_leds` of multi-strip controllers and reject layouts that cannot work.
    pub fn normalize(&mut self) -> Result<(), String> {
        let e = &self.effects;
        for (state, fx) in [("idle", &e.idle), ("warming", &e.warming), ("fight", &e.fight), ("cooldown", &e.cooldown)] {
            for seg in &fx.segments {
                if let Some(b) = seg.blend.as_deref().filter(|b| blend_mode_id(b).is_none()) {
                    return Err(format!("wled.effects.{state}: unknown blend mode '{b}' (one of {})", BLEND_MODES.join(", ")));
                }
            }
        }
        let mut seen = std::collections::BTreeSet::new();
        for (i, c) in self.controllers.iter_mut().enumerate() {
            if !c.strips.is_empty() {
                if let Some(s) = c.strips.iter().find(|s| s.leds == 0) {
                    return Err(format!("wled controller {}: strip of column {} has 0 leds", c.ip, s.column));
                }
                let sum: usize = c.strips.iter().map(|s| s.leds).sum();
                if c.num_leds == 0 {
                    c.num_leds = sum;
                } else if c.num_leds != sum {
                    return Err(format!(
                        "wled controller {}: num_leds {} != sum of its strips ({sum})",
                        c.ip, c.num_leds
                    ));
                }
            } else if c.num_leds == 0 && c.auto_layout {
                c.num_leds = DEFAULT_STRIP_LEDS;
            } else if c.num_leds == 0 {
                return Err(format!("wled controller {}: num_leds (or strips) is required", c.ip));
            }
            for s in c.strip_specs(i) {
                if !seen.insert(s.column) {
                    return Err(format!("wled: column {} is driven by more than one strip", s.column));
                }
            }
        }
        if let (Some(n), Some(&max)) = (self.columns, seen.iter().next_back()) {
            if max >= n {
                return Err(format!("wled: column {max} but columns is {n}"));
            }
        }
        // Resolve list-index columns once, here, so later removals cannot shift them.
        pin_columns(&mut self.controllers);
        Ok(())
    }
}

/// Lowest column no strip of `controllers` drives, or `None` when `configured` columns are all taken.
/// Without `configured` the arena can grow, so there is always a free column.
pub fn next_free_column(controllers: &[WledController], configured: Option<usize>) -> Option<usize> {
    let used: std::collections::BTreeSet<usize> = controllers
        .iter()
        .enumerate()
        .flat_map(|(i, c)| c.strip_specs(i))
        .map(|s| s.column)
        .collect();
    let free = (0..).find(|c| !used.contains(c))?;
    match configured {
        Some(n) if free >= n => None,
        _ => Some(free),
    }
}

/// Columns in the arena for `controllers`: `configured` or the highest column + 1.
pub fn columns_total(controllers: &[WledController], configured: Option<usize>) -> usize {
    let used = controllers
        .iter()
        .enumerate()
        .flat_map(|(i, c)| c.strip_specs(i))
        .map(|s| s.column + 1)
        .max()
        .unwrap_or(0);
    used.max(configured.unwrap_or(0))
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
    /// Overlaid segments (WLED 16+): when non-empty the effect is these layers, bottom first,
    /// all over the same LED range (each strip of a controller gets its own copy). The fields
    /// above except `brightness` (global) and `transition` are then ignored. See docs/LEDS.md.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub segments: Vec<WledSegment>,
    /// Single-segment look used when the controller runs WLED older than 16 (no blend modes).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback: Option<Box<WledEffect>>,
}

/// One overlaid WLED segment layer.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct WledSegment {
    /// Effect name or numeric id (like `WledEffect::effect`).
    pub effect: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub palette: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intensity: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub colors: Option<Vec<[u8; 3]>>,
    /// Segment brightness (WLED seg `bri`)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub brightness: Option<u8>,
    /// Blend mode against the layers below: a name (see [`blend_mode_id`]) or a number.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blend: Option<String>,
}

/// WLED 16 segment blend modes (`bm`), in firmware order (FX.h `blendMode`; FX_fcn.cpp
/// `case 6` is multiply, `case 16` stencil).
pub const BLEND_MODES: [&str; 17] = [
    "normal", "bottom", "add", "subtract", "difference", "average", "multiply", "divide",
    "lighten", "darken", "screen", "overlay", "hardlight", "softlight", "dodge", "burn", "stencil",
];

/// `bm` for a blend mode name (case-insensitive; "top" = "normal") or a number 0..=16.
pub fn blend_mode_id(name: &str) -> Option<u8> {
    let n = name.trim().to_lowercase();
    if let Ok(i) = n.parse::<u8>() {
        return (usize::from(i) < BLEND_MODES.len()).then_some(i);
    }
    let n = if n == "top" { "normal" } else { n.as_str() }.to_string();
    BLEND_MODES.iter().position(|m| *m == n).map(|i| i as u8)
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
    /// Radar: evidence units per `StationRadar` event while the station reports a target
    /// (those events come at most every 500 ms). The thresholds live on the station's own
    /// presence bool; the energies are not used.
    pub radar_units: f32,
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
            radar_units: 0.5,
            source_weights: [
                ("mic".to_string(), 1.0),
                ("punch".to_string(), 1.0),
                ("radar".to_string(), 1.0),
            ]
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
        Self::from_sources(Figment::from(Yaml::file("config.yaml")).merge(Env::prefixed("BRAIN_")))
    }

    /// Like `load`, from `path` (a missing file means defaults), then `overrides`: dotted keys
    /// (`audio.shout_margin_db`) with YAML scalar values, applied last. Used by `eval-wav`.
    pub fn load_with(
        path: &std::path::Path,
        overrides: &[(String, serde_yaml::Value)],
    ) -> Result<Self, Box<figment::Error>> {
        let mut user = Figment::from(Yaml::file(path)).merge(Env::prefixed("BRAIN_"));
        for (k, v) in overrides {
            user = user.merge(Serialized::default(k, v));
        }
        Self::from_sources(user)
    }

    /// Defaults overlaid with `user` (YAML + env). A WLED effect the user configures replaces
    /// the default effect as a whole: merging key by key leaked defaults the user left out
    /// (e.g. the default idle `palette: ocean` under an idle effect with no palette).
    fn from_sources(user: Figment) -> Result<Self, Box<figment::Error>> {
        let mut cfg: Config = Figment::new()
            .merge(Serialized::defaults(Self::default()))
            .merge(user.clone())
            .extract()
            .map_err(Box::new)?;
        for (key, slot) in [
            ("idle", &mut cfg.wled.effects.idle),
            ("warming", &mut cfg.wled.effects.warming),
            ("fight", &mut cfg.wled.effects.fight),
            ("cooldown", &mut cfg.wled.effects.cooldown),
        ] {
            let path = format!("wled.effects.{key}");
            if user.find_value(&path).is_ok() {
                *slot = user.extract_inner(&path).map_err(Box::new)?;
            }
        }
        cfg.wled
            .normalize()
            .map_err(|e| Box::new(figment::Error::from(e)))?;
        Ok(cfg)
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
                mdns_name: default_mdns_name(),
            },
            arena: ArenaConfig {
                warming_timeout_sec: default_warming_timeout_sec(),
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
                punch_refractory_ms: default_punch_refractory_ms(),
            },
            audio: AudioConfig {
                sample_rate: 44100,
                channels: 1,
                buffer_size: 1024,
                shout_min_duration_sec: 0.12,
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
                shout_meter_lo_db: d_meter_lo(),
                shout_meter_hi_db: d_meter_hi(),
                shout_meter_gate: d_true(),
                shout_meter_release_ms: d_meter_release(),
                clip_dir: default_clip_dir(),
                clip_seconds: default_clip_seconds(),
                clip_on_shout: default_clip_on_shout(),
                clip_cooldown_sec: default_clip_cooldown_sec(),
                clip_raw: d_true(),
                input: d_true(),
                reject_monitor: d_true(),
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
                columns: None,
                udp_fps: 30,
                effects: WledEffects {
                    // Same looks as config.yaml (see docs/LEDS.md): idle and cooldown are
                    // built-in WLED effects, warming and fight are UDP realtime.
                    // Idle on WLED 16+: a dim ember Solid under a white Fade (slowest, ~6 s)
                    // blended by multiply. `fallback` is the single-segment Tri Fade for 0.x.
                    idle: WledEffect {
                        effect: "Solid".to_string(),
                        segments: vec![
                            WledSegment {
                                effect: "Solid".to_string(),
                                colors: Some(vec![[255, 25, 0], [0, 0, 0], [0, 0, 0]]),
                                brightness: Some(90),
                                blend: Some("normal".to_string()),
                                ..Default::default()
                            },
                            WledSegment {
                                effect: "Fade".to_string(),
                                speed: Some(0),
                                intensity: Some(128),
                                colors: Some(vec![[255, 255, 255], [0, 0, 0], [0, 0, 0]]),
                                brightness: Some(255),
                                blend: Some("multiply".to_string()),
                                ..Default::default()
                            },
                        ],
                        fallback: Some(Box::new(WledEffect {
                            effect: "Tri Fade".to_string(),
                            speed: Some(96), // cycle 5.04 s
                            brightness: Some(44), // keeps the peak under the 1000 mA ABL limit
                            colors: Some(vec![[255, 25, 0], [0, 0, 0], [0, 0, 0]]),
                            ..Default::default()
                        })),
                        ..Default::default()
                    },
                    warming: WledEffect {
                        effect: "realtime".to_string(),
                        palette: None,
                        speed: None,
                        ..Default::default()
                    },
                    fight: WledEffect {
                        effect: "realtime".to_string(),
                        palette: None,
                        speed: None,
                        ..Default::default()
                    },
                    cooldown: WledEffect {
                        effect: "Fire 2012".to_string(),
                        palette: Some("Lava".to_string()),
                        speed: Some(150),
                        ..Default::default()
                    },
                },
            },
            tasmota: TasmotaConfig { plugs: vec![] },
            schedule: ScheduleConfig::default(),
            presence: PresenceConfig::default(),
            ble_status: BleStatusConfig::default(),
            speaker: SpeakerConfig::default(),
            sound: SoundConfig::default(),
            station: StationConfig::default(),
            led: LedConfig::default(),
        }
    }
}

#[cfg(test)]
mod load_tests {
    use super::*;

    const REAL_YAML: &str = include_str!("../config.yaml");

    #[test]
    fn configured_effect_replaces_default_not_merges() {
        let cfg = Config::from_sources(Figment::from(Yaml::string(REAL_YAML))).unwrap();
        let idle = &cfg.wled.effects.idle;
        assert_eq!(idle.segments.len(), 2);
        assert_eq!(idle.fallback.as_ref().unwrap().effect, "Tri Fade");
        assert_eq!(idle.palette, None, "default ocean palette must not leak");
        // config.yaml and the built-in defaults describe the same idle look
        assert_eq!(*idle, Config::default().wled.effects.idle);
        // an effect absent from the YAML keeps its default
        let cfg = Config::from_sources(Figment::from(Yaml::string("wled:\n  effects:\n    idle:\n      effect: Fade\n"))).unwrap();
        assert_eq!(cfg.wled.effects.cooldown, Config::default().wled.effects.cooldown);
        assert_eq!(cfg.wled.effects.idle.palette, None);
        // a configured idle replaces the default overlay and fallback as a whole
        assert!(cfg.wled.effects.idle.segments.is_empty());
        assert!(cfg.wled.effects.idle.fallback.is_none());
    }

    #[test]
    fn blend_mode_names_and_validation() {
        assert_eq!(blend_mode_id("multiply"), Some(6));
        assert_eq!(blend_mode_id("Multiply"), Some(6));
        assert_eq!(blend_mode_id("normal"), Some(0));
        assert_eq!(blend_mode_id("top"), Some(0));
        assert_eq!(blend_mode_id("16"), Some(16));
        assert_eq!(blend_mode_id("17"), None);
        assert_eq!(blend_mode_id("nope"), None);
        let bad = "wled:\n  effects:\n    idle:\n      effect: Solid\n      segments:\n        - {effect: Fade, blend: nope}\n";
        assert!(load(bad).is_err());
    }

    fn load(yaml: &str) -> Result<Config, Box<figment::Error>> {
        Config::from_sources(Figment::from(Yaml::string(yaml)))
    }

    #[test]
    fn legacy_controller_is_one_strip_in_its_own_column() {
        let cfg = load("wled:\n  controllers:\n    - {ip: a, num_leds: 180}\n    - {ip: b, num_leds: 90}\n").unwrap();
        let strip = StripConfig { column: 1, leds: 90, reverse: false };
        assert_eq!(cfg.wled.controllers[1].strip_specs(1), vec![strip]);
        assert_eq!(columns_total(&cfg.wled.controllers, cfg.wled.columns), 2);
    }

    #[test]
    fn multi_strip_controller_derives_num_leds_and_columns() {
        let cfg = load("wled:\n  controllers:\n    - ip: a\n      strips: [{column: 0, leds: 180}, {column: 1, leds: 180, reverse: true}]\n    - {ip: b, num_leds: 180}\n");
        // controller b is implicitly column 1, which a already drives
        assert!(cfg.unwrap_err().to_string().contains("column 1"));
        let cfg = load("wled:\n  controllers:\n    - ip: a\n      strips: [{column: 0, leds: 180}, {column: 1, leds: 180, reverse: true}]\n    - ip: b\n      strips: [{column: 2, leds: 180}]\n").unwrap();
        assert_eq!(cfg.wled.controllers[0].num_leds, 360);
        assert!(cfg.wled.controllers[0].strips[1].reverse);
        assert_eq!(columns_total(&cfg.wled.controllers, cfg.wled.columns), 3);
    }

    #[test]
    fn normalize_pins_implicit_columns() {
        let cfg = load("wled:\n  controllers:\n    - {ip: a, num_leds: 90}\n    - {ip: b, num_leds: 180}\n").unwrap();
        let strip = |column, leds| StripConfig { column, leds, reverse: false };
        assert_eq!(cfg.wled.controllers[0].strips, vec![strip(0, 90)]);
        assert_eq!(cfg.wled.controllers[1].strips, vec![strip(1, 180)]);
    }

    #[test]
    fn auto_layout_controller_needs_only_an_ip() {
        let cfg = load("wled:\n  controllers:\n    - {ip: a, auto_layout: true}\n").unwrap();
        assert_eq!(cfg.wled.controllers[0].num_leds, DEFAULT_STRIP_LEDS);
        assert!(cfg.wled.controllers[0].auto_layout);
        assert!(load("wled:\n  controllers:\n    - {ip: a}\n").is_err());
    }

    #[test]
    fn invalid_strip_layouts_are_rejected() {
        let sum = load("wled:\n  controllers:\n    - {ip: a, num_leds: 100, strips: [{column: 0, leds: 60}, {column: 1, leds: 60}]}\n");
        assert!(sum.unwrap_err().to_string().contains("sum of its strips"));
        let dup = load("wled:\n  controllers:\n    - {ip: a, strips: [{column: 0, leds: 60}, {column: 0, leds: 60}]}\n");
        assert!(dup.unwrap_err().to_string().contains("column 0"));
        let beyond = load("wled:\n  columns: 4\n  controllers:\n    - {ip: a, strips: [{column: 4, leds: 60}]}\n");
        assert!(beyond.is_err());
        assert!(load("wled:\n  controllers:\n    - {ip: a}\n").is_err());
        let five = load("wled:\n  columns: 5\n  controllers:\n    - {ip: a, num_leds: 180}\n").unwrap();
        assert_eq!(columns_total(&five.wled.controllers, five.wled.columns), 5);
    }
}
