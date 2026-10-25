//! Pure DSP audio front-end (shadow-warrior-xj8).
//!
//! Pipeline: mono samples at device rate -> band-pass biquads (~250Hz-4kHz, doubles as
//! anti-alias filter) -> streaming linear resample to 16kHz -> 256-sample (16ms) frames ->
//! per-frame features + adaptive noise floor -> ring buffer of audio and feature history.
//! No hardware or async dependencies; everything is unit-testable with synthetic signals.


use serde::Serialize;
use std::collections::VecDeque;
use std::f32::consts::PI;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const TARGET_RATE: u32 = 16_000;
pub const FRAME_SIZE: usize = 256; // 16ms @ 16kHz (earshot requirement)
pub const FRAME_SECS: f32 = FRAME_SIZE as f32 / TARGET_RATE as f32;

// ---------------------------------------------------------------- biquads

#[derive(Clone, Debug)]
pub struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl Biquad {
    fn from_coeffs(b0: f32, b1: f32, b2: f32, a0: f32, a1: f32, a2: f32) -> Self {
        Self { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: a1 / a0, a2: a2 / a0, z1: 0.0, z2: 0.0 }
    }

    pub fn lowpass(fs: f32, fc: f32, q: f32) -> Self {
        let w = 2.0 * PI * fc / fs;
        let (s, c) = w.sin_cos();
        let alpha = s / (2.0 * q);
        Self::from_coeffs((1.0 - c) / 2.0, 1.0 - c, (1.0 - c) / 2.0, 1.0 + alpha, -2.0 * c, 1.0 - alpha)
    }

    pub fn highpass(fs: f32, fc: f32, q: f32) -> Self {
        let w = 2.0 * PI * fc / fs;
        let (s, c) = w.sin_cos();
        let alpha = s / (2.0 * q);
        Self::from_coeffs((1.0 + c) / 2.0, -(1.0 + c), (1.0 + c) / 2.0, 1.0 + alpha, -2.0 * c, 1.0 - alpha)
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }
}

/// 4th-order Butterworth high-pass + 4th-order low-pass cascade.
#[derive(Clone, Debug)]
pub struct BandPass {
    stages: Vec<Biquad>,
}

impl BandPass {
    pub fn new(fs: f32, low_hz: f32, high_hz: f32) -> Self {
        // Butterworth 4th-order section Qs
        const QS: [f32; 2] = [0.5412, 1.3066];
        // Keep corner safely below Nyquist for low device rates.
        let high = high_hz.min(fs * 0.45);
        let mut stages = Vec::new();
        for q in QS {
            stages.push(Biquad::highpass(fs, low_hz, q));
        }
        for q in QS {
            stages.push(Biquad::lowpass(fs, high, q));
        }
        Self { stages }
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        self.stages.iter_mut().fold(x, |v, s| s.process(v))
    }
}

// ---------------------------------------------------------------- resampler

/// Streaming linear resampler to 16kHz. Input must already be band-limited.
#[derive(Clone, Debug)]
pub struct StreamResampler {
    step: f64, // input samples per output sample
    pos: f64,  // position of next output; virtual sequence is [prev, input[0], input[1], ...]
    prev: f32,
}

impl StreamResampler {
    pub fn new(input_rate: u32) -> Self {
        Self { step: input_rate as f64 / TARGET_RATE as f64, pos: 0.0, prev: 0.0 }
    }

    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        if (self.step - 1.0).abs() < 1e-9 {
            out.extend_from_slice(input);
            return;
        }
        let n = input.len() as f64;
        while self.pos < n {
            let i = self.pos.floor();
            let frac = (self.pos - i) as f32;
            let a = if i < 1.0 { self.prev } else { input[i as usize - 1] };
            let b = input[i as usize];
            out.push(a + (b - a) * frac);
            self.pos += self.step;
        }
        self.pos -= n;
        if let Some(&l) = input.last() {
            self.prev = l;
        }
    }
}

// ---------------------------------------------------------------- noise floor

/// Asymmetric tracker: follows drops quickly, rises very slowly, so speech does not
/// drag the floor up but changes in ambient noise are eventually followed.
#[derive(Clone, Debug)]
pub struct NoiseFloor {
    floor_db: f32,
    initialised: bool,
    rise_per_frame: f32, // max dB per frame upward
    fall_coeff: f32,     // EMA coefficient when below floor
}

impl NoiseFloor {
    pub fn new() -> Self {
        Self::with_rates(0.5, 1.0)
    }

    /// `rise_db_per_sec`: max upward drift; `fall_time_s`: time constant when falling.
    pub fn with_rates(rise_db_per_sec: f32, fall_time_s: f32) -> Self {
        Self {
            floor_db: -80.0,
            initialised: false,
            rise_per_frame: rise_db_per_sec * FRAME_SECS,
            fall_coeff: (FRAME_SECS / fall_time_s).min(1.0),
        }
    }

    pub fn update(&mut self, band_db: f32) -> f32 {
        if !self.initialised {
            self.floor_db = band_db;
            self.initialised = true;
        } else if band_db < self.floor_db {
            self.floor_db += (band_db - self.floor_db) * self.fall_coeff;
        } else {
            self.floor_db += (band_db - self.floor_db).min(self.rise_per_frame);
        }
        self.floor_db
    }

    #[allow(dead_code)] // for upcoming detectors/tests
    pub fn floor_db(&self) -> f32 {
        self.floor_db
    }
}

impl Default for NoiseFloor {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------- features

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct FrameFeatures {
    /// Monotonic frame index since start.
    pub index: u64,
    pub band_db: f32,
    pub noise_floor_db: f32,
    pub db_above_floor: f32,
    pub vad_prob: f32,
    pub pitch_hz: f32,
    /// Normalised autocorrelation peak, 0..1.
    pub pitch_conf: f32,
    pub spectral_centroid_hz: f32,
    /// Slope (dB per kHz) of a linear fit to the log spectrum.
    pub spectral_tilt_db_per_khz: f32,
    /// Geometric / arithmetic mean of power spectrum, 0..1 (1 = white noise).
    pub spectral_flatness: f32,
    /// Our speakers were playing during this frame.
    pub playback_active: bool,
}

/// Voice-activity probability source. Implemented for `earshot::Detector`; tests use stubs.
pub trait VadProbe: Send {
    /// `frame` is exactly FRAME_SIZE samples at 16kHz in [-1, 1].
    fn probability(&mut self, frame: &[f32]) -> f32;
}

impl VadProbe for earshot::Detector {
    fn probability(&mut self, frame: &[f32]) -> f32 {
        let i16s: Vec<i16> = frame.iter().map(|&s| (s.clamp(-1.0, 1.0) * 32767.0) as i16).collect();
        self.predict_i16(&i16s)
    }
}

#[allow(dead_code)] // for upcoming detectors/tests
/// VAD stub that always returns a fixed probability.
pub struct ConstVad(pub f32);
impl VadProbe for ConstVad {
    fn probability(&mut self, _: &[f32]) -> f32 {
        self.0
    }
}

pub fn rms_db(frame: &[f32]) -> f32 {
    if frame.is_empty() {
        return -100.0;
    }
    let ms = frame.iter().map(|s| s * s).sum::<f32>() / frame.len() as f32;
    if ms > 1e-10 {
        10.0 * ms.log10()
    } else {
        -100.0
    }
}

/// In-place radix-2 FFT (len must be a power of two).
fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -2.0 * PI / len as f32;
        let (wr, wi) = (ang.cos(), ang.sin());
        for start in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0f32, 0.0f32);
            for k in 0..len / 2 {
                let (a, b) = (start + k, start + k + len / 2);
                let tr = re[b] * cr - im[b] * ci;
                let ti = re[b] * ci + im[b] * cr;
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
                let ncr = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = ncr;
            }
        }
        len <<= 1;
    }
}

/// Returns (centroid_hz, tilt_db_per_khz, flatness) from a Hann-windowed FFT.
pub fn spectral_features(frame: &[f32]) -> (f32, f32, f32) {
    debug_assert_eq!(frame.len(), FRAME_SIZE);
    let n = frame.len();
    let mut re: Vec<f32> = frame
        .iter()
        .enumerate()
        .map(|(i, &s)| s * (0.5 - 0.5 * (2.0 * PI * i as f32 / n as f32).cos()))
        .collect();
    let mut im = vec![0.0; n];
    fft(&mut re, &mut im);
    let bin_hz = TARGET_RATE as f32 / n as f32;
    // Skip DC; use bins 1..n/2.
    let power: Vec<f32> = (1..n / 2).map(|k| re[k] * re[k] + im[k] * im[k] + 1e-12).collect();
    let freqs: Vec<f32> = (1..n / 2).map(|k| k as f32 * bin_hz).collect();
    let total: f32 = power.iter().sum();
    let centroid = power.iter().zip(&freqs).map(|(p, f)| p * f).sum::<f32>() / total;
    let m = power.len() as f32;
    let log_mean = power.iter().map(|p| p.ln()).sum::<f32>() / m;
    let flatness = (log_mean.exp() / (total / m)).clamp(0.0, 1.0);
    // Linear regression of dB vs kHz.
    let db: Vec<f32> = power.iter().map(|p| 10.0 * p.log10()).collect();
    let fk: Vec<f32> = freqs.iter().map(|f| f / 1000.0).collect();
    let (mf, md) = (fk.iter().sum::<f32>() / m, db.iter().sum::<f32>() / m);
    let num: f32 = fk.iter().zip(&db).map(|(f, d)| (f - mf) * (d - md)).sum();
    let den: f32 = fk.iter().map(|f| (f - mf) * (f - mf)).sum();
    (centroid, num / den, flatness)
}

/// Autocorrelation pitch estimate over 70..500 Hz. Returns (f0_hz, confidence 0..1).
pub fn pitch_autocorr(frame: &[f32]) -> (f32, f32) {
    let r0: f32 = frame.iter().map(|s| s * s).sum();
    if r0 < 1e-9 {
        return (0.0, 0.0);
    }
    let min_lag = TARGET_RATE as usize / 500;
    let max_lag = (TARGET_RATE as usize / 70).min(frame.len() - 1);
    let mut nrs = Vec::with_capacity(max_lag - min_lag + 1);
    for lag in min_lag..=max_lag {
        let (a, b) = (&frame[..frame.len() - lag], &frame[lag..]);
        let r: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
        let e1: f32 = a.iter().map(|s| s * s).sum();
        let e2: f32 = b.iter().map(|s| s * s).sum();
        let nr = r / (e1 * e2).sqrt().max(1e-9);
        nrs.push((lag, nr));
    }
    // Prefer the shortest lag within 90% of the max peak, avoiding sub-harmonic picks,
    // and require it to be a local maximum.
    let max_nr = nrs.iter().map(|x| x.1).fold(0.0f32, f32::max);
    let mut best = (0usize, 0.0f32);
    for i in 1..nrs.len().saturating_sub(1) {
        let (lag, nr) = nrs[i];
        if nr >= 0.9 * max_nr && nr >= nrs[i - 1].1 && nr >= nrs[i + 1].1 {
            best = (lag, nr);
            break;
        }
    }
    if best.0 == 0 || best.1 < 0.1 {
        return (0.0, best.1.max(0.0));
    }
    (TARGET_RATE as f32 / best.0 as f32, best.1.clamp(0.0, 1.0))
}

// ---------------------------------------------------------------- ring buffer

#[derive(Clone, Debug)]
pub struct RingBuffer<T> {
    buf: VecDeque<T>,
    cap: usize,
}

impl<T: Clone> RingBuffer<T> {
    pub fn new(cap: usize) -> Self {
        Self { buf: VecDeque::with_capacity(cap), cap }
    }
    pub fn push_slice(&mut self, items: &[T]) {
        for it in items {
            self.push(it.clone());
        }
    }
    pub fn push(&mut self, item: T) {
        if self.buf.len() == self.cap {
            self.buf.pop_front();
        }
        self.buf.push_back(item);
    }
    #[allow(dead_code)] // for upcoming detectors/tests
    pub fn len(&self) -> usize {
        self.buf.len()
    }
    #[allow(dead_code)] // for upcoming detectors/tests
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }
    #[allow(dead_code)] // for upcoming detectors/tests
    pub fn capacity(&self) -> usize {
        self.cap
    }
    /// Most recent `n` items (or fewer), oldest first.
    pub fn last_n(&self, n: usize) -> Vec<T> {
        self.buf.iter().skip(self.buf.len().saturating_sub(n)).cloned().collect()
    }
    #[allow(dead_code)] // for upcoming detectors/tests
    pub fn snapshot(&self) -> Vec<T> {
        self.buf.iter().cloned().collect()
    }
}

// ---------------------------------------------------------------- front-end

#[derive(Clone, Debug)]
pub struct FrontEndConfig {
    pub low_hz: f32,
    pub high_hz: f32,
    pub ring_seconds: f32,
}

impl Default for FrontEndConfig {
    fn default() -> Self {
        Self { low_hz: 250.0, high_hz: 4000.0, ring_seconds: 5.0 }
    }
}

pub struct FrontEnd {
    bandpass: BandPass,
    resampler: StreamResampler,
    vad: Box<dyn VadProbe>,
    floor: NoiseFloor,
    pending: Vec<f32>,
    scratch: Vec<f32>,
    audio: RingBuffer<f32>,
    features: RingBuffer<FrameFeatures>,
    frame_index: u64,
    playback_active: bool,
}

impl FrontEnd {
    pub fn new(input_rate: u32, cfg: FrontEndConfig, vad: Box<dyn VadProbe>) -> Self {
        let audio_cap = (cfg.ring_seconds * TARGET_RATE as f32) as usize;
        let feat_cap = (cfg.ring_seconds / FRAME_SECS).ceil() as usize;
        Self {
            bandpass: BandPass::new(input_rate as f32, cfg.low_hz, cfg.high_hz),
            resampler: StreamResampler::new(input_rate),
            vad,
            floor: NoiseFloor::new(),
            pending: Vec::with_capacity(FRAME_SIZE * 2),
            scratch: Vec::new(),
            audio: RingBuffer::new(audio_cap),
            features: RingBuffer::new(feat_cap),
            frame_index: 0,
            playback_active: false,
        }
    }

    pub fn set_playback_active(&mut self, active: bool) {
        self.playback_active = active;
    }

    /// Feed mono samples at the device rate. Returns one `(filtered 16k frame, features)`
    /// per completed 16ms frame.
    pub fn process(&mut self, mono: &[f32]) -> Vec<(Vec<f32>, FrameFeatures)> {
        let filtered: Vec<f32> = mono.iter().map(|&x| self.bandpass.process(x)).collect();
        self.scratch.clear();
        self.resampler.process(&filtered, &mut self.scratch);
        self.pending.extend_from_slice(&self.scratch);

        let mut out = Vec::new();
        while self.pending.len() >= FRAME_SIZE {
            let frame: Vec<f32> = self.pending.drain(..FRAME_SIZE).collect();
            let f = self.analyse(&frame);
            self.audio.push_slice(&frame);
            self.features.push(f.clone());
            out.push((frame, f));
        }
        out
    }

    fn analyse(&mut self, frame: &[f32]) -> FrameFeatures {
        let band_db = rms_db(frame);
        let floor = self.floor.update(band_db);
        let (centroid, tilt, flat) = spectral_features(frame);
        let (pitch_hz, pitch_conf) = pitch_autocorr(frame);
        let f = FrameFeatures {
            index: self.frame_index,
            band_db,
            noise_floor_db: floor,
            db_above_floor: band_db - floor,
            vad_prob: self.vad.probability(frame),
            pitch_hz,
            pitch_conf,
            spectral_centroid_hz: centroid,
            spectral_tilt_db_per_khz: tilt,
            spectral_flatness: flat,
            playback_active: self.playback_active,
        };
        self.frame_index += 1;
        f
    }

    #[allow(dead_code)] // for upcoming detectors/tests
    pub fn audio_ring(&self) -> &RingBuffer<f32> {
        &self.audio
    }
    #[allow(dead_code)] // for upcoming detectors/tests
    pub fn feature_history(&self) -> &RingBuffer<FrameFeatures> {
        &self.features
    }

    /// Snapshot the last `seconds` of audio + features for the clip recorder.
    pub fn snapshot_clip(&self, seconds: f32) -> (Vec<f32>, Vec<FrameFeatures>) {
        let ns = (seconds * TARGET_RATE as f32) as usize;
        let nf = (seconds / FRAME_SECS).ceil() as usize;
        (self.audio.last_n(ns), self.features.last_n(nf))
    }
}

// ---------------------------------------------------------------- clip recorder

#[derive(Serialize)]
struct ClipMeta<'a> {
    reason: &'a str,
    sample_rate: u32,
    samples: usize,
    frame_size: usize,
    frames: &'a [FrameFeatures],
}

/// Writes `<dir>/<stamp>_<reason>.wav` and a matching `.json` of per-frame features.
pub struct ClipRecorder {
    dir: PathBuf,
}

impl ClipRecorder {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn record(
        &self,
        reason: &str,
        stamp: &str,
        audio: &[f32],
        features: &[FrameFeatures],
    ) -> std::io::Result<PathBuf> {
        std::fs::create_dir_all(&self.dir)?;
        let safe: String =
            reason.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' }).collect();
        let base = format!("{stamp}_{safe}");
        let wav_path = self.dir.join(format!("{base}.wav"));
        write_wav_16(&wav_path, audio, TARGET_RATE)?;
        let meta = ClipMeta {
            reason,
            sample_rate: TARGET_RATE,
            samples: audio.len(),
            frame_size: FRAME_SIZE,
            frames: features,
        };
        let json = serde_json::to_vec_pretty(&meta).map_err(std::io::Error::other)?;
        std::fs::write(self.dir.join(format!("{base}.json")), json)?;
        Ok(wav_path)
    }
}

/// Minimal mono 16-bit PCM WAV writer.
pub fn write_wav_16(path: &Path, samples: &[f32], rate: u32) -> std::io::Result<()> {
    let data_len = (samples.len() * 2) as u32;
    let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
    f.write_all(b"RIFF")?;
    f.write_all(&(36 + data_len).to_le_bytes())?;
    f.write_all(b"WAVEfmt ")?;
    f.write_all(&16u32.to_le_bytes())?;
    f.write_all(&1u16.to_le_bytes())?; // PCM
    f.write_all(&1u16.to_le_bytes())?; // mono
    f.write_all(&rate.to_le_bytes())?;
    f.write_all(&(rate * 2).to_le_bytes())?;
    f.write_all(&2u16.to_le_bytes())?;
    f.write_all(&16u16.to_le_bytes())?;
    f.write_all(b"data")?;
    f.write_all(&data_len.to_le_bytes())?;
    for &s in samples {
        f.write_all(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes())?;
    }
    f.flush()
}

// ---------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f32, rate: u32, secs: f32, amp: f32) -> Vec<f32> {
        (0..(rate as f32 * secs) as usize)
            .map(|i| amp * (2.0 * PI * freq * i as f32 / rate as f32).sin())
            .collect()
    }

    fn noise(n: usize, amp: f32, seed: u32) -> Vec<f32> {
        let mut s = seed;
        (0..n)
            .map(|_| {
                s = s.wrapping_mul(1664525).wrapping_add(1013904223);
                amp * ((s >> 8) as f32 / (1 << 23) as f32 - 1.0)
            })
            .collect()
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|s| s * s).sum::<f32>() / x.len() as f32).sqrt()
    }

    fn bp_gain(freq: f32, rate: u32) -> f32 {
        let mut bp = BandPass::new(rate as f32, 250.0, 4000.0);
        let out: Vec<f32> = sine(freq, rate, 0.5, 1.0).iter().map(|&x| bp.process(x)).collect();
        rms(&out[out.len() / 2..]) / std::f32::consts::FRAC_1_SQRT_2
    }

    #[test]
    fn bandpass_passes_mid_attenuates_edges() {
        assert!(bp_gain(1000.0, 48000) > 0.9);
        assert!(bp_gain(50.0, 48000) < 0.05, "low rejection");
        assert!(bp_gain(12000.0, 48000) < 0.05, "high rejection / anti-alias");
    }

    #[test]
    fn antialias_suppresses_above_nyquist_before_decimation() {
        // 10kHz at 48k would alias to 6kHz at 16k without filtering.
        let mut fe = FrontEnd::new(48000, FrontEndConfig::default(), Box::new(ConstVad(0.0)));
        let frames = fe.process(&sine(10000.0, 48000, 1.0, 0.5));
        let tail = &frames[frames.len() / 2..];
        assert!(tail.iter().all(|(_, f)| f.band_db < -40.0), "aliased energy leaked");
    }

    #[test]
    fn resampler_frame_count_and_tone() {
        let mut r = StreamResampler::new(48000);
        let mut out = Vec::new();
        for c in sine(500.0, 48000, 1.0, 1.0).chunks(480) {
            r.process(c, &mut out);
        }
        assert!((out.len() as i64 - 16000).abs() <= 1, "{}", out.len());
        assert!((rms(&out) - 0.707).abs() < 0.02);
    }

    #[test]
    fn noise_floor_tracks_and_ignores_bursts() {
        let mut nf = NoiseFloor::new();
        for _ in 0..200 {
            nf.update(-50.0);
        }
        assert!((nf.floor_db() + 50.0).abs() < 0.5);
        for _ in 0..62 {
            nf.update(-10.0);
        }
        assert!(nf.floor_db() < -49.0, "{}", nf.floor_db());
        for _ in 0..300 {
            nf.update(-70.0);
        }
        assert!(nf.floor_db() < -69.0);
    }

    #[test]
    fn tone_burst_shows_above_floor_and_pitch() {
        let mut fe = FrontEnd::new(16000, FrontEndConfig::default(), Box::new(ConstVad(0.9)));
        fe.set_playback_active(true);
        // 1s of quiet noise then 0.5s 440Hz burst
        let mut sig = noise(16000, 0.001, 1);
        let n2 = noise(8000, 0.001, 2);
        sig.extend(sine(440.0, 16000, 0.5, 0.3).iter().zip(n2).map(|(a, b)| a + b));
        let frames = fe.process(&sig);
        let quiet = &frames[40].1;
        let loud = &frames[frames.len() - 5].1;
        assert!(quiet.db_above_floor.abs() < 3.0, "{}", quiet.db_above_floor);
        assert!(loud.db_above_floor > 30.0, "{}", loud.db_above_floor);
        assert!((loud.pitch_hz - 440.0).abs() < 15.0, "{}", loud.pitch_hz);
        assert!(loud.pitch_conf > 0.9);
        assert!((loud.spectral_centroid_hz - 440.0).abs() < 100.0);
        assert!(loud.spectral_flatness < 0.1);
        assert!(loud.vad_prob > 0.8 && loud.playback_active);
    }

    #[test]
    fn white_noise_is_flat_and_unpitched() {
        let (c, _tilt, flat) = spectral_features(&noise(FRAME_SIZE, 0.3, 7));
        assert!(flat > 0.4, "{flat}");
        assert!(c > 3000.0 && c < 5000.0, "{c}");
        let (_, conf) = pitch_autocorr(&noise(FRAME_SIZE, 0.3, 9));
        assert!(conf < 0.5);
    }

    #[test]
    fn ring_buffer_wraps() {
        let mut r = RingBuffer::new(4);
        r.push_slice(&[1, 2, 3, 4, 5, 6]);
        assert_eq!(r.snapshot(), vec![3, 4, 5, 6]);
        assert_eq!(r.last_n(2), vec![5, 6]);
        assert_eq!(r.last_n(10).len(), 4);
    }

    #[test]
    fn frontend_ring_holds_five_seconds() {
        let mut fe = FrontEnd::new(16000, FrontEndConfig::default(), Box::new(ConstVad(0.0)));
        fe.process(&sine(440.0, 16000, 7.0, 0.2));
        assert_eq!(fe.audio_ring().len(), fe.audio_ring().capacity());
        assert_eq!(fe.audio_ring().capacity(), 80000);
        let (a, f) = fe.snapshot_clip(2.0);
        assert_eq!(a.len(), 32000);
        assert_eq!(f.len(), 125);
    }

    #[test]
    fn clip_recorder_writes_wav_and_json() {
        let dir = std::env::temp_dir().join(format!("sw_clip_test_{}", std::process::id()));
        let mut fe = FrontEnd::new(16000, FrontEndConfig::default(), Box::new(ConstVad(0.5)));
        fe.process(&sine(440.0, 16000, 1.0, 0.2));
        let (a, f) = fe.snapshot_clip(1.0);
        let rec = ClipRecorder::new(&dir);
        let p = rec.record("shout/test", "20260101T000000", &a, &f).unwrap();
        let wav = std::fs::read(&p).unwrap();
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(wav.len(), 44 + a.len() * 2);
        assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), 16000);
        let json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(p.with_extension("json")).unwrap()).unwrap();
        assert_eq!(json["frames"].as_array().unwrap().len(), f.len());
        assert_eq!(json["reason"], "shout/test");
        std::fs::remove_dir_all(dir).ok();
    }
}
