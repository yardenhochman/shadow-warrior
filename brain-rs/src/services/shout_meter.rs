//! Shout meter: a continuous, low-latency loudness signal for the LED shout meter.
//!
//! `ShoutDetected` fires ~300-500 ms after onset (it needs a sustained, scored shout), far
//! too late to drive a live meter. This runs on the same per-frame (16 ms) front-end
//! features as the detector, without touching it:
//!
//! ```text
//! raw   = clamp((db_above_floor - lo) / (hi - lo), 0, 1)     lo/hi = shout_meter_{lo,hi}_db
//!                                                            (+ playback margin while our speakers play)
//! gate  = 0.25 + 0.75 * passed/4     passed = voiced, centroid, tilt, flatness checks
//!                                    (shout_meter_gate: false -> 1)
//! level = attack: jump to raw*gate, release: exponential, tau = shout_meter_release_ms
//! ```
//!
//! The gate keeps talking, music and claps from lighting the meter fully: a loud frame that
//! fails every check reads at 25 %, one that passes all four at 100 %. The result is published
//! as `Event::ShoutLevel` at most every 2nd frame (~31 Hz) and a final 0 once it has decayed.
//! Both audio sources (local mic and the station stream) feed the same front-end, so both work.

use super::audio_frontend::{FrameFeatures, FRAME_SECS};
use crate::config::AudioConfig;

/// Never publish more often than one event per this many front-end frames (2 = ~31 Hz).
pub const PUBLISH_EVERY_FRAMES: u64 = 2;
/// Gate value of a frame that passes none of the spectral checks.
const GATE_FLOOR: f32 = 0.25;
/// Levels below this count as silent (so the decay ends in an exact 0).
const SILENT: f32 = 0.005;
/// Frames skipped by a lossy channel are credited, up to this many (as in the detector).
const MAX_CREDITED_FRAMES: u64 = 8;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShoutLevel {
    pub level: f32,
    pub db_above_floor: f32,
}

#[derive(Debug, Default)]
pub struct ShoutMeter {
    level: f32,
    last_index: Option<u64>,
    last_publish_index: Option<u64>,
    last_published: f32,
}

impl ShoutMeter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Current smoothed level, 0..1.
    pub fn level(&self) -> f32 {
        self.level
    }

    /// Gate in `0.25..=1` from the detector's per-frame feature checks.
    pub fn gate(f: &FrameFeatures, cfg: &AudioConfig) -> f32 {
        if !cfg.shout_meter_gate {
            return 1.0;
        }
        let voiced = f.pitch_conf >= cfg.shout_pitch_conf_min
            && (cfg.shout_f0_min_hz..=cfg.shout_f0_max_hz).contains(&f.pitch_hz);
        let passed = [
            voiced,
            f.spectral_centroid_hz >= cfg.shout_centroid_min_hz,
            f.spectral_tilt_db_per_khz >= cfg.shout_tilt_min_db_per_khz,
            f.spectral_flatness <= cfg.shout_flatness_max,
        ]
        .iter()
        .filter(|&&b| b)
        .count() as f32;
        GATE_FLOOR + (1.0 - GATE_FLOOR) * passed / 4.0
    }

    /// Instantaneous (unsmoothed) meter reading of one frame.
    pub fn raw(f: &FrameFeatures, cfg: &AudioConfig) -> f32 {
        let shift = if f.playback_active { cfg.shout_playback_margin_db } else { 0.0 };
        let lo = cfg.shout_meter_lo_db + shift;
        let span = (cfg.shout_meter_hi_db - cfg.shout_meter_lo_db).max(1.0);
        let x = ((f.db_above_floor - lo) / span).clamp(0.0, 1.0);
        if x.is_finite() {
            x * Self::gate(f, cfg)
        } else {
            0.0
        }
    }

    /// Feed one frame; `Some` when an event is due.
    pub fn push(&mut self, f: &FrameFeatures, cfg: &AudioConfig) -> Option<ShoutLevel> {
        let frames = match self.last_index {
            Some(last) if f.index > last => (f.index - last).clamp(1, MAX_CREDITED_FRAMES),
            _ => 1,
        };
        self.last_index = Some(f.index);
        let raw = Self::raw(f, cfg);
        if raw >= self.level {
            let att = cfg.shout_meter_attack_ms / 1000.0;
            self.level = if att <= FRAME_SECS * 0.5 {
                raw
            } else {
                let k = 1.0 - (-(frames as f32) * FRAME_SECS / att).exp();
                self.level + (raw - self.level) * k
            };
        } else {
            let tau = (cfg.shout_meter_release_ms / 1000.0).max(FRAME_SECS);
            let k = (-(frames as f32) * FRAME_SECS / tau).exp();
            self.level = (self.level * k).max(raw);
        }
        if self.level < SILENT {
            self.level = 0.0;
        }
        let due = self
            .last_publish_index
            .is_none_or(|p| f.index.saturating_sub(p) >= PUBLISH_EVERY_FRAMES);
        let active = self.level > 0.0 || self.last_published > 0.0;
        if !(due && active) {
            return None;
        }
        self.last_publish_index = Some(f.index);
        self.last_published = self.level;
        Some(ShoutLevel { level: self.level, db_above_floor: f.db_above_floor })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    /// The math tests use round numbers: 6..35 dB, gate on, immediate attack (not the shipped defaults).
    fn cfg() -> AudioConfig {
        let mut a = Config::default().audio;
        a.shout_meter_lo_db = 6.0;
        a.shout_meter_hi_db = 35.0;
        a.shout_meter_gate = true;
        a.shout_meter_attack_ms = 0.0;
        a
    }

    fn quiet(i: u64) -> FrameFeatures {
        FrameFeatures {
            index: i,
            band_db: -50.0,
            noise_floor_db: -50.0,
            db_above_floor: 0.0,
            vad_prob: 0.1,
            pitch_hz: 0.0,
            pitch_conf: 0.05,
            spectral_centroid_hz: 600.0,
            spectral_tilt_db_per_khz: -30.0,
            spectral_flatness: 0.7,
            playback_active: false,
        }
    }

    /// A frame that passes all four shout checks, `above` dB over the floor.
    fn shout(i: u64, above: f32) -> FrameFeatures {
        FrameFeatures {
            band_db: -50.0 + above,
            db_above_floor: above,
            vad_prob: 0.9,
            pitch_hz: 380.0,
            pitch_conf: 0.8,
            spectral_centroid_hz: 1800.0,
            spectral_tilt_db_per_khz: -8.0,
            spectral_flatness: 0.15,
            ..quiet(i)
        }
    }

    #[test]
    fn step_input_rises_within_two_frames() {
        let (c, mut m) = (cfg(), ShoutMeter::new());
        for i in 0..10 {
            m.push(&quiet(i), &c);
        }
        assert_eq!(m.level(), 0.0);
        m.push(&shout(10, 35.0), &c);
        assert!(m.level() > 0.95, "immediate attack: {}", m.level());
        m.push(&shout(11, 35.0), &c);
        assert_eq!(m.level(), 1.0);
    }

    #[test]
    fn maps_db_above_floor_between_lo_and_hi() {
        let c = cfg(); // 6..35
        assert_eq!(ShoutMeter::raw(&shout(0, 6.0), &c), 0.0);
        assert!((ShoutMeter::raw(&shout(0, 20.5), &c) - 0.5).abs() < 1e-5);
        assert_eq!(ShoutMeter::raw(&shout(0, 60.0), &c), 1.0);
        let mut f = shout(0, 20.5);
        f.playback_active = true; // our own speakers raise the bottom by the playback margin
        assert!(ShoutMeter::raw(&f, &c) < 0.3);
    }

    #[test]
    fn release_is_about_the_configured_time_constant() {
        let (c, mut m) = (cfg(), ShoutMeter::new());
        m.push(&shout(0, 35.0), &c);
        // 200 ms = 12.5 frames: ~1/e
        for i in 1..=12 {
            m.push(&quiet(i), &c);
        }
        let l = m.level();
        assert!((0.33..0.42).contains(&l), "{l}");
        for i in 13..120 {
            m.push(&quiet(i), &c);
        }
        assert_eq!(m.level(), 0.0, "decays to exact zero");
    }

    #[test]
    fn gate_dims_a_flat_noise_frame_and_can_be_switched_off() {
        let mut c = cfg();
        let mut noise = shout(0, 35.0);
        noise.pitch_conf = 0.05;
        noise.spectral_flatness = 0.9;
        noise.spectral_centroid_hz = 300.0;
        noise.spectral_tilt_db_per_khz = -40.0;
        let dim = ShoutMeter::raw(&noise, &c);
        assert!((dim - 0.25).abs() < 1e-6, "all checks failed -> floor, not zero: {dim}");
        noise.spectral_centroid_hz = 1500.0; // one check passes
        assert!(ShoutMeter::raw(&noise, &c) > dim);
        assert_eq!(ShoutMeter::raw(&shout(0, 35.0), &c), 1.0);
        c.shout_meter_gate = false;
        assert_eq!(ShoutMeter::raw(&noise, &c), 1.0);
    }

    #[test]
    fn publishes_at_most_every_second_frame_and_ends_with_a_zero() {
        let (c, mut m) = (cfg(), ShoutMeter::new());
        let mut events = Vec::new();
        for i in 0..150u64 {
            let f = if i < 50 { shout(i, 30.0) } else { quiet(i) };
            if let Some(e) = m.push(&f, &c) {
                events.push((i, e.level));
            }
        }
        for w in events.windows(2) {
            assert!(w[1].0 - w[0].0 >= PUBLISH_EVERY_FRAMES, "{events:?}");
        }
        assert!(events.len() <= 75, "{}", events.len());
        assert_eq!(events.last().unwrap().1, 0.0, "a final zero is published");
        let n = events.len();
        for i in 150..250 {
            assert!(m.push(&quiet(i), &c).is_none(), "silence publishes nothing");
        }
        assert_eq!(n, events.len());
    }

    #[test]
    fn skipped_frames_decay_by_real_time() {
        let (c, mut a, mut b) = (cfg(), ShoutMeter::new(), ShoutMeter::new());
        a.push(&shout(0, 35.0), &c);
        b.push(&shout(0, 35.0), &c);
        a.push(&quiet(6), &c); // 6 frames lost
        for i in 1..=6 {
            b.push(&quiet(i), &c);
        }
        assert!((a.level() - b.level()).abs() < 1e-4, "{} {}", a.level(), b.level());
    }

    #[test]
    fn attack_time_makes_a_click_small_and_a_sustained_shout_full() {
        let mut c = Config::default().audio; // shipped defaults: lo 35, hi 60, attack 50 ms
        c.shout_meter_gate = false;
        let loud = |i: u64| FrameFeatures { db_above_floor: 70.0, ..quiet(i) };
        // a 2-frame click (~32 ms)
        let mut m = ShoutMeter::new();
        let mut peak = 0.0f32;
        for i in 0..2 {
            m.push(&loud(i), &c);
            peak = peak.max(m.level());
        }
        assert!(peak > 0.1 && peak < 0.5, "click peak {peak}");
        // a sustained shout (~0.5 s) gets to full scale
        let mut m = ShoutMeter::new();
        for i in 0..32 {
            m.push(&loud(i), &c);
        }
        assert!(m.level() > 0.99, "shout level {}", m.level());
    }

    #[test]
    fn talking_level_stays_below_the_meter_start() {
        let c = Config::default().audio;
        // talking peaks ~28 dB above the station floor; the meter starts at 35
        let f = FrameFeatures { db_above_floor: 28.0, ..quiet(0) };
        assert_eq!(ShoutMeter::raw(&f, &c), 0.0);
    }
}
