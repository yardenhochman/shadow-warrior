//! Dashboard health for audio in (microphone or station stream) and audio out (speaker,
//! output sink). Plain data folded from events by `status_registry`; the derived fields
//! (`status`, `route`, `speaker_state`) are filled at read time.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Input band level (dB, `AudioLevelChanged`) at or above which the input counts as
/// "signal present". Quieter is "silent" (a dead or null device reads far below this).
pub const SIGNAL_MIN_DB: f32 = -60.0;
/// A level older than this means no audio is flowing.
pub const LEVEL_STALE_SEC: i64 = 3;
/// Peak hold time.
pub const PEAK_HOLD_SEC: i64 = 3;

/// Which features are configured; lets the dashboard say "disabled in config" instead of
/// showing an error. Set once by `main` via `StatusRegistry::set_features`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct FeatureFlags {
    /// A capture is wanted: `audio.input` or the station stream.
    pub audio_input: bool,
    pub station_enabled: bool,
    pub station_stream_audio: bool,
    pub provision_enabled: bool,
    pub speaker_configured: bool,
}

/// Audio in: which device the brain listens to and how loud it is.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct AudioInHealth {
    /// "microphone" | "station" once capturing.
    pub source: Option<String>,
    /// Device name as cpal reports it.
    pub device: Option<String>,
    /// PipeWire/Pulse node behind an alias device ("default").
    pub pulse_source: Option<String>,
    pub sample_rate: Option<u32>,
    /// Latest band level, dB.
    pub level_db: Option<f32>,
    /// Peak level over the last few seconds, dB.
    pub peak_db: Option<f32>,
    /// Slow ambient band level, dBFS (adaptive sensitivity); None until the estimate is warm.
    pub ambient_db: Option<f32>,
    /// dB currently added to the shout margin and meter lo/hi by adaptive sensitivity.
    pub adaptive_offset_db: Option<f32>,
    pub level_at: Option<DateTime<Utc>>,
    #[serde(skip)]
    pub peak_at: Option<DateTime<Utc>>,
    /// Why there is no microphone, if so.
    pub reason: Option<String>,
    /// Derived: disabled | no_microphone | no_data | silent | signal.
    pub status: String,
}

impl AudioInHealth {
    pub fn level(&mut self, db: f32, now: DateTime<Utc>) {
        self.level_db = Some(db);
        self.level_at = Some(now);
        let hold_over = self.peak_at.is_none_or(|t| (now - t).num_seconds() >= PEAK_HOLD_SEC);
        if hold_over || self.peak_db.is_none_or(|p| db > p) {
            self.peak_db = Some(db);
            self.peak_at = Some(now);
        }
    }

    pub fn derive(&mut self, features: &FeatureFlags, connected: bool, now: DateTime<Utc>) {
        let fresh = self.level_at.is_some_and(|t| (now - t).num_seconds() <= LEVEL_STALE_SEC);
        self.status = if !features.audio_input {
            "disabled"
        } else if !connected {
            "no_microphone"
        } else if !fresh {
            "no_data"
        } else if self.level_db.unwrap_or(f32::MIN) >= SIGNAL_MIN_DB {
            "signal"
        } else {
            "silent"
        }
        .into();
    }
}

/// Audio out: the Bluetooth speaker supervisor and the sink the music goes to.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct AudioOutHealth {
    pub sink: Option<String>,
    pub sink_description: Option<String>,
    pub sink_bluetooth: bool,
    pub volume_pct: Option<u32>,
    pub muted: Option<bool>,
    /// When `pactl` last reported a sink (None: no sink info, e.g. no `pactl`).
    pub checked_at: Option<DateTime<Utc>>,
    /// Consecutive failed speaker reconnects, and the wait until the next try.
    pub speaker_attempts: u32,
    pub speaker_retry_in_sec: Option<u64>,
    /// Derived: not_configured | connected | connecting | failed.
    pub speaker_state: String,
    /// Derived: bluetooth | builtin | unknown (where the default sink sends sound).
    pub route: String,
}

impl AudioOutHealth {
    pub fn derive(&mut self, features: &FeatureFlags, speaker_connected: bool, speaker_has_error: bool) {
        self.speaker_state = if !features.speaker_configured {
            "not_configured"
        } else if speaker_connected {
            "connected"
        } else if self.speaker_attempts > 0 || speaker_has_error {
            "failed"
        } else {
            "connecting"
        }
        .into();
        self.route = if self.sink.is_none() {
            "unknown"
        } else if self.sink_bluetooth {
            "bluetooth"
        } else {
            "builtin"
        }
        .into();
    }
}
