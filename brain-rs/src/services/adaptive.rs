//! Adaptive sensitivity: relax the shout gates in a quiet room, tighten them in a noisy one.
//!
//! The front-end noise floor follows the *recent* minimum, so `db_above_floor` is relative:
//! in a noisy hall the floor rises and a shout is a smaller step above it, in a very quiet
//! room small sounds look huge. The right thresholds depend on the absolute ambient level, which
//! this tracks slowly (minutes) and turns into one number, `offset_db`:
//!
//! ```text
//! ambient   EMA (tau = adaptive_window_sec) of band_db over frames that are NOT transients:
//!           frames with db_above_floor > TRANSIENT_DB, playback frames, and HOLD_SEC after them
//!           are skipped, so shouts, claps and talking never drag the estimate up.
//!           Warm-up: a running mean until one window has been seen.
//! target    gain * soft_deadband(ambient - adaptive_ref_db), clamped to [-max_down, +max_up]
//!           (soft deadband: 0 within +-deadband, then continuous: no step, no breathing)
//! offset    follows target, slew-limited to adaptive_slew_db_per_sec; 0 until WARMUP_SEC of
//!           ambient has been collected, and always 0 when adaptive_enabled is false
//! ```
//!
//! The offset is added to `shout_margin_db` and to `shout_meter_lo_db` / `hi_db` by
//! [`apply`], which returns the effective config the detector and meter then use unchanged.

use super::audio_frontend::{FrameFeatures, FRAME_SECS};
use crate::config::AudioConfig;

/// Frames this far above the adaptive floor are events (speech, shouts, claps), not ambient.
pub const TRANSIENT_DB: f32 = 8.0;
/// Ambient updates stay frozen this long after the last transient (reverb tail, hangover).
pub const HOLD_SEC: f32 = 1.0;
/// Seconds of accepted ambient frames before the offset is allowed to leave 0.
pub const WARMUP_SEC: f32 = 20.0;
/// Frames skipped by a lossy channel are credited, up to this many (as in the detector).
const MAX_CREDITED_FRAMES: u64 = 8;
/// Effective margin / meter lower bound never goes below this, dB.
const MIN_EFFECTIVE_DB: f32 = 3.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AdaptiveState {
    /// Slow ambient band level, dBFS (NaN before the first ambient frame).
    pub ambient_db: f32,
    /// dB added to the shout margin and the meter lo/hi right now.
    pub offset_db: f32,
}

#[derive(Debug, Default)]
pub struct AmbientTracker {
    ambient: Option<f32>,
    /// Seconds of ambient accepted so far (saturates at the window).
    accepted_sec: f32,
    hold_left_sec: f32,
    offset: f32,
    last_index: Option<u64>,
}

/// 0 within +-deadband of the reference, then grows from 0 (continuous).
fn soft_deadband(x: f32, deadband: f32) -> f32 {
    let d = deadband.max(0.0);
    if x > d {
        x - d
    } else if x < -d {
        x + d
    } else {
        0.0
    }
}

impl AmbientTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn state(&self) -> AdaptiveState {
        AdaptiveState { ambient_db: self.ambient.unwrap_or(f32::NAN), offset_db: self.offset }
    }

    /// Target offset for a given ambient level (before slew limiting and warm-up).
    pub fn target_offset(ambient_db: f32, cfg: &AudioConfig) -> f32 {
        let dev = soft_deadband(ambient_db - cfg.adaptive_ref_db, cfg.adaptive_deadband_db);
        (cfg.adaptive_gain * dev).clamp(-cfg.adaptive_max_down_db.max(0.0), cfg.adaptive_max_up_db.max(0.0))
    }

    /// Feed one frame, get the state to use for it.
    pub fn push(&mut self, f: &FrameFeatures, cfg: &AudioConfig) -> AdaptiveState {
        let frames = match self.last_index {
            Some(last) if f.index > last => (f.index - last).clamp(1, MAX_CREDITED_FRAMES),
            _ => 1,
        };
        self.last_index = Some(f.index);
        let dt = frames as f32 * FRAME_SECS;

        let transient = f.db_above_floor > TRANSIENT_DB || f.playback_active || !f.band_db.is_finite();
        if transient {
            self.hold_left_sec = HOLD_SEC;
        } else if self.hold_left_sec > 0.0 {
            self.hold_left_sec -= dt;
        } else {
            let window = cfg.adaptive_window_sec.max(1.0);
            self.accepted_sec = (self.accepted_sec + dt).min(window);
            match self.ambient {
                None => self.ambient = Some(f.band_db),
                Some(a) => {
                    // running mean while young (accepted_sec < window), EMA after
                    let k = (dt / self.accepted_sec).max(1.0 - (-dt / window).exp()).min(1.0);
                    self.ambient = Some(a + (f.band_db - a) * k);
                }
            }
        }

        let warm = self.accepted_sec >= WARMUP_SEC.min(cfg.adaptive_window_sec);
        let want = match self.ambient {
            Some(a) if cfg.adaptive_enabled && warm => Self::target_offset(a, cfg),
            _ => 0.0,
        };
        let step = cfg.adaptive_slew_db_per_sec.max(0.0) * dt;
        self.offset += (want - self.offset).clamp(-step, step);
        self.state()
    }
}

/// `cfg` with the adaptive offset applied to the margin and the meter range.
pub fn apply(cfg: &AudioConfig, offset_db: f32) -> AudioConfig {
    let mut c = cfg.clone();
    if offset_db != 0.0 {
        c.shout_margin_db = (cfg.shout_margin_db + offset_db).max(MIN_EFFECTIVE_DB);
        c.shout_meter_lo_db = (cfg.shout_meter_lo_db + offset_db).max(MIN_EFFECTIVE_DB);
        c.shout_meter_hi_db = (cfg.shout_meter_hi_db + offset_db).max(c.shout_meter_lo_db + 1.0);
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn cfg() -> AudioConfig {
        Config::default().audio
    }

    fn frame(i: u64, band: f32, above: f32) -> FrameFeatures {
        FrameFeatures {
            index: i,
            band_db: band,
            noise_floor_db: band - above,
            db_above_floor: above,
            vad_prob: 0.1,
            pitch_hz: 0.0,
            pitch_conf: 0.0,
            spectral_centroid_hz: 600.0,
            spectral_tilt_db_per_khz: -30.0,
            spectral_flatness: 0.7,
            playback_active: false,
        }
    }

    /// `secs` of steady ambient at `band` dB starting at frame `start`; returns the next frame
    /// index and the last state.
    fn run(t: &mut AmbientTracker, c: &AudioConfig, start: u64, secs: f32, band: f32) -> (u64, AdaptiveState) {
        let n = (secs / FRAME_SECS) as u64;
        let mut s = t.state();
        for i in start..start + n {
            s = t.push(&frame(i, band, 1.0), c);
        }
        (start + n, s)
    }

    #[test]
    fn at_reference_offset_is_zero() {
        let (c, mut t) = (cfg(), AmbientTracker::new());
        let (_, s) = run(&mut t, &c, 0, 300.0, c.adaptive_ref_db + 2.0);
        assert_eq!(s.offset_db, 0.0, "inside the deadband");
    }

    #[test]
    fn quiet_room_relaxes_within_limit() {
        let (c, mut t) = (cfg(), AmbientTracker::new());
        let (_, s) = run(&mut t, &c, 0, 300.0, c.adaptive_ref_db - 40.0);
        assert!(s.offset_db < -5.0, "{s:?}");
        assert!((s.offset_db + c.adaptive_max_down_db).abs() < 1e-3, "40 dB quiet saturates: {s:?}");
    }

    #[test]
    fn noisy_room_tightens_within_limit() {
        let (c, mut t) = (cfg(), AmbientTracker::new());
        let (_, s) = run(&mut t, &c, 0, 300.0, c.adaptive_ref_db + 10.0);
        // gain 0.5 * (10 - deadband 4) = 3 dB
        assert!((s.offset_db - 3.0).abs() < 0.1, "{s:?}");
        let (_, s) = run(&mut AmbientTracker::new(), &c, 0, 300.0, c.adaptive_ref_db + 60.0);
        assert!((s.offset_db - c.adaptive_max_up_db).abs() < 1e-3, "{s:?}");
    }

    #[test]
    fn shouts_do_not_drag_the_estimate_up() {
        let (c, mut t) = (cfg(), AmbientTracker::new());
        let (mut i, _) = run(&mut t, &c, 0, 60.0, -88.0);
        let before = t.state().ambient_db;
        // 16 shouts: 1.5 s at 68 dB above the floor, 0.64 s gaps (shorter than the hold)
        for _ in 0..16 {
            for _ in 0..94 {
                t.push(&frame(i, -20.0, 68.0), &c);
                i += 1;
            }
            for _ in 0..40 {
                t.push(&frame(i, -88.0, 0.0), &c);
                i += 1;
            }
        }
        assert!((t.state().ambient_db - before).abs() < 0.5, "{} -> {}", before, t.state().ambient_db);
    }

    #[test]
    fn sustained_noise_near_the_floor_is_ambient() {
        // a noisy hall raises the floor with it: frames stay near the floor and count as ambient
        let (c, mut t) = (cfg(), AmbientTracker::new());
        let (i, _) = run(&mut t, &c, 0, 120.0, -88.0);
        let (_, s) = run(&mut t, &c, i, 600.0, -60.0);
        assert!(s.ambient_db > -65.0 && s.offset_db > 4.0, "{s:?}");
    }

    #[test]
    fn offset_slew_is_limited() {
        let (c, mut t) = (cfg(), AmbientTracker::new());
        let (i, _) = run(&mut t, &c, 0, 120.0, c.adaptive_ref_db);
        let mut c2 = c.clone();
        c2.adaptive_window_sec = 5.0; // fast ambient so that the slew limit is what binds
        let (mut prev, mut worst) = (t.state().offset_db, 0.0f32);
        for k in 0..62 * 60 {
            let s = t.push(&frame(i + k, c.adaptive_ref_db + 40.0, 0.0), &c2);
            worst = worst.max((s.offset_db - prev).abs());
            prev = s.offset_db;
        }
        assert!(worst <= c.adaptive_slew_db_per_sec * FRAME_SECS + 1e-4, "{worst}");
        assert!(prev > 10.0, "it did get there: {prev}");
    }

    #[test]
    fn no_offset_before_warmup() {
        let (c, mut t) = (cfg(), AmbientTracker::new());
        let (_, s) = run(&mut t, &c, 0, 10.0, c.adaptive_ref_db - 40.0);
        assert_eq!(s.offset_db, 0.0);
    }

    #[test]
    fn disabled_is_identity() {
        let mut c = cfg();
        c.adaptive_enabled = false;
        let mut t = AmbientTracker::new();
        let (_, s) = run(&mut t, &c, 0, 300.0, c.adaptive_ref_db - 40.0);
        assert_eq!(s.offset_db, 0.0);
        let e = apply(&c, s.offset_db);
        assert_eq!(e.shout_margin_db, c.shout_margin_db);
        assert_eq!(e.shout_meter_lo_db, c.shout_meter_lo_db);
        assert_eq!(e.shout_meter_hi_db, c.shout_meter_hi_db);
    }

    #[test]
    fn apply_shifts_margin_and_meter() {
        let c = cfg();
        let e = apply(&c, -8.0);
        assert_eq!(e.shout_margin_db, c.shout_margin_db - 8.0);
        assert_eq!(e.shout_meter_lo_db, c.shout_meter_lo_db - 8.0);
        assert_eq!(e.shout_meter_hi_db, c.shout_meter_hi_db - 8.0);
        assert!(apply(&c, -100.0).shout_margin_db >= MIN_EFFECTIVE_DB);
    }
}
