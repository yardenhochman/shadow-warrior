// LED effect presets: a preset gives every stage of the game its own look. The rig (wled.js)
// renders each stage with the preset chosen for it (the base preset, or a per-stage override),
// so WARMING can come from Inferno while the hits come from Storm.
//
// Stage kinds:
//   ambient  fn(L, t, g, P, dt)  writes the whole frame (L.set / L.glow); t and dt are already
//            scaled by the stage's Speed. The rig applies Intensity and Colour shift afterwards.
//   fx       fx(L, f, a, P)      one effect instance f, age a (Speed-scaled), drawn into the effect
//            buffers (L.blob / L.band / L.dimAll); Intensity and Size (blob widths) are applied by
//            the rig, Colour shift too (hits: on the signature hue, others: on every colour).
// 'proposal' and 'brain-rs' wrap the original effects in wled.js (legacy: true / brain: true).
import { hsv, SIGNATURES, ACCENTS, VICTORY } from './hitfx.js';

const clamp01 = (x) => Math.min(Math.max(x, 0), 1);
const smooth = (x) => { x = clamp01(x); return x * x * (3 - 2 * x); };
const lerp = (a, b, t) => a + (b - a) * t;
const fract = (x) => x - Math.floor(x);
const hash = (a, b) => { const x = Math.sin(a * 127.1 + b * 311.7) * 43758.5453; return x - Math.floor(x); };
const inner = (c) => c === 1 || c === 2;
function pal(stops) {
  const p = stops.map((h) => [parseInt(h.slice(1, 3), 16) / 255, parseInt(h.slice(3, 5), 16) / 255, parseInt(h.slice(5, 7), 16) / 255]);
  return (x) => {
    x = clamp01(x) * (p.length - 1);
    const i = Math.min(Math.floor(x), p.length - 2), t = x - i, a = p[i], b = p[i + 1];
    return [lerp(a[0], b[0], t), lerp(a[1], b[1], t), lerp(a[2], b[2], t)];
  };
}
const scale = (rgb, k) => [rgb[0] * k, rgb[1] * k, rgb[2] * k];
const HEAT = pal(['#000000', '#2a0000', '#7a0500', '#d42000', '#ff6a00', '#ffb020', '#fff0b0']);
const FROST = pal(['#000006', '#020a30', '#0a2a80', '#3a6ad0', '#8ab4ff', '#e8f0ff']);
const CLOUD = pal(['#000000', '#05060f', '#0e1430', '#1c2450', '#34407a']);

export const STAGES = [
  { id: 'idle', name: 'Idle / attract', kind: 'ambient', state: 'IDLE' },
  { id: 'warming', name: 'Warming energy bar', kind: 'ambient', state: 'WARMING' },
  { id: 'open', name: 'Fight start', kind: 'fx' },
  { id: 'fight', name: 'Fight ambient (rage)', kind: 'ambient', state: 'FIGHT' },
  { id: 'jab', name: 'Jab hits', kind: 'hit' },
  { id: 'hook', name: 'Hook hits', kind: 'hit' },
  { id: 'heavy', name: 'Heavy hits', kind: 'hit' },
  { id: 'shout', name: 'Shout ripple (FIGHT)', kind: 'fx' },
  { id: 'victory', name: 'Victory finale', kind: 'fx' },
];
export const STAGE_BY_ID = Object.fromEntries(STAGES.map((s) => [s.id, s]));

// Fine-tuning per stage (applies to whichever preset that stage uses). Same shape as TUNABLES.
export const FX_PARAMS = [
  { key: 'speed', name: 'Speed', unit: 'x', def: 1, min: 0.3, max: 3, step: 0.05,
    help: 'How fast this stage\'s effect runs. 2 = twice as fast, and a one-shot effect lasts half as long.',
    more: 'Scales the effect\'s own clock. For hits, the opening and the victory it also scales the duration (a 1.2 s heavy at speed 2 is over in 0.6 s). For the idle, warming and fight ambients it speeds up breathing, rain, chasers and flames. The game timers do not change.' },
  { key: 'size', name: 'Size', unit: 'x', def: 1, min: 0.4, max: 2.5, step: 0.05,
    help: 'Width of the sparks, dots and bands; in the ambients the height of flames, waves or rain trails.',
    more: 'Multiplies the width of every soft dot an effect draws, and the length of bands, flames and trails where the effect has one. Small values give crisp single-LED accents (a real column has 180 LEDs, so 1 LED is about 0.006 of a column); large values give fat, soft blobs.' },
  { key: 'gain', name: 'Intensity', unit: 'x', def: 1, min: 0, max: 1.6, step: 0.05,
    help: 'Brightness of this stage\'s effect. 0 switches it off.',
    more: 'Multiplies the effect\'s light. For hits it stacks with "Punch effect strength" in the tunables. Whatever the value, the whole frame stays under the strip cap (mean level 0.72), so above 1 effects get bolder but the strip never turns into a white wall.' },
  { key: 'hue', name: 'Colour shift', unit: 'deg', def: 0, min: -180, max: 180, step: 5,
    help: 'Rotates the effect\'s colours around the colour wheel (0 = the preset\'s own colours).',
    more: 'For hits the shift moves the signature colour, and colours in the teal/aqua band (150-200 deg) are skipped, so a punch never looks like a shout. For the ambients, the opening, the shout ripple and the victory every colour is rotated (whites stay white). The shout ripple is limited to +-25 deg so it stays teal.' },
];
export const fxParamDefaults = () => Object.fromEntries(FX_PARAMS.map((p) => [p.key, p.def]));

// ------------------------------------------------------------------ shared helpers
/** WARMING bookkeeping shared by the presets: stage 0..3 from the bar, flare + kick on a new stage. */
function warmBase(L, t, g) {
  const e = clamp01(g.energyPct ?? 0), stage = e < 0.25 ? 0 : e < 0.5 ? 1 : e < 0.75 ? 2 : 3;
  if (stage > L.wStage) { L.stageKick = 1; L.shoutWaves.push({ t0: L.now, k: 1.1 }); }
  L.wStage = stage;
  return { e, stage, lvl: clamp01(g.level ?? 0) };
}
/** Shout kicks in WARMING: a pulse runs up the bar and bursts at the fill line. */
function kicks(L, e, rgb, w = 0.035) {
  for (const s of L.shoutWaves) {
    const a = L.now - s.t0, xf = a * (2.2 + e), k = s.k;
    for (let c = 0; c < 4; c++) {
      if (xf < e + 0.04) L.glow(c, xf, w + 0.02 * k, rgb, 0.6 * k);
      else L.glow(c, e, w + 0.01 + 0.06 * k, rgb, 0.7 * k * Math.exp(-(a - (e + 0.04) / (2.2 + e)) / 0.12));
    }
  }
}
const fill = (L, fn) => { for (let c = 0; c < 4; c++) for (let i = 0; i < L.n; i++) { const [r, g, b] = fn(c, (i + 0.5) / L.n, i); L.set(c, i, r, g, b); } };
/** Stateless particles: k-th particle of column c with phase 0..1 at time t (rate per second). */
const phase = (t, c, k, rate) => fract(t * rate * (0.7 + 0.6 * hash(c + 3, k)) + hash(k, c));

// Retro: chunky blocks (12 per column, ~15 strip LEDs each at 180) with a dark gap.
const NB = 12;
function block(L, c, b, rgb, k) { if (b < 0 || b >= NB) return; L.band(c, b / NB + 0.012, (b + 1) / NB - 0.012, rgb, k, 0.002); }
function fillBlocks(L, fn) {
  for (let c = 0; c < 4; c++) for (let i = 0; i < L.n; i++) {
    const x = (i + 0.5) / L.n, b = Math.floor(x * NB), inGap = Math.abs(x * NB - Math.round(x * NB)) < 0.13;
    const [r, g, bl] = inGap ? [0, 0, 0] : fn(c, b, x);
    L.set(c, i, r, g, bl);
  }
}

/**
 * Victory finale (VICTORY.dur s): the shadow's crimson floods the columns and shatters
 * (0.25-0.9 s), the winner's colours erupt from the floor and build (to VICTORY.climax),
 * the climax burst, then a calm golden afterglow that fades out. `style` changes the build.
 */
function finale(L, f, a, P, o) {
  const SH = 352, CL = VICTORY.climax, W = o.win, sz = P.size;
  if (a < 0.25) { // freeze: everything goes the shadow's colour
    L.dimAll(0.04);
    for (let c = 0; c < 4; c++) L.band(c, 0, 1, hsv(SH, 0.95, 1), 0.7 * smooth(a / 0.08), 0.01);
    return;
  }
  if (a < 0.9) { // shatter: crimson shards break apart and fall, hot white cracks; the winner's glow wakes at the floor
    const as = a - 0.25; L.dimAll(0.04);
    for (let c = 0; c < 4; c++) L.band(c, 0, 0.04 + 0.12 * smooth((as - 0.25) / 0.4), hsv(W[c % W.length], 0.9, 1), 0.3 * smooth((as - 0.2) / 0.4), 0.04);
    for (let c = 0; c < 4; c++) for (let s = 0; s < 8; s++) {
      const a0 = s / 8, b0 = (s + 1) / 8, h = hash(c * 9 + s, 3);
      const dx = -(0.05 + 0.35 * h) * as - 0.8 * as * as * (0.3 + h), gap = 0.008 + 0.035 * Math.min(1, as * 3);
      const env = 1 - smooth(as / 0.62);
      L.band(c, a0 + gap + dx, b0 - gap + dx, hsv(SH + 14 * h, 0.95, 1), 0.65 * env, 0.006);
      if (as < 0.35) L.blob(c, b0 + dx, 0.008, [1, 0.9, 0.85], 0.7 * (1 - as / 0.35));
    }
    return;
  }
  if (a < CL) { // build: the winner's colours rise, taller and faster towards the climax
    const q = (a - 0.9) / (CL - 0.9); L.dimAll(0.05);
    for (let c = 0; c < 4; c++) {
      const hue = W[c % W.length], top = 0.15 + 0.85 * smooth(q);
      if (o.style === 'pixels') {
        const nb = Math.floor(top * NB);
        for (let b = 0; b <= nb; b++) block(L, c, b, hsv(W[(b + c) % W.length], 0.95, 1), 0.3 + 0.35 * q);
        if (Math.floor(a * 10) % 2) block(L, c, nb + 1, [1, 1, 1], 0.6);
        continue;
      }
      L.band(c, 0, top, hsv(hue, 0.9, 1), 0.1 + 0.22 * q, 0.04);
      if (o.style === 'crystal') {
        for (let k = 0; k < 6; k++) { const x = top * hash(c, k + Math.floor(a * 6)); L.blob(c, x, 0.012, [0.85, 0.92, 1], 0.6 * q); }
        L.band(c, top - 0.04 * sz, top, hsv(W[(c + 1) % W.length], 0.4, 1), 0.7, 0.01);
      } else if (o.style === 'bolts') {
        if (hash(c, Math.floor(a * (6 + 18 * q))) > 0.72 - 0.25 * q) for (let i = 0; i < L.n; i++) { const x = (i + 0.5) / L.n; if (x < top && hash(c + a, Math.floor(x * 12)) > 0.3) L.add(c, i, hsv(W[c % W.length], 0.35, 1), 0.8); }
      } else if (o.style === 'chase') {
        for (let k = 0; k < 3; k++) { const x = fract(a * (0.8 + 2.5 * q) + k / 3 + c * 0.25) * top; L.band(c, x - 0.025 * sz, x + 0.025 * sz, hsv(W[(k + c) % W.length], 0.85, 1), 0.85, 0.005); }
      } else { // fountain
        for (let k = 0; k < 5; k++) { const ph = phase(a, c, k, 1 + 2.5 * q), x = ph * top * 1.05; L.blob(c, x, 0.02, hsv(W[(k + c) % W.length], 0.6, 1), 0.8 * (1 - ph * 0.5)); }
      }
      L.blob(c, top, 0.03, hsv(hue, 0.3, 1), 0.6 + 0.3 * q);
    }
    return;
  }
  const ac = a - CL, endFade = 1 - smooth((a - (f.dur - 1.4)) / 1.4);
  if (ac < 0.6) { // climax: the columns erupt in the winner's colours with a white-gold flash
    L.dimAll(0.15);
    for (let c = 0; c < 4; c++) {
      L.band(c, 0, 1, hsv(45, 0.25, 1), 0.9 * Math.exp(-ac / 0.1), 0.01);
      for (let k = 0; k < W.length; k++) {
        const x = fract(k / W.length + ac * 1.6 + c * 0.11);
        if (o.style === 'pixels') block(L, c, Math.floor(x * NB), hsv(W[k], 0.9, 1), 0.7 * Math.exp(-ac / 0.4));
        else L.band(c, x - 0.07 * sz, x + 0.07 * sz, hsv(W[k], 0.85, 1), 0.65 * Math.exp(-ac / 0.4), 0.03);
      }
    }
  }
  // afterglow: calm golden breathing, brightest at the floor, slow sparkles, fading to the end
  const k = 0.32 * smooth((ac - 0.15) / 0.5) * endFade * (0.8 + 0.2 * Math.sin(ac * 2.2));
  if (ac > 0.15) L.dimAll(Math.max(0.15, 1 - 4 * k));
  for (let c = 0; c < 4; c++) {
    for (let s = 0; s < 4; s++) L.band(c, s / 4, (s + 1) / 4, hsv(40 + 4 * s, 0.8 - 0.1 * s, 1), k * (1 - 0.18 * s), 0.12);
    for (let j = 0; j < 3; j++) { const ph = phase(ac, c, j, 0.35); L.blob(c, ph, 0.012, hsv(48, 0.35, 1), 1.6 * k * Math.sin(Math.PI * ph)); }
  }
}

// ------------------------------------------------------------------ presets
const BRAIN_NONE = (why) => ({ name: `none (${why})`, none: true });

export const PRESETS = {
  'brain-rs': {
    name: 'brain-rs effects (config.yaml)', blurb: 'What brain-rs asks WLED for today: Breathe/Ocean, Rainbow, the realtime red pulse.',
    accents: ACCENTS,
    idle: { name: 'WLED breathe / ocean / sx 128', brain: 'IDLE' },
    warming: { name: 'WLED rainbow / rainbow / sx 200', brain: 'WARMING' },
    open: BRAIN_NONE('brain-rs has no fight-start effect'),
    fight: { name: 'realtime red pulse (generate_fight_frame)', brain: 'FIGHT' },
    jab: BRAIN_NONE('the realtime frame ignores punches'),
    hook: BRAIN_NONE('the realtime frame ignores punches'),
    heavy: BRAIN_NONE('the realtime frame ignores punches'),
    shout: BRAIN_NONE('brain-rs shows nothing for a shout in FIGHT'),
    victory: BRAIN_NONE('brain-rs has no knock-out'),
  },

  proposal: {
    name: 'Sim proposal effects', blurb: 'The original simulator proposal: violet idle, 4-stage energy bar, fire, the six heavy signatures.',
    accents: ACCENTS, legacy: true,
    idle: { name: 'Violet breathing + rolling wave' },
    warming: { name: 'Energy bar in 4 stages, shout kicks' },
    open: { name: 'Suck-in, then burst into the fire', dur: 1.7 },
    fight: { name: 'Fire that grows with punching, red warning at 10 s' },
    jab: { kinds: { tick: { name: 'Tick spark' }, flick: { name: 'Flick up' }, twin: { name: 'Twin sparks' } } },
    hook: { kinds: { sweep: { name: 'Band sweep' }, ring: { name: 'Ring' }, double: { name: 'Double pop' } } },
    heavy: { kinds: Object.fromEntries(Object.entries(SIGNATURES.heavy).map(([k, v]) => [k, { ...v, name: v.label }])) },
    shout: { name: 'Teal sound-wave ripples' },
    victory: { name: 'Crimson shatter, fire fountains, golden afterglow', dur: VICTORY.dur, fx: (L, f, a, P) => finale(L, f, a, P, { style: 'fountain', win: [44, 22, 305, 268] }) },
  },

  inferno: {
    name: 'Inferno', blurb: 'Lava, embers and heat shimmer: everything burns up from the floor.',
    accents: [6, 20, 34, 48, 352],
    idle: { name: 'Lava lamp', fn(L, t, g, P) {
      fill(L, (c, x) => { const v = 0.035 + 0.025 * Math.sin(t * 0.7 + c * 1.3 + x * 5); return [v, v * 0.16, 0]; });
      for (let c = 0; c < 4; c++) for (let j = 0; j < 3; j++) {
        const ph = phase(t, c, j, 0.045), xc = -0.1 + 1.2 * ph, w = 0.06 * P.size * (0.8 + 0.5 * hash(c, j + 5)), k = 0.4 * Math.sin(Math.PI * ph);
        L.glow(c, xc, w, hsv(10 + 14 * hash(c, j + 9), 0.95, 1), k); L.glow(c, xc, w * 0.45, hsv(36, 0.75, 1), k * 0.6);
      }
    } },
    warming: { name: 'Heat gauge: embers, flames, white-hot shimmer', fn(L, t, g, P) {
      const { e, stage } = warmBase(L, t, g);
      fill(L, (c, x, i) => {
        if (x < e - 0.01) {
          const rel = x / Math.max(e, 0.01), fl = 0.8 + 0.2 * Math.sin(t * (5 + 9 * e) + i * 0.9 + c * 2);
          let h = (0.28 + 0.5 * rel) * (0.5 + 0.5 * e) * fl + 0.3 * L.stageKick;
          if (stage >= 3) h += 0.12 * Math.sin(x * 46 - t * 26 + c) + 0.1 * hash(i + c * 99, Math.floor(t * 24)); // heat shimmer
          return HEAT(Math.min(0.97, h));
        }
        if (x < e + 0.012) return HEAT(0.85 + 0.1 * Math.sin(t * 13));
        const ab = x - e;
        if (stage >= 2 && ab < 0.12 * P.size && hash(c * 31 + i, Math.floor(t * 14)) > 0.45) return scale(HEAT(0.6), 0.9 * (1 - ab / (0.12 * P.size))); // flames lick over the line
        return [0.05 * Math.max(0, 1 - ab * 4), 0.008, 0];
      });
      for (let c = 0; c < 4; c++) for (let k = 0; k < [0, 4, 7, 11][stage]; k++) { const ph = phase(t, c, k, 0.9); L.glow(c, e + ph * 0.5 * P.size, 0.012, hsv(28, 0.85, 1), 0.65 * (1 - ph)); }
      kicks(L, e, hsv(42, 0.5, 1));
    } },
    open: { name: 'Eruption from the floor', dur: 1.8, fx(L, f, a, P) {
      const A = 0.35;
      if (a < A) { L.dimAll(0.1); for (let c = 0; c < 4; c++) L.band(c, 0, 0.1 + 0.15 * a / A, hsv(8, 1, 1), (0.3 + 0.4 * a / A) * (0.7 + 0.3 * Math.sin(a * 90)), 0.04); return; }
      const ab = a - A; if (ab < 0.03) L.rage = Math.max(L.rage, 0.5);
      L.dimAll(0.25 + 0.75 * smooth(ab / 1.2));
      const env = Math.exp(-ab / 0.6);
      for (let c = 0; c < 4; c++) {
        const fr = Math.max(0, (ab - (inner(c) ? 0 : 0.06)) * 2.8);
        L.band(c, 0, Math.min(1, fr), hsv(14, 1, 1), 0.55 * env, 0.03);
        if (fr < 1.05) L.blob(c, fr, 0.05, hsv(45, 0.4, 1), 0.9 * env);
        for (let j = 0; j < 4 && fr > 1; j++) { const x = 1 - (fr - 1) * (0.5 + 0.4 * hash(c, j)) - 0.2 * j; L.blob(c, x, 0.02, hsv(30, 0.9, 1), 0.6 * env); }
      }
    } },
    fight: { name: 'Rising lava, embers above', fn(L, t, g, P) {
      const rg = L.rage, lv = Math.min(0.95, (0.08 + 0.72 * Math.pow(rg, 0.8)) * P.size), br = 0.3 + 0.7 * rg;
      fill(L, (c, x, i) => {
        const surf = lv + 0.025 * Math.sin(t * 3 + c * 1.7) + 0.02 * Math.sin(t * 7.1 + c + x * 3);
        if (x < surf) { const d = surf - x; return scale(HEAT(0.3 + 0.45 * Math.exp(-d * 7) + 0.12 * Math.sin(x * 28 - t * 4 + c) * rg), br); }
        return [0.03 * Math.max(0, 1 - (x - surf) * 6), 0.004, 0];
      });
      for (let c = 0; c < 4; c++) for (let k = 0; k < 2 + Math.round(8 * rg); k++) { const ph = phase(t, c, k, 0.3); L.glow(c, lv + ph * (1 - lv), 0.012, hsv(28, 0.9, 1), 0.55 * (1 - ph)); }
    } },
    jab: { kinds: {
      ember: { name: 'Ember', dur: 0.45, fx(L, f, a) { const x = f.x0 + 0.15 * (1 - Math.exp(-a / 0.15)), k = 1 - a / f.dur; L.blob(f.col, x, 0.03, hsv(f.hue, 0.85, 1), 0.85 * k); L.blob(f.col, x - 0.035, 0.035, hsv(f.hue - 8, 1, 1), 0.3 * k); } },
      cinder: { name: 'Cinder burst', dur: 0.35, fx(L, f, a) { for (const j of [-1, 0, 1]) L.blob(f.col, f.x0 + j * 0.09 * Math.min(1, a / 0.15), 0.015, hsv(f.hue + 10 * j, 0.8, 1), 0.85 * Math.exp(-a / 0.1)); } },
    } },
    hook: { kinds: {
      lick: { name: 'Flame lick', look: 'sweep', dur: 0.6, fx(L, f, a, P) {
        (f.dir > 0 ? [0, 1, 2, 3] : [3, 2, 1, 0]).forEach((c, j) => {
          const ac = a - j * 0.05; if (ac < 0) return;
          const env = ac < 0.03 ? ac / 0.03 : Math.exp(-(ac - 0.03) / 0.13), top = f.x0 + 0.25 * P.size * smooth(ac / 0.15);
          L.band(c, f.x0 - 0.06, top, hsv(f.hue, 0.95, 1), 0.55 * env, 0.04); L.blob(c, top, 0.03, hsv(f.hue + 18, 0.5, 1), 0.6 * env);
        });
      } },
      heatring: { name: 'Heat ring', look: 'ring', dur: 0.6, fx(L, f, a) {
        for (let c = 0; c < 4; c++) { const ac = a - (inner(c) ? 0 : 0.05); if (ac < 0) continue; const r = 1.1 * ac, env = 0.65 * Math.exp(-ac / 0.2);
          for (const s of [1, -1]) { L.blob(c, f.x0 + s * r, 0.035, hsv(f.hue + 12, 0.75, 1), env); L.band(c, Math.min(f.x0, f.x0 + s * r), Math.max(f.x0, f.x0 + s * r), hsv(f.hue - 10, 1, 1), 0.15 * env); } }
      } },
    } },
    heavy: { kinds: {
      eruption: { name: 'ERUPTION', label: 'ERUPTION', hue: 18, big: true, look: 'nova', dur: 1.4, fx(L, f, a) {
        if (a < 0.14) { L.dimAll(0.05); for (let c = 0; c < 4; c++) L.band(c, 0, 0.08, hsv(8, 1, 1), 0.6, 0.03); return; }
        const ab = a - 0.14; L.dimAll(0.2 + 0.8 * smooth(ab / 1.1));
        for (let c = 0; c < 4; c++) {
          const fr = Math.min(1.1, (ab - (inner(c) ? 0 : 0.05)) * 3.5); if (fr < 0) continue;
          const env = Math.exp(-ab / 0.45);
          L.band(c, 0, Math.min(1, fr), hsv(f.hue, 1, 1), 0.6 * env, 0.03); L.blob(c, Math.min(1, fr), 0.06, hsv(48, 0.35, 1), 0.95 * env);
          for (let j = 0; j < 5; j++) { const t2 = ab - 0.28 - 0.05 * j; if (t2 > 0) L.blob(c, 1 - 2.2 * t2 * t2 - 0.25 * t2 * hash(c, j), 0.02, hsv(30, 0.9, 1), 0.7 * Math.exp(-t2 / 0.6)); }
        }
      } },
      meltdown: { name: 'MELTDOWN', label: 'MELTDOWN', hue: 40, look: 'shatter', dur: 1.3, fx(L, f, a) {
        L.dimAll(a < 0.4 ? 0.15 : 0.15 + 0.85 * smooth((a - 0.4) / 0.8));
        if (a < 0.08) { for (let c = 0; c < 4; c++) L.band(c, 0, 1, hsv(46, 0.35, 1), 0.85, 0.01); return; }
        const ad = a - 0.08;
        for (let c = 0; c < 4; c++) {
          const lvl = Math.max(0, 1 - 0.9 * ad * ad * 2.2), env = Math.exp(-ad / 0.7);
          L.band(c, 0, lvl, hsv(f.hue - 15, 1, 1), 0.4 * env, 0.06);
          for (let j = 0; j < 4; j++) { const x0 = 0.3 + 0.6 * hash(c, j), x = x0 - 1.6 * ad * ad * (0.6 + 0.8 * hash(j, c)); if (x > 0) L.blob(c, x, 0.02, hsv(f.hue, 0.6, 1), 0.9 * env); }
        }
      } },
      firestorm: { name: 'FIRESTORM', label: 'FIRESTORM', hue: 5, big: true, look: 'split', dur: 1.2, fx(L, f, a) {
        L.dimAll(0.35 + 0.65 * smooth(a / f.dur));
        for (let j = 0; j < 10; j++) { const tj = a - j * 0.055; if (tj < 0) continue; const c = j % 4, x = f.x0 - 0.3 + 0.12 * j * 0.6 + tj * 0.4, env = Math.exp(-tj / 0.3);
          L.band(c, x - 0.09, x + 0.09, hsv(f.hue + 6 * j, 1, 1), 0.7 * env, 0.04); L.blob(c, x + 0.09, 0.025, hsv(45, 0.4, 1), 0.8 * env); }
      } },
    } },
    shout: { name: 'Steam ripple (teal) that cools the fire', dur: 1.1, fx(L, f, a) {
      const k = f.k ?? 1; L.dimAll(1 - 0.35 * k * Math.exp(-a / 0.3));
      for (let c = 0; c < 4; c++) for (let j = 0; j < 2; j++) { const ac = a - (inner(c) ? 0 : 0.06) - j * 0.12; if (ac < 0) continue; const r = (0.5 + 0.6 * k) * ac, env = (0.65 - 0.2 * j) * k * Math.exp(-ac / 0.35);
        L.blob(c, f.x0 + r, 0.025, hsv(176, 0.5, 1), env); L.blob(c, f.x0 - r, 0.025, hsv(176, 0.5, 1), env); L.blob(c, f.x0 + 0.4 * ac, 0.05, hsv(185, 0.12, 1), 0.25 * env); }
    } },
    victory: { name: 'Crimson shatter, fire fountains, golden embers', dur: VICTORY.dur, fx: (L, f, a, P) => finale(L, f, a, P, { style: 'fountain', win: [44, 26, 8, 52] }) },
  },

  ice: {
    name: 'Ice / ghost', blurb: 'Cold palette: ghostly wisps, frost that crawls up the columns, ice that shatters.',
    accents: [212, 228, 252, 276, 296],
    idle: { name: 'Ghost wisps and frost twinkle', fn(L, t, g, P) {
      fill(L, (c, x, i) => { const tw = hash(i + c * 97, Math.floor(t * 1.5)) > 0.97 ? 0.25 : 0; return scale(FROST(0.28 + 0.05 * Math.sin(t * 0.5 + c)), 0.35 + tw * 2); });
      for (let c = 0; c < 4; c++) for (let j = 0; j < 2; j++) { const ph = phase(t, c, j, 0.07); L.glow(c, ph * 1.1, 0.07 * P.size, hsv(220 + 40 * hash(c, j), 0.35, 1), 0.28 * Math.sin(Math.PI * ph)); }
    } },
    warming: { name: 'Frost crawl: crystals, spikes, cold-white shimmer', fn(L, t, g, P) {
      const { e, stage } = warmBase(L, t, g);
      fill(L, (c, x, i) => {
        const edge = e + (stage >= 1 ? 0.04 * P.size * (hash(c, Math.floor(x * 30)) - 0.3) : 0); // jagged frost line
        if (x < edge) {
          const rel = x / Math.max(e, 0.01), sp = hash(i + c * 131, Math.floor(t * (2 + 6 * e))) > 0.9 ? 0.3 : 0;
          let v = 0.24 + 0.36 * rel + sp + 0.3 * L.stageKick;
          if (stage >= 2) v += 0.15 * Math.max(0, Math.sin(x * 22 - t * 6 + c)); // crawl ripple
          const col = FROST(Math.min(0.98, v * (0.6 + 0.4 * e)));
          return stage >= 3 ? [col[0] + 0.12 * Math.max(0, Math.sin(t * 17 + c)), col[1], col[2] + 0.05] : col;
        }
        const ab = x - edge;
        if (stage >= 2 && ab < 0.1 * P.size && hash(c * 7 + Math.floor(x * 40), Math.floor(t * 3)) > 0.82) return FROST(0.8); // crystal spikes
        return scale(FROST(0.3), 0.25 * Math.max(0, 1 - ab * 3));
      });
      if (stage >= 3 && hash(Math.floor(t * 6), 1) > 0.7) { const c = Math.floor(hash(Math.floor(t * 6), 2) * 4); L.glowBand(c, Math.max(0, e - 0.25), e, [0.9, 0.95, 1], 0.5); } // crack flashes
      kicks(L, e, hsv(220, 0.25, 1));
    } },
    open: { name: 'Columns freeze, then the ice shatters', dur: 1.8, fx(L, f, a) {
      const A = 0.35;
      if (a < A) { const q = a / A; L.dimAll(0.15); for (let c = 0; c < 4; c++) { L.band(c, 0, 0.5 * q, hsv(215, 0.55, 1), 0.4, 0.02); L.band(c, 1 - 0.5 * q, 1, hsv(215, 0.55, 1), 0.4, 0.02); } return; }
      const ab = a - A; if (ab < 0.03) L.rage = Math.max(L.rage, 0.45);
      L.dimAll(0.2 + 0.8 * smooth(ab / 1.2));
      for (let c = 0; c < 4; c++) for (let s = 0; s < 7; s++) { const h = hash(c, s), dx = -1.4 * ab * ab * (0.4 + h), env = Math.exp(-ab / 0.5);
        L.band(c, s / 7 + 0.01 + dx, (s + 1) / 7 - 0.01 + dx, hsv(210 + 30 * h, 0.4, 1), 0.7 * env, 0.006); }
    } },
    fight: { name: 'Frostbite: cold glow and crystals that grow with rage', fn(L, t, g, P) {
      const rg = L.rage, top = Math.min(1, (0.15 + 0.7 * rg) * P.size);
      fill(L, (c, x, i) => {
        const base = x < top ? 0.22 + 0.5 * rg * (1 - x / top) : 0.12 * Math.max(0, 1 - (x - top) * 5);
        const tw = hash(i + c * 211, Math.floor(t * (3 + 10 * rg))) > 0.95 - 0.08 * rg ? 0.35 : 0;
        return FROST(Math.min(0.97, base + tw + 0.05 * Math.sin(t * 2 + c)));
      });
      for (let c = 0; c < 4; c++) for (let j = 0; j < 1 + Math.round(3 * rg); j++) { const ph = phase(t, c, j, 0.15 + 0.4 * rg); L.glow(c, ph, 0.05 * P.size, hsv(265, 0.3, 1), 0.22 * Math.sin(Math.PI * ph)); }
    } },
    jab: { kinds: {
      chip: { name: 'Ice chip', dur: 0.3, fx(L, f, a) { const k = Math.exp(-a / 0.08); L.blob(f.col, f.x0, 0.025, hsv(f.hue, 0.45, 1), 0.9 * k); L.blob(f.col, f.x0 + 0.04, 0.01, [1, 1, 1], 0.7 * k * (a < 0.1 ? 1 : 0.3)); } },
      flake: { name: 'Snowflake twinkle', dur: 0.4, fx(L, f, a) { const k = (1 - a / f.dur) * (0.7 + 0.3 * Math.sin(a * 60)); for (const d of [-0.05, 0, 0.05]) L.blob(f.col, f.x0 + d, 0.012, hsv(f.hue, d ? 0.6 : 0.2, 1), (d ? 0.6 : 0.95) * k); } },
    } },
    hook: { kinds: {
      frostsweep: { name: 'Frost sweep', look: 'sweep', dur: 0.65, fx(L, f, a, P) {
        (f.dir > 0 ? [0, 1, 2, 3] : [3, 2, 1, 0]).forEach((c, j) => { const ac = a - j * 0.06; if (ac < 0) return; const env = ac < 0.02 ? ac / 0.02 : Math.exp(-(ac - 0.02) / 0.15);
          L.band(c, f.x0 - 0.14 * P.size, f.x0 + 0.14 * P.size, hsv(f.hue, 0.55, 1), 0.5 * env, 0.05);
          for (let k = 0; k < 4; k++) L.blob(c, f.x0 + 0.3 * (hash(c, k) - 0.5), 0.008, [1, 1, 1], 0.8 * env * hash(k, Math.floor(ac * 30))); });
      } },
      coldring: { name: 'Cold ring', look: 'ring', dur: 0.6, fx(L, f, a) {
        for (let c = 0; c < 4; c++) { const ac = a - (inner(c) ? 0 : 0.05); if (ac < 0) continue; const r = 1.2 * ac, env = 0.6 * Math.exp(-ac / 0.22);
          for (const s of [1, -1]) { L.blob(c, f.x0 + s * r, 0.025, hsv(f.hue, 0.4, 1), env); L.blob(c, f.x0 + s * r * 0.8, 0.03, hsv(f.hue + 40, 0.8, 1), 0.4 * env); } }
      } },
    } },
    heavy: { kinds: {
      avalanche: { name: 'AVALANCHE', label: 'AVALANCHE', hue: 215, big: true, look: 'shatter', dur: 1.3, fx(L, f, a) {
        L.dimAll(0.15 + 0.85 * smooth((a - 0.3) / 0.9));
        for (let c = 0; c < 4; c++) { const fr = 1 - Math.min(1.2, (a - (inner(c) ? 0 : 0.06)) * 2.6); if (fr > 1) continue; const env = Math.exp(-a / 0.55);
          L.band(c, Math.max(0, fr), 1, hsv(f.hue, 0.3, 1), 0.45 * env, 0.02); L.blob(c, fr, 0.05, [1, 1, 1], 0.9 * env);
          for (let j = 0; j < 4; j++) L.blob(c, fr - 0.05 - 0.15 * j * hash(c, j), 0.02, hsv(f.hue + 20, 0.5, 1), 0.6 * env); }
      } },
      glacier: { name: 'GLACIER', label: 'GLACIER', hue: 228, look: 'split', dur: 1.2, fx(L, f, a, P) {
        L.dimAll(0.3 + 0.7 * smooth(a / f.dur));
        for (let c = 0; c < 4; c++) { const ac = a - (inner(c) ? 0 : 0.08); if (ac < 0) continue; const r = Math.min(0.5 * P.size, 1.6 * ac), env = ac < 0.5 ? 1 : Math.exp(-(ac - 0.5) / 0.2);
          L.band(c, f.x0 - r, f.x0 + r, hsv(f.hue, 0.6, 1), 0.55 * env, 0.006);
          for (let k = 0; k < 5; k++) L.blob(c, f.x0 + r * (2 * hash(c, k) - 1), 0.006, [1, 1, 1], ac > 0.4 ? 0.9 * env : 0); }
      } },
      spectre: { name: 'SPECTRE', label: 'SPECTRE', hue: 275, big: true, look: 'nova', dur: 1.35, fx(L, f, a) {
        const B = 0.2; if (a < B) { L.dimAll(0); for (const c of [1, 2]) L.blob(c, f.x0, 0.02, hsv(f.hue, 0.4, 1), 0.3); return; }
        const ab = a - B; L.dimAll(0.25 + 0.75 * smooth(ab / 0.9));
        for (let c = 0; c < 4; c++) { L.band(c, 0, 1, hsv(f.hue, 0.15, 1), 0.65 * Math.exp(-ab / 0.07), 0.01);
          for (let j = 0; j < 3; j++) { const x = f.x0 + (j - 1) * 0.2 + 0.25 * Math.sin(ab * 5 + c + j), env = Math.exp(-ab / 0.55); L.blob(c, x + ab * 0.5, 0.06, hsv(f.hue - 15 * j, 0.55, 1), 0.6 * env); } }
      } },
    } },
    shout: { name: 'Frost breath: teal ripples with sparkle', dur: 1.1, fx(L, f, a) {
      const k = f.k ?? 1;
      for (let c = 0; c < 4; c++) for (let j = 0; j < 3; j++) { const ac = a - (inner(c) ? 0 : 0.06) - j * 0.1; if (ac < 0) continue; const r = (0.5 + 0.6 * k) * ac, env = (0.6 - 0.15 * j) * k * Math.exp(-ac / 0.3);
        L.blob(c, f.x0 + r, 0.02, hsv(178 + 8 * j, 0.45, 1), env); L.blob(c, f.x0 - r, 0.02, hsv(178 + 8 * j, 0.45, 1), env);
        if (hash(c + j, Math.floor(a * 40)) > 0.6) L.blob(c, f.x0 + r * 0.6, 0.006, [1, 1, 1], env); }
    } },
    victory: { name: 'Crimson shatter, ice crystals, cold-gold afterglow', dur: VICTORY.dur, fx: (L, f, a, P) => finale(L, f, a, P, { style: 'crystal', win: [212, 240, 272, 46] }) },
  },

  storm: {
    name: 'Storm', blurb: 'Rain and lightning: a dark sky that gets wilder the harder you hit.',
    accents: [48, 232, 262, 290, 40],
    idle: { name: 'Drizzle and distant sheet lightning', fn(L, t, g, P) {
      const flash = hash(Math.floor(t * 2), 7) > 0.93 ? 0.25 * (1 - fract(t * 2)) : 0;
      fill(L, (c, x) => scale(CLOUD(0.35 + 0.5 * x), 0.5 + flash * 3));
      for (let c = 0; c < 4; c++) for (let k = 0; k < 4; k++) { const ph = phase(t, c, k, 0.55); L.glow(c, 1 - ph, 0.012, hsv(220, 0.35, 1), 0.35); L.glow(c, 1 - ph + 0.03 * P.size, 0.02 * P.size, hsv(225, 0.6, 1), 0.12); }
    } },
    warming: { name: 'Gathering storm: clouds fill, rain, inner flashes, crawling bolts', fn(L, t, g, P) {
      const { e, stage } = warmBase(L, t, g);
      const fl = stage >= 2 && hash(Math.floor(t * 5), 3) > 0.75 ? 1 - fract(t * 5) : 0;
      fill(L, (c, x, i) => {
        if (x < e) { const rel = x / Math.max(e, 0.01), churn = 0.5 + 0.5 * Math.sin(x * 14 - t * (1 + 2 * e) + c * 2) * Math.sin(x * 5 + t * 0.7);
          const col = CLOUD(0.45 + 0.4 * rel + 0.15 * churn + 0.3 * L.stageKick);
          return [col[0] + 0.5 * fl * rel, col[1] + 0.5 * fl * rel, col[2] + 0.6 * fl * rel]; }
        if (x < e + 0.012) return hsv(235, 0.45, 0.8 + 0.2 * Math.sin(t * 12));
        return scale(CLOUD(0.4), 0.25 * Math.max(0, 1 - (x - e) * 3));
      });
      const drops = [2, 4, 7, 10][stage];
      for (let c = 0; c < 4; c++) for (let k = 0; k < drops; k++) { const ph = phase(t, c, k, 0.6 + 0.5 * e), x = 1 - ph * (1 - e); L.glow(c, x, 0.011 * P.size, hsv(222, 0.3, 1), 0.4 + 0.3 * e); }
      if (stage >= 3) for (let c = 0; c < 4; c++) if (hash(c, Math.floor(t * 9)) > 0.7) for (let i = 0; i < L.n; i++) { const x = (i + 0.5) / L.n; if (Math.abs(x - e) < 0.08 && hash(c + i, Math.floor(t * 30)) > 0.4) L.glow(c, x, 0.006, hsv(50, 0.25, 1), 0.8); }
      kicks(L, e, hsv(230, 0.3, 1));
    } },
    open: { name: 'First strike: one huge bolt, then the downpour', dur: 1.7, fx(L, f, a) {
      const A = 0.35;
      if (a < A) { L.dimAll(0.05 + 0.1 * (a / A)); return; }
      const ab = a - A; if (ab < 0.03) L.rage = Math.max(L.rage, 0.5);
      L.dimAll(0.15 + 0.85 * smooth(ab / 1.2));
      const fl = ab < 0.06 ? 1 : ab < 0.1 ? 0.2 : ab < 0.16 ? 0.8 : Math.exp(-(ab - 0.16) / 0.2);
      for (let c = 0; c < 4; c++) { for (let i = 0; i < L.n; i++) { const x = (i + 0.5) / L.n, h = hash(c * 13, Math.floor(x * 16)); L.add(c, i, hsv(230 + 30 * h, h > 0.6 ? 0.2 : 0.6, 1), 0.9 * fl * (0.5 + 0.5 * h)); }
        for (let k = 0; k < 6; k++) { const ph = fract(ab * 1.4 + hash(c, k)); L.blob(c, 1 - ph, 0.012, hsv(220, 0.3, 1), 0.6 * (1 - smooth(ab / 1.3))); } }
    } },
    fight: { name: 'Downpour: rain and flashes grow with rage', fn(L, t, g, P) {
      const rg = L.rage, flash = hash(Math.floor(t * (1 + 6 * rg)), 11) > 0.9 - 0.1 * rg ? (0.15 + 0.35 * rg) * (1 - fract(t * (1 + 6 * rg))) : 0;
      fill(L, (c, x) => { const cl = CLOUD(Math.min(1, 0.3 + (0.3 + 0.4 * rg) * x * x)); return [cl[0] * (0.6 + 0.6 * rg) + flash * x * 0.8, cl[1] * (0.6 + 0.6 * rg) + flash * x * 0.85, cl[2] * (0.6 + 0.6 * rg) + flash * x]; });
      for (let c = 0; c < 4; c++) for (let k = 0; k < 3 + Math.round(9 * rg); k++) { const ph = phase(t, c, k, 0.7 + 0.8 * rg); L.glow(c, 1 - ph, 0.01, hsv(222, 0.3, 1), 0.3 + 0.3 * rg); L.glow(c, 1 - ph + 0.03 * P.size, 0.02 * P.size, hsv(228, 0.6, 1), 0.1); }
    } },
    jab: { kinds: {
      drop: { name: 'Raindrop streak', dur: 0.4, fx(L, f, a) { const x = f.x0 - 1.2 * a, k = 1 - a / f.dur; L.blob(f.col, x, 0.018, hsv(f.hue, 0.5, 1), 0.9 * k); L.blob(f.col, x + 0.04, 0.03, hsv(f.hue, 0.8, 1), 0.3 * k); } },
      spark: { name: 'Static spark', dur: 0.3, fx(L, f, a) { if (Math.floor(a * 50) % 2) return; L.blob(f.col, f.x0 + 0.03 * (hash(f.seed, Math.floor(a * 50)) - 0.5), 0.02, hsv(f.hue, 0.4, 1), 0.95 * (1 - a / f.dur)); } },
    } },
    hook: { kinds: {
      gust: { name: 'Gust', look: 'sweep', dur: 0.55, fx(L, f, a, P) {
        (f.dir > 0 ? [0, 1, 2, 3] : [3, 2, 1, 0]).forEach((c, j) => { const ac = a - j * 0.045; if (ac < 0) return; const env = ac < 0.02 ? ac / 0.02 : Math.exp(-(ac - 0.02) / 0.1), x = f.x0 + 0.08 * j - 0.1;
          L.band(c, x - 0.1 * P.size, x + 0.1 * P.size, hsv(f.hue, 0.55, 1), 0.55 * env, 0.06); });
      } },
      rumble: { name: 'Rumble', look: 'double', dur: 0.6, fx(L, f, a) {
        for (let c = 0; c < 4; c++) { const env = Math.exp(-a / 0.2) * (0.6 + 0.4 * Math.sin(a * 55 + c)); L.band(c, f.x0 - 0.2, f.x0 + 0.2, hsv(f.hue, 0.7, 1), 0.4 * env, 0.08); }
      } },
    } },
    heavy: { kinds: {
      lightning: { name: 'LIGHTNING', label: 'LIGHTNING', hue: 52, big: true, look: 'thunder', dur: 1.0, fx(L, f, a) {
        L.dimAll(0.2 + 0.8 * smooth(a / f.dur));
        for (const [c, at] of [[1, 0], [3, 0.09], [0, 0.18], [2, 0.3]]) { const as = a - at; if (as < 0) continue; const env = as < 0.05 ? 1 : Math.exp(-(as - 0.05) / 0.07), fl = as > 0.025 && as < 0.04 ? 0.3 : 1;
          for (let i = 0; i < L.n; i++) { const x = (i + 0.5) / L.n, blk = Math.floor(x * 18), h = hash(f.seed + c, blk); if (h < 0.2) continue;
            L.add(c, i, hsv(h > 0.75 ? f.hue : 235, h > 0.75 ? 0.4 : 0.55, 1), 0.95 * env * fl * (0.4 + 0.6 * h)); } }
      } },
      clap: { name: 'THUNDERCLAP', label: 'THUNDERCLAP', hue: 240, look: 'nova', dur: 1.2, fx(L, f, a) {
        const B = 0.18; if (a < B) { L.dimAll(0); return; }
        const ab = a - B; L.dimAll(0.1 + 0.9 * smooth((ab - 0.15) / 0.9));
        for (let c = 0; c < 4; c++) { L.band(c, 0, 1, hsv(f.hue, 0.12, 1), 0.85 * Math.exp(-ab / 0.06), 0.01); L.band(c, 0, 1, hsv(f.hue + 25, 0.7, 1), 0.25 * Math.exp(-ab / 0.4) * (0.6 + 0.4 * Math.sin(ab * 40 + c)), 0.05); }
      } },
      hail: { name: 'HAIL', label: 'HAIL', hue: 210, look: 'gold', dur: 1.4, fx(L, f, a) {
        L.dimAll(0.4 + 0.6 * smooth(a / f.dur));
        for (let c = 0; c < 4; c++) for (let k = 0; k < 9; k++) { const t0 = 0.6 * hash(c, k); if (a < t0) continue; const at = a - t0, x0 = 0.6 + 0.4 * hash(k, c);
          let x = x0 - 2.4 * at * at; let bounce = 0; if (x < 0.02) { bounce = at - Math.sqrt(x0 / 2.4); x = 0.02 + Math.max(0, 0.35 * bounce - 1.5 * bounce * bounce); }
          L.blob(c, x, 0.012, [0.9, 0.95, 1], 0.9 * (1 - a / f.dur)); }
      } },
    } },
    shout: { name: 'Wind call: teal ripples blow the rain back up', dur: 1.1, fx(L, f, a) {
      const k = f.k ?? 1;
      for (let c = 0; c < 4; c++) { for (let j = 0; j < 2; j++) { const ac = a - (inner(c) ? 0 : 0.06) - j * 0.13; if (ac < 0) continue; const r = (0.55 + 0.6 * k) * ac, env = (0.6 - 0.2 * j) * k * Math.exp(-ac / 0.33);
        L.blob(c, f.x0 + r, 0.022, hsv(170 + 10 * j, 0.55, 1), env); L.blob(c, f.x0 - r, 0.022, hsv(170 + 10 * j, 0.55, 1), env); }
        for (let q = 0; q < 3; q++) L.blob(c, fract(hash(c, q) + a * 1.2), 0.01, hsv(185, 0.3, 1), 0.4 * k * (1 - a / f.dur)); }
    } },
    victory: { name: 'Crimson shatter, a storm of bolts, golden clearing sky', dur: VICTORY.dur, fx: (L, f, a, P) => finale(L, f, a, P, { style: 'bolts', win: [50, 232, 268, 42] }) },
  },

  neon: {
    name: 'Neon dojo', blurb: 'Thin neon lines on black: chasing segments, slashes, outlines.',
    accents: [300, 325, 275, 40, 350],
    idle: { name: 'Neon outline with a slow chaser', fn(L, t, g, P) {
      fill(L, () => [0, 0, 0]);
      for (let c = 0; c < 4; c++) { L.glow(c, 0.01, 0.01, hsv(300, 0.8, 1), 0.5); L.glow(c, 0.99, 0.01, hsv(275, 0.8, 1), 0.5); }
      const p = fract(t * 0.18) * 4, c = Math.floor(p), x = fract(p);
      L.glowBand(c, x - 0.03 * P.size, x + 0.03 * P.size, hsv(320, 0.85, 1), 0.7, 0.005);
      L.glowBand(c, x - 0.12 * P.size, x - 0.03 * P.size, hsv(300, 0.9, 1), 0.15, 0.01);
    } },
    warming: { name: 'Neon meter: ticks, chasers, colour shift, tick strobe', fn(L, t, g, P) {
      const { e, stage } = warmBase(L, t, g), hue = lerp(285, 375, e);
      fill(L, (c, x) => {
        const tick = Math.abs(x * 10 - Math.round(x * 10)) < 0.06;
        if (x < e) return hsv(hue - 30 * (1 - x / Math.max(e, 0.01)), 0.9, (tick ? (stage >= 3 && Math.floor(t * 12 + c) % 2 ? 0.9 : 0.55) : 0.06) + 0.35 * L.stageKick);
        if (x < e + 0.014) return hsv(hue + 15, 0.35, 1);
        return tick ? hsv(270, 0.8, 0.07) : [0, 0, 0];
      });
      if (stage >= 2) for (let c = 0; c < 4; c++) L.glow(c, e + 0.03, 0.006, hsv(hue + 40, 0.6, 1), 0.7); // double line
      for (let k = 0; k < [0, 1, 2, 4][stage]; k++) for (let c = 0; c < 4; c++) { const x = fract(t * (0.6 + 0.3 * k) + k * 0.37 + c * 0.25) * e; L.glowBand(c, x - 0.02 * P.size, x + 0.02 * P.size, hsv(hue + 30, 0.6, 1), 0.85, 0.004); }
      kicks(L, e, hsv(hue + 20, 0.35, 1), 0.02);
    } },
    open: { name: 'Lines zip up column by column, then all flash', dur: 1.6, fx(L, f, a) {
      L.dimAll(a < 1 ? 0.1 : 0.1 + 0.9 * smooth((a - 1) / 0.5));
      for (let c = 0; c < 4; c++) { const ac = a - c * 0.08, x = Math.min(1, ac * 4); if (ac < 0) continue; L.band(c, 0, x, hsv(305 - 10 * c, 0.85, 1), 0.12, 0.005); L.blob(c, x, 0.015, [1, 0.85, 1], 0.9 * (x < 1 ? 1 : Math.exp(-(ac - 0.25) / 0.1))); }
      if (a > 0.6 && a < 1.1) { const k = Math.exp(-(a - 0.6) / 0.12); for (let c = 0; c < 4; c++) { L.band(c, 0, 0.04, hsv(320, 0.7, 1), k, 0.005); L.band(c, 0.96, 1, hsv(280, 0.7, 1), k, 0.005); L.band(c, 0, 1, hsv(300, 0.5, 1), 0.3 * k, 0.01); } }
      if (a > 0.62 && a < 0.66) L.rage = Math.max(L.rage, 0.45);
    } },
    fight: { name: 'Chasers: more and faster with rage', fn(L, t, g, P) {
      const rg = L.rage; fill(L, () => [0, 0, 0]);
      for (let c = 0; c < 4; c++) { L.glow(c, 0.01, 0.012, hsv(300, 0.8, 1), 0.35 + 0.4 * rg); L.glow(c, 0.99, 0.012, hsv(275, 0.8, 1), 0.35 + 0.4 * rg); }
      const nch = 1 + Math.round(5 * rg);
      for (let k = 0; k < nch; k++) for (let c = 0; c < 4; c++) { const dir = (k + c) % 2 ? 1 : -1, x = fract(dir * t * (0.25 + 0.9 * rg) * (1 + 0.2 * k) + hash(c, k)), len = (0.025 + 0.03 * rg) * P.size;
        L.glowBand(c, x - len, x + len, hsv([300, 325, 275, 40][k % 4], 0.85, 1), 0.35 + 0.5 * rg, 0.004); }
    } },
    jab: { kinds: {
      blip: { name: 'Neon blip', dur: 0.3, fx(L, f, a) { L.band(f.col, f.x0 - 0.02, f.x0 + 0.02, hsv(f.hue, 0.85, 1), 0.95 * Math.exp(-a / 0.09), 0.003); } },
      dash: { name: 'Dash up', dur: 0.35, fx(L, f, a) { const x = f.x0 + 0.9 * a; L.band(f.col, x - 0.03, x + 0.01, hsv(f.hue, 0.85, 1), 0.9 * (1 - a / f.dur), 0.003); } },
    } },
    hook: { kinds: {
      slash: { name: 'Slash', look: 'sweep', dur: 0.55, fx(L, f, a, P) {
        (f.dir > 0 ? [0, 1, 2, 3] : [3, 2, 1, 0]).forEach((c, j) => { const ac = a - j * 0.04; if (ac < 0) return; const x = f.x0 + 0.08 * (j - 1.5), env = ac < 0.015 ? 1 : Math.exp(-(ac - 0.015) / 0.12);
          L.band(c, x - 0.012 * P.size, x + 0.012 * P.size, [1, 0.9, 1], env, 0.003); L.band(c, x - 0.05 * P.size, x + 0.05 * P.size, hsv(f.hue, 0.9, 1), 0.3 * env, 0.004); });
      } },
      bracket: { name: 'Bracket', look: 'ring', dur: 0.5, fx(L, f, a) {
        for (let c = 0; c < 4; c++) { const r = 0.25 * Math.exp(-a / 0.08), env = a < 0.25 ? 1 : Math.exp(-(a - 0.25) / 0.08);
          L.band(c, f.x0 + r, f.x0 + r + 0.02, hsv(f.hue, 0.85, 1), 0.8 * env, 0.003); L.band(c, f.x0 - r - 0.02, f.x0 - r, hsv(f.hue, 0.85, 1), 0.8 * env, 0.003); }
      } },
    } },
    heavy: { kinds: {
      katana: { name: 'KATANA', label: 'KATANA', hue: 330, big: true, look: 'split', dur: 1.1, fx(L, f, a) {
        L.dimAll(0.15 + 0.85 * smooth((a - 0.3) / 0.8));
        for (let c = 0; c < 4; c++) { const at = a - c * 0.03; if (at < 0) continue; const env = Math.exp(-at / 0.5), gap = Math.min(0.12, at * 0.5);
          if (at < 0.12) L.band(c, f.x0 - 0.008, f.x0 + 0.008, [1, 1, 1], 1, 0.002);
          L.band(c, 0, f.x0 - gap, hsv(f.hue, 0.85, 1), 0.3 * env, 0.004); L.band(c, f.x0 + gap, 1, hsv(f.hue - 30, 0.85, 1), 0.3 * env, 0.004);
          L.band(c, f.x0 - gap - 0.015, f.x0 - gap, [1, 0.8, 1], 0.9 * env, 0.003); L.band(c, f.x0 + gap, f.x0 + gap + 0.015, [1, 0.8, 1], 0.9 * env, 0.003); }
      } },
      overdrive: { name: 'OVERDRIVE', label: 'OVERDRIVE', hue: 290, look: 'strobe', dur: 1.0, fx(L, f, a) {
        L.dimAll(0.1 + 0.9 * smooth((a - 0.6) / 0.4));
        for (let c = 0; c < 4; c++) for (let k = 0; k < 6; k++) { const x = fract((k % 2 ? 1 : -1) * a * 2.4 + k / 6 + c * 0.13); L.band(c, x - 0.03, x + 0.03, hsv(f.hue + 60 * Math.floor(a * 8 + k), 0.8, 1), 0.85 * (1 - a / f.dur), 0.004); }
      } },
      burst: { name: 'NEON BURST', label: 'NEON BURST', hue: 315, big: true, look: 'nova', dur: 1.2, fx(L, f, a) {
        const B = 0.18; if (a < B) { L.dimAll(0); for (let c = 0; c < 4; c++) { L.blob(c, 0.01, 0.01, hsv(f.hue, 0.8, 1), 0.6); L.blob(c, 0.99, 0.01, hsv(f.hue, 0.8, 1), 0.6); } return; }
        const ab = a - B; L.dimAll(0.2 + 0.8 * smooth(ab / 0.9));
        for (let c = 0; c < 4; c++) for (let k = 0; k < 4; k++) { const r = (1.2 + 0.5 * k) * ab, env = Math.exp(-ab / 0.45); for (const s of [1, -1]) L.band(c, f.x0 + s * r - 0.015, f.x0 + s * r + 0.015, hsv(f.hue - 25 * k, 0.75, 1), 0.85 * env, 0.003); }
      } },
    } },
    shout: { name: 'Sonic lines: thin teal lines move outwards', dur: 1.0, fx(L, f, a) {
      const k = f.k ?? 1;
      for (let c = 0; c < 4; c++) for (let j = 0; j < 3; j++) { const ac = a - (inner(c) ? 0 : 0.05) - j * 0.1; if (ac < 0) continue; const r = (0.55 + 0.6 * k) * ac, env = (0.75 - 0.2 * j) * k * Math.exp(-ac / 0.3);
        for (const s of [1, -1]) L.band(c, f.x0 + s * r - 0.008, f.x0 + s * r + 0.008, hsv(174 + 8 * j, 0.6, 1), env, 0.003); }
    } },
    victory: { name: 'Crimson shatter, racing neon chasers, golden outline glow', dur: VICTORY.dur, fx: (L, f, a, P) => finale(L, f, a, P, { style: 'chase', win: [305, 330, 275, 45] }) },
  },

  retro: {
    name: 'Retro arcade', blurb: 'Chunky 8-bit pixel blocks, scanline sweeps and a power-up meter.',
    accents: [0, 52, 112, 232, 300],
    idle: { name: 'Insert coin: blinking blocks, scanline sweep', fn(L, t, g, P) {
      const scan = fract(t * 0.25) * NB, blink = Math.floor(t * 1.2) % 2;
      fillBlocks(L, (c, b) => {
        const s = Math.max(0, 1 - Math.abs(b - scan) / (1.2 * P.size));
        const coin = (b === 5 || b === 6) && (c + b + blink) % 2 ? 0.22 : 0;
        return hsv([232, 300, 52, 112][c], 0.9, 0.05 + 0.35 * s + coin);
      });
    } },
    warming: { name: 'Power-up meter: blocks per stage, level-up blink', fn(L, t, g, P) {
      const { e, stage } = warmBase(L, t, g), nb = e * NB, hue = [112, 52, 28, 0][stage], up = L.stageKick > 0.2 && Math.floor(t * 10) % 2;
      const scan = fract(t * (0.4 + 0.8 * e)) * NB;
      fillBlocks(L, (c, b) => {
        if (b < Math.floor(nb)) { const flash = up ? 0.5 : 0, s = Math.max(0, 1 - Math.abs(b - scan)) * (stage >= 1 ? 0.35 : 0); return hsv(hue + (stage >= 2 ? 25 * ((b + c) % 2) : 0), 0.95 - flash, Math.min(1, 0.35 + 0.25 * b / NB + flash + s)); }
        if (b === Math.floor(nb)) return hsv(hue, 0.6, 0.15 + 0.6 * fract(nb) * (0.6 + 0.4 * Math.sin(t * 14)));
        return stage >= 3 && Math.floor(t * 8 + b + c) % 4 === 0 ? hsv(0, 0.9, 0.18) : [0.015, 0.015, 0.03];
      });
      kicks(L, e, hsv(hue, 0.3, 1), 0.04);
    } },
    open: { name: 'READY? 3-2-1 blinks, then FIGHT! blocks fill', dur: 1.8, fx(L, f, a) {
      L.dimAll(a < 1 ? 0.08 : 0.08 + 0.92 * smooth((a - 1) / 0.8));
      if (a < 0.35) { const on = Math.floor(a / 0.117) % 2 === 0; for (let c = 0; c < 4; c++) for (const b of [5, 6]) block(L, c, b, hsv(52, 0.9, 1), on ? 0.8 : 0); return; }
      const ab = a - 0.35; if (ab < 0.03) L.rage = Math.max(L.rage, 0.45);
      const nb = Math.min(NB, Math.floor(ab * 30)), env = Math.exp(-ab / 0.6);
      for (let c = 0; c < 4; c++) for (let b = NB - nb; b < NB; b++) block(L, c, b, hsv([0, 52, 112, 232, 300][(b + c) % 5], 0.9, 1), 0.7 * env);
    } },
    fight: { name: 'Equalizer bars driven by rage, scanline', fn(L, t, g, P) {
      const rg = L.rage, scan = fract(t * 0.8) * NB;
      fillBlocks(L, (c, b) => {
        const h = (0.12 + 0.75 * rg * (0.55 + 0.45 * Math.sin(t * (3 + 3 * c) + c * 2))) * NB * P.size;
        const s = Math.max(0, 1 - Math.abs(b - scan)) * 0.12;
        if (b < h) return hsv(b > 0.75 * NB ? 0 : b > 0.5 * NB ? 52 : 112, 0.95, (0.25 + 0.45 * rg) * (b + 1 > h ? fract(h) + 0.3 : 1) + s);
        return [s * 0.3, s * 0.3, s * 0.5];
      });
    } },
    jab: { kinds: {
      pixel: { name: 'Pixel', dur: 0.3, fx(L, f, a) { block(L, f.col, Math.floor(f.x0 * NB), hsv(f.hue, 0.9, 1), a < 0.2 ? 0.9 : 0); } },
      coin: { name: 'Coin hop', dur: 0.4, fx(L, f, a) { const b = Math.floor(f.x0 * NB) + Math.min(2, Math.floor(a / 0.1)); block(L, f.col, b, hsv(52, 0.85, 1), 0.9); } },
    } },
    hook: { kinds: {
      runner: { name: 'Block runner', look: 'sweep', dur: 0.55, fx(L, f, a) {
        const order = f.dir > 0 ? [0, 1, 2, 3] : [3, 2, 1, 0], j = Math.min(3, Math.floor(a / 0.1)); block(L, order[j], Math.floor(f.x0 * NB), hsv(f.hue, 0.9, 1), 0.9);
        if (j > 0) block(L, order[j - 1], Math.floor(f.x0 * NB), hsv(f.hue, 0.9, 1), 0.3);
      } },
      stack: { name: 'Combo stack', look: 'double', dur: 0.55, fx(L, f, a) {
        const b0 = Math.floor(f.x0 * NB), n = Math.min(3, 1 + Math.floor(a / 0.08));
        for (const c of [1, 2]) for (let k = 0; k < n; k++) block(L, c, b0 + k - 1, hsv(f.hue + 30 * k, 0.9, 1), a < 0.4 ? 0.85 : 0);
      } },
    } },
    heavy: { kinds: {
      koflash: { name: 'K.O. FLASH', label: 'K.O. FLASH', hue: 52, look: 'strobe', dur: 1.0, fx(L, f, a) {
        const k = Math.floor(a / 0.12); L.dimAll(k < 5 ? 0.05 : 0.05 + 0.95 * smooth((a - 0.6) / 0.4));
        if (k < 5 && a - k * 0.12 < 0.07) for (let c = 0; c < 4; c++) for (let b = 0; b < NB; b++) if ((b + c + k) % 2) block(L, c, b, hsv([f.hue, 0, 300, 232, 112][k], 0.85, 1), 0.85);
      } },
      blast: { name: 'PIXEL BLAST', label: 'PIXEL BLAST', hue: 0, big: true, look: 'nova', dur: 1.2, fx(L, f, a) {
        const B = 0.15; if (a < B) { L.dimAll(0); block(L, 1, Math.floor(f.x0 * NB), [1, 1, 1], 0.6); block(L, 2, Math.floor(f.x0 * NB), [1, 1, 1], 0.6); return; }
        const ab = a - B, r = Math.floor(ab * 16), b0 = Math.floor(f.x0 * NB), env = Math.exp(-ab / 0.5); L.dimAll(0.2 + 0.8 * smooth(ab / 0.9));
        for (let c = 0; c < 4; c++) for (const s of [1, -1]) { block(L, c, b0 + s * r, hsv(f.hue + 50 * r, 0.9, 1), 0.9 * env); block(L, c, b0 + s * (r - 1), hsv(f.hue + 50 * r + 30, 0.9, 1), 0.35 * env); }
      } },
      hadoken: { name: 'FIREBALL', label: 'FIREBALL', hue: 22, big: true, look: 'gold', dur: 1.2, fx(L, f, a) {
        L.dimAll(0.3 + 0.7 * smooth(a / f.dur));
        for (let c = 0; c < 4; c++) { const at = a - (inner(c) ? 0 : 0.1); if (at < 0) continue; const b = Math.floor(f.x0 * NB + at * 14), env = Math.exp(-at / 0.6);
          block(L, c, b, hsv(f.hue + 25, 0.6, 1), 0.95 * env); block(L, c, b - 1, hsv(f.hue, 0.95, 1), 0.6 * env); block(L, c, b - 2, hsv(f.hue - 15, 1, 1), 0.25 * env); }
      } },
    } },
    shout: { name: '8-bit ping: teal blocks ripple outwards', dur: 1.0, fx(L, f, a) {
      const k = f.k ?? 1, b0 = Math.floor(f.x0 * NB), r = Math.floor(a * 12 * (0.6 + 0.5 * k)), env = 0.75 * k * (1 - a / f.dur);
      for (let c = 0; c < 4; c++) { block(L, c, b0 + r, hsv(174, 0.6, 1), env); block(L, c, b0 - r, hsv(174, 0.6, 1), env); }
    } },
    victory: { name: 'Crimson shatter, block power-up, golden high score', dur: VICTORY.dur, fx: (L, f, a, P) => finale(L, f, a, P, { style: 'pixels', win: [0, 52, 112, 232, 300] }) },
  },
};
export const PRESET_IDS = Object.keys(PRESETS);

/** The hit table a preset gives the HitPicker: kinds per tier (with hue/label/look) and accent hues. */
export function hitTable(id) {
  const p = PRESETS[id] || PRESETS.proposal;
  if (p.jab.none) return hitTable('proposal'); // brain-rs: the screen and the sound still get a signature
  const kinds = {};
  for (const tier of ['jab', 'hook', 'heavy']) kinds[tier] = Object.fromEntries(Object.entries(p[tier].kinds).map(([k, v]) => [k, { hue: v.hue, label: v.label, big: v.big, look: v.look || k }]));
  return { preset: p.jab.none ? 'proposal' : id, kinds, accents: p.accents };
}
