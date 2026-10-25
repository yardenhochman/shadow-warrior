//! Shout detector: pure logic over per-frame (16ms) front-end features.
//!
//! Pipeline position (see beads shadow-warrior-1): the front-end already does
//! band-pass -> adaptive noise floor -> features. This module does:
//!   3. VAD gate with hangover (VAD is only an activation gate)
//!   4. shout features: level above floor (raised while playback is active),
//!      voiced F0 in range, spectral centroid/tilt, low flatness
//!   5. score accumulator + sustained-duration requirement -> `ShoutDetection`
//!
//! Distance rejection comes from the margin over the adaptive floor, not from
//! an absolute dB threshold.
//!
//! Phase-3 hook: a trained model (logistic regression / tiny CNN via tract) can
//! replace `ShoutDetector::frame_is_shouty` (or be blended into it); the
//! accumulator and gating stay as they are. Clips for training data come from
//! the clip recorder (`AudioManager::record_clip`).

use super::audio_frontend::{FrameFeatures, FRAME_SECS};
use crate::config::AudioConfig;

/// A detected shout.
#[derive(Clone, Debug, PartialEq)]
pub struct ShoutDetection {
    /// 0.25..1.0, grows with how far above the margin the shout was.
    pub intensity: f32,
    /// Sustained shouty time, seconds.
    pub duration_sec: f32,
    /// Band level of the triggering frame, dB.
    pub level_db: f32,
}

#[derive(Debug, Default)]
pub struct ShoutDetector {
    last_index: Option<u64>,
    hangover_left: u32,
    score: f32,
    /// Shouty frames in the current run (gaps not counted).
    run_frames: u32,
    /// Consecutive non-shouty frames since last shouty one.
    gap: u32,
    strength_sum: f32,
}

impl ShoutDetector {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one frame. Frames skipped by a lossy channel (index gaps, up to 8)
    /// are credited with the same features so timing stays correct.
    pub fn push(&mut self, f: &FrameFeatures, cfg: &AudioConfig) -> Option<ShoutDetection> {
        let missed = match self.last_index {
            Some(last) if f.index > last => ((f.index - last) as usize).clamp(1, 8),
            _ => 1,
        };
        self.last_index = Some(f.index);
        let mut out = None;
        for _ in 0..missed {
            if let Some(d) = self.step(f, cfg) {
                out = Some(d);
            }
        }
        out
    }

    /// VAD gate with hangover.
    fn gate_open(&mut self, f: &FrameFeatures, cfg: &AudioConfig) -> bool {
        if f.vad_prob > cfg.vad_threshold {
            self.hangover_left = cfg.vad_hangover_frames;
            true
        } else if self.hangover_left > 0 {
            self.hangover_left -= 1;
            true
        } else {
            false
        }
    }

    /// Returns Some(strength 1..2) when the frame looks like a near shout.
    fn frame_is_shouty(&mut self, f: &FrameFeatures, cfg: &AudioConfig) -> Option<f32> {
        let gate = self.gate_open(f, cfg);
        let margin = cfg.shout_margin_db
            + if f.playback_active { cfg.shout_playback_margin_db } else { 0.0 };
        if !gate || f.db_above_floor < margin {
            return None;
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
        .count() as u32;
        if passed < cfg.shout_min_features {
            return None;
        }
        Some(((f.db_above_floor - margin) / 10.0 + 1.0).min(2.0))
    }

    fn step(&mut self, f: &FrameFeatures, cfg: &AudioConfig) -> Option<ShoutDetection> {
        match self.frame_is_shouty(f, cfg) {
            Some(strength) => {
                self.gap = 0;
                self.run_frames += 1;
                self.strength_sum += strength;
                self.score += strength * cfg.shout_sensitivity;
                let dur = self.run_frames as f32 * FRAME_SECS;
                if self.score >= cfg.shout_score_threshold && dur >= cfg.shout_min_duration_sec {
                    let avg = self.strength_sum / self.run_frames as f32;
                    let det = ShoutDetection {
                        intensity: (avg * 0.5).clamp(0.25, 1.0),
                        duration_sec: dur,
                        level_db: f.band_db,
                    };
                    self.reset_run();
                    self.score = 0.0;
                    return Some(det);
                }
            }
            None => {
                self.gap += 1;
                self.score *= cfg.shout_score_decay;
                if self.score < 0.1 {
                    self.score = 0.0;
                }
                if self.gap > cfg.shout_gap_frames {
                    self.reset_run();
                }
            }
        }
        None
    }

    fn reset_run(&mut self) {
        self.run_frames = 0;
        self.gap = 0;
        self.strength_sum = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn cfg() -> AudioConfig {
        Config::default().audio
    }

    /// Quiet background frame (party bass leakage / ambience).
    fn bg(i: u64) -> FrameFeatures {
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
            ..bg(i)
        }
    }

    /// Run frames; return (frame index, detection) for each fire.
    fn run(frames: impl Iterator<Item = FrameFeatures>, cfg: &AudioConfig) -> Vec<(u64, ShoutDetection)> {
        let mut d = ShoutDetector::new();
        frames.filter_map(|f| d.push(&f, cfg).map(|x| (f.index, x))).collect()
    }

    fn latency_ms(fire_idx: u64, onset: u64) -> f32 {
        (fire_idx - onset + 1) as f32 * FRAME_SECS * 1000.0
    }

    #[test]
    fn steady_party_noise_never_fires() {
        // 10 minutes of party: VAD fires on vocals with pitch, but level hugs the floor.
        let frames = (0..37_500u64).map(|i| {
            let mut f = bg(i);
            f.vad_prob = 0.8;
            f.pitch_hz = 220.0;
            f.pitch_conf = 0.6;
            f.db_above_floor = (i % 7) as f32; // up to 6 dB fluctuations
            f
        });
        assert!(run(frames, &cfg()).is_empty());
    }

    #[test]
    fn distant_crowd_low_margin_rejected() {
        // Perfect shout-like spectrum but only 8 dB over floor (far away).
        assert!(run((0..2000).map(|i| shout(i, 8.0)), &cfg()).is_empty());
    }

    #[test]
    fn sudden_near_shout_detected_within_300ms() {
        let onset = 100u64;
        let frames = (0..300).map(|i| if i >= onset { shout(i, 28.0) } else { bg(i) });
        let hits = run(frames, &cfg());
        assert!(!hits.is_empty());
        let ms = latency_ms(hits[0].0, onset);
        assert!(ms < 300.0, "latency {ms}ms");
        assert!((0.25..=1.0).contains(&hits[0].1.intensity));
    }

    #[test]
    fn latency_holds_for_marginal_shout_with_vad_lag() {
        // Shout just above margin; VAD lags onset by 3 frames.
        let onset = 50u64;
        let frames = (0..200).map(|i| {
            let mut f = if i >= onset { shout(i, 17.0) } else { bg(i) };
            if i >= onset && i < onset + 3 {
                f.vad_prob = 0.1;
            }
            f
        });
        let hits = run(frames, &cfg());
        assert!(!hits.is_empty());
        let ms = latency_ms(hits[0].0, onset);
        assert!(ms < 300.0, "latency {ms}ms");
    }

    #[test]
    fn bag_thump_rejected() {
        // Very loud, 4 frames, VAD-negative, flat, low centroid.
        let frames = (0..200).map(|i| {
            let mut f = bg(i);
            if (80..84).contains(&i) {
                f.db_above_floor = 35.0;
                f.band_db = -15.0;
            }
            f
        });
        assert!(run(frames, &cfg()).is_empty());
    }

    #[test]
    fn loud_unvoiced_noise_rejected_even_if_vad_fires() {
        // Wind gust: loud, VAD open, but flat, no pitch, low centroid, steep tilt.
        let frames = (0..200).map(|i| {
            let mut f = bg(i);
            f.db_above_floor = 30.0;
            f.vad_prob = 0.9;
            f
        });
        assert!(run(frames, &cfg()).is_empty());
    }

    #[test]
    fn short_burst_rejected_sustained_accepted() {
        let c = cfg();
        let short = (0..200).map(|i| if (50..55).contains(&i) { shout(i, 28.0) } else { bg(i) });
        assert!(run(short, &c).is_empty());
        let long = (0..200).map(|i| if (50..80).contains(&i) { shout(i, 28.0) } else { bg(i) });
        assert!(!run(long, &c).is_empty());
    }

    #[test]
    fn min_duration_is_enforced() {
        let mut c = cfg();
        c.shout_min_duration_sec = 0.5;
        let frames = (0..200).map(|i| if i >= 10 { shout(i, 40.0) } else { bg(i) });
        let hits = run(frames, &c);
        assert!(!hits.is_empty());
        assert!(latency_ms(hits[0].0, 10) >= 500.0);
    }

    #[test]
    fn playback_raises_margin() {
        let c = cfg();
        let mk = |pb: bool, above: f32| {
            (0..200).map(move |i| {
                let mut f = shout(i, above);
                f.playback_active = pb;
                f
            })
        };
        // 20 dB is above the 15 dB margin but below 15 + 8 with playback.
        assert!(!run(mk(false, 20.0), &c).is_empty());
        assert!(run(mk(true, 20.0), &c).is_empty());
        // A loud near shout still breaks through playback.
        assert!(!run(mk(true, 32.0), &c).is_empty());
    }

    #[test]
    fn vad_hangover_bridges_dropouts() {
        let flicker = |i: u64| {
            let mut f = shout(i, 28.0);
            if i % 4 == 3 {
                f.vad_prob = 0.0;
            }
            f
        };
        let c = cfg();
        // With hangover the gate never closes: every frame is shouty, fast detection.
        let with = run((0..200).map(flicker), &c);
        assert!(!with.is_empty());
        let mut c0 = c.clone();
        c0.vad_hangover_frames = 0;
        c0.shout_gap_frames = 0;
        // Without hangover or gap tolerance the run keeps resetting: never reaches 0.2s.
        assert!(run((0..200).map(flicker), &c0).is_empty());
        // VAD permanently closed: never fires.
        let closed = (0..200).map(|i| {
            let mut f = shout(i, 28.0);
            f.vad_prob = 0.0;
            f
        });
        assert!(run(closed, &c).is_empty());
    }

    #[test]
    fn dropped_frames_are_credited() {
        // Only every 2nd frame is delivered (watch channel coalescing).
        let frames = (0..300u64)
            .filter(|i| i % 2 == 0)
            .map(|i| if i >= 20 { shout(i, 28.0) } else { bg(i) });
        let hits = run(frames, &cfg());
        assert!(!hits.is_empty());
        assert!(latency_ms(hits[0].0, 20) < 300.0);
    }

    #[test]
    fn old_config_without_new_keys_loads() {
        let y = "sample_rate: 16000\nchannels: 1\nbuffer_size: 1024\nshout_threshold_db: -15.0\nshout_min_duration_sec: 0.5\nshout_sensitivity: 1.0\nvad_threshold: 0.5\n";
        let c: AudioConfig = serde_yaml::from_str(y).unwrap();
        assert_eq!(c.shout_margin_db, 15.0);
        assert_eq!(c.vad_hangover_frames, 6);
    }
}
