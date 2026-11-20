// Synthesised sound (no assets): punch thumps, shout chime, fight start, KO, victory and
// fight music: the playlist (music.js) when loaded, else a synth drum loop.
import { VICTORY } from './hitfx.js';
import { FIGHT_STARTS } from '../sound-lab/js/fightstart.js';
import { buildImpact } from '../sound-lab/js/impacts.js';
import { defeat as buildDefeat } from '../sound-lab/js/ambience.js';
import { BreakRule, buildBreak, isBreakStuffTrack, BREAK_BUS_GAIN } from '../sound-lab/js/breakstuff.js';

// Punch sound families the game can switch between (header picker). 'current' = the synth below.
export const IMPACT_CHOICES = [{ id: 'current', name: 'Punch: original' }, { id: 'cinema', name: 'Punch: Cinematic Boom' }, { id: 'anime', name: 'Punch: Anime Crack' }];

// Fight-start sounds the game can switch between (header picker). 'current' = buildArenaOpen below.
export const OPEN_CHOICES = [
  { id: 'current', name: 'Arena opening (original)' },
  ...['blast', 'bellcrowd', 'bell3', 'rustbell', 'pipe', 'alarm', 'blownbell', 'drum'].map((id) => FIGHT_STARTS.find((f) => f.id === id)).filter(Boolean)
    .map((f) => ({ id: f.id, name: f.rough ? `${f.name} (rough)` : f.name, build: f.build, loud: f.loud })),
];
export class Sfx {
  constructor() { this.ctx = null; this.enabled = true; this.music = null; this.noise = null; this.playlist = null; this.openId = 'blast'; this.impactId = 'current'; this.impactSeed = 1; }

  ensure() {
    if (!this.ctx) {
      const AC = window.AudioContext || window.webkitAudioContext;
      this.ctx = new AC();
      this.master = this.ctx.createGain(); this.master.gain.value = 0.7; this.master.connect(this.ctx.destination);
      const len = this.ctx.sampleRate;
      this.noise = this.ctx.createBuffer(1, len, this.ctx.sampleRate);
      const d = this.noise.getChannelData(0);
      for (let i = 0; i < len; i++) d[i] = Math.random() * 2 - 1;
    }
    if (this.ctx.state === 'suspended') this.ctx.resume();
    return this.ctx;
  }

  env(g, t, a, peak, dec) { g.gain.setValueAtTime(0.0001, t); g.gain.exponentialRampToValueAtTime(peak, t + a); g.gain.exponentialRampToValueAtTime(0.0001, t + a + dec); }

  punch(power, sig) {
    if (!this.enabled || !this.ctx) return;
    const c = this.ctx, t = c.currentTime, p = power / 100;
    const look = sig?.look || sig?.kind;
    const nova = look === 'nova', t1 = nova ? t + 0.2 + 0.06 * (sig.esc || 0) : t; // nova: the hit lands after the blackout beat
    if (this.impactId !== 'current') { buildImpact(this.impactId, c, this.master, t1, Math.max(1, Math.min(100, power)), this.impactSeed++); return; }
    const o = c.createOscillator(), g = c.createGain();
    o.frequency.setValueAtTime(140 + 60 * p, t1); o.frequency.exponentialRampToValueAtTime(40, t1 + 0.18);
    this.env(g, t1, 0.004, 0.5 + 0.5 * p, 0.2 + 0.15 * p);
    o.connect(g).connect(this.master); o.start(t1); o.stop(t1 + 0.5);
    this.noiseHit(t1, 'lowpass', 900 + 2500 * p, 0.25 + 0.4 * p, 0.09);
    if (!sig || sig.kind === 'classic') return;
    if (sig.tier === 'hook') { this.noiseHit(t + 0.01, 'bandpass', 1400 + 8 * sig.hue, 0.18, 0.06); return; }
    if (sig.tier !== 'heavy') return;
    // Heavy: a sub drop under every signature, plus a layer of its own.
    const sub = c.createOscillator(), sg = c.createGain();
    sub.frequency.setValueAtTime(75, t1); sub.frequency.exponentialRampToValueAtTime(30, t1 + 0.55);
    this.env(sg, t1, 0.006, 0.55, 0.6); sub.connect(sg).connect(this.master); sub.start(t1); sub.stop(t1 + 0.8);
    switch (look) {
      case 'thunder': for (const d of [0, 0.08, 0.19, 0.31]) { this.noiseHit(t + d, 'highpass', 2500, 0.22, 0.05); this.noiseHit(t + d + 0.01, 'bandpass', 600, 0.15, 0.14); } break;
      case 'gold': [1320, 1760, 2637].forEach((f, i) => this.tone(t + 0.04 + i * 0.05, 'sine', f, 0.07, 0.7)); break;
      case 'shatter': this.noiseHit(t + 0.03, 'highpass', 4000, 0.3, 0.35); [2093, 3136, 2637].forEach((f, i) => this.tone(t + 0.05 + i * 0.04, 'triangle', f * (0.97 + 0.06 * Math.random()), 0.05, 0.3)); break;
      case 'strobe': [880, 1175, 1568, 1319, 988].slice(0, 3 + Math.round(2 * (sig.esc || 0))).forEach((f, i) => this.tone(t + i * 0.13, 'square', f, 0.04, 0.06)); break;
      case 'split': { const s2 = c.createOscillator(), g2 = c.createGain(), f2 = c.createBiquadFilter(); s2.type = 'sawtooth'; f2.type = 'bandpass'; f2.Q.value = 4;
        f2.frequency.setValueAtTime(300, t); f2.frequency.exponentialRampToValueAtTime(2400, t + 0.35); s2.frequency.value = 110;
        this.env(g2, t, 0.01, 0.12, 0.4); s2.connect(f2).connect(g2).connect(this.master); s2.start(t); s2.stop(t + 0.5); break; }
      case 'nova': { // reverse whoosh into the hit
        const n = c.createBufferSource(); n.buffer = this.noise; const f = c.createBiquadFilter(); f.type = 'bandpass'; f.frequency.setValueAtTime(400, t); f.frequency.exponentialRampToValueAtTime(3000, t1);
        const g3 = c.createGain(); g3.gain.setValueAtTime(0.0001, t); g3.gain.exponentialRampToValueAtTime(0.25, t1 - 0.01); g3.gain.linearRampToValueAtTime(0, t1);
        n.connect(f).connect(g3).connect(this.master); n.start(t); n.stop(t1 + 0.05); break; }
      default: break;
    }
  }

  noiseHit(t, type, freq, peak, dec) {
    const c = this.ctx, n = c.createBufferSource(); n.buffer = this.noise;
    const f = c.createBiquadFilter(); f.type = type; f.frequency.value = freq;
    const g = c.createGain(); this.env(g, t, 0.002, peak, dec);
    n.connect(f).connect(g).connect(this.master); n.start(t); n.stop(t + dec + 0.1);
  }
  tone(t, type, freq, peak, dec) {
    const c = this.ctx, o = c.createOscillator(), g = c.createGain(); o.type = type; o.frequency.value = freq;
    this.env(g, t, 0.003, peak, dec); o.connect(g).connect(this.master); o.start(t); o.stop(t + dec + 0.05);
  }

  shout(intensity) {
    if (!this.enabled || !this.ctx) return;
    const c = this.ctx, t = c.currentTime;
    for (const [fr, dl] of [[660, 0], [990, 0.05]]) {
      const o = c.createOscillator(), g = c.createGain(); o.type = 'triangle';
      o.frequency.value = fr * (1 + intensity * 0.25);
      this.env(g, t + dl, 0.01, 0.12, 0.25); o.connect(g).connect(this.master); o.start(t + dl); o.stop(t + dl + 0.4);
    }
  }

  boom() {
    if (!this.enabled || !this.ctx) return;
    const c = this.ctx, t = c.currentTime;
    const o = c.createOscillator(), g = c.createGain(); o.type = 'sawtooth';
    o.frequency.setValueAtTime(80, t); o.frequency.exponentialRampToValueAtTime(30, t + 1.2);
    const f = c.createBiquadFilter(); f.type = 'lowpass'; f.frequency.value = 400;
    this.env(g, t, 0.01, 0.8, 1.3); o.connect(f).connect(g).connect(this.master); o.start(t); o.stop(t + 1.5);
  }

  /** FIGHT starts: the arena opens (riser -> impact at ARENA_HIT s, synced with the LED burst). Once per fight start. */
  arenaOpen() {
    if (!this.enabled || !this.ctx) return;
    const now = performance.now();
    if (this.lastOpen && now - this.lastOpen < 1500) return;
    this.lastOpen = now;
    const pick = OPEN_CHOICES.find((o) => o.id === this.openId);
    // 'loud' starts skip the master volume (0.7) so the blast lands at full scale over the music
    if (pick && pick.build) pick.build(this.ctx, pick.loud ? this.ctx.destination : this.master, this.ctx.currentTime + 0.01);
    else buildArenaOpen(this.ctx, this.master, this.ctx.currentTime + 0.01, this.noise);
  }

  /** Play a recorded voice line (mp3/wav URL) over everything, through the master bus. */
  async voice(url, gain = 1) {
    if (!this.enabled) return;
    this.ensure(); this.voiceCache = this.voiceCache || new Map();
    try {
      let buf = this.voiceCache.get(url);
      if (!buf) { const r = await fetch(url); if (!r.ok) return; buf = await this.ctx.decodeAudioData(await r.arrayBuffer()); this.voiceCache.set(url, buf); }
      const src = this.ctx.createBufferSource(), g = this.ctx.createGain(); g.gain.value = gain; src.buffer = buf; src.connect(g).connect(this.master); src.start();
    } catch {}
  }

  /** While "Break Stuff" plays, punches now and then smash something sized to the punch (sound lab Break Stuff objects, Yarden 20.11). */
  breakStuff(power, combo) {
    if (!this.enabled || !this.ctx || !this.playlist?.playing || !isBreakStuffTrack(this.playlist.current)) return null;
    // object size follows the punch: jabs rarely break something small (glass, cup, twig), hooks medium, heavies big
    this.breakRule = this.breakRule || new BreakRule({ chance: { jab: 0.06, hook: 0.2, heavy: 0.5 } });
    const b = this.breakRule.decide(power, { now: this.ctx.currentTime, combo });
    if (!b) return null;
    if (!this.breakBus) { this.breakBus = this.ctx.createGain(); this.breakBus.gain.value = BREAK_BUS_GAIN; this.breakBus.connect(this.master); }
    buildBreak(b.kind, this.ctx, this.breakBus, this.ctx.currentTime + 0.03, b.alt, b.gain);
    return b;
  }

  /** Game over without a single K.O.: three sinking notes and a tape-stop dive (sound lab Defeat stinger). */
  defeat() {
    if (!this.enabled || !this.ctx) return;
    this.stopMusic(); buildDefeat(this.ctx, this.master, this.ctx.currentTime + 0.05);
  }

  /** Shadow defeated: the victory fanfare (~6 s), its climax on VICTORY.climax with the LED finale. */
  victory() {
    if (!this.enabled || !this.ctx) return;
    buildVictory(this.ctx, this.master, this.ctx.currentTime + 0.01, this.noise);
  }

  ko() {
    if (!this.enabled || !this.ctx) return;
    this.boom();
    const c = this.ctx, t = c.currentTime;
    [523, 659, 784, 1046].forEach((fr, i) => {
      const o = c.createOscillator(), g = c.createGain(); o.type = 'square';
      o.frequency.value = fr; this.env(g, t + i * 0.08, 0.01, 0.08, 0.3);
      o.connect(g).connect(this.master); o.start(t + i * 0.08); o.stop(t + i * 0.08 + 0.4);
    });
  }

  startMusic() {
    if (!this.enabled) return;
    if (this.playlist && this.playlist.available) { if (!this.playlist.playing) this.playlist.play(); return; }
    if (!this.ctx || this.music) return;
    const c = this.ctx, bpm = 128, beat = 60 / bpm;
    let next = c.currentTime + 0.05, step = 0;
    const tick = () => {
      while (next < c.currentTime + 0.2) {
        const t = next;
        if (step % 4 === 0 || step % 16 === 10) { // kick
          const o = c.createOscillator(), g = c.createGain();
          o.frequency.setValueAtTime(110, t); o.frequency.exponentialRampToValueAtTime(42, t + 0.12);
          this.env(g, t, 0.003, 0.45, 0.22); o.connect(g).connect(this.master); o.start(t); o.stop(t + 0.3);
        }
        if (step % 2 === 1) { // hat
          const n = c.createBufferSource(); n.buffer = this.noise;
          const f = c.createBiquadFilter(); f.type = 'highpass'; f.frequency.value = 7000;
          const g = c.createGain(); this.env(g, t, 0.001, 0.05, 0.04);
          n.connect(f).connect(g).connect(this.master); n.start(t); n.stop(t + 0.06);
        }
        if (step % 8 === 4) { // snare-ish
          const n = c.createBufferSource(); n.buffer = this.noise;
          const f = c.createBiquadFilter(); f.type = 'bandpass'; f.frequency.value = 1800;
          const g = c.createGain(); this.env(g, t, 0.002, 0.18, 0.12);
          n.connect(f).connect(g).connect(this.master); n.start(t); n.stop(t + 0.2);
        }
        if (step % 4 === 2) { // bass
          const o = c.createOscillator(), g = c.createGain(); o.type = 'sawtooth';
          const notes = [55, 55, 65.4, 49];
          o.frequency.value = notes[Math.floor(step / 16) % 4];
          const f = c.createBiquadFilter(); f.type = 'lowpass'; f.frequency.value = 300;
          this.env(g, t, 0.005, 0.12, beat * 0.9); o.connect(f).connect(g).connect(this.master); o.start(t); o.stop(t + beat);
        }
        next += beat / 4; step++;
      }
    };
    this.music = setInterval(tick, 50); tick();
  }

  stopMusic() { if (this.playlist) this.playlist.stop(); if (this.music) { clearInterval(this.music); this.music = null; } }
}

export const ARENA_HIT = 0.35;
/**
 * The arena-opening sound (~2.8 s), built on any (Offline)AudioContext into `dest`:
 * a noise riser into a sub-boom impact, an inharmonic gong with a long tail, a war-drum
 * double hit and a crowd-like filtered noise swell, through a compressor so it sits over the
 * fight music without clipping.
 */
export function buildArenaOpen(c, dest, t0, noise) {
  if (!noise) {
    noise = c.createBuffer(1, c.sampleRate, c.sampleRate);
    const d = noise.getChannelData(0); for (let i = 0; i < d.length; i++) d[i] = Math.random() * 2 - 1;
  }
  const A = t0 + ARENA_HIT;
  const comp = c.createDynamicsCompressor();
  comp.threshold.value = -14; comp.knee.value = 8; comp.ratio.value = 6; comp.attack.value = 0.003; comp.release.value = 0.25;
  // bus -> compressor -> fast limiter -> out: peak stays a few dB under full scale over the music
  const lim = c.createDynamicsCompressor();
  lim.threshold.value = -6; lim.knee.value = 0; lim.ratio.value = 20; lim.attack.value = 0.001; lim.release.value = 0.1;
  const out = c.createGain(); out.gain.value = 0.85;
  const bus = c.createGain(); bus.gain.value = 0.7; bus.connect(comp).connect(lim).connect(out).connect(dest);
  const env = (g, t, a, peak, dec) => { g.gain.setValueAtTime(0.0001, t); g.gain.exponentialRampToValueAtTime(peak, t + a); g.gain.exponentialRampToValueAtTime(0.0001, t + a + dec); };
  const noiseSrc = (t, dur) => { const n = c.createBufferSource(); n.buffer = noise; n.loop = true; n.start(t); n.stop(t + dur); return n; };
  // riser: band-passed noise sweeping up, cut right at the impact
  { const f = c.createBiquadFilter(); f.type = 'bandpass'; f.Q.value = 2; f.frequency.setValueAtTime(250, t0); f.frequency.exponentialRampToValueAtTime(3500, A);
    const g = c.createGain(); g.gain.setValueAtTime(0.0001, t0); g.gain.exponentialRampToValueAtTime(0.5, A - 0.01); g.gain.linearRampToValueAtTime(0.0001, A + 0.02);
    noiseSrc(t0, ARENA_HIT + 0.05).connect(f).connect(g).connect(bus); }
  // sub boom
  { const o = c.createOscillator(), g = c.createGain(); o.frequency.setValueAtTime(95, A); o.frequency.exponentialRampToValueAtTime(30, A + 1.3);
    env(g, A, 0.005, 1.0, 1.7); o.connect(g).connect(bus); o.start(A); o.stop(A + 1.9); }
  // impact crack
  { const f = c.createBiquadFilter(); f.type = 'lowpass'; f.frequency.value = 3000; const g = c.createGain(); env(g, A, 0.002, 0.6, 0.18); noiseSrc(A, 0.3).connect(f).connect(g).connect(bus); }
  // gong: inharmonic partials, long tail, slight beating
  [[98, 0.16, 2.4], [99.3, 0.1, 2.4], [151, 0.12, 2.0], [233, 0.1, 1.7], [311, 0.07, 1.4], [437, 0.05, 1.1], [612, 0.035, 0.8], [880, 0.02, 0.5]].forEach(([fr, pk, dec]) => {
    const o = c.createOscillator(), g = c.createGain(); o.frequency.value = fr; env(g, A + 0.004, 0.008, pk, dec); o.connect(g).connect(bus); o.start(A); o.stop(A + dec + 0.1);
  });
  // war drums: double hit
  for (const [d, pk] of [[0.55, 0.7], [0.74, 0.85]]) {
    const t = A + d, o = c.createOscillator(), g = c.createGain();
    o.frequency.setValueAtTime(130, t); o.frequency.exponentialRampToValueAtTime(48, t + 0.25);
    env(g, t, 0.003, pk, 0.35); o.connect(g).connect(bus); o.start(t); o.stop(t + 0.5);
    const f = c.createBiquadFilter(); f.type = 'lowpass'; f.frequency.value = 700; const ng = c.createGain(); env(ng, t, 0.002, 0.3, 0.08);
    noiseSrc(t, 0.15).connect(f).connect(ng).connect(bus);
  }
  // crowd roar: two band-passed noise layers swelling up and fading, with a slow wobble
  for (const [fc, pk] of [[500, 0.22], [1300, 0.12]]) {
    const f = c.createBiquadFilter(); f.type = 'bandpass'; f.Q.value = 0.8; f.frequency.value = fc;
    const g = c.createGain(); g.gain.setValueAtTime(0.0001, A); g.gain.exponentialRampToValueAtTime(pk, A + 0.7); g.gain.exponentialRampToValueAtTime(0.0001, A + 2.4);
    const lfo = c.createOscillator(), lg = c.createGain(); lfo.frequency.value = 5.5; lg.gain.value = pk * 0.25; lfo.connect(lg).connect(g.gain); lfo.start(A); lfo.stop(A + 2.5);
    noiseSrc(A, 2.5).connect(f).connect(g).connect(bus);
  }
  return A + 2.5;
}

/**
 * The victory sound (~6.3 s), built on any (Offline)AudioContext into `dest`: the shadow shatters
 * (glass crash + sub drop), a brass-like arpeggio climbs with a riser into the climax at
 * VICTORY.climax (big major chord, timpani double hit, cymbal swell, crowd cheer), then the chord
 * rings out under a few golden chimes. Through a compressor and a limiter: peak stays below 0 dBFS.
 */
export function buildVictory(c, dest, t0, noise) {
  if (!noise) {
    noise = c.createBuffer(1, c.sampleRate, c.sampleRate);
    const d = noise.getChannelData(0); for (let i = 0; i < d.length; i++) d[i] = Math.random() * 2 - 1;
  }
  const C = t0 + VICTORY.climax;
  const comp = c.createDynamicsCompressor();
  comp.threshold.value = -16; comp.knee.value = 8; comp.ratio.value = 5; comp.attack.value = 0.004; comp.release.value = 0.3;
  const lim = c.createDynamicsCompressor();
  lim.threshold.value = -6; lim.knee.value = 0; lim.ratio.value = 20; lim.attack.value = 0.001; lim.release.value = 0.12;
  const out = c.createGain(); out.gain.value = 0.8;
  const bus = c.createGain(); bus.gain.value = 0.6; bus.connect(comp).connect(lim).connect(out).connect(dest);
  const env = (g, t, a, peak, dec) => { g.gain.setValueAtTime(0.0001, t); g.gain.exponentialRampToValueAtTime(peak, t + a); g.gain.exponentialRampToValueAtTime(0.0001, t + a + dec); };
  const noiseSrc = (t, dur) => { const n = c.createBufferSource(); n.buffer = noise; n.loop = true; n.start(t); n.stop(t + dur); return n; };
  const filt = (type, f, q = 0.7) => { const b = c.createBiquadFilter(); b.type = type; b.frequency.value = f; b.Q.value = q; return b; };
  // 1. shatter: bright glass crash and inharmonic tinkles, a sub drop under it
  { const g = c.createGain(); env(g, t0, 0.002, 0.55, 0.5); noiseSrc(t0, 0.7).connect(filt('highpass', 3500)).connect(g).connect(bus); }
  [2350, 3170, 4020, 2780, 3710, 5100].forEach((f, i) => { const o = c.createOscillator(), g = c.createGain(); o.type = 'triangle'; o.frequency.value = f; const t = t0 + 0.03 + i * 0.045; env(g, t, 0.002, 0.06, 0.35); o.connect(g).connect(bus); o.start(t); o.stop(t + 0.45); });
  { const o = c.createOscillator(), g = c.createGain(); o.frequency.setValueAtTime(90, t0); o.frequency.exponentialRampToValueAtTime(28, t0 + 1.0); env(g, t0, 0.005, 0.8, 1.1); o.connect(g).connect(bus); o.start(t0); o.stop(t0 + 1.3); }
  // 2. build: arpeggio climbing faster (C major), noise riser into the climax
  const arp = [261.6, 329.6, 392, 523.3, 659.3, 784, 1046.5, 1318.5];
  let t = t0 + 0.9;
  for (let i = 0; t < C - 0.05; i++) {
    const f = arp[i % arp.length], o = c.createOscillator(), g = c.createGain(), lp = filt('lowpass', 1800 + 300 * i);
    o.type = 'sawtooth'; o.frequency.value = f; env(g, t, 0.006, 0.07 + 0.006 * i, 0.16); o.connect(lp).connect(g).connect(bus); o.start(t); o.stop(t + 0.25);
    t += Math.max(0.07, 0.2 - 0.013 * i);
  }
  { const f = filt('bandpass', 300, 2); f.frequency.setValueAtTime(300, t0 + 0.9); f.frequency.exponentialRampToValueAtTime(5000, C);
    const g = c.createGain(); g.gain.setValueAtTime(0.0001, t0 + 0.9); g.gain.exponentialRampToValueAtTime(0.32, C - 0.01); g.gain.linearRampToValueAtTime(0.0001, C + 0.03);
    noiseSrc(t0 + 0.9, C - t0 - 0.85).connect(f).connect(g).connect(bus); }
  // 3. climax: big major chord (saw + square through an opening low-pass), timpani, cymbal, cheer
  [130.8, 196, 261.6, 329.6, 392, 523.3, 659.3].forEach((f, i) => {
    for (const [type, det, pk] of [['sawtooth', -4, 0.07], ['square', 5, 0.035]]) {
      const o = c.createOscillator(), g = c.createGain(), lp = filt('lowpass', 600);
      lp.frequency.setValueAtTime(600, C); lp.frequency.exponentialRampToValueAtTime(4200, C + 0.25); lp.frequency.exponentialRampToValueAtTime(900, C + 3.6);
      o.type = type; o.frequency.value = f; o.detune.value = det;
      g.gain.setValueAtTime(0.0001, C); g.gain.exponentialRampToValueAtTime(pk * (i < 2 ? 1.3 : 1), C + 0.03); g.gain.setValueAtTime(pk * (i < 2 ? 1.3 : 1), C + 0.5); g.gain.exponentialRampToValueAtTime(0.0001, C + 3.8);
      o.connect(lp).connect(g).connect(bus); o.start(C); o.stop(C + 3.9);
    }
  });
  for (const [d, pk] of [[0, 0.9], [0.22, 0.75]]) {
    const tt = C + d, o = c.createOscillator(), g = c.createGain(); o.frequency.setValueAtTime(110, tt); o.frequency.exponentialRampToValueAtTime(52, tt + 0.3);
    env(g, tt, 0.003, pk, 0.6); o.connect(g).connect(bus); o.start(tt); o.stop(tt + 0.8);
  }
  { const g = c.createGain(); g.gain.setValueAtTime(0.0001, C - 0.6); g.gain.exponentialRampToValueAtTime(0.16, C + 0.05); g.gain.exponentialRampToValueAtTime(0.0001, C + 2.6); noiseSrc(C - 0.6, 3.3).connect(filt('highpass', 6000)).connect(g).connect(bus); }
  for (const [fc, pk] of [[600, 0.18], [1500, 0.1]]) {
    const g = c.createGain(); g.gain.setValueAtTime(0.0001, C); g.gain.exponentialRampToValueAtTime(pk, C + 0.5); g.gain.exponentialRampToValueAtTime(0.0001, C + 3.2);
    const lfo = c.createOscillator(), lg = c.createGain(); lfo.frequency.value = 6.5; lg.gain.value = pk * 0.3; lfo.connect(lg).connect(g.gain); lfo.start(C); lfo.stop(C + 3.3);
    noiseSrc(C, 3.3).connect(filt('bandpass', fc, 0.9)).connect(g).connect(bus);
  }
  // 4. afterglow: soft golden chimes over the ringing chord
  [[1568, 0.6], [2093, 1.0], [1760, 1.5], [2637, 2.1], [2093, 2.7]].forEach(([f, d]) => { const o = c.createOscillator(), g = c.createGain(); o.frequency.value = f; env(g, C + d, 0.004, 0.05, 1.2); o.connect(g).connect(bus); o.start(C + d); o.stop(C + d + 1.3); });
  return C + 3.9;
}
