// LED rendering for the four Perspex columns.
// mode 'brain-rs': what brain-rs/src/services/wled_manager.rs + config.yaml `wled.effects`
//   ask for per state (WLED built-ins approximated in JS, FIGHT realtime frame ported).
// mode 'proposal': SIM PROPOSAL effects (energy-bar warming, punch shockwaves, ...).
// Each column is a Float32Array of RGB in 0..1, index 0 = bottom LED.
import { ArenaState } from './state_machine.js';

export const COLUMNS = 4;

const lerp = (a, b, t) => a + (b - a) * t;
const clamp01 = (x) => Math.min(Math.max(x, 0), 1);
function hsv(h, s, v) {
  h = ((h % 360) + 360) % 360;
  const f = (n) => { const k = (n + h / 60) % 6; return v - v * s * Math.max(Math.min(k, 4 - k, 1), 0); };
  return [f(5), f(3), f(1)];
}
const hex = (h) => [parseInt(h.slice(1, 3), 16) / 255, parseInt(h.slice(3, 5), 16) / 255, parseInt(h.slice(5, 7), 16) / 255];
function palette(stops) { return stops.map(hex); }
function samplePal(pal, x) {
  x = ((x % 1) + 1) % 1;
  const p = x * pal.length, i = Math.floor(p), t = p - i;
  const a = pal[i % pal.length], b = pal[(i + 1) % pal.length];
  return [lerp(a[0], b[0], t), lerp(a[1], b[1], t), lerp(a[2], b[2], t)];
}
function samplePalClamp(pal, x) {
  x = clamp01(x) * (pal.length - 1);
  const i = Math.min(Math.floor(x), pal.length - 2), t = x - i;
  const a = pal[i], b = pal[i + 1];
  return [lerp(a[0], b[0], t), lerp(a[1], b[1], t), lerp(a[2], b[2], t)];
}

// FastLED palettes used by WLED ("Ocean" = OceanColors_p, "Lava" = LavaColors_p).
const OCEAN = palette(['#191970', '#00008b', '#191970', '#000080', '#00008b', '#0000cd', '#2e8b57', '#008080',
  '#5f9ea0', '#0000ff', '#008b8b', '#6495ed', '#7fffd4', '#2e8b57', '#00ffff', '#87cefa']);
const LAVA = palette(['#000000', '#800000', '#000000', '#800000', '#8b0000', '#800000', '#8b0000', '#8b0000',
  '#8b0000', '#8b0000', '#ff0000', '#ffa500', '#ffffff', '#ffa500', '#ff0000', '#8b0000']);
const HEAT = palette(['#000000', '#330000', '#990000', '#ff2200', '#ff7700', '#ffcc33', '#ffffcc']);
const LAVA_HEAT = palette(['#000000', '#3a0000', '#800000', '#c01000', '#ff4000', '#ffa500', '#fff0c0']);

/** Fire2012 heat simulation (WLED "Fire 2012" / FastLED example). */
class Fire {
  constructor(n) { this.heat = new Float32Array(n); }
  step(cooling, sparking, dtScale = 1) {
    const h = this.heat, n = h.length;
    for (let i = 0; i < n; i++) h[i] = Math.max(0, h[i] - Math.random() * ((cooling * 10) / n + 2) / 255 * dtScale);
    for (let k = n - 1; k >= 2; k--) h[k] = (h[k - 1] + h[k - 2] * 2) / 3;
    if (Math.random() * 255 < sparking * dtScale) {
      const y = Math.floor(Math.random() * Math.min(7, n));
      h[y] = Math.min(1, h[y] + (160 + Math.random() * 95) / 255);
    }
  }
}

export class LedRig {
  constructor(numLeds) { this.mode = 'proposal'; this.resize(numLeds); this.waves = []; this.flash = 0; this.shoutFlash = 0; this.lastT = null; }

  resize(n) {
    this.n = n;
    this.cols = Array.from({ length: COLUMNS }, () => new Float32Array(n * 3));
    this.fires = Array.from({ length: COLUMNS }, () => new Fire(n));
  }

  /** Sim proposal: shockwave from the bag outwards (inner columns first). */
  punch(power) {
    const p = power / 100;
    this.waves.push({ t0: performance.now() / 1000, p });
    if (p > 0.75) this.flash = Math.max(this.flash, p);
    if (this.waves.length > 24) this.waves.shift();
  }
  shout(intensity) { this.shoutFlash = Math.max(this.shoutFlash, 0.4 + intensity * 0.6); }

  /**
   * @param {string} state ArenaState
   * @param {object} g {energyPct, fightRemaining, fightDuration, cooldownProgress, level (0..1 mic level)}
   */
  render(state, g) {
    const t = performance.now() / 1000;
    const dt = this.lastT == null ? 1 / 60 : Math.min(t - this.lastT, 0.1);
    this.lastT = t;
    const frameScale = dt * 60;
    if (this.mode === 'brain-rs') this.renderBrainRs(state, t, frameScale);
    else this.renderProposal(state, t, g, frameScale);
    this.flash *= Math.pow(0.04, dt);
    this.shoutFlash *= Math.pow(0.05, dt);
    this.waves = this.waves.filter((w) => t - w.t0 < 2.5);
  }

  set(c, i, r, g, b) { const a = this.cols[c]; a[i * 3] = r; a[i * 3 + 1] = g; a[i * 3 + 2] = b; }
  fillAll(fn) { for (let c = 0; c < COLUMNS; c++) for (let i = 0; i < this.n; i++) { const [r, g, b] = fn(c, i); this.set(c, i, r, g, b); } }

  // ---------------------------------------------------------------- brain-rs
  renderBrainRs(state, t, frameScale) {
    const n = this.n, ms = t * 1000;
    switch (state) {
      case ArenaState.Suspended: // set_power(false)
        this.fillAll(() => [0, 0, 0]); break;
      case ArenaState.Idle: { // effects.idle: breathe, palette ocean, speed 128 (WLED mode_breath)
        let counter = Math.floor(ms * ((128 >> 3) + 10)) & 0xffff;
        counter = (counter >> 2) + (counter >> 4);
        let v = 0;
        if (counter < 16384) { if (counter > 8192) counter = 8192 - (counter - 8192); v = Math.sin(counter / 65536 * 2 * Math.PI) * 32767 / 103; }
        const lum = (30 + v) / 255;
        this.fillAll((c, i) => samplePal(OCEAN, i / n).map((x) => x * lum));
        break;
      }
      case ArenaState.Warming: { // effects.warming: rainbow, palette rainbow, speed 200 (WLED mode_rainbow_cycle)
        const counter = ((Math.floor(ms * ((200 >> 2) + 2)) & 0xffff) >> 8);
        this.fillAll((c, i) => hsv(((i * 256 / n + counter) % 256) / 256 * 360, 1, 1));
        break;
      }
      case ArenaState.Fight: { // effects.fight: realtime -> generate_fight_frame (uniform red pulse)
        const b = Math.sin(ms * 0.005) * 0.5 + 0.5;
        this.fillAll(() => [b, 0, 0]);
        break;
      }
      case ArenaState.Cooldown: { // effects.cooldown: fire, palette lava, speed 150
        for (let c = 0; c < COLUMNS; c++) {
          this.fires[c].step(255 - 150 - 50, 128, frameScale);
          for (let i = 0; i < n; i++) { const [r, g, b] = samplePalClamp(LAVA_HEAT, this.fires[c].heat[i]); this.set(c, i, r, g, b); }
        }
        break;
      }
      default: this.fillAll(() => [0, 0, 0]);
    }
  }

  // ---------------------------------------------------------------- proposal
  waveAt(c, i, t) {
    // Shockwave travelling up the column; inner columns (1,2) get it first.
    const delay = (c === 1 || c === 2) ? 0 : 0.07;
    let v = 0;
    for (const w of this.waves) {
      const age = t - w.t0 - delay;
      if (age < 0) continue;
      const speed = 1.2 + w.p * 2.2; // column-heights per second
      const front = age * speed;
      const x = i / this.n;
      const width = 0.06 + w.p * 0.16;
      const d = front - x;
      if (d >= 0 && d < width * 3) v += w.p * Math.exp(-d / width) * Math.exp(-age * 1.6);
    }
    return v;
  }

  renderProposal(state, t, g, frameScale) {
    const n = this.n;
    switch (state) {
      case ArenaState.Suspended:
        this.fillAll(() => [0, 0, 0]); break;
      case ArenaState.Idle: {
        // Slow violet breathing with a wave rolling across the arc to attract people.
        this.fillAll((c, i) => {
          const x = i / n;
          const breath = 0.08 + 0.32 * (0.5 + 0.5 * Math.sin(t * 0.9 - c * 0.7));
          const roll = 0.5 + 0.5 * Math.sin(t * 1.4 - x * 6 - c * 1.1);
          const v = breath * (0.55 + 0.45 * roll);
          return hsv(250 - 25 * x + 10 * Math.sin(t * 0.2), 0.75, v);
        });
        break;
      }
      case ArenaState.Warming: {
        // Energy bar on every column; colour heats up as it fills; shout = flash.
        const e = clamp01(g.energyPct ?? 0);
        const top = e * n;
        const lvl = clamp01(g.level ?? 0);
        this.fillAll((c, i) => {
          const x = i / n;
          if (i < top) {
            const heat = x; // colour by height: blue -> violet -> red -> orange -> white
            const hue = lerp(230, 380, Math.min(heat * 1.15, 1));
            const sat = heat > 0.9 ? lerp(1, 0.2, (heat - 0.9) * 10) : 1;
            const shimmer = 0.75 + 0.25 * Math.sin(t * 10 + i * 0.9 + c);
            const v = Math.min(1, (0.5 + 0.5 * x) * shimmer + this.shoutFlash * 0.6);
            return hsv(hue % 360, sat, v);
          }
          if (Math.abs(i - top) < 1.5) { const p = 0.6 + 0.4 * Math.sin(t * 14); return [p, p, p]; }
          // Above the bar: faint breathing + live mic level sparkle.
          const base = 0.03 + 0.03 * Math.sin(t * 2 + c);
          const spark = Math.random() < lvl * 0.15 ? lvl : 0;
          return hsv(260, 0.6, base + spark * 0.6);
        });
        break;
      }
      case ArenaState.Fight: {
        const left = g.fightRemaining ?? 99;
        const warn = left < 10 ? (0.5 + 0.5 * Math.sin(t * 12)) : 0;
        for (let c = 0; c < COLUMNS; c++) {
          this.fires[c].step(70, 110, frameScale);
          for (let i = 0; i < n; i++) {
            const x = i / n;
            let [r, gg, b] = samplePalClamp(HEAT, this.fires[c].heat[i] * 0.85);
            const pulse = 0.12 + 0.08 * Math.sin(t * 8.4); // ~80 bpm throb
            r = Math.max(r, pulse * (1 - x * 0.6));
            const w = this.waveAt(c, i, t);
            r += w * 1.0; gg += w * 0.75; b += w * 0.45;
            const f = this.flash * 0.8; r += f; gg += f; b += f;
            if (warn && x > 0.85) { r = Math.max(r, warn); gg *= 0.3; b *= 0.3; }
            this.set(c, i, Math.min(r, 1), Math.min(gg, 1), Math.min(b, 1));
          }
        }
        break;
      }
      case ArenaState.Cooldown: {
        // Embers cool down over the cooldown period and settle to calm blue.
        const p = clamp01(g.cooldownProgress ?? 0);
        for (let c = 0; c < COLUMNS; c++) {
          this.fires[c].step(lerp(90, 220, p), lerp(90, 10, p), frameScale);
          for (let i = 0; i < n; i++) {
            const [r, gg, b] = samplePalClamp(LAVA_HEAT, this.fires[c].heat[i] * (1 - p * 0.7));
            const calm = p * (0.06 + 0.05 * Math.sin(t * 0.8 + c)) * (1 - i / n * 0.5);
            this.set(c, i, r, gg + calm * 0.3, b + calm);
          }
        }
        break;
      }
      default: this.fillAll(() => [0, 0, 0]);
    }
  }

  /** Average colour per column (for floor glow). */
  average(c) {
    const a = this.cols[c]; let r = 0, g = 0, b = 0;
    for (let i = 0; i < this.n; i++) { r += a[i * 3]; g += a[i * 3 + 1]; b += a[i * 3 + 2]; }
    return [r / this.n, g / this.n, b / this.n];
  }
}

export const EFFECT_LABELS = {
  'brain-rs': {
    SUSPENDED: 'off (set_power false)',
    IDLE: 'WLED breathe / ocean / sx 128',
    WARMING: 'WLED rainbow / rainbow / sx 200',
    FIGHT: 'realtime: generate_fight_frame red pulse',
    COOLDOWN: 'WLED "fire" / lava / sx 150 (see README)',
  },
  proposal: {
    SUSPENDED: 'off',
    IDLE: 'violet breathing + rolling wave',
    WARMING: 'energy bar fill, shout flash',
    FIGHT: 'fire + punch shockwaves',
    COOLDOWN: 'embers cooling to calm blue',
  },
};
