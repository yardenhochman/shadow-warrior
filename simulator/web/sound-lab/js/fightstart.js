// Fight-start candidates: the arena switching on like a stadium at night.
// Same contract as buildArenaOpen() in ../js/sfx.js: build(c, dest, t0) on any (Offline)AudioContext,
// returns the end time; each runs through its own compressor + limiter so the peak stays under 0 dBFS
// over the music. `hit` = seconds from t0 to the moment the LEDs should go full (for the visuals).
import { rng, env, noise, noiseHit, tone, partials, reverb, drive, renderOffline } from './dsp.js';
import { buildImpact } from './impacts.js';

function chain(c, dest) {
  const comp = c.createDynamicsCompressor(); comp.threshold.value = -16; comp.knee.value = 8; comp.ratio.value = 5; comp.attack.value = 0.003; comp.release.value = 0.3;
  const lim = c.createDynamicsCompressor(); lim.threshold.value = -7; lim.knee.value = 0; lim.ratio.value = 20; lim.attack.value = 0.001; lim.release.value = 0.12;
  const out = c.createGain(); out.gain.value = 0.8; const bus = c.createGain(); bus.gain.value = 0.75;
  bus.connect(comp).connect(lim).connect(out).connect(dest);
  // stadium: a long dark hall plus two slapbacks off the far stands
  const hall = reverb(c, { sec: 3.5, decay: 1.4, bright: 0.35, pre: 0.05, seed: 21 }), hw = c.createGain(); hw.gain.value = 0.45;
  const send = c.createGain(); send.connect(hall).connect(hw).connect(comp);
  const d = c.createDelay(1.5); d.delayTime.value = 0.31; const fb = c.createGain(); fb.gain.value = 0.38; const lp = c.createBiquadFilter(); lp.type = 'lowpass'; lp.frequency.value = 1400;
  const dw = c.createGain(); dw.gain.value = 0.5; send.connect(d); d.connect(lp).connect(fb).connect(d); lp.connect(dw).connect(comp);
  return { bus, send, both: (() => { const g = c.createGain(); g.connect(bus); g.connect(send); return g; })() };
}

/** A contactor/relay bank slamming closed: low thunk, metal clack, a spark of arcing. */
function clunk(c, o, t, size = 1, r = Math.random) {
  tone(c, o.both, t, { f: 95 * size ** -0.3, f2: 38, glide: 0.18, peak: 0.55 * size, dec: 0.25 + 0.2 * size });
  noiseHit(c, o.both, t, { type: 'bandpass', f: 2300 + r() * 600, q: 2, peak: 0.35 * size, dec: 0.03, r });
  noiseHit(c, o.both, t + 0.012, { type: 'bandpass', f: 1100, q: 3, peak: 0.25 * size, dec: 0.05, r });
  partials(c, o.both, t + 0.004, 640 + r() * 120, [1, 1.47, 2.09, 3.3], { peak: 0.12 * size, dec: 0.25, r, spread: 0.05 });
  for (let i = 0; i < 5; i++) noiseHit(c, o.bus, t + 0.02 + r() * 0.08, { type: 'highpass', f: 5000, peak: 0.05, dec: 0.004 + 0.006 * r(), r }); // arc sparks
}

/** Mains hum + metal-halide ballast ring swelling in from t, held until end. */
function hum(c, o, t, end, level = 0.05, mains = 50, r = Math.random) {
  const g = c.createGain(); g.gain.setValueAtTime(0.0001, t); g.gain.exponentialRampToValueAtTime(level, t + 0.35); g.gain.setValueAtTime(level, end - 0.6); g.gain.exponentialRampToValueAtTime(0.0001, end);
  const f = c.createBiquadFilter(); f.type = 'lowpass'; f.frequency.value = 1400; f.connect(g).connect(o.bus);
  [[1, 0.6], [2, 1], [3, 0.5], [4, 0.35], [6, 0.2], [8, 0.12]].forEach(([h, a]) => { const x = c.createOscillator(), xg = c.createGain(); x.frequency.value = mains * h * (1 + (r() - 0.5) * 0.002); xg.gain.value = a; x.connect(xg).connect(f); x.start(t); x.stop(end); });
  const buzz = c.createOscillator(), bg = c.createGain(), bf = c.createBiquadFilter(); buzz.type = 'sawtooth'; buzz.frequency.value = mains * 2; bf.type = 'bandpass'; bf.frequency.value = 2400; bf.Q.value = 3;
  bg.gain.value = 0.25; buzz.connect(bf).connect(bg).connect(g); buzz.start(t); buzz.stop(end);
  const ring = c.createOscillator(), rg = c.createGain(); ring.frequency.value = 2900 + r() * 400; rg.gain.setValueAtTime(0.0001, t); rg.gain.exponentialRampToValueAtTime(level * 0.1, t + 0.9); rg.gain.exponentialRampToValueAtTime(0.0001, end);
  ring.connect(rg).connect(o.bus); ring.start(t); ring.stop(end);
}

function crowd(c, o, t, dur, peak = 0.25, r = Math.random) {
  for (const [fc, k] of [[420, 1], [900, 0.7], [1500, 0.45], [2600, 0.25]]) {
    const f = c.createBiquadFilter(); f.type = 'bandpass'; f.Q.value = 0.9; f.frequency.value = fc;
    const g = c.createGain(); g.gain.setValueAtTime(0.0001, t); g.gain.exponentialRampToValueAtTime(peak * k, t + 0.6); g.gain.exponentialRampToValueAtTime(peak * k * 0.6, t + dur * 0.6); g.gain.exponentialRampToValueAtTime(0.0001, t + dur);
    const l = c.createOscillator(), lg = c.createGain(); l.frequency.value = 4 + r() * 3; lg.gain.value = peak * k * 0.2; l.connect(lg).connect(g.gain); l.start(t); l.stop(t + dur);
    noise(c, t, dur, 'pink', r).connect(f).connect(g).connect(o.both);
  }
  for (let i = 0; i < 2; i++) { const t1 = t + 0.4 + r() * 0.8; tone(c, o.bus, t1, { f: 2100 + r() * 300, f2: 2900, glide: 0.25, peak: 0.03, dec: 0.4 }); } // whistles
}

function sub(c, o, t, peak = 1, dur = 1.6) {
  const x = c.createOscillator(), g = c.createGain(); x.frequency.setValueAtTime(90, t); x.frequency.exponentialRampToValueAtTime(28, t + dur * 0.8);
  env(g.gain, t, 0.005, peak, dur); x.connect(g).connect(o.bus); x.start(t); x.stop(t + dur + 0.1);
  noiseHit(c, o.both, t, { type: 'lowpass', f: 2500, peak: 0.5 * peak, dec: 0.2 });
}

function drums(c, o, t, hits, r) {
  for (const [d, pk] of hits) { tone(c, o.both, t + d, { f: 120, f2: 46, glide: 0.25, peak: pk, dec: 0.4 }); noiseHit(c, o.both, t + d, { type: 'lowpass', f: 700, peak: 0.3 * pk, dec: 0.08, r }); }
}

/** Boxing ring bell: hammer click + brass-bell modal partials (doubled for beating), long ring. */
function bell(c, dest, t, { f = 1020, size = 1, dec = 2.2, r = Math.random } = {}) {
  noiseHit(c, dest, t, { type: 'highpass', f: 3500, peak: 0.25 * size, dec: 0.006, r }); // hammer
  noiseHit(c, dest, t, { type: 'bandpass', f: 5200, q: 4, peak: 0.12 * size, dec: 0.02, r });
  [[1, 0.3, 1], [1.003, 0.18, 1], [2.32, 0.2, 0.6], [2.327, 0.1, 0.6], [3.98, 0.12, 0.4], [5.47, 0.08, 0.28], [6.95, 0.05, 0.2], [0.5, 0.06, 0.8]].forEach(([k, pk, dk]) =>
    tone(c, dest, t, { f: f * k, peak: pk * size, dec: dec * dk, a: 0.0006 }));
}
function bellOut(c, dest, wet = 0.3, sec = 2.4) {
  const o = chain(c, dest), rv = reverb(c, { sec, decay: 1.6, bright: 0.6, pre: 0.02, seed: 40 }), w = c.createGain(); w.gain.value = wet;
  const inp = c.createGain(); inp.connect(o.bus); inp.connect(rv).connect(w).connect(o.bus); return { o, inp };
}

// ---- rough family (Yarden, 20.11: the clean bells sound too tonal; wants rough) ----------------
/** inp -> highpass -> tanh drive -> optional bit-crush -> lowpass -> dest: a cheap, overdriven PA. */
function dirt(c, dest, { amount = 5, hp = 120, lp = 4200, crush = 0, gain = 0.7 } = {}) {
  const inp = c.createGain(), h = c.createBiquadFilter(), l = c.createBiquadFilter(), g = c.createGain();
  h.type = 'highpass'; h.frequency.value = hp; l.type = 'lowpass'; l.frequency.value = lp; g.gain.value = gain;
  let x = inp.connect(h).connect(drive(c, amount));
  if (crush) { const w = c.createWaveShaper(), n = 2048, cv = new Float32Array(n); for (let i = 0; i < n; i++) { const v = (i / (n - 1)) * 2 - 1; cv[i] = Math.round(v * crush) / crush; } w.curve = cv; x = x.connect(w); }
  x.connect(l).connect(g).connect(dest); return inp;
}
/** Struck junk metal: inharmonic, beating partials, a hard clank and a rattle; no clear pitch. */
function junk(c, dest, t, { f = 380, ratios = [1, 1.37, 2.76, 3.9, 5.4, 8.93], size = 1, dec = 0.9, r = Math.random } = {}) {
  noiseHit(c, dest, t, { type: 'bandpass', f: 1800 + r() * 900, q: 1.2, peak: 0.6 * size, dec: 0.025, r });
  noiseHit(c, dest, t, { type: 'highpass', f: 4000, peak: 0.3 * size, dec: 0.012, r });
  ratios.forEach((k, i) => { const ff = f * k * (1 + (r() - 0.5) * 0.04); [0, 7 + r() * 9].forEach((dt) => tone(c, dest, t, { type: i < 2 ? 'triangle' : 'sine', f: ff, detune: dt, peak: 0.16 * size / (1 + i * 0.5), dec: dec / (1 + i * 0.3), a: 0.0006 })); });
  for (let i = 0; i < 6; i++) noiseHit(c, dest, t + 0.03 + r() * 0.25, { type: 'bandpass', f: 2500 + r() * 3000, q: 3, peak: 0.06 * size, dec: 0.01, r }); // loose bits rattling
}
function roughRoom(c, dest, wet = 0.3, sec = 1.4) {
  const o = chain(c, dest), rv = reverb(c, { sec, decay: 2.2, bright: 0.35, pre: 0.012, seed: 51 }), w = c.createGain(); w.gain.value = wet;
  const inp = c.createGain(); inp.connect(o.bus); inp.connect(rv).connect(w).connect(o.bus); return { o, inp };
}
/** Chanting crowd: a rough "HEY!" bark from many throats (formant-filtered noise + buzzy saws). */
function hey(c, dest, t, peak = 0.3, r = Math.random) {
  const g = c.createGain(); env(g.gain, t, 0.03, peak, 0.35); const v = c.createGain(); v.connect(g).connect(dest);
  [[600, 1], [1700, 0.6], [2600, 0.3]].forEach(([fc, k]) => { const bp = c.createBiquadFilter(); bp.type = 'bandpass'; bp.Q.value = 4; bp.frequency.value = fc; const kg = c.createGain(); kg.gain.value = k * 2; noise(c, t, 0.5, 'pink', r).connect(bp).connect(kg).connect(v); });
  for (let i = 0; i < 8; i++) { const x = c.createOscillator(), xg = c.createGain(); x.type = 'sawtooth'; x.frequency.setValueAtTime(140 + r() * 120, t); x.frequency.exponentialRampToValueAtTime(100 + r() * 60, t + 0.4); xg.gain.value = 0.02; const bp = c.createBiquadFilter(); bp.type = 'bandpass'; bp.frequency.value = 700; bp.Q.value = 1.5; x.connect(bp).connect(xg).connect(v); x.start(t); x.stop(t + 0.45); }
}

// Explosion: the bullet-time heavy without its run-up (Yarden 20.11): the Cinematic Boom heavy at
// half speed, trimmed to the blast, pushed loud, with a sub and a burst of debris on top.
const blastHits = new Map();
const blastHit = (sr) => { if (!blastHits.has(sr)) blastHits.set(sr, renderOffline(2.4, (cc, d, t) => buildImpact('cinema', cc, d, t, 100, 5), sr)); return blastHits.get(sr); };
async function blast(c, dest, t0) {
  const hit = await blastHit(c.sampleRate), t = Math.max(t0, c.currentTime + 0.01), r = rng(71);
  const lim = c.createDynamicsCompressor(); lim.threshold.value = -4; lim.knee.value = 0; lim.ratio.value = 20; lim.attack.value = 0.001; lim.release.value = 0.15;
  const out = c.createGain(); out.gain.value = 1.0; const bus = c.createGain(); bus.gain.value = 1.0;
  bus.connect(drive(c, 2.2)).connect(lim).connect(out).connect(dest);
  const hs = c.createBufferSource(), hg = c.createGain(); hs.buffer = hit; hs.playbackRate.value = 0.5;
  hg.gain.setValueAtTime(1.6, t); hg.gain.setValueAtTime(1.6, t + 0.9); hg.gain.exponentialRampToValueAtTime(0.0001, t + 2.4); // trim the long tail
  hs.connect(hg).connect(bus); hs.start(t); hs.stop(t + 2.5);
  tone(c, bus, t, { f: 70, f2: 24, glide: 0.9, peak: 0.95, dec: 1.5 });                        // sub drop
  noiseHit(c, bus, t, { type: 'lowpass', f: 3200, peak: 0.9, dec: 0.32, color: 'pink', r });     // the blast itself
  noiseHit(c, bus, t, { type: 'lowpass', f: 400, peak: 0.7, dec: 0.9, color: 'brown', r });       // pressure wave
  for (let i = 0; i < 14; i++) noiseHit(c, bus, t + 0.25 + r() * 1.2, { type: 'bandpass', f: 1500 + r() * 3500, q: 2, peak: 0.12 * r(), dec: 0.02, r }); // debris
  return t0 + 2.6;
}

export const FIGHT_STARTS = [
  { id: 'banks', name: 'Floodlight banks', dur: 4.2, hit: 1.9,
    desc: 'Four contactor banks KA-CHUNK on one after another, each echoing off the far stands, the mains hum thickening with each; then a sub boom and the crowd.',
    leds: 'Columns switch on with each clunk, outer pair first (1, 4, then 2, 3), dim and flickering with the hum; full white flash on the boom, then fade to fight colours.',
    build(c, dest, t0) { const r = rng(31), o = chain(c, dest), A = t0 + 1.9;
      [0, 0.45, 0.85, 1.22].forEach((d, i) => { clunk(c, o, t0 + d, 0.75 + 0.1 * i, r); hum(c, o, t0 + d + 0.02, t0 + 3.6, 0.018 + 0.008 * i, 50, r); });
      sub(c, o, A, 0.9); crowd(c, o, A + 0.05, 2.3, 0.22, r); return t0 + 4.2; } },
  { id: 'titan', name: 'Banks + titan hit', dur: 4.0, hit: 1.35,
    desc: 'Three quick banks, then the last one is a massive slam with a tonal hit: inharmonic gong and a low brass chord, under a sub drop.',
    leds: 'Three columns pop on fast (1, 4, 2); the fourth (3) slams on with the hit and all four flash; the gong tail is a slow shimmer down from the tops.',
    build(c, dest, t0) { const r = rng(32), o = chain(c, dest), A = t0 + 1.35;
      [0, 0.32, 0.62].forEach((d, i) => { clunk(c, o, t0 + d, 0.7, r); hum(c, o, t0 + d, t0 + 3.4, 0.02 + 0.01 * i, 50, r); });
      clunk(c, o, A, 1.3, r); sub(c, o, A, 1.0, 1.9);
      [[98, 0.18, 2.6], [99.6, 0.12, 2.6], [147, 0.1, 2.2], [233, 0.09, 1.8], [349, 0.06, 1.4], [523, 0.04, 1.0]].forEach(([f, pk, d]) => tone(c, o.both, A + 0.004, { f, peak: pk, dec: d, a: 0.006 }));
      const lp = c.createBiquadFilter(); lp.type = 'lowpass'; lp.Q.value = 3; lp.frequency.setValueAtTime(300, A); lp.frequency.exponentialRampToValueAtTime(1600, A + 0.08); lp.frequency.exponentialRampToValueAtTime(250, A + 1.6);
      const g = c.createGain(); env(g.gain, A, 0.02, 0.09, 1.6); lp.connect(g).connect(o.both);
      [73.4, 110, 146.8, 174.6].forEach((f) => { const x = c.createOscillator(); x.type = 'sawtooth'; x.frequency.value = f; x.connect(lp); x.start(A); x.stop(A + 1.8); });
      crowd(c, o, A + 0.25, 2.3, 0.18, r); return t0 + 4.0; } },
  { id: 'breaker', name: 'Blackout breaker', dur: 3.6, hit: 0.75,
    desc: 'Near silence (just a fridge-quiet room tone), one enormous main-breaker slam, every light buzzing at once, then a war-drum double hit.',
    leds: 'Everything black; on the slam all four columns at once to full, buzzing (fast 50/100 Hz-style flicker for 0.3 s); each war drum is a pulse up the columns.',
    build(c, dest, t0) { const r = rng(33), o = chain(c, dest), A = t0 + 0.75;
      noiseHit(c, o.bus, t0, { type: 'lowpass', f: 300, a: 0.3, peak: 0.01, dec: 0.4, color: 'brown', r });
      clunk(c, o, A, 1.5, r); clunk(c, o, A + 0.012, 1.0, r); sub(c, o, A, 0.9, 1.4);
      hum(c, o, A + 0.01, t0 + 3.3, 0.07, 50, r);
      drums(c, o, A + 0.85, [[0, 0.7], [0.22, 0.9]], r); crowd(c, o, A + 1.1, 1.8, 0.14, r); return t0 + 3.6; } },
  { id: 'powerup', name: 'Arena power-up', dur: 3.8, hit: 1.95,
    desc: 'A generator spins up (rising turbine whine and rumble), three heavy clunks as it reaches speed, then the crowd roars over the sub.',
    leds: 'A dim glow climbs all four columns with the whine (height = spin speed); the three clunks lock columns 1+4, then 2+3, then all at full; flash on the roar.',
    build(c, dest, t0) { const r = rng(34), o = chain(c, dest), A = t0 + 1.95;
      const w = c.createOscillator(), wg = c.createGain(), wf = c.createBiquadFilter(); w.type = 'sawtooth'; wf.type = 'bandpass'; wf.Q.value = 4;
      w.frequency.setValueAtTime(40, t0); w.frequency.exponentialRampToValueAtTime(420, t0 + 1.6); w.frequency.exponentialRampToValueAtTime(400, A + 1);
      wf.frequency.setValueAtTime(200, t0); wf.frequency.exponentialRampToValueAtTime(2200, t0 + 1.6);
      wg.gain.setValueAtTime(0.0001, t0); wg.gain.exponentialRampToValueAtTime(0.12, t0 + 1.5); wg.gain.exponentialRampToValueAtTime(0.03, A + 0.3); wg.gain.exponentialRampToValueAtTime(0.0001, A + 1.4);
      w.connect(wf).connect(wg).connect(o.both); w.start(t0); w.stop(A + 1.5);
      const tn = c.createBiquadFilter(); tn.type = 'bandpass'; tn.Q.value = 1; tn.frequency.setValueAtTime(150, t0); tn.frequency.exponentialRampToValueAtTime(1800, t0 + 1.6);
      const tg = c.createGain(); tg.gain.setValueAtTime(0.0001, t0); tg.gain.exponentialRampToValueAtTime(0.15, t0 + 1.55); tg.gain.exponentialRampToValueAtTime(0.0001, A + 0.6);
      noise(c, t0, A - t0 + 0.7, 'pink', r).connect(tn).connect(tg).connect(o.both);
      [1.6, 1.78, A - t0].forEach((d, i) => clunk(c, o, t0 + d, 0.8 + 0.25 * i, r));
      hum(c, o, t0 + 1.6, t0 + 3.5, 0.04, 50, r); sub(c, o, A, 0.85); crowd(c, o, A, 1.8, 0.24, r); return t0 + 3.8; } },
  { id: 'vacuum', name: 'Vacuum (surprise)', dur: 3.4, hit: 1.25,
    desc: 'The stadium inhales: a reversed swell sucks all the air in, a dead-silent beat, then lights, a sub and a huge "AAH" choir chord stab hit together.',
    leds: 'Columns drain downward with the inhale (whatever was lit sinks to the base), black for the silent beat, then all four detonate from the base up on the stab.',
    build(c, dest, t0) { const r = rng(35), o = chain(c, dest), A = t0 + 1.25;
      const f = c.createBiquadFilter(); f.type = 'bandpass'; f.Q.value = 1.2; f.frequency.setValueAtTime(5000, t0); f.frequency.exponentialRampToValueAtTime(250, A - 0.2);
      const g = c.createGain(); g.gain.setValueAtTime(0.0001, t0); g.gain.exponentialRampToValueAtTime(0.35, A - 0.21); g.gain.linearRampToValueAtTime(0, A - 0.2);
      noise(c, t0, A - t0, 'white', r).connect(f).connect(g).connect(o.bus);
      tone(c, o.bus, t0 + 0.05, { f: 55, f2: 110, glide: 1.0, a: 0.9, peak: 0.25, dec: 0.12 });
      clunk(c, o, A, 1.2, r); sub(c, o, A, 1.0, 1.8); hum(c, o, A, t0 + 3.1, 0.03, 50, r);
      // choir stab: sawtooth chord through vowel formants (ah: 730, 1090, 2440 Hz)
      const vg = c.createGain(); env(vg.gain, A, 0.03, 0.22, 1.7); const vo = c.createGain(); vg.connect(o.both);
      [[730, 1], [1090, 0.6], [2440, 0.3]].forEach(([fc, k]) => { const bp = c.createBiquadFilter(); bp.type = 'bandpass'; bp.Q.value = 6; bp.frequency.value = fc; const kg = c.createGain(); kg.gain.value = k * 3; vo.connect(bp).connect(kg).connect(vg); });
      [110, 130.8, 164.8, 220, 261.6].forEach((fr) => [-9, 0, 8].forEach((dt) => { const x = c.createOscillator(); x.type = 'sawtooth'; x.frequency.value = fr; x.detune.value = dt; const xg = c.createGain(); xg.gain.value = 0.08; x.connect(xg).connect(vo); x.start(A); x.stop(A + 1.9); }));
      crowd(c, o, A + 0.3, 1.8, 0.12, r); return t0 + 3.4; } },
  { id: 'bell3', name: 'Ring bell: ding-ding-ding', dur: 3.2, hit: 0.0,
    desc: 'The classic: three quick strikes of a brass ring bell, slightly uneven like a real timekeeper, ringing out in a small hall.',
    leds: 'Three white-gold pulses, all columns, one per ding, each decaying with the ring; fight colours fade in under the third.',
    build(c, dest, t0) { const r = rng(41), { inp } = bellOut(c, dest, 0.22, 1.8); [0, 0.27, 0.55].forEach((d, i) => bell(c, inp, t0 + d, { size: i === 2 ? 1 : 0.85, dec: i === 2 ? 2.4 : 1.2, r })); return t0 + 3.2; } },
  { id: 'bellbig', name: 'Ring bell: one huge strike', dur: 4.0, hit: 0.0,
    desc: 'A single cinematic strike of a bell the size of the room: pitched down, sub underneath, a long cathedral tail.',
    leds: 'One full flash, then the brightness decays exactly with the ring over ~3 s while the fight colours rise underneath.',
    build(c, dest, t0) { const r = rng(42), { o, inp } = bellOut(c, dest, 0.6, 4); bell(c, inp, t0, { f: 520, size: 1.5, dec: 3.6, r }); sub(c, o, t0, 0.7, 1.6); return t0 + 4.0; } },
  { id: 'bellbanks', name: 'Bell + floodlights', dur: 4.0, hit: 1.25,
    desc: 'Two floodlight banks clunk on in the dark, then the ring bell rings ding-ding-ding as the last bank and the sub hit land together.',
    leds: 'Columns 1+4 on with the first clunk, 2+3 with the second; the dings are three full pulses on top.',
    build(c, dest, t0) { const r = rng(43), { o, inp } = bellOut(c, dest, 0.25, 2); const A = t0 + 1.25;
      [0, 0.5].forEach((d, i) => { clunk(c, o, t0 + d, 0.8, r); hum(c, o, t0 + d, t0 + 3.6, 0.025 + 0.01 * i, 50, r); });
      clunk(c, o, A, 1.1, r); sub(c, o, A, 0.8); [0, 0.25, 0.5].forEach((d) => bell(c, inp, A + d, { size: 0.9, dec: d === 0.5 ? 2.2 : 1.1, r })); return t0 + 4.0; } },
  { id: 'bellcrowd', name: 'Bell + crowd roar', dur: 3.8, hit: 0.0,
    desc: 'Ding-ding-ding and a packed arena erupting under it: roar, whistles, a drum hit on the third ding.',
    leds: 'Three pulses with the dings, then a fast random sparkle across all columns for the crowd (like camera flashes).',
    build(c, dest, t0) { const r = rng(44), { o, inp } = bellOut(c, dest, 0.22, 1.8);
      [0, 0.27, 0.55].forEach((d, i) => bell(c, inp, t0 + d, { size: 0.9, dec: i === 2 ? 2.2 : 1.1, r })); crowd(c, o, t0 + 0.1, 3.4, 0.3, r); drums(c, o, t0 + 0.55, [[0, 0.8]], r); return t0 + 3.8; } },
  { id: 'rustbell', name: 'Rusty bell: ding-ding-ding', dur: 3.0, hit: 0.0, rough: true,
    desc: 'The ring bell, but a dented, rusty one: no clean note, beating out-of-tune partials, a loose clapper rattle, through an overdriven PA in a concrete room.',
    leds: 'Three ragged pulses (a little flicker on each), dirty orange-white instead of gold.',
    build(c, dest, t0) { const r = rng(61), { inp } = roughRoom(c, dest, 0.3, 1.3), d = dirt(c, inp, { amount: 6, hp: 250, lp: 5000 });
      [0, 0.24, 0.5].forEach((dd, i) => junk(c, d, t0 + dd + r() * 0.03, { f: 690, ratios: [1, 1.19, 2.31, 2.9, 4.2, 5.95], size: i === 2 ? 1 : 0.8, dec: i === 2 ? 1.8 : 0.8, r })); return t0 + 3.0; } },
  { id: 'pipe', name: 'Steel pipe x3 + roar', dur: 3.8, hit: 0.0, rough: true,
    desc: 'Someone beats a scaffolding pipe with a hammer three times, hard; a low sub kick under each, then the crowd erupts, all slightly distorted.',
    leds: 'Three hard white hits, columns shaking (random brightness jitter), then crowd sparkle.',
    build(c, dest, t0) { const r = rng(62), { o, inp } = roughRoom(c, dest, 0.25, 1.6), d = dirt(c, inp, { amount: 7, hp: 90, lp: 6000 });
      [0, 0.3, 0.62].forEach((dd, i) => { junk(c, d, t0 + dd, { f: 330, ratios: [1, 2.76, 5.4, 8.93, 13.3], size: 0.9 + 0.15 * i, dec: 0.7 + 0.5 * (i === 2), r }); tone(c, o.bus, t0 + dd, { f: 110, f2: 40, glide: 0.15, peak: 0.6, dec: 0.3 }); });
      crowd(c, o, t0 + 0.7, 3.0, 0.32, r); return t0 + 3.8; } },
  { id: 'alarm', name: 'Fire-alarm bell', dur: 3.2, hit: 0.0, rough: true,
    desc: 'An electric school/fire-alarm bell: the hammer rattles the gong 22 times a second for a second and a half, grating and urgent, then cuts with a thump and the crowd.',
    leds: 'Fast red-white strobe on all columns while it rings; hard cut to fight colours.',
    build(c, dest, t0) { const r = rng(63), { o, inp } = roughRoom(c, dest, 0.2, 1.0), d = dirt(c, inp, { amount: 4, hp: 300, lp: 7000 });
      const n = 33; for (let i = 0; i < n; i++) junk(c, d, t0 + i / 22 + (r() - 0.5) * 0.004, { f: 1180, ratios: [1, 2.42, 3.1, 4.9], size: 0.55, dec: 0.18, r });
      const end = t0 + n / 22; tone(c, o.bus, end, { f: 90, f2: 35, glide: 0.2, peak: 0.8, dec: 0.4 }); crowd(c, o, end, 1.6, 0.26, r); return t0 + 3.2; } },
  { id: 'blownbell', name: 'Bell + roar, blown speaker', dur: 3.8, hit: 0.0, rough: true,
    desc: 'Bell + crowd roar (J) as if played through a cheap megaphone that is way past its limit: crushed, crackling, mid-range only.',
    leds: 'Like J, but the pulses clip: flat full-brightness tops with crackle.',
    build(c, dest, t0) { const r = rng(64), o = chain(c, dest), d = dirt(c, o.both, { amount: 12, hp: 400, lp: 3200, crush: 6, gain: 0.5 });
      [0, 0.27, 0.55].forEach((dd, i) => bell(c, d, t0 + dd, { size: 1.2, dec: i === 2 ? 2.0 : 1.0, r })); crowd(c, { both: d, bus: d }, t0 + 0.1, 3.4, 0.35, r); drums(c, o, t0 + 0.55, [[0, 0.9]], r); return t0 + 3.8; } },
  { id: 'drum', name: 'Oil drum + "HEY!"', dur: 3.4, hit: 0.0, rough: true,
    desc: 'Three slams on an empty oil barrel, a crowd barking "HEY!" on each, and a final roar. Raw, tribal, no bell at all.',
    leds: 'Each slam punches all columns from the base up; the HEYs are short red flashes.',
    build(c, dest, t0) { const r = rng(65), { o, inp } = roughRoom(c, dest, 0.35, 1.8), d = dirt(c, inp, { amount: 5, hp: 60, lp: 3500 });
      [0, 0.42, 0.84].forEach((dd, i) => { const t = t0 + dd; tone(c, d, t, { f: 85, f2: 52, glide: 0.2, peak: 0.9, dec: 0.5 }); junk(c, d, t, { f: 140, ratios: [1, 1.6, 2.3, 3.4, 4.7], size: 0.9, dec: 0.6, r }); hey(c, o.both, t + 0.06, 0.28 + 0.06 * i, r); });
      crowd(c, o, t0 + 1.1, 2.2, 0.3, r); return t0 + 3.4; } },
  { id: 'blast', name: 'Explosion (bullet-time hit)', dur: 2.6, hit: 0.0, rough: true,
    desc: 'Just the bullet-time heavy impact, loud: no backwards swell, no tail. The full shout meter detonates: a half-speed Cinematic Boom heavy, a sub drop, a blast of pressure and falling debris.',
    leds: 'Everything at full white on the blast, then a fast fade into fight colours with flickering embers for the debris.',
    build: (c, dest, t0) => blast(c, dest, t0) },
];
