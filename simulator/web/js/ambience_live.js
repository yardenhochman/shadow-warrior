// Live ambience under the game (Yarden 20.11):
//  - IDLE: the sound lab's "Dojo night wind" as a long seamless loop (still night: soft wind, crickets).
//  - Someone present (WARMING): a heartbeat that starts barely audible and slow, and grows louder
//    and faster as the shout meter fills.
// Everything fades in/out with the state; FIGHT and COOLDOWN are silent here (music takes over).
import { dojoWind } from '../sound-lab/js/ambience.js';

const LOOP_SEC = 60, XFADE_SEC = 4, WIND_LEVEL = 0.55, FADE = 1.5;

/** Render the wind once and make it loop seamlessly (the tail is crossfaded into the head). */
async function renderWindLoop(sampleRate) {
  const pre = 2, total = pre + LOOP_SEC + XFADE_SEC + 2;
  const oc = new OfflineAudioContext(1, Math.ceil(total * sampleRate), sampleRate);
  dojoWind(oc, oc.destination, 0, total, 0.12, 0.18, 11);
  const x = (await oc.startRendering()).getChannelData(0);
  const n = Math.round(LOOP_SEC * sampleRate), xf = Math.round(XFADE_SEC * sampleRate), s0 = Math.round(pre * sampleRate);
  const out = new AudioBuffer({ length: n, sampleRate, numberOfChannels: 1 }), y = out.getChannelData(0);
  for (let i = 0; i < n; i++) y[i] = x[s0 + i];
  for (let i = 0; i < xf; i++) { const a = i / xf; y[i] = Math.sqrt(a) * x[s0 + i] + Math.sqrt(1 - a) * x[s0 + n + i]; }
  return out;
}

export class LiveAmbience {
  constructor(sfx) { this.sfx = sfx; this.wind = null; this.windGain = null; this.loading = false; this.beatBus = null; this.nextBeat = 0; }

  setup() {
    const c = this.sfx.ctx;
    if (this.beatBus || !c) return;
    this.windGain = c.createGain(); this.windGain.gain.value = 0.0001; this.windGain.connect(this.sfx.bus('wind'));
    // heartbeat bus: warm saturation + lowpass, so the thump carries on small speakers too (Yarden 20.11: louder, more pronounced)
    const lp = c.createBiquadFilter(); lp.type = 'lowpass'; lp.frequency.value = 600;
    const sat = c.createWaveShaper(), n = 1024, cv = new Float32Array(n); for (let i = 0; i < n; i++) { const x = (i / (n - 1)) * 2 - 1; cv[i] = Math.tanh(2.5 * x) / Math.tanh(2.5); } sat.curve = cv;
    const out = c.createGain(); out.gain.value = 0.45; // balanced 20.11 (0.8 was too strong): full meter peaks ~0.5
    this.beatBus = c.createGain(); this.beatBus.connect(sat).connect(lp).connect(out).connect(this.sfx.bus('heart', true)); // mixer channel, past the master
    this.loading = true;
    renderWindLoop(c.sampleRate).then((buf) => {
      const src = c.createBufferSource(); src.buffer = buf; src.loop = true; src.connect(this.windGain); src.start(); this.wind = src;
    }).catch(() => {}).finally(() => { this.loading = false; });
  }

  /** Called every frame with the arena state and the shout meter (0..1). */
  update(state, energyPct) {
    const c = this.sfx.ctx;
    if (!c || c.state !== 'running') return;
    this.setup();
    const on = this.sfx.enabled, t = c.currentTime;
    // wind: only while idle
    const wantWind = on && state === 'IDLE' ? WIND_LEVEL : 0.0001;
    if (this.windTarget !== wantWind) { this.windTarget = wantWind; const g = this.windGain.gain; g.cancelScheduledValues(t); g.setValueAtTime(Math.max(g.value, 0.0001), t); g.exponentialRampToValueAtTime(wantWind, t + FADE); }
    // heartbeat: someone is here and the meter is filling
    if (!on || state !== 'WARMING' || !this.sfx.isOn('heart')) { this.nextBeat = 0; return; }
    const e = Math.max(0, Math.min(1, energyPct || 0));
    if (this.nextBeat < t) this.nextBeat = t + 0.05;
    if (this.nextBeat - t > 0.12) return; // schedule just ahead, so speed/volume follow the meter
    const bpm = 58 + 92 * e, gap = 60 / bpm, level = 0.14 + 0.86 * e ** 1.3;
    this.beat(this.nextBeat, level, gap);
    this.nextBeat += gap;
  }

  beat(t, level, gap) {
    const c = this.sfx.ctx;
    const thump = (at, f, f2, pk, dec) => {
      const o = c.createOscillator(), g = c.createGain();
      o.frequency.setValueAtTime(f, at); o.frequency.exponentialRampToValueAtTime(f2, at + 0.09);
      g.gain.setValueAtTime(0.0001, at); g.gain.exponentialRampToValueAtTime(pk, at + 0.004); g.gain.exponentialRampToValueAtTime(0.0001, at + dec);
      o.connect(g).connect(this.beatBus); o.start(at); o.stop(at + dec + 0.03);
    };
    const knock = (at, pk) => { // the "skin" of the beat: a short muffled knock you hear even where the sub is lost
      const len = Math.ceil(0.05 * c.sampleRate), b = c.createBuffer(1, len, c.sampleRate), d = b.getChannelData(0);
      for (let i = 0; i < len; i++) d[i] = (Math.random() * 2 - 1) * (1 - i / len) ** 3;
      const src = c.createBufferSource(), bp = c.createBiquadFilter(), g = c.createGain(); src.buffer = b; bp.type = 'bandpass'; bp.frequency.value = 220; bp.Q.value = 1.2; g.gain.value = pk;
      src.connect(bp).connect(g).connect(this.beatBus); src.start(at);
    };
    const dub = t + gap * 0.3;
    thump(t, 75, 42, 1.1 * level, 0.16); thump(t, 130, 70, 0.45 * level, 0.07); knock(t, 1.4 * level);       // lub
    thump(dub, 64, 38, 0.8 * level, 0.14); thump(dub, 115, 62, 0.3 * level, 0.06); knock(dub, 1.0 * level); // dub
  }
}
