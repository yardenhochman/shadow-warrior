//! RMS / peak level meter for the stereo ICS-43434 I2S stream.
//!
//! The ESP32 I2S peripheral is configured for 32-bit Philips slots; the
//! ICS-43434 puts a 24-bit two's-complement sample MSB-aligned in each slot,
//! so the sample is `word >> 8` (arithmetic). Words arrive interleaved
//! `L, R, L, R, ...` (L = WS low = mic with SELECT/LR tied to GND).

use station_proto::AudioLevel;

/// 24-bit full scale.
pub const FULL_SCALE: f64 = (1u32 << 23) as f64;

/// Convert one 32-bit I2S slot to a 24-bit sample.
#[inline]
pub fn slot_to_sample(word: i32) -> i32 {
    word >> 8
}

/// dBFS of a mean square value (in 24-bit sample units squared).
pub fn mean_square_to_dbfs(ms: f64) -> f32 {
    if ms <= 0.0 {
        return AudioLevel::FLOOR_DBFS;
    }
    let db = 10.0 * libm::log10(ms / (FULL_SCALE * FULL_SCALE));
    (db as f32).max(AudioLevel::FLOOR_DBFS)
}

/// dBFS of an absolute sample value.
pub fn abs_to_dbfs(a: u32) -> f32 {
    if a == 0 {
        return AudioLevel::FLOOR_DBFS;
    }
    let db = 20.0 * libm::log10(a as f64 / FULL_SCALE);
    (db as f32).max(AudioLevel::FLOOR_DBFS)
}

/// Accumulates interleaved stereo slots and emits an [`AudioLevel`] every `window` frames.
pub struct LevelMeter {
    window: u16,
    frames: u16,
    sumsq: [u64; 2],
    peak: u32,
    /// Odd word left over when a buffer ended mid-frame (DMA reads are byte
    /// counts, so this should not happen, but keep channels aligned if it does).
    pending_left: Option<i32>,
}

impl LevelMeter {
    /// `window` frames per report, e.g. 4800 at 48 kHz = 100 ms. Must be > 0.
    pub fn new(window: u16) -> Self {
        assert!(window > 0);
        Self { window, frames: 0, sumsq: [0; 2], peak: 0, pending_left: None }
    }

    /// Push raw 32-bit I2S slot words (interleaved L, R).
    pub fn push_slots(&mut self, words: &[i32], mut on_level: impl FnMut(AudioLevel)) {
        let mut it = words.iter().copied();
        loop {
            let l = match self.pending_left.take() {
                Some(l) => l,
                None => match it.next() {
                    Some(l) => l,
                    None => return,
                },
            };
            let Some(r) = it.next() else {
                self.pending_left = Some(l);
                return;
            };
            self.push_frame(slot_to_sample(l), slot_to_sample(r));
            if self.frames == self.window {
                on_level(self.take());
            }
        }
    }

    /// Push raw bytes as read from the I2S driver (little-endian 32-bit words).
    /// Trailing bytes that do not make a whole word are ignored.
    pub fn push_bytes(&mut self, bytes: &[u8], mut on_level: impl FnMut(AudioLevel)) {
        let mut words = [0i32; 64];
        for chunk in bytes.chunks(words.len() * 4) {
            let n = chunk.len() / 4;
            for (w, b) in words.iter_mut().zip(chunk.chunks_exact(4)) {
                *w = i32::from_le_bytes([b[0], b[1], b[2], b[3]]);
            }
            self.push_slots(&words[..n], &mut on_level);
        }
    }

    #[inline]
    fn push_frame(&mut self, l: i32, r: i32) {
        // |s| <= 2^23, s^2 <= 2^46; 65535 frames sum to < 2^62: no overflow.
        self.sumsq[0] += (l as i64 * l as i64) as u64;
        self.sumsq[1] += (r as i64 * r as i64) as u64;
        self.peak = self.peak.max(l.unsigned_abs()).max(r.unsigned_abs());
        self.frames += 1;
    }

    fn take(&mut self) -> AudioLevel {
        let n = self.frames as f64;
        let lvl = AudioLevel {
            rms_dbfs_l: mean_square_to_dbfs(self.sumsq[0] as f64 / n),
            rms_dbfs_r: mean_square_to_dbfs(self.sumsq[1] as f64 / n),
            peak_dbfs: abs_to_dbfs(self.peak),
            frames: self.frames,
        };
        self.frames = 0;
        self.sumsq = [0; 2];
        self.peak = 0;
        lvl
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;

    fn slot(sample: i32) -> i32 {
        // MSB-align a 24-bit sample in a 32-bit slot, low byte garbage like real hardware may send.
        (sample << 8) | 0x5A
    }

    fn run(words: &[i32], window: u16) -> Vec<AudioLevel> {
        let mut m = LevelMeter::new(window);
        let mut out = Vec::new();
        m.push_slots(words, |l| out.push(l));
        out
    }

    #[test]
    fn slot_shift_is_arithmetic() {
        assert_eq!(slot_to_sample(slot(-1)), -1);
        assert_eq!(slot_to_sample(slot(-(1 << 23))), -(1 << 23));
        assert_eq!(slot_to_sample(slot((1 << 23) - 1)), (1 << 23) - 1);
    }

    #[test]
    fn silence_is_floor() {
        let out = run(&[0; 200], 100);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].rms_dbfs_l, AudioLevel::FLOOR_DBFS);
        assert_eq!(out[0].peak_dbfs, AudioLevel::FLOOR_DBFS);
        assert_eq!(out[0].frames, 100);
    }

    #[test]
    fn full_scale_square_is_0dbfs() {
        let fs = (1 << 23) - 1;
        let words: Vec<i32> = (0..400).map(|i| if (i / 2) % 2 == 0 { slot(fs) } else { slot(-fs) }).collect();
        let out = run(&words, 200);
        assert!(out[0].rms_dbfs_l.abs() < 0.01, "{:?}", out[0]);
        assert!(out[0].rms_dbfs_r.abs() < 0.01);
        assert!(out[0].peak_dbfs.abs() < 0.01);
    }

    #[test]
    fn sine_minus_20dbfs_and_channels_independent() {
        // Left: sine at 0.1 FS -> RMS = 0.1/sqrt(2) -> -23.01 dBFS, peak -20 dBFS.
        // Right: silent.
        let a = 0.1 * FULL_SCALE;
        let n = 4800;
        let mut words = Vec::new();
        for i in 0..n {
            let s = (a * libm::sin(2.0 * core::f64::consts::PI * 1000.0 * i as f64 / 48000.0)) as i32;
            words.push(slot(s));
            words.push(slot(0));
        }
        let out = run(&words, n as u16);
        assert_eq!(out.len(), 1);
        assert!((out[0].rms_dbfs_l - (-23.0103)).abs() < 0.05, "{:?}", out[0]);
        assert_eq!(out[0].rms_dbfs_r, AudioLevel::FLOOR_DBFS);
        assert!((out[0].peak_dbfs - (-20.0)).abs() < 0.05);
    }

    #[test]
    fn split_buffers_keep_channel_alignment() {
        // L constant 1000, R silent, fed in odd-sized chunks.
        let words: Vec<i32> = (0..2000).map(|i| if i % 2 == 0 { slot(1000) } else { slot(0) }).collect();
        let mut m = LevelMeter::new(500);
        let mut out = Vec::new();
        for c in words.chunks(7) {
            m.push_slots(c, |l| out.push(l));
        }
        assert_eq!(out.len(), 2);
        for l in out {
            assert!((l.rms_dbfs_l - abs_to_dbfs(1000)).abs() < 1e-3);
            assert_eq!(l.rms_dbfs_r, AudioLevel::FLOOR_DBFS);
        }
    }

    #[test]
    fn push_bytes_matches_push_slots() {
        let words: Vec<i32> = (0..1000).map(|i| slot((i * 37 % 5000) - 2500)).collect();
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let a = run(&words, 100);
        let mut m = LevelMeter::new(100);
        let mut b = Vec::new();
        m.push_bytes(&bytes, |l| b.push(l));
        assert_eq!(a, b);
    }

    #[test]
    fn floating_input_all_ones_is_quiet_not_crash() {
        // A floating DIN often reads as all-ones words (-1 >> 8 = -1).
        let out = run(&[-1; 9600], 4800);
        assert_eq!(out.len(), 1);
        assert!(out[0].rms_dbfs_l < -130.0);
    }
}
