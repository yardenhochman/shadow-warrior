// Synthesised sound (no assets): punch thumps, shout chime, fight start, KO and
// a simple drum loop standing in for music_manager's FIGHT playlist.
export class Sfx {
  constructor() { this.ctx = null; this.enabled = true; this.music = null; this.noise = null; }

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

  punch(power) {
    if (!this.enabled || !this.ctx) return;
    const c = this.ctx, t = c.currentTime, p = power / 100;
    const o = c.createOscillator(), g = c.createGain();
    o.frequency.setValueAtTime(140 + 60 * p, t); o.frequency.exponentialRampToValueAtTime(40, t + 0.18);
    this.env(g, t, 0.004, 0.5 + 0.5 * p, 0.2 + 0.15 * p);
    o.connect(g).connect(this.master); o.start(t); o.stop(t + 0.5);
    const n = c.createBufferSource(); n.buffer = this.noise;
    const f = c.createBiquadFilter(); f.type = 'lowpass'; f.frequency.value = 900 + 2500 * p;
    const ng = c.createGain(); this.env(ng, t, 0.002, 0.25 + 0.4 * p, 0.09);
    n.connect(f).connect(ng).connect(this.master); n.start(t); n.stop(t + 0.2);
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
    if (!this.enabled || !this.ctx || this.music) return;
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

  stopMusic() { if (this.music) { clearInterval(this.music); this.music = null; } }
}
