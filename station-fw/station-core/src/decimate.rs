//! 48 kHz -> 16 kHz decimator for the raw audio stream (`Msg::AudioChunk`).
//!
//! A linear-phase FIR low-pass (Kaiser-windowed sinc) runs at the input rate and every
//! third output is kept. Design: 191 taps, cutoff (-6 dB) 7.5 kHz, Kaiser beta 6.2. That
//! gives a passband flat within +-0.01 dB up to 7 kHz and at least 64 dB of attenuation
//! from 8 kHz (the output Nyquist) up, so anything that would alias back into the band is
//! at least 64 dB down. Group delay is 95 input samples (1.98 ms). DC gain is exactly 1
//! (taps normalised to sum 1). The taps are symmetric, so each output costs 96
//! multiplies per channel.
//!
//! Nothing else is done to the signal: no AGC, no noise suppression, no DC removal. The
//! 24-bit input is scaled by 2^-8 to 16 bits and rounded to nearest (ties away from
//! zero). dBFS therefore keep their meaning (relative to full scale either way). Outputs
//! that do not fit in i16 saturate and are counted in [`Decimator::clipped`].

use alloc::vec;
use alloc::vec::Vec;

use crate::level::slot_to_sample;

pub const INPUT_RATE_HZ: u32 = 48_000;
pub const FACTOR: usize = 3;
pub const OUTPUT_RATE_HZ: u32 = INPUT_RATE_HZ / FACTOR as u32;
pub const TAPS: usize = 191;
pub const CUTOFF_HZ: f64 = 7_500.0;
pub const KAISER_BETA: f64 = 6.2;
/// Group delay in input samples.
pub const DELAY_SAMPLES: usize = (TAPS - 1) / 2;
const HALF: usize = TAPS / 2; // 95; tap HALF is the centre

/// Zeroth-order modified Bessel function of the first kind (power series).
fn bessel_i0(x: f64) -> f64 {
    let mut sum = 1.0;
    let mut term = 1.0;
    let q = x * x / 4.0;
    let mut k = 1.0;
    while term > sum * 1e-17 {
        term *= q / (k * k);
        sum += term;
        k += 1.0;
    }
    sum
}

/// The filter taps, normalised to unit DC gain. Computed in f64, stored as f32.
pub fn design_taps() -> Vec<f32> {
    let fc = CUTOFF_HZ / INPUT_RATE_HZ as f64; // cycles per sample
    let m = (TAPS - 1) as f64;
    let i0b = bessel_i0(KAISER_BETA);
    let mut h: Vec<f64> = (0..TAPS)
        .map(|i| {
            let n = i as f64 - m / 2.0;
            let x = 2.0 * fc * n;
            let sinc = if n == 0.0 { 1.0 } else { libm::sin(core::f64::consts::PI * x) / (core::f64::consts::PI * x) };
            let r = 2.0 * i as f64 / m - 1.0;
            let w = bessel_i0(KAISER_BETA * libm::sqrt((1.0 - r * r).max(0.0))) / i0b;
            2.0 * fc * sinc * w
        })
        .collect();
    let s: f64 = h.iter().sum();
    for v in &mut h {
        *v /= s;
    }
    h.into_iter().map(|v| v as f32).collect()
}

/// Stereo decimate-by-3 with 24-bit in, 16-bit out.
pub struct Decimator {
    /// Taps 0..=HALF (the rest mirror them).
    h: Vec<f32>,
    /// A ring of the last TAPS stereo inputs, each written twice (at i and i + TAPS) so
    /// the window is always one contiguous slice. Channels interleaved so one pass over
    /// the taps serves both.
    hist: Vec<[f32; 2]>,
    pos: usize,
    phase: usize,
    /// Saturated output samples since the last [`Decimator::take_clipped`].
    clipped: u32,
}

impl Default for Decimator {
    fn default() -> Self {
        Self::new()
    }
}

impl Decimator {
    pub fn new() -> Self {
        let mut h = design_taps();
        h.truncate(HALF + 1);
        Self { h, hist: vec![[0.0; 2]; 2 * TAPS], pos: 0, phase: 0, clipped: 0 }
    }

    /// Clear the history (start of a new stream). The first DELAY_SAMPLES outputs then
    /// ramp in from silence.
    pub fn reset(&mut self) {
        self.hist.iter_mut().for_each(|v| *v = [0.0; 2]);
        self.pos = 0;
        self.phase = 0;
        self.clipped = 0;
    }

    /// Saturated output samples since the last call.
    pub fn take_clipped(&mut self) -> u32 {
        core::mem::take(&mut self.clipped)
    }

    /// Output capacity (i16 values, interleaved) needed for `in_frames` input stereo frames.
    pub const fn out_len(in_frames: usize) -> usize {
        2 * in_frames.div_ceil(FACTOR)
    }

    /// Both channels' filter output for the current window. Iterators (no indexing) so
    /// the inner loop has no bounds checks; trip count comes from slice lengths (see the
    /// Xtensa note in README).
    #[inline]
    fn dot(&self) -> (f32, f32) {
        let w = &self.hist[self.pos..self.pos + TAPS];
        let (a, rest) = w.split_at(HALF);
        let (mid, b) = rest.split_at(1);
        let c = self.h[HALF];
        // Two accumulator pairs: the FPU add latency, not the multiplies, bounds this loop.
        let (mut l0, mut r0) = (c * mid[0][0], c * mid[0][1]);
        let (mut l1, mut r1) = (0.0f32, 0.0f32);
        let mut it = self.h.iter().zip(a).zip(b.iter().rev());
        while let Some(((h0, x0), y0)) = it.next() {
            l0 += h0 * (x0[0] + y0[0]);
            r0 += h0 * (x0[1] + y0[1]);
            let Some(((h1, x1), y1)) = it.next() else { break };
            l1 += h1 * (x1[0] + y1[0]);
            r1 += h1 * (x1[1] + y1[1]);
        }
        (l0 + l1, r0 + r1)
    }

    fn quantize(&mut self, y: f32) -> i16 {
        let v = libm::roundf(y * (1.0 / 256.0));
        if v > i16::MAX as f32 {
            self.clipped += 1;
            i16::MAX
        } else if v < i16::MIN as f32 {
            self.clipped += 1;
            i16::MIN
        } else {
            v as i16
        }
    }

    /// Push one stereo frame of 24-bit samples; returns an output frame every third call.
    #[inline]
    pub fn push_frame(&mut self, l: i32, r: i32) -> Option<(i16, i16)> {
        let p = self.pos;
        let s = [l as f32, r as f32];
        self.hist[p] = s;
        self.hist[p + TAPS] = s;
        self.pos = if p + 1 == TAPS { 0 } else { p + 1 };
        self.phase += 1;
        if self.phase < FACTOR {
            return None;
        }
        self.phase = 0;
        let (yl, yr) = self.dot();
        Some((self.quantize(yl), self.quantize(yr)))
    }

    /// Push raw I2S bytes (little-endian 32-bit slots, interleaved L, R; sample = word >> 8)
    /// and append interleaved i16 output to `out`. Returns the number of i16 values
    /// written. `out` must hold [`Decimator::out_len`] of the input frame count; extra
    /// outputs are dropped (and should never happen). A trailing partial frame is ignored.
    pub fn push_bytes(&mut self, bytes: &[u8], out: &mut [i16]) -> usize {
        let mut n = 0;
        for f in bytes.chunks_exact(8) {
            let l = slot_to_sample(i32::from_le_bytes([f[0], f[1], f[2], f[3]]));
            let r = slot_to_sample(i32::from_le_bytes([f[4], f[5], f[6], f[7]]));
            if let Some((a, b)) = self.push_frame(l, r) {
                if n + 2 <= out.len() {
                    out[n] = a;
                    out[n + 1] = b;
                    n += 2;
                }
            }
        }
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;

    const FS: f64 = INPUT_RATE_HZ as f64;

    /// Feed a stereo tone (L at amplitude `a` 24-bit units, R at a/2) and return outputs.
    fn run_tone(freq: f64, a: f64, secs: f64) -> Vec<(i16, i16)> {
        let mut d = Decimator::new();
        let n = (FS * secs) as usize;
        let mut out = Vec::new();
        for i in 0..n {
            let x = libm::sin(2.0 * core::f64::consts::PI * freq * i as f64 / FS);
            if let Some(o) = d.push_frame((a * x) as i32, (0.5 * a * x) as i32) {
                out.push(o);
            }
        }
        assert_eq!(d.take_clipped(), 0);
        out
    }

    /// RMS (in output LSB) of one channel, skipping the filter's start-up transient.
    fn rms(out: &[(i16, i16)], ch: usize) -> f64 {
        let s = &out[200..];
        let ms: f64 = s.iter().map(|o| if ch == 0 { o.0 } else { o.1 } as f64).map(|v| v * v).sum::<f64>() / s.len() as f64;
        libm::sqrt(ms)
    }

    fn gain_db(freq: f64) -> f64 {
        let a = 0.25 * (1u32 << 23) as f64; // -12 dBFS
        let out = run_tone(freq, a, 0.25);
        let want = a / 256.0 / core::f64::consts::SQRT_2;
        20.0 * libm::log10(rms(&out, 0) / want)
    }

    #[test]
    fn taps_symmetric_unit_dc() {
        let h = design_taps();
        assert_eq!(h.len(), TAPS);
        for k in 0..TAPS {
            assert_eq!(h[k], h[TAPS - 1 - k]);
        }
        let s: f64 = h.iter().map(|&v| v as f64).sum();
        assert!((s - 1.0).abs() < 1e-6, "{s}");
    }

    #[test]
    fn passband_flat() {
        for f in [100.0, 440.0, 1000.0, 3000.0, 5000.0, 6000.0, 7000.0] {
            let g = gain_db(f);
            assert!(g.abs() < 0.05, "{f} Hz: {g:.3} dB");
        }
    }

    #[test]
    fn stopband_attenuated() {
        // 8 kHz and up would alias; everything there must be >= 60 dB down. At -12 dBFS
        // input, -60 dB leaves about 2 LSB RMS, so measure with a larger input.
        let a = 0.9 * (1u32 << 23) as f64;
        for f in [8000.0, 9000.0, 10000.0, 12000.0, 15000.0, 20000.0] {
            let out = run_tone(f, a, 0.25);
            let want = a / 256.0 / core::f64::consts::SQRT_2;
            let g = 20.0 * libm::log10(rms(&out, 0).max(1e-9) / want);
            assert!(g < -60.0, "{f} Hz: {g:.1} dB");
        }
    }

    #[test]
    fn tone_frequency_preserved() {
        // 1 kHz in -> 1 kHz out: 16 output samples per period. Check zero crossings.
        let out = run_tone(1000.0, 1e6, 0.5);
        let s: Vec<i16> = out[200..].iter().map(|o| o.0).collect();
        let crossings = s.windows(2).filter(|w| (w[0] < 0) != (w[1] < 0)).count();
        let secs = s.len() as f64 / OUTPUT_RATE_HZ as f64;
        let f = crossings as f64 / 2.0 / secs;
        assert!((f - 1000.0).abs() < 5.0, "{f}");
    }

    #[test]
    fn dc_preserved_and_channels_independent() {
        let mut d = Decimator::new();
        let mut last = (0, 0);
        for _ in 0..3000 {
            if let Some(o) = d.push_frame(256 * 1000, -256 * 123) {
                last = o;
            }
        }
        assert_eq!(last, (1000, -123));
    }

    #[test]
    fn rounding_to_nearest() {
        let mut d = Decimator::new();
        let mut last = (0, 0);
        for _ in 0..3000 {
            // 10.6 and -10.6 LSB of 16 bits
            if let Some(o) = d.push_frame(2714, -2714) {
                last = o;
            }
        }
        assert_eq!(last, (11, -11));
    }

    #[test]
    fn full_scale_clips_and_counts() {
        let mut d = Decimator::new();
        let fs = (1 << 23) - 1;
        let mut n = 0;
        let mut last = (0, 0);
        for _ in 0..3000 {
            if let Some(o) = d.push_frame(fs, -(1 << 23)) {
                n += 1;
                last = o;
            }
        }
        assert_eq!(last, (i16::MAX, i16::MIN));
        // +FS rounds to 32768 -> saturates every output once settled; -FS fits exactly.
        let c = d.take_clipped();
        assert!(c > n as u32 / 2, "clipped {c} of {n}");
        assert_eq!(d.take_clipped(), 0);
    }

    #[test]
    fn push_bytes_matches_push_frame() {
        let frames: Vec<(i32, i32)> = (0..2000).map(|i| ((i * 7919 % 100_000) - 50_000, (i * 104_729 % 60_000) - 30_000)).collect();
        let mut bytes = Vec::new();
        for &(l, r) in &frames {
            bytes.extend_from_slice(&((l << 8) | 0x11).to_le_bytes());
            bytes.extend_from_slice(&((r << 8) | 0x22).to_le_bytes());
        }
        let mut a = Decimator::new();
        let want: Vec<i16> = frames.iter().filter_map(|&(l, r)| a.push_frame(l, r)).flat_map(|(x, y)| [x, y]).collect();
        let mut b = Decimator::new();
        let mut out = std::vec![0i16; Decimator::out_len(frames.len())];
        let mut got = Vec::new();
        // odd split sizes (whole frames only, as the I2S driver delivers)
        for c in bytes.chunks(8 * 37) {
            let n = b.push_bytes(c, &mut out);
            got.extend_from_slice(&out[..n]);
        }
        assert_eq!(got, want);
        assert_eq!(got.len(), 2 * (2000 / 3));
    }

    #[test]
    fn reset_clears_state() {
        let mut d = Decimator::new();
        for _ in 0..999 {
            d.push_frame(1 << 20, 1 << 20);
        }
        d.reset();
        assert_eq!(d.push_frame(0, 0), None);
        assert_eq!(d.push_frame(0, 0), None);
        assert_eq!(d.push_frame(0, 0), Some((0, 0)));
    }
}
