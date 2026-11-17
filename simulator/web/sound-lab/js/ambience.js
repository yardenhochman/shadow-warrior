// Ambience, game-state extras, stingers and the surprise ideas. Each item is
// { id, name, dur, desc, when, build(c, dest, t0, opts) }; builds are plain WebAudio.
import { rng, env, noise, noiseHit, tone, partials, drive, withReverb, reverb, renderOffline } from './dsp.js';
import { buildImpact } from './impacts.js';

/** Dojo night wind; energy(t) 0..1 opens it up: crickets fade out, the wind rises and whistles, chimes join. */
export function dojoWind(c, dest, t0, dur, e0 = 0, e1 = 1, seed = 3) {
  const r = rng(seed), T = t0 + dur, ramp = (p, a, b) => { p.setValueAtTime(a, t0); p.linearRampToValueAtTime(b, T - 0.5); p.linearRampToValueAtTime(0.0001, T); };
  const lp = c.createBiquadFilter(); lp.type = 'lowpass'; lp.frequency.setValueAtTime(350 + 1800 * e0, t0); lp.frequency.linearRampToValueAtTime(350 + 1800 * e1, T);
  const g = c.createGain(); ramp(g.gain, 0.07 + 0.11 * e0, 0.07 + 0.11 * e1);
  const gust = c.createOscillator(), gg = c.createGain(); gust.frequency.value = 0.17; gg.gain.value = 0.035; gust.connect(gg).connect(g.gain); gust.start(t0); gust.stop(T);
  const gust2 = c.createOscillator(), gg2 = c.createGain(); gust2.frequency.value = 0.41; gg2.gain.value = 0.02; gust2.connect(gg2).connect(g.gain); gust2.start(t0); gust2.stop(T);
  noise(c, t0, dur, 'pink', r).connect(lp).connect(g).connect(dest);
  // whistle: resonant band wandering, louder with energy
  const bp = c.createBiquadFilter(); bp.type = 'bandpass'; bp.Q.value = 18; bp.frequency.setValueAtTime(500 + 700 * e0, t0); bp.frequency.linearRampToValueAtTime(500 + 900 * e1, T);
  const wl = c.createOscillator(), wlg = c.createGain(); wl.frequency.value = 0.23; wlg.gain.value = 160; wl.connect(wlg).connect(bp.frequency); wl.start(t0); wl.stop(T);
  const wg = c.createGain(); ramp(wg.gain, 0.08 * e0 * e0 + 0.01, 0.08 * e1 * e1 + 0.01); noise(c, t0, dur, 'white', r).connect(bp).connect(wg).connect(dest);
  // crickets: 3-pulse chirps, gone by half energy
  for (let t = t0 + 0.3; t < T - 0.4; t += 0.75 + r() * 0.4) {
    const e = e0 + (e1 - e0) * (t - t0) / dur, k = Math.max(0, 1 - 2 * e); if (k <= 0.01) continue;
    for (let i = 0; i < 3; i++) tone(c, dest, t + i * 0.045, { f: 4300 + r() * 150, peak: 0.018 * k, dec: 0.025 });
  }
  // wind chimes once energy passes 0.55
  const pent = [1046.5, 1174.7, 1396.9, 1568, 1760, 2093];
  for (let t = t0 + 0.5; t < T - 0.8; t += 0.35 + r() * 0.9) {
    const e = e0 + (e1 - e0) * (t - t0) / dur; if (e < 0.55 || r() > e) continue;
    partials(c, dest, t, pent[Math.floor(r() * pent.length)], [1, 2.76, 5.4], { peak: 0.03 * e, dec: 1.4, r });
  }
}

export function heartbeat(c, dest, t0, dur, bpm0 = 70, bpm1 = 140) {
  let t = t0 + 0.05;
  const f = c.createBiquadFilter(); f.type = 'lowpass'; f.frequency.value = 160; f.connect(dest);
  while (t < t0 + dur - 0.4) {
    const x = (t - t0) / dur, bpm = bpm0 + (bpm1 - bpm0) * x, gap = 60 / bpm;
    tone(c, f, t, { f: 62, f2: 40, glide: 0.08, peak: 0.55 + 0.25 * x, dec: 0.11 });
    tone(c, f, t + gap * 0.32, { f: 54, f2: 36, glide: 0.08, peak: 0.38 + 0.2 * x, dec: 0.1 });
    t += gap;
  }
}

export function crowdSwell(c, dest, t0, dur = 4, seed = 4) {
  const r = rng(seed), bus = withReverb(c, dest, 0.3, { sec: 2, decay: 2.5, bright: 0.4 }), T = t0 + dur;
  for (const [fc, k] of [[300, 0.9], [730, 1], [1090, 0.6], [2440, 0.25]]) {
    const f = c.createBiquadFilter(); f.type = 'bandpass'; f.Q.value = 2.5; f.frequency.setValueAtTime(fc * 0.8, t0); f.frequency.linearRampToValueAtTime(fc, t0 + dur * 0.4);
    const g = c.createGain(); g.gain.setValueAtTime(0.0001, t0); g.gain.exponentialRampToValueAtTime(0.32 * k, t0 + dur * 0.4); g.gain.exponentialRampToValueAtTime(0.0001, T);
    noise(c, t0, dur, 'pink', r).connect(f).connect(g).connect(bus);
  }
  for (let i = 0; i < 70; i++) { const t = t0 + dur * (0.2 + 0.6 * r()); noiseHit(c, bus, t, { type: 'bandpass', f: 1200 + r() * 1200, q: 1.5, peak: 0.08 * r(), dec: 0.012, r }); }
  tone(c, bus, t0 + dur * 0.35, { f: 1900, f2: 2900, glide: 0.3, a: 0.05, peak: 0.04, dec: 0.4 });
}

export function shadowBreath(c, dest, t0, dur = 6, seed = 6) {
  const r = rng(seed), bus = withReverb(c, dest, 0.35, { sec: 2.5, decay: 2, bright: 0.25 });
  for (let t = t0 + 0.1; t < t0 + dur - 2.4; t += 2.9) {
    const bp = c.createBiquadFilter(); bp.type = 'bandpass'; bp.Q.value = 2; bp.frequency.setValueAtTime(700, t); bp.frequency.linearRampToValueAtTime(1500, t + 1.1);
    const g = c.createGain(); g.gain.setValueAtTime(0.0001, t); g.gain.exponentialRampToValueAtTime(0.4, t + 1.0); g.gain.exponentialRampToValueAtTime(0.0001, t + 1.2);
    noise(c, t, 1.3, 'pink', r).connect(bp).connect(g).connect(bus);
    const te = t + 1.35, lp = c.createBiquadFilter(); lp.type = 'lowpass'; lp.frequency.value = 650;
    const ge = c.createGain(); ge.gain.setValueAtTime(0.0001, te); ge.gain.exponentialRampToValueAtTime(0.5, te + 0.15); ge.gain.exponentialRampToValueAtTime(0.0001, te + 1.3);
    noise(c, te, 1.4, 'pink', r).connect(lp).connect(ge).connect(bus);
    const o = c.createOscillator(), og = c.createGain(), of = c.createBiquadFilter(); o.type = 'sawtooth'; o.frequency.setValueAtTime(48, te); o.frequency.linearRampToValueAtTime(40, te + 1.2);
    of.type = 'lowpass'; of.frequency.value = 260; const trem = c.createOscillator(), tg = c.createGain(); trem.frequency.value = 23; tg.gain.value = 0.05;
    og.gain.setValueAtTime(0.0001, te); og.gain.exponentialRampToValueAtTime(0.1, te + 0.2); og.gain.exponentialRampToValueAtTime(0.0001, te + 1.25);
    trem.connect(tg).connect(og.gain); o.connect(of).connect(og).connect(bus); o.start(te); o.stop(te + 1.3); trem.start(te); trem.stop(te + 1.3);
  }
}

/** Shout charge, staged like the WARMING bar: four steps up, then the "full" sparkle. */
export function chargeRiser(c, dest, t0, stages = [0, 0.9, 1.8, 2.7], seed = 7) {
  const r = rng(seed), notes = [110, 130.8, 164.8, 220];
  stages.forEach((d, i) => {
    const t = t0 + d, f = c.createBiquadFilter(), g = c.createGain(); f.type = 'lowpass'; f.Q.value = 6;
    f.frequency.setValueAtTime(300, t); f.frequency.exponentialRampToValueAtTime(1200 + 700 * i, t + 0.75);
    g.gain.setValueAtTime(0.0001, t); g.gain.exponentialRampToValueAtTime(0.08 + 0.02 * i, t + 0.6); g.gain.exponentialRampToValueAtTime(0.0001, t + 0.9);
    f.connect(g).connect(dest);
    [1, 1.5, 2].forEach((k) => [-6, 6].forEach((dt) => { const o = c.createOscillator(); o.type = 'sawtooth'; o.frequency.setValueAtTime(notes[i] * k, t); o.frequency.linearRampToValueAtTime(notes[i] * k * 1.06, t + 0.9); o.detune.value = dt; o.connect(f); o.start(t); o.stop(t + 0.95); }));
    noiseHit(c, dest, t + 0.55, { type: 'bandpass', f: 2000 + 1000 * i, q: 2, a: 0.3, peak: 0.05, dec: 0.1, r });
  });
  const tf = t0 + stages[3] + 0.9;
  [1760, 2217, 2637, 3520].forEach((f, i) => tone(c, dest, tf + i * 0.04, { f, peak: 0.07, dec: 0.8 }));
  tone(c, dest, tf, { f: 55, f2: 40, glide: 0.4, peak: 0.4, dec: 0.6 });
}

export function victory(c, dest, t0, seed = 8) {
  const r = rng(seed), bus = withReverb(c, dest, 0.35, { sec: 2.5, decay: 2, bright: 0.6 });
  tone(c, bus, t0, { f: 80, f2: 35, glide: 0.8, peak: 0.6, dec: 1.0 });
  [[0, [261.6, 329.6, 392]], [0.22, [293.7, 370, 440]], [0.44, [392, 493.9, 587.3, 784]]].forEach(([d, ch], i) => {
    const t = t0 + d, f = c.createBiquadFilter(), g = c.createGain(); f.type = 'lowpass'; f.frequency.setValueAtTime(500, t); f.frequency.exponentialRampToValueAtTime(3500, t + 0.08); f.frequency.exponentialRampToValueAtTime(900, t + (i === 2 ? 2 : 0.3));
    env(g.gain, t, 0.02, i === 2 ? 0.12 : 0.08, i === 2 ? 2.0 : 0.25); f.connect(g).connect(bus);
    ch.forEach((fr) => [-7, 7].forEach((dt) => { const o = c.createOscillator(); o.type = 'sawtooth'; o.frequency.value = fr; o.detune.value = dt; o.connect(f); o.start(t); o.stop(t + (i === 2 ? 2.2 : 0.4)); }));
  });
  for (let i = 0; i < 16; i++) tone(c, bus, t0 + 0.45 + r() * 1.2, { f: 2000 + r() * 4000, peak: 0.025, dec: 0.3 });
}

export function defeat(c, dest, t0) {
  const bus = withReverb(c, dest, 0.4, { sec: 3, decay: 1.6, bright: 0.3 });
  [[0, 293.7], [0.42, 277.2], [0.84, 261.6]].forEach(([d, f]) => {
    const t = t0 + d, o = c.createOscillator(), lp = c.createBiquadFilter(), g = c.createGain(); o.type = 'sawtooth'; o.frequency.value = f / 2;
    lp.type = 'lowpass'; lp.Q.value = 5; lp.frequency.setValueAtTime(300, t); lp.frequency.linearRampToValueAtTime(1400, t + 0.15); lp.frequency.linearRampToValueAtTime(350, t + 0.38);
    env(g.gain, t, 0.03, 0.12, 0.38); o.connect(lp).connect(g).connect(bus); o.start(t); o.stop(t + 0.45);
  });
  const t = t0 + 1.26, o = c.createOscillator(), lp = c.createBiquadFilter(), g = c.createGain(); o.type = 'sawtooth';
  o.frequency.setValueAtTime(123.5, t); o.frequency.exponentialRampToValueAtTime(30, t + 1.6); // tape-stop dive
  lp.type = 'lowpass'; lp.frequency.setValueAtTime(1200, t); lp.frequency.exponentialRampToValueAtTime(150, t + 1.6);
  const lfo = c.createOscillator(), lg = c.createGain(); lfo.frequency.value = 5; lg.gain.value = 500; lfo.connect(lg).connect(lp.frequency); lfo.start(t); lfo.stop(t + 1.8);
  env(g.gain, t, 0.03, 0.15, 1.7); o.connect(lp).connect(g).connect(bus); o.start(t); o.stop(t + 1.8);
  tone(c, bus, t + 1.6, { f: 60, f2: 30, glide: 0.3, peak: 0.5, dec: 0.6 });
}

// ---- surprises ----
/** Singing bowls instead of a riser: each WARMING stage strikes a bowl; together they form a chord. */
export function singingBowls(c, dest, t0, seed = 9) {
  const r = rng(seed), bus = withReverb(c, dest, 0.4, { sec: 3.5, decay: 1.2, bright: 0.4 });
  [[0, 220], [1.0, 277.2], [2.0, 329.6], [3.0, 440]].forEach(([d, f], i) => {
    const t = t0 + d; noiseHit(c, bus, t, { type: 'bandpass', f: 1500, q: 2, peak: 0.06, dec: 0.02, r });
    [[1, 0.14], [1.004, 0.1], [2.71, 0.06], [2.72, 0.04], [5.12, 0.025]].forEach(([k, pk]) => {
      const o = c.createOscillator(), g = c.createGain(); o.frequency.value = f * k; env(g.gain, t, 0.01, pk, 4.5 - d * 0.6);
      const am = c.createOscillator(), ag = c.createGain(); am.frequency.value = 3 + i; ag.gain.value = pk * 0.3; am.connect(ag).connect(g.gain); am.start(t); am.stop(t + 5);
      o.connect(g).connect(bus); o.start(t); o.stop(t + 5);
    });
  });
}

/** Combo ladder: every hit of a combo plays one step up a minor pentatonic; 5 and 10 land on a chord. */
export function comboLadder(c, dest, t0, family = 'leather', seed = 10) {
  const r = rng(seed), scale = [0, 3, 5, 7, 10, 12, 15, 17, 19, 22], base = 220;
  for (let i = 0; i < 10; i++) {
    const t = t0 + i * 0.32 - (i > 5 ? (i - 5) * 0.05 : 0), p = i === 4 || i === 9 ? 88 : 35 + r() * 25;
    buildImpact(family, c, dest, t, p, seed + i);
    const f = base * Math.pow(2, scale[i] / 12);
    tone(c, dest, t + 0.005, { type: 'triangle', f, peak: 0.07, dec: 0.35 });
    if (i === 4 || i === 9) [1.25, 1.5, 2].forEach((k) => tone(c, dest, t + 0.01, { type: 'triangle', f: f * k, peak: 0.06, dec: 0.9 }));
  }
}

function drumLoop(c, dest, t0, bars = 2, bpm = 128) {
  const beat = 60 / bpm;
  for (let s = 0; s < bars * 16; s++) {
    const t = t0 + s * beat / 4;
    if (s % 4 === 0) tone(c, dest, t, { f: 110, f2: 42, glide: 0.12, peak: 0.6, dec: 0.22 });
    if (s % 8 === 4) noiseHit(c, dest, t, { type: 'bandpass', f: 1800, peak: 0.3, dec: 0.12 });
    if (s % 2 === 1) noiseHit(c, dest, t, { type: 'highpass', f: 7000, peak: 0.08, dec: 0.04 });
    if (s % 4 === 2) { const o = c.createOscillator(), f = c.createBiquadFilter(), g = c.createGain(); o.type = 'sawtooth'; o.frequency.value = [55, 55, 65.4, 49][Math.floor(s / 16) % 4]; f.type = 'lowpass'; f.frequency.value = 300; env(g.gain, t, 0.005, 0.2, beat * 0.9); o.connect(f).connect(g).connect(dest); o.start(t); o.stop(t + beat); }
  }
}

/** Tape-stop K.O.: the fight music itself grinds to a halt on the final blow. */
export async function tapeStopKO(c, dest, t0) {
  const loop = await renderOffline(3.8, (cc, d, t) => drumLoop(cc, d, t, 2), c.sampleRate);
  const s = c.createBufferSource(); s.buffer = loop; const g = c.createGain(); g.gain.value = 0.5; s.connect(g).connect(dest);
  const tk = t0 + 1.875 * 1.5; s.playbackRate.setValueAtTime(1, t0); s.playbackRate.setValueAtTime(1, tk); s.playbackRate.exponentialRampToValueAtTime(0.06, tk + 1.1);
  g.gain.setValueAtTime(0.5, tk + 0.8); g.gain.linearRampToValueAtTime(0, tk + 1.15); s.start(t0); s.stop(tk + 1.2);
  buildImpact('cinema', c, dest, tk, 100, 77);
}

/** Bullet-time heavy: the world muffles, the hit swells in backwards, then lands at half speed. */
export async function bulletTime(c, dest, t0) {
  const hit = await renderOffline(2.4, (cc, d, t) => buildImpact('cinema', cc, d, t + 0.1, 95, 5), c.sampleRate);
  const rev = c.createBuffer(1, hit.length, hit.sampleRate); const a = hit.getChannelData(0), b = rev.getChannelData(0);
  for (let i = 0; i < a.length; i++) b[i] = a[a.length - 1 - i];
  const rs = c.createBufferSource(); rs.buffer = rev; const rg = c.createGain(); rg.gain.value = 0.25; const rl = c.createBiquadFilter(); rl.type = 'lowpass'; rl.frequency.value = 1400;
  rs.connect(rl).connect(rg).connect(dest); rs.start(t0); // ~2.4 s swell
  const hs = c.createBufferSource(); hs.buffer = hit; hs.playbackRate.value = 0.5; const hg = c.createGain(); hg.gain.value = 0.7; hs.connect(hg).connect(dest); hs.start(t0 + 2.2);
  tone(c, dest, t0 + 2.4, { f: 50, f2: 30, glide: 0.6, peak: 0.5, dec: 1.0 });
}

/** Energy full = the floor ice cracks: dispersive "pew" chirps, sharp cracks, a low groan. */
export function iceCrack(c, dest, t0, seed = 12) {
  const r = rng(seed), lift = c.createGain(); lift.gain.value = 2; lift.connect(dest);
  const bus = withReverb(c, lift, 0.3, { sec: 2.5, decay: 1.8, bright: 0.7 });
  tone(c, bus, t0, { type: 'sawtooth', f: 38, f2: 30, glide: 1.5, a: 0.3, peak: 0.08, dec: 1.4 });
  for (let i = 0; i < 9; i++) {
    const t = t0 + 0.05 + Math.pow(r(), 0.8) * 1.6;
    noiseHit(c, bus, t, { type: 'highpass', f: 2500, peak: 0.25 * r() + 0.08, dec: 0.006, r });
    tone(c, bus, t + 0.002, { f: 6000 + r() * 2000, f2: 700 + r() * 400, glide: 0.07 + r() * 0.05, peak: 0.07, dec: 0.12 });
  }
}

export const AMBIENCE = [
  { id: 'wind', name: 'Dojo night wind', dur: 14, when: 'IDLE / WARMING; follows the energy bar', desc: 'Energy 0 to 100% over 14 s: crickets in a still night go quiet, the wind rises and starts to whistle, wind chimes join near full.',
    build: (c, d, t) => dojoWind(c, d, t, 14, 0, 1) },
  { id: 'heart', name: 'Heartbeat', dur: 10, when: 'Shadow near K.O., or last 10 s of the round', desc: 'Low, felt-more-than-heard lub-dub, speeding from 70 to 140 BPM.', build: (c, d, t) => heartbeat(c, d, t, 10) },
  { id: 'crowd', name: 'Crowd swell', dur: 4, when: 'Combo milestones (5, 10, ...)', desc: 'A distant arena crowd going "ooh-AAH": vowel-shaped noise, claps, one whistle.', build: (c, d, t) => crowdSwell(c, d, t, 4) },
  { id: 'breath', name: 'Shadow breath', dur: 6, when: 'Between punches (after ~2 s with no hit)', desc: 'Slow inhale, growling exhale, in a dark room. The shadow waiting.', build: (c, d, t) => shadowBreath(c, d, t, 6) },
  { id: 'riser', name: 'Shout charge riser', dur: 4.6, when: 'WARMING: one step per bar stage, sparkle on full', desc: 'Four rising synth steps (one per WARMING stage), then a bright "full" sparkle with a sub.', build: (c, d, t) => chargeRiser(c, d, t) },
  { id: 'victory', name: 'Victory stinger', dur: 3, when: 'Shadow K.O.', desc: 'Sub hit and a three-chord brass-like fanfare with sparkles.', build: (c, d, t) => victory(c, d, t) },
  { id: 'defeat', name: 'Defeat stinger', dur: 3.6, when: 'Time out / game over', desc: 'Three sinking "wah" notes and a tape-stop dive into a thud.', build: (c, d, t) => defeat(c, d, t) },
];

export const SURPRISES = [
  { id: 'bowls', name: 'Singing bowls charge', dur: 6, when: 'WARMING stages, instead of the riser', desc: 'Each shout stage strikes a bowl; they keep ringing and stack into a chord. Pairs with the demon monk.', build: (c, d, t) => singingBowls(c, d, t) },
  { id: 'ladder', name: 'Combo ladder', dur: 3.6, when: 'Every hit in a combo', desc: 'Each consecutive hit adds a note up a minor pentatonic; the 5th and 10th land a chord. Your combo plays a melody.', build: (c, d, t) => comboLadder(c, d, t) },
  { id: 'tapestop', name: 'Tape-stop K.O.', dur: 4.3, when: 'The K.O. blow', desc: 'The fight music itself grinds to a halt under the final hit, as if the world stopped.', build: (c, d, t) => tapeStopKO(c, d, t) },
  { id: 'bullet', name: 'Bullet-time heavy', dur: 4.6, when: 'Max-power heavy or the K.O. blow', desc: 'The hit swells in backwards, then lands at half speed with a long sub. Slow motion for the ears.', build: (c, d, t) => bulletTime(c, d, t) },
  { id: 'ice', name: 'Ice floor cracks', dur: 2.6, when: 'Energy bar full (WARMING to FIGHT)', desc: 'Your shout cracks the frozen floor: dispersive ice "pew" chirps, sharp cracks, a low groan.', build: (c, d, t) => iceCrack(c, d, t) },
];
