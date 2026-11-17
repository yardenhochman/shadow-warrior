// Scripted mini-fights, rendered offline into one buffer, with captions for the timeline.
import { SR, rng, env, noiseHit, tone, drive, normalize } from './dsp.js';
import { buildImpact, tierOf } from './impacts.js';
import { dojoWind, singingBowls, heartbeat, crowdSwell, victory, bulletTime } from './ambience.js';
import { FIGHT_STARTS } from './fightstart.js';
import { BreakRule, buildBreak, BREAK_BUS_GAIN } from './breakstuff.js';

const raw = new Map();
async function voice(c, file) {
  if (!raw.has(file)) raw.set(file, fetch(file).then((r) => { if (!r.ok) throw new Error(`${file}: ${r.status}`); return r.arrayBuffer(); }));
  return c.decodeAudioData((await raw.get(file)).slice(0));
}
function play(c, dest, buf, t, gain = 1) { const s = c.createBufferSource(), g = c.createGain(); s.buffer = buf; g.gain.value = gain; s.connect(g).connect(dest); s.start(t); return buf.duration; }

function fightLoop(c, dest, t0, t1, bpm = 128) {
  const beat = 60 / bpm; let s = 0;
  for (let t = t0; t < t1; t += beat / 4, s++) {
    if (s % 4 === 0) tone(c, dest, t, { f: 110, f2: 42, glide: 0.12, peak: 0.5, dec: 0.22 });
    if (s % 8 === 4) noiseHit(c, dest, t, { type: 'bandpass', f: 1800, peak: 0.22, dec: 0.12 });
    if (s % 2 === 1) noiseHit(c, dest, t, { type: 'highpass', f: 7000, peak: 0.06, dec: 0.04 });
    if (s % 4 === 2) { const o = c.createOscillator(), f = c.createBiquadFilter(), g = c.createGain(); o.type = 'sawtooth'; o.frequency.value = [55, 55, 65.4, 49][Math.floor(s / 16) % 4]; f.type = 'lowpass'; f.frequency.value = 320; env(g.gain, t, 0.005, 0.16, beat * 0.9); o.connect(f).connect(g).connect(dest); o.start(t); o.stop(t + beat); }
  }
}

/** An original drop-tuned rock groove at a Break-Stuff-like tempo (no part of the song itself). */
function rockBeat(c, dest, t0, t1, bpm = 106) {
  const beat = 60 / bpm, q = beat / 4, gtr = c.createGain(), sh = drive(c, 8), lp = c.createBiquadFilter();
  lp.type = 'lowpass'; lp.frequency.value = 2600; gtr.gain.value = 0.11; sh.connect(lp).connect(gtr).connect(dest);
  const riff = [41.2, 0, 41.2, 41.2, 0, 49, 0, 41.2, 55, 0, 41.2, 0, 51.9, 49, 0, 0]; // E1-ish chugs, original pattern
  let s = 0;
  for (let t = t0; t < t1; t += q, s++) {
    const st = s % 16;
    if ([0, 6, 8, 11].includes(st)) tone(c, dest, t, { f: 95, f2: 40, glide: 0.1, peak: 0.55, dec: 0.2 });
    if (st === 4 || st === 12) { noiseHit(c, dest, t, { type: 'bandpass', f: 1900, q: 0.8, peak: 0.32, dec: 0.16 }); tone(c, dest, t, { f: 190, f2: 150, glide: 0.05, peak: 0.15, dec: 0.08 }); }
    if (s % 2 === 0) noiseHit(c, dest, t, { type: 'highpass', f: 6500, peak: st % 4 === 2 ? 0.09 : 0.05, dec: st % 8 === 6 ? 0.18 : 0.04 });
    const f = riff[st]; if (f) for (const k of [1, 1.5, 2]) { const o = c.createOscillator(), g = c.createGain(); o.type = 'sawtooth'; o.frequency.value = f * 2 * k; env(g.gain, t, 0.003, 0.5, q * 0.85); o.connect(g).connect(sh); o.start(t); o.stop(t + q); }
  }
}

/** Demo 1, "Demon temple": pre-fight insult, singing-bowl charge, floodlights, a combo, bullet-time K.O. */
export async function demoTemple(c, out) {
  const ev = [], at = (t, text) => ev.push({ t, text });
  const V = (f) => voice(c, `voice/${f}.mp3`);
  const [zero, energy, combo3, weak, ko, chuckle] = await Promise.all(['hebrew/zero1', 'monk/energy', 'monk/combo3', 'hebrew/weak3', 'monk/ko', 'laugh/chuckle'].map(V));
  const music = c.createGain(); music.gain.value = 0.45; music.connect(out);
  dojoWind(c, out, 0, 11.6, 0, 1); at(0, 'IDLE: night wind, crickets. Someone walks in...');
  play(c, out, zero, 0.3, 0.85); at(0.3, 'Shadow (Hebrew, butler-demon): "Excuse me... you... you... never mind. Zero."');
  singingBowls(c, out, 4.0); at(4.0, 'WARMING: each shout stage strikes a singing bowl');
  play(c, out, energy, 7.0, 0.9); at(7.0, 'Demon monk: "Your breath is full. The fire is awake."');
  const fsB = FIGHT_STARTS[0]; fsB.build(c, out, 10.6); at(10.6, `Fight start B: ${fsB.name}`);
  dojoWind(c, out, 11.6, 15, 0.6, 0.4, 5);
  fightLoop(c, music, 13.0, 26.2);
  const hit = (t, p, fam) => { buildImpact(fam || (p >= 75 ? 'cinema' : 'leather'), c, out, t, p, Math.round(t * 100)); at(t, `${tierOf(p)} ${p}`); };
  hit(13.6, 28); hit(13.95, 34); hit(14.3, 61);
  play(c, out, combo3, 14.6, 0.9); at(14.6, 'Demon monk: "Three. Let it flow."');
  hit(17.3, 14); play(c, out, chuckle, 17.5, 0.8); at(17.5, 'Weak jab: taunt chuckle');
  play(c, out, weak, 18.6, 0.85); at(18.6, 'Shadow (Hebrew): "Thank you so much for the massage. Very pleasant."');
  hit(21.0, 66); hit(21.3, 31); hit(21.6, 58); hit(21.9, 38); hit(22.2, 90);
  crowdSwell(c, out, 22.2, 3.5); at(22.2, 'Combo 5: crowd swell');
  heartbeat(c, out, 22.6, 4.0, 110, 150); at(22.6, 'Shadow low HP: heartbeat');
  hit(23.4, 84);
  await bulletTime(c, out, 24.0); at(24.0, 'K.O. blow in bullet time: reverse swell...'); at(26.2, '...lands at half speed');
  music.gain.setValueAtTime(0.45, 26.15); music.gain.linearRampToValueAtTime(0, 26.3);
  play(c, out, ko, 26.4, 0.9); at(26.4, 'Demon monk: "Be still now. K.O."');
  victory(c, out, 28.6); at(28.6, 'Victory stinger');
  return { dur: 31.5, ev };
}

/** Demo 2, "Break Stuff mode": rock groove, bag hits; the BreakRule decides which punches smash something. */
export async function demoBreak(c, out, seed = 1) {
  const ev = [], r = rng(seed), rule = new BreakRule({ rand: r });
  const music = c.createGain(); music.gain.value = 0.55; music.connect(out);
  const brk = c.createGain(); brk.gain.value = BREAK_BUS_GAIN; brk.connect(out);
  rockBeat(c, music, 0.2, 22.5);
  ev.push({ t: 0.2, text: 'Track detected: "Break Stuff" (stand-in groove, the song is not bundled)' });
  // a scripted fight: [time, power]; combos are hits < 1.2 s apart
  const pattern = [[2.0, 30], [2.4, 22], [2.8, 55], [4.0, 18], [4.4, 40], [4.8, 62], [5.2, 35], [5.6, 80], [7.5, 27], [7.9, 33], [8.3, 48], [9.6, 92],
    [11.0, 25], [11.3, 20], [11.6, 57], [11.9, 31], [12.2, 68], [12.5, 44], [12.8, 76], [14.5, 15], [15.0, 35], [15.4, 58], [15.8, 98],
    [17.5, 28], [17.8, 52], [18.1, 33], [18.4, 70], [18.7, 85], [19.0, 40], [19.3, 95]];
  let combo = 0, lastT = -9;
  for (const [t, p] of pattern) {
    combo = t - lastT < 1.2 ? combo + 1 : 1; lastT = t;
    buildImpact(p >= 75 ? 'cinema' : 'leather', c, out, t, p, Math.round(t * 10));
    const b = rule.decide(p, { now: t, combo });
    if (b) { buildBreak(b.kind, c, brk, t + 0.015, b.alt, b.gain); ev.push({ t, text: `${tierOf(p)} ${p} (combo ${combo}) -> ${b.kind.toUpperCase()} [${b.reason}]` }); }
    else ev.push({ t, text: `${tierOf(p)} ${p} (combo ${combo})`, dim: true });
  }
  return { dur: 23.5, ev };
}

export async function renderDemo(fn, ...args) {
  const c = new OfflineAudioContext(1, Math.ceil(36 * SR), SR), out = c.createGain();
  const comp = c.createDynamicsCompressor(); comp.threshold.value = -10; comp.knee.value = 6; comp.ratio.value = 4; comp.attack.value = 0.003; comp.release.value = 0.2;
  out.gain.value = 0.62; out.connect(comp).connect(c.destination);
  const { dur, ev } = await fn(c, out, ...args);
  const full = await c.startRendering();
  const b = new AudioBuffer({ length: Math.ceil(dur * SR), sampleRate: SR, numberOfChannels: 1 });
  b.copyToChannel(full.getChannelData(0).subarray(0, b.length), 0);
  normalize(b, -18, -1.5); // both demos at the same level, under the -16 LUFS clips
  return { buffer: b, ev: ev.sort((a, x) => a.t - x.t) };
}
