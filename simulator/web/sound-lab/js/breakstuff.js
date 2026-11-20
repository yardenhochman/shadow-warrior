// "Break Stuff mode": while the fight track is Limp Bizkit's "Break Stuff", some punches also
// smash an object, sized by power. All sounds are synthesized here (no samples).
//
// Wiring into the simulator later (nothing here touches it):
//   1. Detect the track. music.js Playlist keeps `this.current = {title, url}`; titles come from
//      playlist.tsv or the file name (music/08-limp-bizkit-break-stuff.mp3 -> "limp bizkit break stuff").
//      `isBreakStuffTrack(playlist.current)` is true for either.
//   2. In the punch path (where sfx.punch(power, sig) is called), when the track matches:
//        const b = breakRule.decide(power, { now: performance.now() / 1000, combo });
//        if (b) buildBreak(b.kind, ctx, breakBus, ctx.currentTime + 0.02, b.alt, b.gain);
//      with breakBus = a gain of BREAK_BUS_GAIN into the master, so breakage sits under the music.
import { rng, env, noise, noiseHit, tone, partials, withReverb } from './dsp.js';
import { tierOf } from './impacts.js';

export const isBreakStuffTrack = (track) => /break\W*stuff/i.test(`${track?.title || ''} ${track?.url || ''}`);
export const BREAK_BUS_GAIN = 0.5; // ~ -6 dB: breakage sits under the music and the bag hit

/**
 * When does a punch break something?
 *   chance by tier, rising with power inside the tier: jab ~3-7 %, hook ~14-26 %, heavy ~40-60 %;
 *   always on a new max-power hit (>= hook) or a combo milestone (5, 10, 15, ...);
 *   at most one break per `cooldown` seconds (forced ones too), never the same object twice in a row.
 */
export class BreakRule {
  constructor({ rand = Math.random, cooldown = 2, chance = { jab: 0.05, hook: 0.2, heavy: 0.5 }, milestoneEvery = 5 } = {}) {
    Object.assign(this, { rand, cooldown, chance, milestoneEvery }); this.reset();
  }
  reset() { this.last = -Infinity; this.maxPower = 0; this.lastKind = null; this.lastAlt = {}; }
  decide(power, { now, combo = 1 } = {}) {
    const tier = tierOf(power), newMax = power > this.maxPower + 3 && power >= 45;
    this.maxPower = Math.max(this.maxPower, power);
    const milestone = combo > 1 && combo % this.milestoneEvery === 0;
    if (now - this.last < this.cooldown) return null;
    const lo = { jab: 1, hook: 45, heavy: 75 }[tier], hi = { jab: 44, hook: 74, heavy: 100 }[tier];
    const p = this.chance[tier] * (0.6 + 0.8 * (power - lo) / (hi - lo));
    const reason = newMax ? 'new max' : milestone ? `combo ${combo}` : this.rand() < p ? `chance ${(p * 100) | 0}%` : null;
    if (!reason) return null;
    const size = { jab: 'small', hook: 'medium', heavy: 'big' }[tier];
    const kinds = BREAKS.filter((b) => b.size === size && b.id !== this.lastKind);
    const kind = kinds[Math.floor(this.rand() * kinds.length)];
    let alt = Math.floor(this.rand() * kind.alts); if (alt === this.lastAlt[kind.id] && kind.alts > 1) alt = (alt + 1) % kind.alts;
    this.last = now; this.lastKind = kind.id; this.lastAlt[kind.id] = alt;
    return { kind: kind.id, size, alt, gain: 0.6 + 0.4 * power / 100, reason };
  }
}

// ---- primitives ----
/** Falling shards: n glassy pings spread over `spread` s with gravity-like clustering. */
function shards(c, dest, t, n, spread, { lo = 2500, hi = 7500, peak = 0.05, r }) {
  for (let i = 0; i < n; i++) {
    const dt = Math.pow(r(), 1.6) * spread, k = 1 - 0.6 * dt / spread;
    partials(c, dest, t + dt, lo + r() * (hi - lo), [1, 1.53, 2.37], { peak: peak * k * (0.4 + r()), dec: 0.03 + 0.08 * r(), r, spread: 0.1 });
    if (r() < 0.4) noiseHit(c, dest, t + dt, { type: 'highpass', f: 4000, peak: peak * 0.6 * k, dec: 0.004, r });
  }
}
function crack(c, dest, t, peak = 0.4, f = 2500, r) {
  noiseHit(c, dest, t, { type: 'highpass', f, peak, dec: 0.008, r }); noiseHit(c, dest, t + 0.001, { type: 'bandpass', f: f * 0.6, q: 2, peak: peak * 0.6, dec: 0.03, r });
}
function thud(c, dest, t, peak = 0.4, f = 90) { tone(c, dest, t, { f, f2: f * 0.45, glide: 0.12, peak, dec: 0.18 }); noiseHit(c, dest, t, { type: 'lowpass', f: 500, peak: peak * 0.5, dec: 0.08 }); }
function splinter(c, dest, t, n, dur, { f = 1400, peak = 0.12, r }) {
  for (let i = 0; i < n; i++) { const dt = dur * Math.pow(i / n, 0.7) + r() * 0.01; noiseHit(c, dest, t + dt, { type: 'bandpass', f: f * (0.6 + r() * 0.9), q: 4, peak: peak * (0.4 + 0.6 * r()) * (1 - 0.5 * i / n), dec: 0.006 + 0.012 * r(), r }); }
}
function creak(c, dest, t, dur = 0.35) {
  const o = c.createOscillator(), f = c.createBiquadFilter(), g = c.createGain(); o.type = 'sawtooth'; o.frequency.setValueAtTime(70, t); o.frequency.linearRampToValueAtTime(45, t + dur);
  f.type = 'bandpass'; f.frequency.value = 900; f.Q.value = 6; env(g.gain, t, 0.03, 0.08, dur); o.connect(f).connect(g).connect(dest); o.start(t); o.stop(t + dur + 0.05);
}

// ---- the objects: build(c, dest, t, r) ----
export const BREAKS = [
  { id: 'glass', size: 'small', name: 'Glass clink', alts: 3, build(c, d, t, r) { partials(c, d, t, 2900 + r() * 900, [1, 2.32, 4.1], { peak: 0.09, dec: 0.5, r }); crack(c, d, t + 0.03, 0.07, 5000, r); shards(c, d, t + 0.04, 3, 0.15, { peak: 0.03, r }); } },
  { id: 'cup', size: 'small', name: 'Cup crack', alts: 3, build(c, d, t, r) { crack(c, d, t, 0.25, 2000, r); partials(c, d, t, 1500 + r() * 500, [1, 2.6, 4.4], { peak: 0.05, dec: 0.12, r }); shards(c, d, t + 0.02, 4, 0.2, { lo: 1500, hi: 4000, peak: 0.03, r }); } },
  { id: 'twig', size: 'small', name: 'Twig snap', alts: 3, build(c, d, t, r) { crack(c, d, t, 0.3, 2600, r); crack(c, d, t + 0.018 + r() * 0.01, 0.2, 1300, r); splinter(c, d, t + 0.03, 6, 0.08, { f: 2000, peak: 0.05, r }); } },
  { id: 'plate', size: 'medium', name: 'Plate smash', alts: 3, build(c, d, t, r) { thud(c, d, t, 0.2, 140); crack(c, d, t, 0.4, 2200, r); partials(c, d, t, 1800 + r() * 400, [1, 2.4, 3.9, 5.6], { peak: 0.08, dec: 0.15, r }); shards(c, d, t + 0.02, 22, 0.6, { lo: 1400, hi: 5000, peak: 0.05, r }); } },
  { id: 'wood', size: 'medium', name: 'Wood splinter', alts: 3, build(c, d, t, r) { creak(c, d, t); crack(c, d, t + 0.3, 0.45, 1500, r); thud(c, d, t + 0.3, 0.3, 110); splinter(c, d, t + 0.31, 28, 0.35, { f: 1300, peak: 0.12, r }); } },
  { id: 'bottle', size: 'medium', name: 'Bottle break', alts: 3, build(c, d, t, r) { noiseHit(c, d, t, { type: 'bandpass', f: 800 + r() * 200, q: 18, peak: 0.25, dec: 0.25, r }); crack(c, d, t, 0.35, 3000, r); shards(c, d, t + 0.03, 18, 0.5, { lo: 2000, hi: 6500, peak: 0.05, r }); } },
  { id: 'window', size: 'big', name: 'Window shatter', alts: 3, build(c, d, t, r) { const b = withReverb(c, d, 0.25, { sec: 1.5, decay: 3 }); thud(c, b, t, 0.35, 80); crack(c, b, t, 0.6, 2000, r); shards(c, b, t + 0.01, 90, 1.4, { peak: 0.055, r }); } },
  { id: 'tv', size: 'big', name: 'TV implosion', alts: 2, build(c, d, t, r) {
      const b = withReverb(c, d, 0.25, { sec: 1.5, decay: 3 });
      tone(c, b, t - 0.0, { f: 11800, peak: 0.015, dec: 0.15 }); // tube whine, cut
      const s = c.createBiquadFilter(); s.type = 'lowpass'; s.frequency.setValueAtTime(200, t); s.frequency.exponentialRampToValueAtTime(3000, t + 0.08);
      const g = c.createGain(); g.gain.setValueAtTime(0.0001, t); g.gain.exponentialRampToValueAtTime(0.22, t + 0.07); g.gain.exponentialRampToValueAtTime(0.0001, t + 0.35); noise(c, t, 0.4, 'pink', r).connect(s).connect(g).connect(b); // inward whump
      thud(c, b, t + 0.07, 0.3, 60); shards(c, b, t + 0.08, 50, 0.9, { lo: 1800, hi: 6000, peak: 0.05, r });
      for (let i = 0; i < 18; i++) noiseHit(c, b, t + 0.1 + r() * 0.7, { type: 'bandpass', f: 3200, q: 3, peak: 0.06, dec: 0.004, r }); // arcing
      const hum = c.createOscillator(), hg = c.createGain(); hum.type = 'sawtooth'; hum.frequency.value = 50; env(hg.gain, t + 0.1, 0.01, 0.04, 0.8); hum.connect(hg).connect(b); hum.start(t + 0.1); hum.stop(t + 1); } },
  { id: 'table', size: 'big', name: 'Table crash', alts: 3, build(c, d, t, r) {
      const b = withReverb(c, d, 0.2, { sec: 1.2, decay: 3.5 }); crack(c, b, t, 0.5, 1200, r); thud(c, b, t, 0.55, 75); splinter(c, b, t + 0.01, 30, 0.4, { f: 1100, peak: 0.12, r });
      [0.28, 0.46, 0.58, 0.66].forEach((dt, i) => thud(c, b, t + dt, 0.35 / (i + 1), 95 + 10 * i)); // legs and top bouncing
      for (let i = 0; i < 8; i++) partials(c, b, t + 0.3 + r() * 0.5, 2400 + r() * 1500, [1, 1.8, 2.9], { peak: 0.03, dec: 0.08, r }); } }, // cutlery
  { id: 'windshield', size: 'big', name: 'Car windshield', alts: 2, build(c, d, t, r) {
      const b = withReverb(c, d, 0.15, { sec: 1, decay: 4 }); thud(c, b, t, 0.45, 70); crack(c, b, t, 0.5, 1600, r);
      for (let i = 0; i < 140; i++) { const dt = Math.pow(r(), 0.9) * 0.5; noiseHit(c, b, t + dt, { type: 'bandpass', f: 1500 + r() * 2500, q: 2, peak: 0.07 * (1 - dt), dec: 0.003 + 0.004 * r(), r }); } // laminated crunch
      [0.75, 1.0, 1.25].forEach((dt) => { const o = c.createOscillator(), g = c.createGain(); o.type = 'square'; o.frequency.setValueAtTime(900, t + dt); o.frequency.linearRampToValueAtTime(1500, t + dt + 0.2); env(g.gain, t + dt, 0.01, 0.03, 0.2); o.connect(g).connect(b); o.start(t + dt); o.stop(t + dt + 0.25); }); } }, // alarm chirps
];

const SIZE_GAIN = { small: 2.4, medium: 1, big: 1 }; // small objects are quiet by nature; lift them so they still read
export function buildBreak(kind, c, dest, t, alt = 0, gain = 1) {
  const b = BREAKS.find((x) => x.id === kind), g = c.createGain(); g.gain.value = gain * SIZE_GAIN[b.size]; g.connect(dest);
  b.build(c, g, t, rng(1000 + alt * 97 + kind.length));
}
// Live game (Yarden 20.11: "can barely hear the breaks"): the song is mastered at ~-6 LUFS, the raw objects sat
// 25-35 dB under it. LIVE_GAIN lifts each object to a size-scaled loudness (big > medium > small, measured offline),
// and liveBreakBus() squeezes the transients (fast compressor + soft clip) so they land over the music without clipping.
export const LIVE_GAIN = { glass: 3.6, cup: 13.7, twig: 26, plate: 15.8, wood: 14.6, bottle: 21.4, window: 13.7, tv: 12.5, table: 10.8, windshield: 13.8 }; // dB
export const liveGain = (kind) => 10 ** ((LIVE_GAIN[kind] || 0) / 20);
export function liveBreakBus(c, dest) {
  const comp = c.createDynamicsCompressor(); comp.threshold.value = -16; comp.knee.value = 4; comp.ratio.value = 12; comp.attack.value = 0.002; comp.release.value = 0.18;
  const makeup = c.createGain(); makeup.gain.value = 2.2;
  const clip = c.createWaveShaper(), n = 2048, cv = new Float32Array(n); for (let i = 0; i < n; i++) { const x = (i / (n - 1)) * 2 - 1; cv[i] = 0.95 * Math.tanh(1.6 * x) / Math.tanh(1.6); } clip.curve = cv;
  const input = c.createGain(); input.connect(comp).connect(makeup).connect(clip).connect(dest);
  return input;
}
export const BREAK_DUR = { small: 0.8, medium: 1.2, big: 2.0 };
