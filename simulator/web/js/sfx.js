// Synthesised sound (no assets): punch thumps, shout chime, fight start, KO and
// fight music: the playlist (music.js) when loaded, else a synth drum loop.
export class Sfx {
  constructor() { this.ctx = null; this.enabled = true; this.music = null; this.noise = null; this.playlist = null; }

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
    const nova = sig?.kind === 'nova', t1 = nova ? t + 0.2 + 0.06 * (sig.esc || 0) : t; // nova: the hit lands after the blackout beat
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
    switch (sig.kind) {
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
    buildArenaOpen(this.ctx, this.master, this.ctx.currentTime + 0.01, this.noise);
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
