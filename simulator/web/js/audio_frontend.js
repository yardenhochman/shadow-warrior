// Port of brain-rs/src/services/audio_frontend.rs (pure DSP front-end).
// mono @ device rate -> band-pass (250 Hz - 4 kHz) -> linear resample to 16 kHz
// -> 256-sample (16 ms) frames -> features + adaptive noise floor.
// Only the VAD is not portable (earshot is a neural model): see vadProxy().

export const TARGET_RATE = 16000;
export const FRAME_SIZE = 256;
export const FRAME_SECS = FRAME_SIZE / TARGET_RATE;

export class Biquad {
  constructor(b0, b1, b2, a0, a1, a2) {
    this.b0 = b0 / a0; this.b1 = b1 / a0; this.b2 = b2 / a0; this.a1 = a1 / a0; this.a2 = a2 / a0;
    this.z1 = 0; this.z2 = 0;
  }
  static lowpass(fs, fc, q) {
    const w = 2 * Math.PI * fc / fs, s = Math.sin(w), c = Math.cos(w), alpha = s / (2 * q);
    return new Biquad((1 - c) / 2, 1 - c, (1 - c) / 2, 1 + alpha, -2 * c, 1 - alpha);
  }
  static highpass(fs, fc, q) {
    const w = 2 * Math.PI * fc / fs, s = Math.sin(w), c = Math.cos(w), alpha = s / (2 * q);
    return new Biquad((1 + c) / 2, -(1 + c), (1 + c) / 2, 1 + alpha, -2 * c, 1 - alpha);
  }
  process(x) {
    const y = this.b0 * x + this.z1;
    this.z1 = this.b1 * x - this.a1 * y + this.z2;
    this.z2 = this.b2 * x - this.a2 * y;
    return y;
  }
}

/** 4th-order Butterworth high-pass + 4th-order low-pass cascade. */
export class BandPass {
  constructor(fs, lowHz, highHz) {
    const QS = [0.5412, 1.3066];
    const high = Math.min(highHz, fs * 0.45);
    this.stages = [...QS.map((q) => Biquad.highpass(fs, lowHz, q)), ...QS.map((q) => Biquad.lowpass(fs, high, q))];
  }
  process(x) { for (const s of this.stages) x = s.process(x); return x; }
}

export class StreamResampler {
  constructor(inputRate) { this.step = inputRate / TARGET_RATE; this.pos = 0; this.prev = 0; }
  process(input, out) {
    if (Math.abs(this.step - 1) < 1e-9) { for (const v of input) out.push(v); return; }
    const n = input.length;
    while (this.pos < n) {
      const i = Math.floor(this.pos);
      const frac = this.pos - i;
      const a = i < 1 ? this.prev : input[i - 1];
      const b = input[i];
      out.push(a + (b - a) * frac);
      this.pos += this.step;
    }
    this.pos -= n;
    if (n) this.prev = input[n - 1];
  }
}

/** Asymmetric tracker: follows drops fast (1 s), rises slowly (0.5 dB/s). */
export class NoiseFloor {
  constructor(riseDbPerSec = 0.5, fallTimeS = 1.0) {
    this.floorDb = -80; this.initialised = false;
    this.risePerFrame = riseDbPerSec * FRAME_SECS;
    this.fallCoeff = Math.min(FRAME_SECS / fallTimeS, 1);
  }
  update(bandDb) {
    if (!this.initialised) { this.floorDb = bandDb; this.initialised = true; }
    else if (bandDb < this.floorDb) this.floorDb += (bandDb - this.floorDb) * this.fallCoeff;
    else this.floorDb += Math.min(bandDb - this.floorDb, this.risePerFrame);
    return this.floorDb;
  }
}

export function rmsDb(frame) {
  if (!frame.length) return -100;
  let ms = 0; for (const s of frame) ms += s * s; ms /= frame.length;
  return ms > 1e-10 ? 10 * Math.log10(ms) : -100;
}

function fft(re, im) {
  const n = re.length;
  for (let i = 1, j = 0; i < n; i++) {
    let bit = n >> 1;
    while (j & bit) { j ^= bit; bit >>= 1; }
    j |= bit;
    if (i < j) { [re[i], re[j]] = [re[j], re[i]]; [im[i], im[j]] = [im[j], im[i]]; }
  }
  for (let len = 2; len <= n; len <<= 1) {
    const ang = -2 * Math.PI / len, wr = Math.cos(ang), wi = Math.sin(ang);
    for (let start = 0; start < n; start += len) {
      let cr = 1, ci = 0;
      for (let k = 0; k < len / 2; k++) {
        const a = start + k, b = start + k + len / 2;
        const tr = re[b] * cr - im[b] * ci, ti = re[b] * ci + im[b] * cr;
        re[b] = re[a] - tr; im[b] = im[a] - ti; re[a] += tr; im[a] += ti;
        const ncr = cr * wr - ci * wi; ci = cr * wi + ci * wr; cr = ncr;
      }
    }
  }
}

const HANN = Float64Array.from({ length: FRAME_SIZE }, (_, i) => 0.5 - 0.5 * Math.cos(2 * Math.PI * i / FRAME_SIZE));

/** Returns [centroidHz, tiltDbPerKhz, flatness] from a Hann-windowed FFT. */
export function spectralFeatures(frame) {
  const n = frame.length;
  const re = new Float64Array(n), im = new Float64Array(n);
  for (let i = 0; i < n; i++) re[i] = frame[i] * HANN[i];
  fft(re, im);
  const binHz = TARGET_RATE / n;
  const m = n / 2 - 1;
  let total = 0, cnum = 0, logSum = 0, sf = 0, sd = 0;
  const db = new Float64Array(m), fk = new Float64Array(m);
  for (let k = 1; k < n / 2; k++) {
    const p = re[k] * re[k] + im[k] * im[k] + 1e-12, f = k * binHz;
    total += p; cnum += p * f; logSum += Math.log(p);
    db[k - 1] = 10 * Math.log10(p); fk[k - 1] = f / 1000;
    sf += fk[k - 1]; sd += db[k - 1];
  }
  const centroid = cnum / total;
  const flatness = Math.min(Math.max(Math.exp(logSum / m) / (total / m), 0), 1);
  const mf = sf / m, md = sd / m;
  let num = 0, den = 0;
  for (let i = 0; i < m; i++) { num += (fk[i] - mf) * (db[i] - md); den += (fk[i] - mf) ** 2; }
  return [centroid, num / den, flatness];
}

/** Autocorrelation pitch over 70..500 Hz. Returns [f0Hz, confidence]. */
export function pitchAutocorr(frame) {
  let r0 = 0; for (const s of frame) r0 += s * s;
  if (r0 < 1e-9) return [0, 0];
  const minLag = Math.floor(TARGET_RATE / 500);
  const maxLag = Math.min(Math.floor(TARGET_RATE / 70), frame.length - 1);
  const nrs = [];
  for (let lag = minLag; lag <= maxLag; lag++) {
    let r = 0, e1 = 0, e2 = 0;
    for (let i = 0; i + lag < frame.length; i++) {
      const x = frame[i], y = frame[i + lag];
      r += x * y; e1 += x * x; e2 += y * y;
    }
    nrs.push([lag, r / Math.max(Math.sqrt(e1 * e2), 1e-9)]);
  }
  const maxNr = nrs.reduce((m, x) => Math.max(m, x[1]), 0);
  let best = [0, 0];
  for (let i = 1; i < nrs.length - 1; i++) {
    const [lag, nr] = nrs[i];
    if (nr >= 0.9 * maxNr && nr >= nrs[i - 1][1] && nr >= nrs[i + 1][1]) { best = [lag, nr]; break; }
  }
  if (best[0] === 0 || best[1] < 0.1) return [0, Math.max(best[1], 0)];
  return [TARGET_RATE / best[0], Math.min(Math.max(best[1], 0), 1)];
}

/**
 * SIM PROPOSAL: stand-in for earshot's neural VAD (not portable to the browser).
 * Voicing (pitch confidence) gated by level above floor and low flatness.
 */
export function vadProxy(pitchConf, dbAboveFloor, flatness) {
  const clamp = (x) => Math.min(Math.max(x, 0), 1);
  const voiced = clamp((pitchConf - 0.2) / 0.4);
  const loud = clamp((dbAboveFloor - 2) / 8);
  const tonal = clamp((0.75 - flatness) / 0.4);
  return voiced * (0.35 + 0.65 * loud) * (0.5 + 0.5 * tonal);
}

export class FrontEnd {
  constructor(inputRate, { lowHz = 250, highHz = 4000 } = {}) {
    this.inputRate = inputRate;
    this.bandpass = new BandPass(inputRate, lowHz, highHz);
    this.resampler = new StreamResampler(inputRate);
    this.floor = new NoiseFloor();
    this.pending = [];
    this.frameIndex = 0;
    this.playbackActive = false;
  }
  setPlaybackActive(a) { this.playbackActive = a; }
  resetFloor() { this.floor = new NoiseFloor(); }

  /** Feed mono samples at device rate; returns FrameFeatures for each completed frame. */
  process(mono) {
    const filtered = new Array(mono.length);
    for (let i = 0; i < mono.length; i++) filtered[i] = this.bandpass.process(mono[i]);
    this.resampler.process(filtered, this.pending);
    const out = [];
    while (this.pending.length >= FRAME_SIZE) {
      const frame = this.pending.splice(0, FRAME_SIZE);
      out.push(this.analyse(frame));
    }
    return out;
  }

  analyse(frame) {
    const bandDb = rmsDb(frame);
    const floor = this.floor.update(bandDb);
    const [centroid, tilt, flat] = spectralFeatures(frame);
    const [pitchHz, pitchConf] = pitchAutocorr(frame);
    const f = {
      index: this.frameIndex++,
      band_db: bandDb,
      noise_floor_db: floor,
      db_above_floor: bandDb - floor,
      vad_prob: vadProxy(pitchConf, bandDb - floor, flat),
      pitch_hz: pitchHz,
      pitch_conf: pitchConf,
      spectral_centroid_hz: centroid,
      spectral_tilt_db_per_khz: tilt,
      spectral_flatness: flat,
      playback_active: this.playbackActive,
    };
    return f;
  }
}
