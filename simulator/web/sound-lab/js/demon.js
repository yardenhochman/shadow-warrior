// Browser demon-voice chain for recorded takes (same idea as tools/render_voices.py demon_chain):
// a calm layer slightly lower on top, a sub-octave growl layer driven and tremoloed underneath,
// the whisper band, a generated reverb; then -16 LUFS / -1.5 dBTP. All offline, nothing uploaded.
import { pitchShift, drive, reverb, normalize, SR } from './dsp.js';

export const PRESETS = {
  butler: { name: 'Butler demon', desc: 'Polite and close, the growl underneath grows as the line goes on.', calm: 0.88, growl: 0.5, growlFrom: 0.35, growlTo: 0.9, drive: 4, trem: 38, breath: 0.35, wet: 0.35, room: { sec: 2.5, decay: 2, bright: 0.4, pre: 0.02, seed: 3 } },
  abyss: { name: 'Abyss', desc: 'Huge and slow: an octave and a half down, heavy drive, cathedral tail.', calm: 0.75, growl: 0.42, growlFrom: 0.9, growlTo: 1.3, drive: 7, trem: 30, breath: 0.15, wet: 0.6, room: { sec: 3.5, decay: 1.4, bright: 0.3, pre: 0.04, seed: 4 } },
  imp: { name: 'Imp', desc: 'Nasty little thing: pitched up with a low ghost under it, ring-modulated, small room.', calm: 1.3, growl: 0.6, growlFrom: 0.4, growlTo: 0.6, drive: 3, trem: 0, ring: 70, breath: 0.25, wet: 0.2, room: { sec: 1.2, decay: 4, bright: 0.7, pre: 0.01, seed: 5 } },
};

/** Float32 mono at SR -> processed AudioBuffer (at SR). */
export async function demonize(x, preset = 'butler') {
  const P = PRESETS[preset], pad = P.room.sec + 0.3, n = x.length + Math.ceil(pad * SR);
  const calm = pitchShift(x, P.calm, SR), growl = pitchShift(x, P.growl, SR);
  const c = new OfflineAudioContext(1, n, SR), dur = x.length / SR;
  const buf = (d) => { const b = c.createBuffer(1, d.length, SR); b.copyToChannel(d, 0); const s = c.createBufferSource(); s.buffer = b; s.start(0); return s; };
  const bus = c.createGain(), comp = c.createDynamicsCompressor(); comp.threshold.value = -20; comp.ratio.value = 3; comp.attack.value = 0.01; comp.release.value = 0.2;
  const eq = c.createBiquadFilter(); eq.type = 'lowshelf'; eq.frequency.value = 120; eq.gain.value = 4;
  bus.connect(eq).connect(comp).connect(c.destination);
  const rv = reverb(c, P.room), wg = c.createGain(); wg.gain.value = P.wet; bus.connect(rv).connect(wg).connect(comp);
  // calm
  const cl = c.createBiquadFilter(); cl.type = 'lowpass'; cl.frequency.value = 6500; buf(calm).connect(cl).connect(bus);
  // breath
  const bh = c.createBiquadFilter(); bh.type = 'highpass'; bh.frequency.value = 2500; const bg = c.createGain(); bg.gain.value = P.breath; buf(x).connect(bh).connect(bg).connect(bus);
  // growl: drive, tremolo (gravel), optional ring mod, ramp in over the line
  const pre = c.createGain(); pre.gain.value = 3; const sh = drive(c, P.drive), gl = c.createBiquadFilter(); gl.type = 'lowpass'; gl.frequency.value = 2200;
  const gg = c.createGain(); gg.gain.setValueAtTime(P.growlFrom, 0); gg.gain.linearRampToValueAtTime(P.growlTo, Math.max(0.2, dur * 0.75));
  let node = buf(growl).connect(pre).connect(sh).connect(gl);
  if (P.trem) { const tg = c.createGain(), lfo = c.createOscillator(), d = c.createGain(); tg.gain.value = 0.55; lfo.frequency.value = P.trem; d.gain.value = 0.45; lfo.connect(d).connect(tg.gain); lfo.start(0); node = node.connect(tg); }
  if (P.ring) { const rm = c.createGain(), lfo = c.createOscillator(); rm.gain.value = 0; lfo.frequency.value = P.ring; lfo.connect(rm.gain); lfo.start(0); node = node.connect(rm); }
  node.connect(gg).connect(bus);
  const out = await c.startRendering();
  normalize(out, -16, -1.5);
  return out;
}

/** Decode any recorded Blob to mono Float32 at SR. */
export async function decodeMono(blob) {
  const ab = await blob.arrayBuffer();
  const c = new OfflineAudioContext(1, 1, SR);
  const b = await c.decodeAudioData(ab);
  const x = new Float32Array(b.length); for (let ch = 0; ch < b.numberOfChannels; ch++) { const d = b.getChannelData(ch); for (let i = 0; i < x.length; i++) x[i] += d[i] / b.numberOfChannels; }
  if (b.sampleRate === SR) return x;
  const n = Math.round(x.length * SR / b.sampleRate), y = new Float32Array(n), k = b.sampleRate / SR;
  for (let i = 0; i < n; i++) { const p = i * k, j = Math.floor(p), f = p - j; y[i] = (x[j] || 0) * (1 - f) + (x[j + 1] || 0) * f; }
  return y;
}

/** Trim leading/trailing silence (below -45 dBFS) with 80 ms margins. */
export function trim(x) {
  const th = 0.0056, m = Math.round(0.08 * SR); let a = 0, b = x.length - 1;
  while (a < x.length && Math.abs(x[a]) < th) a++; while (b > a && Math.abs(x[b]) < th) b--;
  return x.slice(Math.max(0, a - m), Math.min(x.length, b + m));
}
