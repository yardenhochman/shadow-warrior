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
    this.windGain = c.createGain(); this.windGain.gain.value = 0.0001; this.windGain.connect(this.sfx.master);
    this.beatBus = c.createBiquadFilter(); this.beatBus.type = 'lowpass'; this.beatBus.frequency.value = 170; this.beatBus.connect(this.sfx.master);
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
    if (!on || state !== 'WARMING') { this.nextBeat = 0; return; }
    const e = Math.max(0, Math.min(1, energyPct || 0));
    if (this.nextBeat < t) this.nextBeat = t + 0.05;
    if (this.nextBeat - t > 0.12) return; // schedule just ahead, so speed/volume follow the meter
    const bpm = 58 + 92 * e, gap = 60 / bpm, level = 0.06 + 0.9 * e ** 1.6;
    this.beat(this.nextBeat, level, gap);
    this.nextBeat += gap;
  }

  beat(t, level, gap) {
    const c = this.sfx.ctx;
    const thump = (at, f, f2, pk) => {
      const o = c.createOscillator(), g = c.createGain();
      o.frequency.setValueAtTime(f, at); o.frequency.exponentialRampToValueAtTime(f2, at + 0.08);
      g.gain.setValueAtTime(0.0001, at); g.gain.exponentialRampToValueAtTime(pk, at + 0.004); g.gain.exponentialRampToValueAtTime(0.0001, at + 0.12);
      o.connect(g).connect(this.beatBus); o.start(at); o.stop(at + 0.15);
    };
    thump(t, 62, 40, 0.7 * level); thump(t + gap * 0.32, 54, 36, 0.5 * level); // lub-dub
  }
}
