//! Small in-place radix-2 complex FFT in `f32`, for GCC-PHAT on the station.
//!
//! Iterative decimation-in-time with a precomputed twiddle table (computed in
//! `f64` for accuracy) and bit-reversal table. No `unsafe`; the butterfly loop
//! uses `chunks_exact_mut` / `split_at_mut` so the optimiser can drop bounds
//! checks. N = 2048 takes 2.37 ms on a 240 MHz ESP32, about 50 cycles per
//! butterfly; the inner loop is already FPU code (mul.s/madd.s) and is bound by
//! FP latency. Hand-written assembly (ESP-DSP) would be the next step.

use alloc::vec::Vec;
use core::ops::{Add, Mul, Sub};

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct C32 {
    pub re: f32,
    pub im: f32,
}

impl C32 {
    pub const ZERO: C32 = C32 { re: 0.0, im: 0.0 };

    #[inline]
    pub const fn new(re: f32, im: f32) -> Self {
        Self { re, im }
    }

    #[inline]
    pub fn conj(self) -> Self {
        Self { re: self.re, im: -self.im }
    }

    #[inline]
    pub fn norm_sqr(self) -> f32 {
        self.re * self.re + self.im * self.im
    }

    #[inline]
    pub fn scale(self, s: f32) -> Self {
        Self { re: self.re * s, im: self.im * s }
    }
}

impl Add for C32 {
    type Output = C32;
    #[inline]
    fn add(self, o: C32) -> C32 {
        C32 { re: self.re + o.re, im: self.im + o.im }
    }
}

impl Sub for C32 {
    type Output = C32;
    #[inline]
    fn sub(self, o: C32) -> C32 {
        C32 { re: self.re - o.re, im: self.im - o.im }
    }
}

impl Mul for C32 {
    type Output = C32;
    #[inline]
    fn mul(self, o: C32) -> C32 {
        C32 { re: self.re * o.re - self.im * o.im, im: self.re * o.im + self.im * o.re }
    }
}

/// Plan for one power-of-two size.
pub struct Fft {
    n: usize,
    /// Twiddles grouped per stage, contiguous: for each butterfly half-size `h`
    /// (1, 2, 4, ..., n/2), `tw[h + k] = exp(-2 pi i k / (2h))` for `k < h`.
    /// `tw[0]` is unused. Contiguous per-stage tables plus the special-cased first
    /// two stages took a 2048-point FFT on the ESP32 (240 MHz) from 2.67 to 2.37 ms.
    tw: Vec<C32>,
    /// Bit-reversed index of each position.
    rev: Vec<u16>,
}

impl Fft {
    /// `n` must be a power of two, 4..=65536.
    pub fn new(n: usize) -> Self {
        assert!(n.is_power_of_two() && (4..=65536).contains(&n), "FFT size must be a power of two, 4..=65536");
        let bits = n.trailing_zeros();
        let mut tw = alloc::vec![C32::ZERO; n];
        let mut h = 1;
        while h < n {
            for k in 0..h {
                let a = -core::f64::consts::PI * k as f64 / h as f64;
                tw[h + k] = C32::new(libm::cos(a) as f32, libm::sin(a) as f32);
            }
            h *= 2;
        }
        let rev = (0..n).map(|i| ((i as u32).reverse_bits() >> (32 - bits)) as u16).collect();
        Self { n, tw, rev }
    }

    pub fn len(&self) -> usize {
        self.n
    }

    pub fn is_empty(&self) -> bool {
        false
    }

    /// `exp(-2 pi i k / n)` for `k < n/2` (the last stage's twiddles).
    pub fn half_twiddles(&self) -> &[C32] {
        &self.tw[self.n / 2..]
    }

    /// Forward transform in place, `X[k] = sum x[n] exp(-2 pi i k n / N)` (unscaled).
    pub fn forward(&self, x: &mut [C32]) {
        assert_eq!(x.len(), self.n);
        for (i, &r) in self.rev.iter().enumerate() {
            let r = r as usize;
            if r > i {
                x.swap(i, r);
            }
        }
        // Stages 1 and 2 have trivial twiddles (1 and -j).
        for p in x.chunks_exact_mut(2) {
            let (a, b) = (p[0], p[1]);
            p[0] = a + b;
            p[1] = a - b;
        }
        for p in x.chunks_exact_mut(4) {
            let (a0, a1, b0) = (p[0], p[1], p[2]);
            let b1 = C32::new(p[3].im, -p[3].re); // p[3] * -j
            p[0] = a0 + b0;
            p[2] = a0 - b0;
            p[1] = a1 + b1;
            p[3] = a1 - b1;
        }
        let mut half = 4;
        while half < self.n {
            let tw = &self.tw[half..2 * half];
            for block in x.chunks_exact_mut(2 * half) {
                let (lo, hi) = block.split_at_mut(half);
                for ((a, b), w) in lo.iter_mut().zip(hi.iter_mut()).zip(tw) {
                    let t = *b * *w;
                    *b = *a - t;
                    *a = *a + t;
                }
            }
            half *= 2;
        }
    }

    /// Inverse transform in place, unscaled (the result is `N` times the true inverse).
    pub fn inverse_unscaled(&self, x: &mut [C32]) {
        for v in x.iter_mut() {
            *v = v.conj();
        }
        self.forward(x);
        for v in x.iter_mut() {
            *v = v.conj();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;

    fn naive_dft(x: &[C32]) -> Vec<C32> {
        let n = x.len();
        (0..n)
            .map(|k| {
                let (mut re, mut im) = (0f64, 0f64);
                for (j, v) in x.iter().enumerate() {
                    let a = -2.0 * core::f64::consts::PI * (k * j % n) as f64 / n as f64;
                    let (c, s) = (libm::cos(a), libm::sin(a));
                    re += v.re as f64 * c - v.im as f64 * s;
                    im += v.re as f64 * s + v.im as f64 * c;
                }
                C32::new(re as f32, im as f32)
            })
            .collect()
    }

    fn signal(n: usize) -> Vec<C32> {
        let mut s = 0x1234_5678u32;
        (0..n)
            .map(|_| {
                s ^= s << 13;
                s ^= s >> 17;
                s ^= s << 5;
                let a = (s as f32 / u32::MAX as f32) - 0.5;
                s ^= s << 13;
                s ^= s >> 17;
                s ^= s << 5;
                C32::new(a, (s as f32 / u32::MAX as f32) - 0.5)
            })
            .collect()
    }

    #[test]
    fn matches_naive_dft() {
        for n in [4, 8, 16, 64, 256, 2048] {
            let x = signal(n);
            let want = naive_dft(&x);
            let mut got = x.clone();
            Fft::new(n).forward(&mut got);
            for (g, w) in got.iter().zip(&want) {
                assert!((*g - *w).norm_sqr().sqrt() < 2e-5 * n as f32, "n={n} {g:?} vs {w:?}");
            }
        }
    }

    #[test]
    fn inverse_roundtrip_2048() {
        let n = 2048;
        let x = signal(n);
        let mut y = x.clone();
        let f = Fft::new(n);
        f.forward(&mut y);
        f.inverse_unscaled(&mut y);
        for (a, b) in x.iter().zip(&y) {
            let b = b.scale(1.0 / n as f32);
            assert!((*a - b).norm_sqr().sqrt() < 1e-5, "{a:?} vs {b:?}");
        }
    }

    #[test]
    fn impulse_is_flat() {
        let mut x = std::vec![C32::ZERO; 16];
        x[0] = C32::new(1.0, 0.0);
        Fft::new(16).forward(&mut x);
        assert!(x.iter().all(|v| (v.re - 1.0).abs() < 1e-6 && v.im.abs() < 1e-6));
    }
}
