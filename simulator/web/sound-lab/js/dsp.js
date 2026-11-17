// Shared DSP for the Sound Lab: seeded randomness, envelopes, noise, generated reverbs,
// loudness/peak metering (BS.1770 K-weighting), a WSOLA pitch shifter, WAV and zip writers.
// Everything takes an (Offline)AudioContext so a sound can be rendered offline, measured,
// and the very same buffer played back.

export const SR = 48000;

/** mulberry32: deterministic 0..1 generator, so "alternate B" is always the same B. */
export function rng(seed = 1) {
  let a = (seed * 2654435761) >>> 0;
  return () => { a = (a + 0x6d2b79f5) >>> 0; let t = a; t = Math.imul(t ^ (t >>> 15), t | 1); t ^= t + Math.imul(t ^ (t >>> 7), t | 61); return ((t ^ (t >>> 14)) >>> 0) / 4294967296; };
}
export const lerp = (a, b, x) => a + (b - a) * x;
export const clamp = (x, a = 0, b = 1) => Math.min(b, Math.max(a, x));

const noiseCache = new WeakMap();
/** 2 s of white (or pink/brown) noise, cached per context. */
export function noiseBuffer(c, color = 'white') {
  let m = noiseCache.get(c); if (!m) { m = {}; noiseCache.set(c, m); }
  if (m[color]) return m[color];
  const n = c.sampleRate * 2, b = c.createBuffer(1, n, c.sampleRate), d = b.getChannelData(0), r = rng(color.length * 7 + 3);
  let b0 = 0, b1 = 0, b2 = 0, last = 0;
  for (let i = 0; i < n; i++) {
    const w = r() * 2 - 1;
    if (color === 'pink') { b0 = 0.997 * b0 + w * 0.029591; b1 = 0.985 * b1 + w * 0.032534; b2 = 0.95 * b2 + w * 0.048056; d[i] = (b0 + b1 + b2 + w * 0.05) * 2.2; }
    else if (color === 'brown') { last = (last + 0.02 * w) / 1.02; d[i] = last * 3.5; }
    else d[i] = w;
  }
  return (m[color] = b);
}

/** Exponential attack/decay envelope on an AudioParam. */
export function env(p, t, a, peak, dec, floor = 0.0001) {
  p.setValueAtTime(floor, t); p.exponentialRampToValueAtTime(Math.max(floor * 1.01, peak), t + Math.max(0.0005, a));
  p.exponentialRampToValueAtTime(floor, t + a + Math.max(0.002, dec));
}

/** A looping noise source started at t for dur seconds, at a random offset. */
export function noise(c, t, dur, color = 'white', r = Math.random) {
  const n = c.createBufferSource(); n.buffer = noiseBuffer(c, color); n.loop = true;
  n.start(t, r() * 1.9); n.stop(t + dur); return n;
}

/** Filtered noise burst: the workhorse for slaps, cracks, grains. */
export function noiseHit(c, dest, t, { type = 'bandpass', f = 1000, q = 1, a = 0.001, peak = 0.3, dec = 0.05, color = 'white', r } = {}) {
  const f1 = c.createBiquadFilter(); f1.type = type; f1.frequency.value = f; f1.Q.value = q;
  const g = c.createGain(); env(g.gain, t, a, peak, dec);
  noise(c, t, a + dec + 0.05, color, r).connect(f1).connect(g).connect(dest); return g;
}

/** Oscillator with envelope and optional pitch glide f -> f2. */
export function tone(c, dest, t, { type = 'sine', f = 440, f2 = null, glide = 0.1, a = 0.002, peak = 0.2, dec = 0.2, detune = 0 } = {}) {
  const o = c.createOscillator(), g = c.createGain(); o.type = type; o.detune.value = detune;
  o.frequency.setValueAtTime(f, t); if (f2) o.frequency.exponentialRampToValueAtTime(f2, t + glide);
  env(g.gain, t, a, peak, dec); o.connect(g).connect(dest); o.start(t); o.stop(t + a + dec + 0.05); return o;
}

/** Struck metal/glass/ceramic: a few inharmonic partials with their own decays. */
export function partials(c, dest, t, f, ratios, { peak = 0.1, dec = 0.3, r = Math.random, spread = 0.02 } = {}) {
  ratios.forEach((k, i) => tone(c, dest, t, { f: f * k * (1 + (r() - 0.5) * spread), peak: peak / (1 + i * 0.6), dec: dec / (1 + i * 0.35), a: 0.0008 }));
}

/** Tanh waveshaper. */
export function drive(c, amount = 3) {
  const w = c.createWaveShaper(), n = 1024, curve = new Float32Array(n);
  for (let i = 0; i < n; i++) { const x = (i / (n - 1)) * 2 - 1; curve[i] = Math.tanh(x * amount) / Math.tanh(amount); }
  w.curve = curve; w.oversample = '4x'; return w;
}

const irCache = new WeakMap();
/** Generated reverb impulse: decaying stereo-less noise, darkening over time. */
export function reverb(c, { sec = 2.2, decay = 2.5, bright = 0.6, pre = 0.01, seed = 5 } = {}) {
  let m = irCache.get(c); if (!m) { m = {}; irCache.set(c, m); }
  const key = [sec, decay, bright, pre, seed].join();
  if (!m[key]) {
    const n = Math.floor(sec * c.sampleRate), b = c.createBuffer(1, n, c.sampleRate), d = b.getChannelData(0), r = rng(seed);
    const p0 = Math.floor(pre * c.sampleRate); let s = 0;
    for (let i = p0; i < n; i++) {
      const tt = (i - p0) / c.sampleRate, lp = Math.exp(-2 * Math.PI * (800 + 9000 * bright * Math.exp(-tt * 1.5)) / c.sampleRate);
      s = (1 - lp) * (r() * 2 - 1) + lp * s; d[i] = s * Math.exp(-decay * tt);
    }
    let e = 0; for (let i = 0; i < n; i++) e += d[i] * d[i]; const k = 0.6 / Math.sqrt(e || 1); for (let i = 0; i < n; i++) d[i] *= k;
    m[key] = b;
  }
  const cv = c.createConvolver(); cv.normalize = false; cv.buffer = m[key]; return cv;
}

/** dest <- dry + wet send through a generated reverb; returns the input node. */
export function withReverb(c, dest, wet = 0.3, opts = {}) {
  const inp = c.createGain(), w = c.createGain(); w.gain.value = wet; const rv = reverb(c, opts);
  inp.connect(dest); inp.connect(rv).connect(w).connect(dest); return inp;
}

/** Render `build(ctx, dest, t0)` offline (mono, 48 kHz) to an AudioBuffer. */
export async function renderOffline(seconds, build, sr = SR) {
  const C = window.OfflineAudioContext || window.webkitOfflineAudioContext;
  const c = new C(1, Math.ceil(seconds * sr), sr);
  const out = c.createGain(); out.connect(c.destination);
  await build(c, out, 0.02);
  return c.startRendering();
}

// ---- metering ------------------------------------------------------------
function biquad(x, b, a) {
  const y = new Float32Array(x.length); let x1 = 0, x2 = 0, y1 = 0, y2 = 0;
  for (let i = 0; i < x.length; i++) { const v = b[0] * x[i] + b[1] * x1 + b[2] * x2 - a[1] * y1 - a[2] * y2; x2 = x1; x1 = x[i]; y2 = y1; y1 = v; y[i] = v; }
  return y;
}
/** K-weighting biquads for any rate (BS.1770 analogue prototypes via bilinear transform). */
function kCoeffs(fs) {
  // high shelf
  let f0 = 1681.974450955533, G = 3.999843853973347, Q = 0.7071752369554196;
  let K = Math.tan(Math.PI * f0 / fs), Vh = Math.pow(10, G / 20), Vb = Math.pow(Vh, 0.4996667741545416);
  let a0 = 1 + K / Q + K * K;
  const s = { b: [(Vh + Vb * K / Q + K * K) / a0, 2 * (K * K - Vh) / a0, (Vh - Vb * K / Q + K * K) / a0], a: [1, 2 * (K * K - 1) / a0, (1 - K / Q + K * K) / a0] };
  f0 = 38.13547087602444; Q = 0.5003270373238773; K = Math.tan(Math.PI * f0 / fs);
  const d = 1 + K / Q + K * K;
  const h = { b: [1, -2, 1], a: [1, 2 * (K * K - 1) / d, (1 - K / Q + K * K) / d] };
  return [s, h];
}
/**
 * {lufs: integrated (gated), peak: sample peak dBFS, tp: 4x-oversampled true peak dBTP}.
 * Clips shorter than one 400 ms block are padded with silence (so a 0.2 s jab still gets a number).
 */
export function measure(buf) {
  const fs = buf.sampleRate; let x = buf.getChannelData ? buf.getChannelData(0) : buf;
  const blk = Math.round(0.4 * fs), hop = Math.round(0.1 * fs);
  if (x.length < blk) { const y = new Float32Array(blk); y.set(x); x = y; }
  let peak = 0; for (let i = 0; i < x.length; i++) peak = Math.max(peak, Math.abs(x[i]));
  // true peak: 4x upsampling with a 32-tap windowed sinc
  let tp = peak; const taps = 16;
  for (let i = taps; i < x.length - taps; i++) {
    if (Math.abs(x[i]) < peak * 0.5) continue;
    for (let ph = 1; ph < 4; ph++) {
      const fr = ph / 4; let v = 0;
      for (let k = -taps + 1; k <= taps; k++) { const u = k - fr; const w = 0.5 + 0.5 * Math.cos(Math.PI * u / taps); v += x[i + k] * (u === 0 ? 1 : Math.sin(Math.PI * u) / (Math.PI * u)) * w; }
      tp = Math.max(tp, Math.abs(v));
    }
  }
  const [s, h] = kCoeffs(fs), y = biquad(biquad(x, s.b, s.a), h.b, h.a);
  const z = [];
  for (let i = 0; i + blk <= y.length; i += hop) { let e = 0; for (let j = i; j < i + blk; j++) e += y[j] * y[j]; z.push(e / blk); }
  const L = (v) => -0.691 + 10 * Math.log10(v || 1e-12);
  let g = z.filter((v) => L(v) > -70);
  const rel = g.length ? L(g.reduce((a, b) => a + b, 0) / g.length) - 10 : -70;
  g = g.filter((v) => L(v) > rel);
  const db = (v) => 20 * Math.log10(v || 1e-9);
  return { lufs: g.length ? L(g.reduce((a, b) => a + b, 0) / g.length) : -70, peak: db(peak), tp: db(tp) };
}

/** Scale a buffer in place so integrated loudness is `lufs` but true peak stays <= ceil. */
export function normalize(buf, lufs = -16, ceil = -1.5) {
  const m = measure(buf); let g = Math.pow(10, (lufs - m.lufs) / 20);
  g = Math.min(g, Math.pow(10, (ceil - m.tp) / 20));
  for (let ch = 0; ch < buf.numberOfChannels; ch++) { const d = buf.getChannelData(ch); for (let i = 0; i < d.length; i++) d[i] *= g; }
  return g;
}

// ---- pitch shifting (for the browser demon chain) --------------------------
/** WSOLA time-stretch: output ~ x.length * factor samples, pitch unchanged. */
export function stretch(x, factor, sr) {
  const N = Math.round(sr * 0.046) & ~1, Hs = N >> 1, Ha = Hs / factor, tol = Math.round(sr * 0.012);
  const win = new Float32Array(N); for (let i = 0; i < N; i++) win[i] = 0.5 - 0.5 * Math.cos(2 * Math.PI * i / N);
  const outLen = Math.ceil(x.length * factor) + N, y = new Float32Array(outLen), wsum = new Float32Array(outLen);
  let prev = 0;
  for (let k = 0; ; k++) {
    const nominal = Math.round(k * Ha), o = k * Hs;
    if (nominal + N + tol >= x.length || o + N >= outLen) break;
    let pos = nominal;
    if (k > 0) {
      const nat = prev + Hs; let best = -Infinity;
      for (let d = -tol; d <= tol; d += 2) {
        const p = nominal + d; if (p < 0) continue; let cc = 0;
        for (let i = 0; i < N; i += 4) cc += x[p + i] * (x[nat + i] || 0);
        if (cc > best) { best = cc; pos = p; }
      }
    }
    for (let i = 0; i < N; i++) { y[o + i] += x[pos + i] * win[i]; wsum[o + i] += win[i]; }
    prev = pos;
  }
  for (let i = 0; i < outLen; i++) if (wsum[i] > 1e-3) y[i] /= wsum[i];
  return y.subarray(0, Math.ceil(x.length * factor));
}
/** Pitch shift by `ratio` keeping duration (formants move with the pitch: good for demons). */
export function pitchShift(x, ratio, sr) {
  const n = Math.floor(x.length / ratio), r = new Float32Array(n);
  for (let i = 0; i < n; i++) { const p = i * ratio, j = Math.floor(p), f = p - j; r[i] = (x[j] || 0) * (1 - f) + (x[j + 1] || 0) * f; }
  return stretch(r, ratio, sr);
}

// ---- files ------------------------------------------------------------------
/** 16-bit PCM mono WAV. */
export function wavBytes(x, sr) {
  const n = x.length, b = new DataView(new ArrayBuffer(44 + n * 2));
  const w = (o, s) => { for (let i = 0; i < s.length; i++) b.setUint8(o + i, s.charCodeAt(i)); };
  w(0, 'RIFF'); b.setUint32(4, 36 + n * 2, true); w(8, 'WAVE'); w(12, 'fmt '); b.setUint32(16, 16, true); b.setUint16(20, 1, true); b.setUint16(22, 1, true);
  b.setUint32(24, sr, true); b.setUint32(28, sr * 2, true); b.setUint16(32, 2, true); b.setUint16(34, 16, true); w(36, 'data'); b.setUint32(40, n * 2, true);
  for (let i = 0; i < n; i++) b.setInt16(44 + i * 2, Math.max(-1, Math.min(1, x[i])) * 32767, true);
  return new Uint8Array(b.buffer);
}
const crcT = (() => { const t = new Uint32Array(256); for (let n = 0; n < 256; n++) { let c = n; for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; t[n] = c >>> 0; } return t; })();
const crc32 = (d) => { let c = 0xffffffff; for (let i = 0; i < d.length; i++) c = crcT[(c ^ d[i]) & 0xff] ^ (c >>> 8); return (c ^ 0xffffffff) >>> 0; };
/** Minimal store-only zip of [{name, data: Uint8Array}]. */
export function zipBlob(files) {
  const enc = new TextEncoder(), parts = [], central = []; let off = 0;
  for (const f of files) {
    const name = enc.encode(f.name), crc = crc32(f.data), h = new DataView(new ArrayBuffer(30));
    h.setUint32(0, 0x04034b50, true); h.setUint16(4, 20, true); h.setUint16(6, 0x0800, true); h.setUint32(14, crc, true);
    h.setUint32(18, f.data.length, true); h.setUint32(22, f.data.length, true); h.setUint16(26, name.length, true);
    parts.push(new Uint8Array(h.buffer), name, f.data);
    const c = new DataView(new ArrayBuffer(46));
    c.setUint32(0, 0x02014b50, true); c.setUint16(4, 20, true); c.setUint16(6, 20, true); c.setUint16(8, 0x0800, true); c.setUint32(16, crc, true);
    c.setUint32(20, f.data.length, true); c.setUint32(24, f.data.length, true); c.setUint16(28, name.length, true); c.setUint32(42, off, true);
    central.push(new Uint8Array(c.buffer), name);
    off += 30 + name.length + f.data.length;
  }
  const size = central.reduce((a, p) => a + p.length, 0), e = new DataView(new ArrayBuffer(22));
  e.setUint32(0, 0x06054b50, true); e.setUint16(8, files.length, true); e.setUint16(10, files.length, true); e.setUint32(12, size, true); e.setUint32(16, off, true);
  return new Blob([...parts, ...central, new Uint8Array(e.buffer)], { type: 'application/zip' });
}
export function download(blob, name) {
  const a = document.createElement('a'); a.href = URL.createObjectURL(blob); a.download = name; document.body.appendChild(a); a.click();
  setTimeout(() => { URL.revokeObjectURL(a.href); a.remove(); }, 1000);
}
