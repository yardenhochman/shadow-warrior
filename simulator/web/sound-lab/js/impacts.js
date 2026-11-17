// Bag impact families. Each is build(c, dest, t, power 1..100, seed) and scales continuously
// with power: jab < 45 <= hook < 75 <= heavy (the simulator's tiers, hitfx.js). The seed picks
// the alternate (A/B/C...) so repeats never sound identical. Plain WebAudio nodes only, so any
// of these can be pasted into sfx.js punch() as-is (pass this.ctx, this.master, currentTime).
import { rng, env, noise, noiseHit, tone, partials, drive, withReverb, lerp, clamp } from './dsp.js';

export const HOOK_AT = 45, HEAVY_AT = 75;
export const tierOf = (p) => (p >= HEAVY_AT ? 'heavy' : p >= HOOK_AT ? 'hook' : 'jab');
const smooth = (p, a, b) => clamp((p - a) / (b - a));

/** Leather & Sand: a real heavy bag. Chest-deep thump, leather slap, sand hiss, chain rattle. */
function leather(c, dest, t, power, seed) {
  const r = rng(seed), p = power / 100, j = (k = 0.08) => 1 + (r() - 0.5) * 2 * k;
  const thump = c.createOscillator(), tg = c.createGain(), lp = c.createBiquadFilter();
  lp.type = 'lowpass'; lp.frequency.value = 500 + 900 * p;
  thump.frequency.setValueAtTime((75 + 55 * p) * j(), t); thump.frequency.exponentialRampToValueAtTime(42 * j(0.05), t + 0.1 + 0.12 * p);
  env(tg.gain, t, 0.002, 0.3 + 0.45 * p, 0.1 + 0.22 * p); thump.connect(lp).connect(tg).connect(dest); thump.start(t); thump.stop(t + 0.6);
  noiseHit(c, dest, t, { type: 'bandpass', f: (1500 + 1700 * p) * j(0.15), q: 0.9, peak: 0.12 + 0.24 * p, dec: 0.025 + 0.03 * p, r }); // leather slap
  noiseHit(c, dest, t + 0.002, { type: 'lowpass', f: 380 + 500 * p, peak: 0.14 + 0.24 * p, dec: 0.06 + 0.12 * p, color: 'pink', r }); // body
  if (p > 0.35) noiseHit(c, dest, t + 0.03, { type: 'bandpass', f: 3200 * j(0.2), q: 0.7, a: 0.04, peak: 0.02 + 0.05 * smooth(power, 35, 100), dec: 0.25 + 0.2 * p, r }); // sand settling
  if (power >= HEAVY_AT - 5) { // chain + hook rattle, second swing thud
    const n = 4 + Math.floor(r() * 4 + 6 * smooth(power, 70, 100));
    for (let i = 0; i < n; i++) { const dt = 0.07 + Math.pow(r(), 0.7) * 0.45; partials(c, dest, t + dt, 1900 * j(0.25), [1, 1.73, 2.71], { peak: 0.035 * (1 - dt), dec: 0.05, r }); }
    tone(c, dest, t + 0.32 + 0.05 * r(), { f: 70, f2: 45, glide: 0.1, peak: 0.12 * p, dec: 0.15 });
  }
}

/** Anime Crack: whoosh-in, whip crack, pitched ting; heavies leave a sparkle trail. */
function anime(c, dest, t, power, seed) {
  const r = rng(seed), p = power / 100, j = (k = 0.1) => 1 + (r() - 0.5) * 2 * k;
  const pre = 0.05 + 0.1 * p, t0 = Math.max(0, t - pre);
  { const f = c.createBiquadFilter(); f.type = 'bandpass'; f.Q.value = 1.5; f.frequency.setValueAtTime(400 * j(), t0); f.frequency.exponentialRampToValueAtTime(4000 * j(), t);
    const g = c.createGain(); g.gain.setValueAtTime(0.0001, t0); g.gain.exponentialRampToValueAtTime(0.08 + 0.15 * p, t - 0.005); g.gain.linearRampToValueAtTime(0.0001, t + 0.01);
    noise(c, t0, pre + 0.03, 'white', r).connect(f).connect(g).connect(dest); }
  noiseHit(c, dest, t, { type: 'highpass', f: 2200 * j(), peak: 0.2 + 0.3 * p, dec: 0.03 + 0.03 * p, r }); // crack
  { const o = c.createOscillator(), g = c.createGain(), f = c.createBiquadFilter(); o.type = 'sawtooth'; f.type = 'lowpass'; f.frequency.value = 2500;
    o.frequency.setValueAtTime((900 + 600 * p) * j(), t); o.frequency.exponentialRampToValueAtTime(110, t + 0.06); env(g.gain, t, 0.001, 0.12 + 0.12 * p, 0.07);
    o.connect(f).connect(g).connect(dest); o.start(t); o.stop(t + 0.15); }
  tone(c, dest, t, { type: 'square', f: 130 * j(), f2: 48, glide: 0.09, peak: 0.12 + 0.3 * p, dec: 0.08 + 0.12 * p }); // body
  if (power >= HOOK_AT - 5) tone(c, dest, t + 0.01, { f: (2400 + 800 * r()) * (power >= HEAVY_AT ? 0.75 : 1), peak: 0.05 + 0.05 * p, dec: 0.25 + 0.3 * p }); // ting
  if (power >= HEAVY_AT - 5) {
    const s = smooth(power, 70, 100);
    tone(c, dest, t, { f: 70, f2: 32, glide: 0.4, peak: 0.25 + 0.25 * s, dec: 0.5 });
    noiseHit(c, dest, t + 0.02, { type: 'highpass', f: 6000, a: 0.02, peak: 0.05 + 0.04 * s, dec: 0.5 + 0.3 * s, r }); // shing
    for (let i = 0; i < 8 + 10 * s; i++) tone(c, dest, t + 0.05 + r() * (0.5 + 0.4 * s), { f: 3000 + r() * 5000, peak: 0.02 + 0.02 * r(), dec: 0.06 + 0.1 * r() });
  }
}

/** Cinematic Boom: sub drop, distorted punch, hall tail; heavies add a low brass "braam" and debris. */
function cinema(c, dest, t, power, seed) {
  const r = rng(seed), p = power / 100, j = (k = 0.08) => 1 + (r() - 0.5) * 2 * k;
  const bus = withReverb(c, dest, 0.18 + 0.4 * p, { sec: 2.6, decay: 2.2, bright: 0.4, seed: 9 });
  const sub = c.createOscillator(), sg = c.createGain(); sub.frequency.setValueAtTime(62 * j(), t); sub.frequency.exponentialRampToValueAtTime(27, t + 0.3 + 0.7 * p);
  env(sg.gain, t, 0.004, 0.3 + 0.4 * p, 0.25 + 0.9 * p); sub.connect(sg).connect(dest); sub.start(t); sub.stop(t + 1.6);
  { const sh = drive(c, 2 + 4 * p), f = c.createBiquadFilter(), g = c.createGain(); f.type = 'lowpass'; f.frequency.value = 900 + 1800 * p; env(g.gain, t, 0.002, 0.1 + 0.18 * p, 0.08 + 0.1 * p);
    noise(c, t, 0.4, 'pink', r).connect(sh).connect(f).connect(g).connect(bus); }
  tone(c, bus, t, { type: 'triangle', f: 160 * j(), f2: 60, glide: 0.08, peak: 0.1 + 0.15 * p, dec: 0.15 });
  if (power >= HOOK_AT) noiseHit(c, bus, t + 0.005, { type: 'bandpass', f: 700 * j(0.2), q: 1.2, peak: 0.08 + 0.08 * p, dec: 0.12, r });
  if (power >= HEAVY_AT - 5) {
    const s = smooth(power, 70, 100), f = c.createBiquadFilter(), g = c.createGain(); f.type = 'lowpass'; f.Q.value = 4;
    f.frequency.setValueAtTime(150, t); f.frequency.exponentialRampToValueAtTime(900 + 900 * s, t + 0.12); f.frequency.exponentialRampToValueAtTime(180, t + 1.3);
    g.gain.setValueAtTime(0.0001, t); g.gain.exponentialRampToValueAtTime(0.05 + 0.05 * s, t + 0.05); g.gain.exponentialRampToValueAtTime(0.0001, t + 1.5);
    f.connect(g).connect(bus);
    for (const [fr, dt] of [[55, 0], [55.4, 7], [82.6, -5], [27.6, 0]]) { const o = c.createOscillator(); o.type = 'sawtooth'; o.frequency.value = fr * j(0.01); o.detune.value = dt; o.connect(f); o.start(t); o.stop(t + 1.6); }
    for (let i = 0; i < 6 + 14 * s; i++) { const dt = 0.15 + Math.pow(r(), 0.6) * 0.9; noiseHit(c, dest, t + dt, { type: 'bandpass', f: 1500 + r() * 3500, q: 3, peak: 0.03 * (1.2 - dt), dec: 0.02 + 0.03 * r(), r }); }
  }
}

/** Taiko Dojo: jab = tight shime-daiko with rim "ka", hook = taiko, heavy = o-daiko in a wooden hall. */
function taiko(c, dest, t, power, seed) {
  const r = rng(seed), p = power / 100, j = (k = 0.05) => 1 + (r() - 0.5) * 2 * k;
  const bus = withReverb(c, dest, 0.12 + 0.3 * p, { sec: 1.6, decay: 3.2, bright: 0.5, pre: 0.015, seed: 12 });
  const f0 = lerp(330, 52, Math.pow(p, 0.7)) * j(), dec = lerp(0.07, 1.1, Math.pow(p, 1.4));
  const o = c.createOscillator(), g = c.createGain(); o.frequency.setValueAtTime(f0 * 1.12, t); o.frequency.exponentialRampToValueAtTime(f0, t + 0.03); o.frequency.exponentialRampToValueAtTime(f0 * 0.88, t + dec);
  env(g.gain, t, 0.002, 0.28 + 0.42 * p, dec); o.connect(g).connect(bus); o.start(t); o.stop(t + dec + 0.1);
  tone(c, bus, t, { f: f0 * 1.59 * j(), peak: 0.08 + 0.08 * p, dec: dec * 0.4 }); // second membrane mode
  tone(c, bus, t, { f: f0 * 2.14 * j(), peak: 0.04, dec: dec * 0.25 });
  noiseHit(c, bus, t, { type: 'bandpass', f: 1800 * j(0.2), q: 2.5, peak: 0.1 + 0.08 * (1 - p), dec: 0.012, r }); // bachi stick
  if (power < HOOK_AT) noiseHit(c, bus, t + 0.09 + 0.02 * r(), { type: 'bandpass', f: 2600 * j(0.2), q: 8, peak: 0.06, dec: 0.025, r }); // rim "ka"
  if (power >= HEAVY_AT - 5) { noiseHit(c, bus, t, { type: 'lowpass', f: 220, peak: 0.25 * smooth(power, 70, 100) + 0.1, dec: 0.5, color: 'brown', r });
    if (r() < 0.6) tone(c, bus, t + 0.32, { f: f0 * 1.05, peak: 0.12, dec: 0.4 }); } // flam
}

export const FAMILIES = [
  { id: 'leather', name: 'Leather & Sand', build: leather, desc: 'The real bag, exaggerated: chest-deep thump, leather slap, sand hiss; heavies make the chain rattle.',
    why: 'Honest and physical: the sound matches what the hand feels, so even a jab feels like it landed. Never gets tiring.' },
  { id: 'anime', name: 'Anime Crack', build: anime, desc: 'A whoosh into a whip-crack and a pitched ting; heavies leave a sparkle trail and a sub hit.',
    why: 'Matches the LED signatures (sparks, strobe, shatter). Bright transients cut through loud music and read well from across a room.' },
  { id: 'cinema', name: 'Cinematic Boom', build: cinema, desc: 'Sub drop, distorted punch and a hall tail; heavies add a low brass "braam" and falling debris.',
    why: 'Makes a heavy feel like an event. Best saved for heavies over a lighter jab family, or for a big-speaker install.' },
  { id: 'taiko', name: 'Taiko Dojo', build: taiko, desc: 'Power morphs the drum itself: small shime-daiko with rim "ka" for jabs, up to a huge o-daiko for heavies.',
    why: 'Fits the demon-monk/dojo world. Punch strength becomes drum size, which you hear instantly, and a combo plays a rhythm.' },
];

export const TIER_POWER = { jab: 25, hook: 58, heavy: 92 };
export const IMPACT_GAIN = 0.6; // headroom: the hottest heavy stays under -1 dBTP
export function buildImpact(familyId, c, dest, t, power, seed = 1) {
  const g = c.createGain(); g.gain.value = IMPACT_GAIN; g.connect(dest);
  FAMILIES.find((f) => f.id === familyId).build(c, g, t, power, seed);
}
