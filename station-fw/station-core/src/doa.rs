//! Direction of arrival from the two station mics with GCC-PHAT.
//!
//! Per frame of [`FRAME_LEN`] stereo samples (about 21 ms at 48 kHz):
//!
//! 1. remove the DC offset of each channel and compute the frame RMS; frames
//!    below [`DoaConfig::gate_dbfs`] stop here, so the noise floor and silence
//!    never produce a direction;
//! 2. zero-pad to [`NFFT`] (linear, not circular, correlation) and take one
//!    complex FFT of `l + j r` (two real FFTs for the price of one);
//! 3. cross spectrum `G = X_L conj(X_R)`, PHAT weighting `G / (|G| + eps)`,
//!    zeroed outside `band_lo_hz..band_hi_hz` (drops DC / rumble and the top
//!    octave where the mics and the room add mostly noise);
//! 4. inverse FFT gives the correlation `r[tau]`; search the positive peak in
//!    `|tau| <= max_lag + |bias| + 1` only, refine it with a parabola through
//!    the peak and its two neighbours;
//! 5. `lag = tau_peak - bias`, `angle = asin(lag c / (fs d))` clamped to
//!    -90..=90 deg, `confidence = r[peak] / median |r|` over all lags.
//!
//! Sign: `lag > 0` means the L channel is late, i.e. the sound reached the R mic
//! first and the source is on the R side (`angle > 0`). See
//! [`station_proto::Direction`].
//!
//! With d = 13 cm, c = 343 m/s, fs = 48 kHz the largest physical lag is 18.2
//! samples; one sample is about 3.1 deg near broadside and much more near the
//! axis (endfire), where `asin` is steep. Above c / 2d = 1.3 kHz the phase wraps
//! (spatial aliasing): broadband sounds (claps, shouts, clicks) still give one
//! clear peak, a pure tone does not.

use alloc::vec;
use alloc::vec::Vec;

use station_proto::Direction;

use crate::fft::{Fft, C32};
use crate::level::{slot_to_sample, FULL_SCALE};

/// Samples per channel per analysis frame (non-overlapping).
pub const FRAME_LEN: usize = 1024;
/// FFT size: `FRAME_LEN` zero-padded to twice its length.
pub const NFFT: usize = 2 * FRAME_LEN;

/// Below this confidence a direction is probably noise. Uncorrelated noise in two
/// channels measures median 4, worst 8.5 over 200 host-test frames; a single
/// broadband source without noise is > 100 (0 dB SNR still well above 10).
/// Advisory only: the station sends every gated frame, consumers decide.
pub const MIN_CONFIDENCE: f32 = 10.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DoaConfig {
    pub sample_rate_hz: f32,
    /// Distance between the two mic ports.
    pub mic_distance_m: f32,
    pub speed_of_sound_m_s: f32,
    /// Frames with RMS (both channels, DC removed) below this are not analysed.
    pub gate_dbfs: f32,
    /// Raw lag measured for a source at 0 deg (broadside); subtracted from every
    /// estimate. Find it with the calibration procedure in the README.
    pub lag_bias_samples: f32,
    pub band_lo_hz: f32,
    pub band_hi_hz: f32,
}

impl Default for DoaConfig {
    fn default() -> Self {
        Self {
            sample_rate_hz: 48_000.0,
            mic_distance_m: 0.13,
            speed_of_sound_m_s: 343.0,
            gate_dbfs: -50.0,
            lag_bias_samples: 0.0,
            band_lo_hz: 150.0,
            band_hi_hz: 10_000.0,
        }
    }
}

impl DoaConfig {
    /// Largest physically possible lag (sound along the mic axis), samples.
    pub fn max_lag_samples(&self) -> f32 {
        self.mic_distance_m * self.sample_rate_hz / self.speed_of_sound_m_s
    }

    /// Bias-corrected lag to angle from broadside, degrees, clamped to -90..=90.
    pub fn lag_to_angle_deg(&self, lag: f32) -> f32 {
        let x = (lag / self.max_lag_samples()).clamp(-1.0, 1.0);
        libm::asinf(x).to_degrees()
    }
}

/// Result of one GCC-PHAT run.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Estimate {
    /// Peak lag before bias correction, samples.
    pub raw_lag: f32,
    pub confidence: f32,
}

/// GCC-PHAT engine with its FFT plans and scratch buffers (about 50 KB on the heap).
pub struct GccPhat {
    fft: Fft,
    /// Half-size plan for the real-output inverse transform.
    ifft_half: Fft,
    buf: Vec<C32>,
    /// |G|^2 per bin.
    pow: Vec<f32>,
    /// Correlation `r[tau]`, `tau mod NFFT`.
    corr: Vec<f32>,
    /// |r| as f32 bits (same order as the floats, since they are >= 0), for the median.
    ranks: Vec<u32>,
    k_lo: usize,
    k_hi: usize,
    search: usize,
}

impl GccPhat {
    pub fn new(cfg: &DoaConfig) -> Self {
        let bin_hz = cfg.sample_rate_hz / NFFT as f32;
        let k_lo = (libm::ceilf(cfg.band_lo_hz / bin_hz) as usize).max(1);
        let k_hi = (libm::floorf(cfg.band_hi_hz / bin_hz) as usize).min(NFFT / 2 - 1);
        assert!(k_lo <= k_hi, "empty GCC-PHAT band");
        let search =
            (libm::ceilf(cfg.max_lag_samples() + libm::fabsf(cfg.lag_bias_samples)) as usize + 1).min(FRAME_LEN - 2);
        Self {
            fft: Fft::new(NFFT),
            ifft_half: Fft::new(NFFT / 2),
            buf: vec![C32::ZERO; NFFT],
            pow: vec![0.0; NFFT / 2],
            corr: vec![0.0; NFFT],
            ranks: vec![0; 2 * FRAME_LEN - 1],
            k_lo,
            k_hi,
            search,
        }
    }

    /// Lag search half-width in samples.
    pub fn search_range(&self) -> usize {
        self.search
    }

    /// `l`, `r`: one frame each, `FRAME_LEN` samples, any scale, DC removed.
    pub fn estimate(&mut self, l: &[f32], r: &[f32]) -> Estimate {
        assert!(l.len() == FRAME_LEN && r.len() == FRAME_LEN);
        load_input(&mut self.buf, l, r);
        self.fft.forward(&mut self.buf);
        phat_cross_spectrum(&mut self.buf, &mut self.pow, self.k_lo, self.k_hi);
        real_inverse(&self.ifft_half, self.fft.half_twiddles(), &mut self.buf, &mut self.corr);
        let (best, peak, frac) = find_peak(&self.corr, self.search);
        let med = median_abs_corr(&self.corr, &mut self.ranks);
        let confidence = if peak > 0.0 { peak / med.max(f32::MIN_POSITIVE) } else { 0.0 };
        Estimate { raw_lag: best as f32 + frac, confidence }
    }
}

/// Index loop on purpose, see `median_abs_corr` (Xtensa LLVM crash with zipped iterators).
#[inline(never)]
fn load_input(buf: &mut [C32], l: &[f32], r: &[f32]) {
    let (head, tail) = buf.split_at_mut(l.len());
    for (i, b) in head.iter_mut().enumerate() {
        *b = C32::new(l[i], r[i]);
    }
    for b in tail {
        *b = C32::ZERO;
    }
}

/// `1 / sqrt(x)` for x > 0: bit-trick seed plus three Newton steps (relative error
/// < 1e-6). Software `sqrtf` and division cost about 0.7-0.8 us each on the ESP32.
#[inline]
fn inv_sqrt(x: f32) -> f32 {
    let mut y = f32::from_bits(0x5f37_59df - (x.to_bits() >> 1));
    for _ in 0..3 {
        y *= 1.5 - 0.5 * x * y * y;
    }
    y
}

/// Split Z = FFT(l + j r) into X_L, X_R, form G = X_L conj(X_R) for bins
/// `k_lo..=k_hi` (and the mirrored bins), PHAT-weight it, zero the rest. Bins k
/// and n-k are handled together because both depend on Z[k] and Z[n-k].
#[inline(never)]
fn phat_cross_spectrum(buf: &mut [C32], pow: &mut [f32], k_lo: usize, k_hi: usize) {
    let n = buf.len();
    let mut pow_sum = 0f32;
    for k in k_lo..=k_hi {
        let zk = buf[k];
        let znk = buf[n - k].conj();
        let xl = (zk + znk).scale(0.5);
        let d = (zk - znk).scale(0.5);
        let xr = C32::new(d.im, -d.re); // d / j
        let g = xl * xr.conj();
        let p = g.norm_sqr();
        pow_sum += p;
        pow[k] = p;
        buf[k] = g;
    }
    // G / sqrt(|G|^2 + eps^2), eps = 1e-6 of the RMS bin magnitude.
    let eps2 = 1e-12 * pow_sum / (k_hi - k_lo + 1) as f32 + f32::MIN_POSITIVE;
    for k in k_lo..=k_hi {
        let g = buf[k].scale(inv_sqrt(pow[k] + eps2));
        buf[k] = g;
        buf[n - k] = g.conj();
    }
    buf[..k_lo].fill(C32::ZERO);
    buf[k_hi + 1..n - k_hi].fill(C32::ZERO);
    buf[n - k_lo + 1..].fill(C32::ZERO);
}

/// Inverse FFT of a Hermitian spectrum `g` (length N, so the result is real) with
/// one N/2-point complex FFT: Z[k] = E[k] + j O[k], E = (G[k] + G[k+N/2]) / 2,
/// O = (G[k] - G[k+N/2]) W^-k / 2, W = exp(-2 pi i / N); then z = IFFT(Z) holds
/// the even samples in `re` and the odd ones in `im`. `out[t]` is N/2 times the
/// true inverse (scale does not matter for the peak or the confidence ratio).
/// `g` is overwritten; `tw[k] = W^k` for k < N/2.
#[inline(never)]
fn real_inverse(half: &Fft, tw: &[C32], g: &mut [C32], out: &mut [f32]) {
    let m = g.len() / 2;
    let (lo, hi) = g.split_at_mut(m);
    for ((a, b), w) in lo.iter_mut().zip(hi.iter()).zip(tw) {
        let e = (*a + *b).scale(0.5);
        let o = ((*a - *b) * w.conj()).scale(0.5);
        *a = C32::new(e.re - o.im, e.im + o.re);
    }
    half.inverse_unscaled(lo);
    for (pair, z) in out.chunks_exact_mut(2).zip(lo.iter()) {
        pair[0] = z.re;
        pair[1] = z.im;
    }
}

#[inline]
fn corr_at(corr: &[f32], tau: isize) -> f32 {
    corr[tau.rem_euclid(corr.len() as isize) as usize]
}

/// Largest correlation within `|tau| <= search`: (integer lag, value, parabolic offset).
#[inline(never)]
fn find_peak(corr: &[f32], search: usize) -> (isize, f32, f32) {
    let s = search as isize;
    let mut best = 0isize;
    let mut peak = f32::NEG_INFINITY;
    for tau in -s..=s {
        let v = corr_at(corr, tau);
        if v > peak {
            peak = v;
            best = tau;
        }
    }
    let (ym, yp) = (corr_at(corr, best - 1), corr_at(corr, best + 1));
    let den = ym - 2.0 * peak + yp;
    let frac = if den < 0.0 { (0.5 * (ym - yp) / den).clamp(-0.5, 0.5) } else { 0.0 };
    (best, peak, frac)
}

/// Median |r| over every lag a `FRAME_LEN` frame can produce (-(FRAME_LEN-1)..FRAME_LEN).
///
/// `ranks.len()` is `2 * FRAME_LEN - 1`. The split point is derived from that
/// runtime length on purpose: loops with a constant trip count of 1024 here made
/// the Xtensa LLVM backend (esp toolchain rustc 1.90.0.0) abort with "Cannot select:
/// i32 = Constant<-4096>"; the same happened in `load_input`.
#[inline(never)]
fn median_abs_corr(corr: &[f32], ranks: &mut [u32]) -> f32 {
    let n = corr.len();
    let half = ranks.len() / 2 + 1;
    let (pos, neg) = ranks.split_at_mut(half);
    for (o, v) in pos.iter_mut().zip(&corr[..half]) {
        *o = libm::fabsf(*v).to_bits();
    }
    for (o, v) in neg.iter_mut().zip(&corr[n + 1 - half..]) {
        *o = libm::fabsf(*v).to_bits();
    }
    let mid = ranks.len() / 2;
    f32::from_bits(*ranks.select_nth_unstable(mid).1)
}

/// What happened to one frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameOutcome {
    pub frame: u32,
    pub rms_dbfs: f32,
    /// `None` when the frame was below the level gate.
    pub direction: Option<Direction>,
}

/// Accumulates interleaved I2S slots into frames and runs the gate + GCC-PHAT.
pub struct DoaEstimator {
    cfg: DoaConfig,
    gcc: GccPhat,
    l: Vec<f32>,
    r: Vec<f32>,
    fill: usize,
    frame: u32,
    pending_left: Option<i32>,
}

impl DoaEstimator {
    pub fn new(cfg: DoaConfig) -> Self {
        Self {
            gcc: GccPhat::new(&cfg),
            cfg,
            l: vec![0.0; FRAME_LEN],
            r: vec![0.0; FRAME_LEN],
            fill: 0,
            frame: 0,
            pending_left: None,
        }
    }

    pub fn config(&self) -> &DoaConfig {
        &self.cfg
    }

    pub fn search_range(&self) -> usize {
        self.gcc.search_range()
    }

    /// One stereo sample, as a fraction of full scale. Returns the outcome when this
    /// sample completes a frame.
    pub fn push_sample(&mut self, l: f32, r: f32) -> Option<FrameOutcome> {
        self.l[self.fill] = l;
        self.r[self.fill] = r;
        self.fill += 1;
        if self.fill < self.l.len() {
            return None;
        }
        self.fill = 0;
        let out = self.process();
        self.frame = self.frame.wrapping_add(1);
        Some(out)
    }

    /// One pair of raw 32-bit I2S slot words (L, R).
    pub fn push_slot_pair(&mut self, l: i32, r: i32) -> Option<FrameOutcome> {
        const SCALE: f32 = (1.0 / FULL_SCALE) as f32;
        self.push_sample(slot_to_sample(l) as f32 * SCALE, slot_to_sample(r) as f32 * SCALE)
    }

    /// Raw 32-bit I2S slot words, interleaved L, R.
    pub fn push_slots(&mut self, words: &[i32], mut on_frame: impl FnMut(FrameOutcome)) {
        let mut it = words.iter().copied();
        loop {
            let l = match self.pending_left.take().or_else(|| it.next()) {
                Some(l) => l,
                None => return,
            };
            let Some(r) = it.next() else {
                self.pending_left = Some(l);
                return;
            };
            if let Some(o) = self.push_slot_pair(l, r) {
                on_frame(o);
            }
        }
    }

    /// Bytes as read from the I2S driver (little-endian 32-bit words).
    pub fn push_bytes(&mut self, bytes: &[u8], mut on_frame: impl FnMut(FrameOutcome)) {
        let mut words = [0i32; 64];
        for chunk in bytes.chunks(words.len() * 4) {
            let n = chunk.len() / 4;
            for (w, b) in words.iter_mut().zip(chunk.chunks_exact(4)) {
                *w = i32::from_le_bytes([b[0], b[1], b[2], b[3]]);
            }
            self.push_slots(&words[..n], &mut on_frame);
        }
    }

    fn process(&mut self) -> FrameOutcome {
        let inv = 1.0 / FRAME_LEN as f32;
        let ml = self.l.iter().sum::<f32>() * inv;
        let mr = self.r.iter().sum::<f32>() * inv;
        let mut e = 0f32;
        for (a, b) in self.l.iter_mut().zip(self.r.iter_mut()) {
            *a -= ml;
            *b -= mr;
            e += *a * *a + *b * *b;
        }
        let ms = e * inv * 0.5;
        let rms_dbfs = if ms > 0.0 { (10.0 * libm::log10f(ms)).max(-144.0) } else { -144.0 };
        let mut out = FrameOutcome { frame: self.frame, rms_dbfs, direction: None };
        if rms_dbfs < self.cfg.gate_dbfs {
            return out;
        }
        let est = self.gcc.estimate(&self.l, &self.r);
        let lag = est.raw_lag - self.cfg.lag_bias_samples;
        out.direction = Some(Direction {
            frame: self.frame,
            lag_samples: lag,
            angle_deg: self.cfg.lag_to_angle_deg(lag),
            confidence: est.confidence,
            rms_dbfs,
        });
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;

    struct Rng(u64);
    impl Rng {
        fn next_f32(&mut self) -> f32 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 >> 40) as f32 / (1u64 << 24) as f32 - 0.5
        }
        /// Roughly Gaussian (sum of 4 uniforms), unit-ish variance.
        fn gauss(&mut self) -> f32 {
            (0..4).map(|_| self.next_f32()).sum::<f32>() * 1.732
        }
    }

    const M: usize = 8192;

    /// White noise `s` and `s` delayed by `delay` samples (band-limited fractional
    /// delay done in the frequency domain), both of length M.
    fn delayed_pair(seed: u64, delay: f64) -> (Vec<f32>, Vec<f32>) {
        let mut rng = Rng(seed);
        let s: Vec<f32> = (0..M).map(|_| rng.gauss()).collect();
        let fft = Fft::new(M);
        let mut x: Vec<C32> = s.iter().map(|&v| C32::new(v, 0.0)).collect();
        fft.forward(&mut x);
        for (k, v) in x.iter_mut().enumerate() {
            let kk = if k <= M / 2 { k as f64 } else { k as f64 - M as f64 };
            let ph = -2.0 * core::f64::consts::PI * kk * delay / M as f64;
            *v = *v * C32::new(libm::cos(ph) as f32, libm::sin(ph) as f32);
        }
        x[M / 2] = C32::ZERO;
        fft.inverse_unscaled(&mut x);
        let d: Vec<f32> = x.iter().map(|v| v.re / M as f32).collect();
        (s, d)
    }

    fn gcc() -> GccPhat {
        GccPhat::new(&DoaConfig::default())
    }

    /// Frame from the middle of a pair, L delayed by `delay`, plus independent noise
    /// at `noise` times the signal RMS.
    fn frame(seed: u64, delay: f64, noise: f32) -> (Vec<f32>, Vec<f32>) {
        let (s, d) = delayed_pair(seed, delay);
        let mut rng = Rng(seed ^ 0xA5A5_5A5A);
        let w = M / 2..M / 2 + FRAME_LEN;
        let l = d[w.clone()].iter().map(|v| (v + noise * rng.gauss()) * 1e-2).collect();
        let r = s[w].iter().map(|v| (v + noise * rng.gauss()) * 1e-2).collect();
        (l, r)
    }

    #[test]
    fn integer_and_fractional_lags_both_signs() {
        let mut g = gcc();
        for (i, &delay) in
            [0.0, 1.0, -1.0, 5.0, -5.0, 3.3, -3.3, 7.5, -7.5, 12.7, -12.7, 17.9, -17.9].iter().enumerate()
        {
            let (l, r) = frame(100 + i as u64, delay, 0.0);
            let e = g.estimate(&l, &r);
            assert!((e.raw_lag as f64 - delay).abs() < 0.1, "delay {delay}: got {e:?}");
            assert!(e.confidence > 50.0, "delay {delay}: {e:?}");
        }
    }

    #[test]
    fn lag_with_added_noise() {
        // Independent noise at the signal level in each channel (0 dB SNR).
        let mut g = gcc();
        for (i, &delay) in [4.25, -9.6, 15.0].iter().enumerate() {
            let (l, r) = frame(7 + i as u64, delay, 1.0);
            let e = g.estimate(&l, &r);
            assert!((e.raw_lag as f64 - delay).abs() < 0.3, "delay {delay}: got {e:?}");
            assert!(e.confidence > MIN_CONFIDENCE, "delay {delay}: {e:?}");
        }
    }

    #[test]
    fn uncorrelated_noise_has_low_confidence() {
        let mut g = gcc();
        let mut rng = Rng(99);
        let mut worst = 0f32;
        for _ in 0..20 {
            let l: Vec<f32> = (0..FRAME_LEN).map(|_| rng.gauss() * 1e-2).collect();
            let r: Vec<f32> = (0..FRAME_LEN).map(|_| rng.gauss() * 1e-2).collect();
            worst = worst.max(g.estimate(&l, &r).confidence);
        }
        assert!(worst < MIN_CONFIDENCE, "uncorrelated noise confidence {worst}");
    }

    #[test]
    fn real_inverse_matches_complex_inverse() {
        let n = 64;
        let mut rng = Rng(5);
        let x: Vec<f32> = (0..n).map(|_| rng.gauss()).collect();
        let f = Fft::new(n);
        let mut spec: Vec<C32> = x.iter().map(|&v| C32::new(v, 0.0)).collect();
        f.forward(&mut spec);
        let mut out = std::vec![0f32; n];
        real_inverse(&Fft::new(n / 2), f.half_twiddles(), &mut spec, &mut out);
        for (a, b) in x.iter().zip(&out) {
            assert!((a * (n / 2) as f32 - b).abs() < 1e-3, "{a} vs {}", b / (n / 2) as f32);
        }
    }

    #[test]
    fn inv_sqrt_is_accurate() {
        for x in [1e-30f32, 1e-12, 0.25, 1.0, 2.0, 12345.0, 1e20] {
            let want = 1.0 / (x as f64).sqrt();
            assert!(((inv_sqrt(x) as f64 - want) / want).abs() < 1e-6, "{x}");
        }
    }

    #[test]
    fn silence_does_not_panic_in_gcc() {
        let mut g = gcc();
        let z = std::vec![0f32; FRAME_LEN];
        let e = g.estimate(&z, &z);
        assert_eq!(e.confidence, 0.0);
        assert!(e.raw_lag.is_finite());
    }

    #[test]
    fn search_range_matches_geometry() {
        // 0.13 m * 48000 / 343 = 18.19 -> 19 + 1
        assert_eq!(gcc().search_range(), 20);
        assert!((DoaConfig::default().max_lag_samples() - 18.192).abs() < 0.01);
    }

    #[test]
    fn angle_math() {
        let c = DoaConfig::default();
        let max = c.max_lag_samples();
        assert_eq!(c.lag_to_angle_deg(0.0), 0.0);
        assert!((c.lag_to_angle_deg(max) - 90.0).abs() < 1e-3);
        assert!((c.lag_to_angle_deg(-max) + 90.0).abs() < 1e-3);
        assert_eq!(c.lag_to_angle_deg(max + 3.0), 90.0);
        assert_eq!(c.lag_to_angle_deg(-max - 3.0), -90.0);
        assert!((c.lag_to_angle_deg(max * 0.5) - 30.0).abs() < 1e-3);
        assert!((c.lag_to_angle_deg(max * -libm::sqrtf(0.5)) + 45.0).abs() < 1e-3);
    }

    fn slot(x: f32) -> i32 {
        (((x as f64 * FULL_SCALE) as i32) << 8) | 0x5A
    }

    /// Interleaved slot words for `frames` frames of a delayed pair at `amp` FS.
    fn words(seed: u64, delay: f64, amp: f32, dc: f32) -> Vec<i32> {
        let (s, d) = delayed_pair(seed, delay);
        let mut w = Vec::new();
        for i in 0..2 * FRAME_LEN {
            w.push(slot(dc + amp * d[1000 + i] / 3.0));
            w.push(slot(dc + amp * s[1000 + i] / 3.0));
        }
        w
    }

    fn run(est: &mut DoaEstimator, w: &[i32], chunk: usize) -> Vec<FrameOutcome> {
        let mut out = Vec::new();
        for c in w.chunks(chunk) {
            est.push_slots(c, |o| out.push(o));
        }
        out
    }

    #[test]
    fn estimator_sign_bias_and_chunking() {
        // L late by 6 samples -> source on the R side -> positive angle.
        let w = words(5, 6.0, 0.1, 0.0);
        let mut a = DoaEstimator::new(DoaConfig::default());
        let out = run(&mut a, &w, 7);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].frame, 0);
        assert_eq!(out[1].frame, 1);
        for o in &out {
            let d = o.direction.expect("loud frame must give a direction");
            assert_eq!(d.frame, o.frame);
            assert!((d.lag_samples - 6.0).abs() < 0.12, "{d:?}");
            assert!(d.angle_deg > 15.0 && d.angle_deg < 25.0, "{d:?}");
            assert!(o.rms_dbfs > -35.0 && o.rms_dbfs < -15.0, "{o:?}");
        }
        // Same input in one chunk gives identical results.
        let mut b = DoaEstimator::new(DoaConfig::default());
        assert_eq!(run(&mut b, &w, w.len()), out);
        // A bias of 6 samples makes it broadside.
        let mut c = DoaEstimator::new(DoaConfig { lag_bias_samples: 6.0, ..DoaConfig::default() });
        for o in run(&mut c, &w, 480) {
            let d = o.direction.unwrap();
            assert!(d.lag_samples.abs() < 0.12 && d.angle_deg.abs() < 0.5, "{d:?}");
        }
    }

    #[test]
    fn dc_offset_is_removed() {
        let w = words(11, -10.0, 0.05, 0.2);
        let mut e = DoaEstimator::new(DoaConfig::default());
        for o in run(&mut e, &w, 960) {
            let d = o.direction.unwrap();
            assert!((d.lag_samples + 10.0).abs() < 0.12, "{d:?}");
            // 0.05/3 * ~1 RMS -> about -36 dBFS; the 0.2 DC (-14 dBFS) must not count.
            assert!(o.rms_dbfs < -30.0, "{o:?}");
        }
    }

    #[test]
    fn silence_and_noise_floor_are_gated() {
        let mut e = DoaEstimator::new(DoaConfig::default());
        let mut out = Vec::new();
        e.push_slots(&std::vec![0i32; 4 * FRAME_LEN], |o| out.push(o));
        assert_eq!(out.len(), 2);
        assert!(out.iter().all(|o| o.direction.is_none() && o.rms_dbfs == -144.0));

        // -68 dBFS-ish correlated noise (the measured idle floor) stays below -50.
        let w = words(3, 2.0, 4e-4 * 3.0, 0.0);
        let out = run(&mut e, &w, 480);
        assert!(out.iter().all(|o| o.direction.is_none() && o.rms_dbfs < -60.0), "{out:?}");

        // Floating DIN (all ones) is silence after DC removal.
        let out = run(&mut e, &std::vec![-1i32; 2 * FRAME_LEN], 480);
        assert!(out.iter().all(|o| o.direction.is_none()));
    }

    #[test]
    fn push_bytes_matches_push_slots() {
        let w = words(21, -3.5, 0.1, 0.0);
        let bytes: Vec<u8> = w.iter().flat_map(|v| v.to_le_bytes()).collect();
        let mut a = DoaEstimator::new(DoaConfig::default());
        let mut b = DoaEstimator::new(DoaConfig::default());
        let x = run(&mut a, &w, w.len());
        let mut y = Vec::new();
        for c in bytes.chunks(3840) {
            b.push_bytes(c, |o| y.push(o));
        }
        assert_eq!(x, y);
    }
}
