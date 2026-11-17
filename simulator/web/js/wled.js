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

// Brightness cap for proposal frames: the mean channel level (0..1) of all LEDs. The real
// strip runs on a current limiter, so a frame never becomes a white wall; above this the
// whole frame is scaled down (colours keep their ratios).
export const FRAME_CAP = 0.72;
const smooth = (x) => { x = clamp01(x); return x * x * (3 - 2 * x); };
// Deterministic hash noise in 0..1 (stable bolt / shard shapes per seed).
const hash = (a, b) => { const x = Math.sin(a * 127.1 + b * 311.7) * 43758.5453; return x - Math.floor(x); };
const HEAVY_DUR = { thunder: 0.95, gold: 1.5, shatter: 1.1, strobe: 0.95, split: 1.0, nova: 1.35, open: 1.7 };

export class LedRig {
  constructor(numLeds) {
    this.mode = 'proposal'; this.resize(numLeds);
    this.waves = []; this.flash = 0; this.shoutFlash = 0; this.rage = 0; this.lastT = null;
    this.fx = [];            // active punch / shout / fight-start effects
    this.variety = true;     // tunable hit_fx_variety: false = the original white flash for every heavy
    this.intensity = 1;      // tunable hit_fx_intensity: scales the signature effects
    this.wStage = 0; this.stageKick = 0; this.wSparks = []; this.wArcs = []; this.wPops = []; this.shoutWaves = [];
    this.nextArc = 0; this.nextPop = 0; this.lastLevel = 0;
  }

  resize(n) {
    this.n = n;
    this.cols = Array.from({ length: COLUMNS }, () => new Float32Array(n * 3));
    this.fires = Array.from({ length: COLUMNS }, () => new Fire(n));
    this.addBuf = Array.from({ length: COLUMNS }, () => new Float32Array(n * 3));
    this.dimBuf = Array.from({ length: COLUMNS }, () => new Float32Array(n));
  }

  /**
   * Sim proposal: shockwave from the bag outwards (inner columns first) plus the punch's
   * signature (hitfx.js). Without a signature (or kind 'classic') heavies get the white flash.
   */
  punch(power, sig) {
    const p = power / 100, t = performance.now() / 1000;
    // Fight intensity: hard punches build it fast, light ones barely; it fades in a few seconds.
    this.rage = Math.min(1, this.rage + 0.08 + 0.4 * p * p);
    const classic = !sig || sig.kind === 'classic' || !this.variety;
    const tint = classic ? null : hsv(sig.hue, 0.75, 1);
    // light taps = small ripple; with a signature the wave is tinted and the signature carries the punch
    this.waves.push({ t0: t, p: p * p * 1.3 * (classic ? 1 : sig.tier === 'heavy' ? 0.55 : 0.8), tint });
    if (classic) { if (p > 0.75) this.flash = Math.max(this.flash, p); } else this.addFx(sig, t, p);
    if (this.waves.length > 24) this.waves.shift();
  }

  /** A shout: in WARMING it kicks the energy bar; in FIGHT it is a teal sound-wave ring (visual only). */
  shout(intensity, state) {
    this.shoutFlash = Math.max(this.shoutFlash, 0.4 + intensity * 0.6);
    const t = performance.now() / 1000;
    if (state === ArenaState.Fight) this.addFx({ tier: 'shout', kind: 'shout', hue: 172, k: intensity }, t, intensity);
    else this.shoutWaves.push({ t0: t, k: intensity });
    if (this.shoutWaves.length > 12) this.shoutWaves.shift();
  }

  /** Full energy bar -> FIGHT: the columns suck in, then explode into the fire (synced with sfx.arenaOpen). */
  fightStart() { this.fx = []; this.addFx({ tier: 'open', kind: 'open', hue: 30 }, performance.now() / 1000, 1); }

  addFx(sig, t, p) {
    const R = Math.random;
    const fx = { ...sig, t0: t, p, x0: 0.38 + 0.16 * R(), seed: R() * 1000 };
    if (sig.tier === 'heavy') {
      const e = sig.esc || 0;
      fx.dur = HEAVY_DUR[sig.kind] * (1 + 0.3 * e);
      if (sig.kind === 'thunder') {
        const k = 3 + Math.round(2 * e), order = [R() < 0.5 ? 1 : 2]; // first bolt on an inner column
        while (order.length < k) { const c = Math.floor(R() * 4); if (c !== order[order.length - 1]) order.push(c); }
        fx.strikes = order.map((c, j) => ({ c, at: [0, 0.08, 0.19, 0.31, 0.46][j], seed: R() * 1000 }));
      } else if (sig.kind === 'gold') {
        fx.parts = [];
        for (let c = 0; c < COLUMNS; c++) for (let j = 0; j < 6 + Math.round(4 * e); j++) {
          fx.parts.push({ c, v: 0.55 + 1.25 * R(), hue: 36 + 14 * R(), life: 0.9 + 0.6 * R() });
        }
      } else if (sig.kind === 'shatter') {
        fx.shards = [];
        for (let c = 0; c < COLUMNS; c++) {
          let a = 0;
          while (a < 1) {
            const b = Math.min(1, a + 0.07 + 0.12 * R());
            fx.shards.push({ c, a, b, v: (a + b) / 2 > fx.x0 ? 0.25 + 0.5 * R() : -(0.05 + 0.3 * R()), hue: 340 + 25 * R(), fade: 0.25 + 0.3 * R() });
            a = b;
          }
        }
      } else if (sig.kind === 'strobe') fx.pops = 3 + Math.round(2 * e);
    } else if (sig.tier === 'hook') fx.dur = 0.6;
    else if (sig.tier === 'jab') fx.dur = 0.32;
    else if (sig.tier === 'shout') { fx.dur = 0.9 + 0.5 * p; fx.x0 = 0.25 + 0.5 * R(); }
    else if (sig.tier === 'open') fx.dur = HEAVY_DUR.open;
    this.fx.push(fx);
    if (this.fx.length > 16) this.fx.shift();
  }

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
    else this.renderProposal(state, t, g, frameScale, dt);
    this.flash *= Math.pow(0.04, dt);
    this.rage *= Math.pow(0.72, dt); // ~3 s half-life: stop punching and it sinks back to a candle
    this.shoutFlash *= Math.pow(0.05, dt);
    this.stageKick *= Math.pow(0.02, dt);
    this.waves = this.waves.filter((w) => t - w.t0 < 2.5);
    this.fx = this.fx.filter((f) => t - f.t0 < f.dur);
    this.shoutWaves = this.shoutWaves.filter((w) => t - w.t0 < 1.2);
  }

  /** Mean channel level of the current frame (0..1); see FRAME_CAP. */
  level() {
    let s = 0;
    for (const a of this.cols) for (let k = 0; k < a.length; k++) s += a[k];
    return s / (COLUMNS * this.n * 3);
  }
  limit(cap = FRAME_CAP) {
    const m = this.level(); this.lastLevel = m;
    if (m <= cap) return;
    const k = cap / m; this.lastLevel = cap;
    for (const a of this.cols) for (let j = 0; j < a.length; j++) a[j] *= k;
  }

  // ---------------------------------------------------------------- effect buffers
  clearFx() { for (let c = 0; c < COLUMNS; c++) { this.addBuf[c].fill(0); this.dimBuf[c].fill(1); } }
  add(c, i, rgb, k) {
    if (k <= 0.002) return;
    const a = this.addBuf[c]; k *= this.gainK;
    a[i * 3] += rgb[0] * k; a[i * 3 + 1] += rgb[1] * k; a[i * 3 + 2] += rgb[2] * k;
  }
  /** soft dot centred at xc (0..1 of the column), w ~ half-width in column heights (>= 1 strip LED at 45) */
  blob(c, xc, w, rgb, k) {
    const n = this.n, lo = Math.max(0, Math.floor((xc - 2.2 * w) * n)), hi = Math.min(n - 1, Math.ceil((xc + 2.2 * w) * n));
    for (let i = lo; i <= hi; i++) { const d = ((i + 0.5) / n - xc) / w; this.add(c, i, rgb, k * Math.exp(-d * d)); }
  }
  /** flat band x0..x1 with soft edges */
  band(c, x0, x1, rgb, k, soft = 0.02) {
    const n = this.n, lo = Math.max(0, Math.floor((x0 - soft) * n)), hi = Math.min(n - 1, Math.ceil((x1 + soft) * n));
    for (let i = lo; i <= hi; i++) {
      const x = (i + 0.5) / n, e = Math.min(smooth((x - x0 + soft) / (2 * soft)), smooth((x1 + soft - x) / (2 * soft)));
      this.add(c, i, rgb, k * e);
    }
  }
  dimAll(k) { for (let c = 0; c < COLUMNS; c++) { const d = this.dimBuf[c]; for (let i = 0; i < this.n; i++) d[i] = Math.min(d[i], k); } }

  renderFx(t) {
    this.clearFx();
    for (const f of this.fx) {
      const a = t - f.t0; if (a < 0 || a > f.dur) continue;
      this.gainK = f.tier === 'open' || f.tier === 'shout' ? 1 : this.intensity;
      const fn = this['fx_' + f.kind]; if (fn) fn.call(this, f, a);
    }
  }

  // -- heavies (power >= 75) ---------------------------------------------------------
  fx_thunder(f, a) {
    // Lightning: bolts strike whole columns one after another; jagged segments, blue-white cores.
    this.dimAll(0.3 + 0.7 * smooth(a / f.dur));
    const glow = 0.1 * Math.exp(-a / 0.45);
    for (let c = 0; c < COLUMNS; c++) this.band(c, 0, 1, hsv(268, 0.9, 1), glow, 0.01);
    for (const s of f.strikes) {
      const as = a - s.at; if (as < 0) continue;
      const env = as < 0.05 ? 1 : Math.exp(-(as - 0.05) / 0.06);
      if (env < 0.01) continue;
      // flicker: one re-strike 30 ms in
      const fl = as > 0.025 && as < 0.04 ? 0.35 : 1;
      for (let i = 0; i < this.n; i++) {
        const x = (i + 0.5) / this.n, blk = Math.floor(x * 14);
        const h = hash(s.seed, blk), gap = hash(s.seed + 7, blk) < 0.18 ? 0.15 : 1;
        const core = h > 0.6;
        this.add(s.c, i, hsv(214 + 50 * hash(s.seed, blk + 99), core ? 0.35 : 0.85, 1), 0.95 * env * fl * gap * (0.4 + 0.6 * h));
      }
      if (s.c > 0 && s.c < 3 && as < 0.1) this.band(s.c === 1 ? 0 : 3, f.x0 - 0.1, f.x0 + 0.1, hsv(240, 0.6, 1), 0.35 * env); // arc spills to the outer column
    }
    this.crown(f, a);
  }

  fx_gold(f, a) {
    // Gold spark fountain from the impact height: sparks rise, slow, fall back and fade on the floor.
    this.dimAll(0.55 + 0.45 * smooth(a / f.dur));
    if (a < 0.16) for (let c = 0; c < COLUMNS; c++) this.blob(c, f.x0, 0.12, hsv(44, 0.55, 1), 0.85 * Math.exp(-a / 0.06));
    const G = 2.3;
    for (const q of f.parts) {
      if (a > q.life) continue;
      const x = f.x0 + q.v * a - 0.5 * G * a * a;
      const life = 1 - a / q.life;
      const hit = x <= 0.02;
      const xc = Math.max(0.015, x);
      const vel = q.v - G * a;
      const k = (hit ? 0.45 : 0.85) * life;
      this.blob(q.c, xc, 0.022, hsv(q.hue, 0.45 + 0.45 * (1 - life), 1), k);
      if (!hit) this.blob(q.c, xc - Math.sign(vel) * 0.04, 0.03, hsv(q.hue - 12, 0.9, 1), 0.35 * k); // trail
    }
    this.crown(f, a);
  }

  fx_shatter(f, a) {
    // Crimson flash, then each column breaks into shards that fall away with dark cracks between.
    this.dimAll(a < 0.5 ? 0.12 : 0.12 + 0.88 * smooth((a - 0.5) / 0.6));
    if (a < 0.09) { for (let c = 0; c < COLUMNS; c++) this.band(c, 0, 1, hsv(352, 0.95, 1), 0.85, 0.01); return; }
    const as = a - 0.09, gap = 0.012 + 0.03 * Math.min(1, as * 4);
    for (const s of f.shards) {
      const env = Math.exp(-as / (s.fade + 0.15 * (f.esc || 0)));
      const dx = s.v * as - (s.v > 0 ? 0 : 0.9 * as * as) ;
      const x0 = s.a + gap + dx, x1 = s.b - gap + dx;
      if (x1 <= 0 || x0 >= 1 || env < 0.02) continue;
      this.band(s.c, x0, x1, hsv(s.hue, 0.95, 1), 0.8 * env, 0.006);
      this.blob(s.c, x1, 0.012, hsv(s.hue + 15, 0.4, 1), 0.5 * env); // hot edge
    }
    this.crown(f, a);
  }

  fx_strobe(f, a) {
    // Colour strobe pops alternating inner pair / outer pair, black between pops.
    const P = 0.13, hues = [300, 48, 355, 265, 20];
    const k = Math.floor(a / P), ph = a - k * P;
    if (k < f.pops) {
      this.dimAll(0.1);
      if (ph < 0.07) for (const c of (k % 2 ? [0, 3] : [1, 2])) this.band(c, 0, 1, hsv(hues[k % hues.length], 0.7, 1), 0.92, 0.01);
    } else {
      const ag = a - f.pops * P;
      this.dimAll(0.1 + 0.9 * smooth(ag / 0.4));
      for (let c = 0; c < COLUMNS; c++) this.band(c, 0, 0.7, hsv(hues[(f.pops - 1) % hues.length], 0.9, 1), 0.28 * Math.exp(-ag / 0.15));
    }
  }

  fx_split(f, a) {
    // Fronts split up and down from the impact height: inner columns first, outer ones 0.1 s later,
    // then a dimmer echo.
    this.dimAll(0.45 + 0.55 * smooth(a / f.dur));
    const sp = 1.5 * (1 + 0.4 * (f.esc || 0));
    for (let c = 0; c < COLUMNS; c++) {
      for (const [d, k] of [[c === 1 || c === 2 ? 0 : 0.1, 1], [(c === 1 || c === 2 ? 0 : 0.1) + 0.28, 0.45]]) {
        const ac = a - d; if (ac < 0) continue;
        const up = f.x0 + sp * ac, dn = f.x0 - sp * ac, env = Math.exp(-ac / 0.4) * k;
        if (up < 1.05) this.blob(c, up, 0.045, hsv(f.hue, 0.3, 1), env);
        if (dn > -0.05) this.blob(c, dn, 0.045, hsv(f.hue, 0.3, 1), env);
        this.band(c, Math.max(0, dn), Math.min(1, up), hsv(f.hue + 15, 1, 1), 0.5 * env * Math.exp(-ac / 0.3));
      }
    }
    this.crown(f, a);
  }

  fx_nova(f, a) {
    // Blackout beat (everything off, one ember holds its breath), then a rainbow-fringed explosion.
    const B = 0.2 + 0.06 * (f.esc || 0);
    if (a < B) {
      this.dimAll(0);
      for (const c of [1, 2]) this.blob(c, f.x0, 0.025, hsv(20, 0.8, 1), 0.25 * (0.5 + 0.5 * Math.sin(a * 60)));
      return;
    }
    const ab = a - B;
    this.dimAll(0.25 + 0.75 * smooth(ab / 0.9));
    for (let c = 0; c < COLUMNS; c++) {
      this.band(c, 0, 1, hsv(30, 0.45, 1), 0.75 * Math.exp(-ab / 0.07), 0.01);
      const r = 2.1 * ab, env = Math.exp(-ab / 0.5);
      for (let i = 0; i < this.n; i++) {
        const x = (i + 0.5) / this.n, dist = r - Math.abs(x - f.x0); // >0 inside the blast
        if (dist < -0.04) continue;
        if (dist < 0.04) { this.add(c, i, hsv(45, 0.25, 1), 0.95 * env * (1 - Math.abs(dist) / 0.04)); continue; }
        const hue = 30 - 160 * Math.min(1, dist / 0.55); // orange -> red -> magenta -> violet behind the front
        this.add(c, i, hsv(hue, 0.95, 1), 0.55 * env * Math.max(0, 1 - dist * 1.1));
      }
    }
    this.crown(f, a);
  }

  /** Escalation: deep in a combo the heavy leaves its colour hanging on the column tops. */
  crown(f, a) {
    const e = f.esc || 0; if (e < 0.45) return;
    const k = (e - 0.35) * 0.55 * smooth(a / 0.15) * (1 - smooth((a - f.dur * 0.6) / (f.dur * 0.4)));
    for (let c = 0; c < COLUMNS; c++) this.band(c, 0.84, 1, hsv(f.hue, 0.85, 1), k * (0.7 + 0.3 * Math.sin(a * 22 + c)));
  }

  // -- hooks (45..74): mid-size, direction-aware ---------------------------------------
  fx_sweep(f, a) {
    const order = f.dir > 0 ? [0, 1, 2, 3] : [3, 2, 1, 0];
    order.forEach((c, j) => {
      const ac = a - j * 0.055; if (ac < 0) return;
      const env = (ac < 0.02 ? ac / 0.02 : Math.exp(-(ac - 0.02) / 0.11));
      this.band(c, f.x0 - 0.17, f.x0 + 0.17, hsv(f.hue, 0.85, 1), 0.62 * env, 0.06);
    });
  }
  fx_ring(f, a) {
    for (let c = 0; c < COLUMNS; c++) {
      const ac = a - (c === 1 || c === 2 ? 0 : 0.05); if (ac < 0) continue;
      const r = 1.2 * ac, env = 0.65 * Math.exp(-ac / 0.2);
      this.blob(c, f.x0 + r, 0.03, hsv(f.hue, 0.7, 1), env);
      this.blob(c, f.x0 - r, 0.03, hsv(f.hue, 0.7, 1), env);
    }
  }
  fx_double(f, a) {
    for (const [at, cs, dh] of [[0, [1, 2], 0], [0.14, [0, 3], 40]]) {
      const ac = a - at; if (ac < 0) continue;
      const env = ac < 0.04 ? 1 : Math.exp(-(ac - 0.04) / 0.06);
      for (const c of cs) this.band(c, f.x0 - 0.13, f.x0 + 0.13, hsv(f.hue + dh, 0.85, 1), 0.6 * env, 0.04);
    }
  }

  // -- jabs (< 45): small, quick accents on one column ---------------------------------
  fx_tick(f, a) { this.blob(f.col, f.x0, 0.045, hsv(f.hue, 0.8, 1), 0.85 * Math.exp(-a / 0.1)); }
  fx_flick(f, a) {
    const x = f.x0 + 0.28 * (1 - Math.exp(-a / 0.08)), k = 0.7 * (1 - a / f.dur);
    this.blob(f.col, x, 0.03, hsv(f.hue, 0.6, 1), k * 1.2);
    this.blob(f.col, x - 0.05, 0.03, hsv(f.hue, 0.95, 1), 0.3 * k);
  }
  fx_twin(f, a) {
    const env = 0.75 * Math.exp(-a / 0.09);
    this.blob(1, f.x0, 0.038, hsv(f.hue, 0.8, 1), env);
    this.blob(2, f.x0 + 0.04, 0.038, hsv(f.hue + 30, 0.8, 1), env);
  }

  // -- shout in FIGHT: a teal sound wave, visual only ------------------------------------
  fx_shout(f, a) {
    // Three ripples expand up and down from a spot, inner columns first; louder = further, brighter.
    const k = f.k ?? 1;
    for (let c = 0; c < COLUMNS; c++) {
      const d0 = c === 1 || c === 2 ? 0 : 0.06;
      for (let j = 0; j < 3; j++) {
        const ac = a - d0 - j * 0.11; if (ac < 0) continue;
        const r = (0.55 + 0.6 * k) * ac, env = (0.7 - 0.18 * j) * k * Math.exp(-ac / (0.25 + 0.2 * k));
        const col = hsv(172 + 12 * j, 0.55 - 0.12 * j, 1);
        this.blob(c, f.x0 + r, 0.022, col, env);
        this.blob(c, f.x0 - r, 0.022, col, env);
      }
      if (a < 0.1) this.blob(c, f.x0, 0.05, hsv(185, 0.25, 1), 0.5 * k * (1 - a / 0.1));
    }
  }

  // -- full bar -> FIGHT ------------------------------------------------------------
  fx_open(f, a) {
    const A = 0.35; // impact lands here (sfx.arenaOpen hits at the same moment)
    if (a < A) {
      const q = a / A;
      this.dimAll(0.15);
      for (let c = 0; c < COLUMNS; c++) {
        this.band(c, 0.5 * q, 1 - 0.5 * q, hsv(15 + 20 * q, 0.9, 1), 0.18 + 0.22 * q, 0.03);
        this.blob(c, 0.5 * q, 0.03, hsv(40, 0.3, 1), 0.5 + 0.4 * q);
        this.blob(c, 1 - 0.5 * q, 0.03, hsv(40, 0.3, 1), 0.5 + 0.4 * q);
      }
      return;
    }
    const ab = a - A;
    if (ab < 0.03) this.rage = Math.max(this.rage, 0.5); // the fire leaps up, then settles to a candle
    this.dimAll(0.3 + 0.7 * smooth(ab / 1.2));
    for (let c = 0; c < COLUMNS; c++) {
      this.band(c, 0, 1, hsv(38, 0.35, 1), 0.8 * Math.exp(-ab / 0.1), 0.01);
      const r = 1.8 * ab, env = Math.exp(-ab / 0.55);
      for (let i = 0; i < this.n; i++) {
        const x = (i + 0.5) / this.n, dist = r - Math.abs(x - 0.5);
        if (dist < -0.035) continue;
        if (dist < 0.035) { this.add(c, i, hsv(45, 0.3, 1), 0.9 * env); continue; }
        this.add(c, i, hsv(28 - 40 * Math.min(1, dist / 0.5), 1, 1), 0.45 * env * Math.max(0, 1 - dist * 1.4));
      }
    }
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
  /** Shockwaves travelling up the column (inner columns 1,2 first), added into out [r,g,b]. */
  waveAt(c, i, t, out) {
    const delay = (c === 1 || c === 2) ? 0 : 0.07;
    for (const w of this.waves) {
      const age = t - w.t0 - delay;
      if (age < 0) continue;
      const speed = 1.2 + w.p * 2.2; // column-heights per second
      const front = age * speed;
      const x = i / this.n;
      const width = 0.06 + w.p * 0.16;
      const d = front - x;
      if (d >= 0 && d < width * 3) {
        const v = w.p * Math.exp(-d / width) * Math.exp(-age * 1.6);
        const tc = w.tint;
        out[0] += v * (tc ? 0.5 + 0.5 * tc[0] : 1); out[1] += v * (tc ? 0.375 + 0.5 * tc[1] : 0.75); out[2] += v * (tc ? 0.225 + 0.5 * tc[2] : 0.45);
      }
    }
    return out;
  }

  // ---------------------------------------------------------------- WARMING stages
  /**
   * The energy bar changes character as it fills (each stage keeps the earlier layers):
   *  0-25 % calm: deep-blue bar, slow breathing glow above the fill line.
   * 25-50 % motion: violet, sparks fly up past the fill line, embers ripple up inside the bar.
   * 50-75 % combined: magenta-red, the fill line crackles, a bright band chases round the columns.
   * 75-100 % tension: red -> orange -> near-white top, flicker, strobe hints, lightning arcs
   *          between neighbouring columns, everything faster.
   * Each shout sends a kick up the bar sized by its intensity; a new stage flares the bar.
   */
  renderWarming(t, g, dt) {
    const n = this.n, e = clamp01(g.energyPct ?? 0), lvl = clamp01(g.level ?? 0), R = Math.random;
    const stage = e < 0.25 ? 0 : e < 0.5 ? 1 : e < 0.75 ? 2 : 3;
    if (stage > this.wStage) { this.stageKick = 1; this.shoutWaves.push({ t0: t, k: 1.1 }); }
    this.wStage = stage;
    const hueTop = e < 0.75 ? lerp(228, 345, e / 0.75) : lerp(345, 392, (e - 0.75) / 0.25);
    const speed = 1 + 2.2 * e, hot = smooth((e - 0.75) / 0.25);
    // sparks rising from the fill line (stage 1+)
    const rate = [0, 2.2, 4.5, 8][stage];
    for (let c = 0; c < COLUMNS; c++) if (R() < rate * dt) this.wSparks.push({ c, x: e, v: (0.25 + 0.45 * R()) * (0.8 + e), t0: t, life: 0.45 + 0.45 * R() });
    this.wSparks = this.wSparks.filter((q) => t - q.t0 < q.life && q.x + q.v * (t - q.t0) < 1.05);
    // lightning arcs between neighbouring columns and strobe hints (stage 3)
    if (stage === 3 && t > this.nextArc) { this.wArcs.push({ c: Math.floor(R() * 3), t0: t, seed: R() * 1000 }); this.nextArc = t + 0.25 + 0.7 * R() * (1.3 - e); }
    if (stage === 3 && t > this.nextPop) { this.wPops.push({ c: Math.floor(R() * 4), t0: t }); this.nextPop = t + 0.45 + 0.6 * R(); }
    this.wArcs = this.wArcs.filter((q) => t - q.t0 < 0.16);
    this.wPops = this.wPops.filter((q) => t - q.t0 < 0.05);

    this.clearFx(); this.gainK = 1;
    for (const q of this.wSparks) {
      const a = t - q.t0, life = 1 - a / q.life;
      this.blob(q.c, q.x + q.v * a, 0.02, hsv(hueTop + 25, 0.45, 1), 0.75 * life);
    }
    for (const w of this.shoutWaves) { // shout kick: a pulse runs up the bar, then bursts at the fill line
      const a = t - w.t0, xf = a * (2.2 + e), k = w.k;
      for (let c = 0; c < COLUMNS; c++) {
        if (xf < e + 0.04) this.blob(c, xf, 0.035 + 0.02 * k, hsv(hueTop, 0.35, 1), 0.65 * k);
        else this.blob(c, e, 0.04 + 0.06 * k, hsv(hueTop + 15, 0.3, 1), 0.7 * k * Math.exp(-(a - (e + 0.04) / (2.2 + e)) / 0.12));
      }
    }
    for (const q of this.wArcs) {
      const a = t - q.t0; if (Math.floor(a * 60) % 2) continue; // flickers
      for (const c of [q.c, q.c + 1]) for (let i = 0; i < n; i++) {
        const x = (i + 0.5) / n; if (x < e - 0.35 || x > e + 0.06) continue;
        const h = hash(q.seed + c, Math.floor(x * 16) + Math.floor(a * 40));
        this.add(c, i, hsv(212, 0.35, 1), h > 0.45 ? 0.85 * h : 0);
      }
    }
    for (const q of this.wPops) this.band(q.c, Math.max(e - 0.2, 0), Math.min(1, e + 0.02), hsv(hueTop + 20, 0.2, 1), 0.7);

    const flick = Array.from({ length: COLUMNS }, () => (stage === 3 ? 0.72 + 0.28 * R() : 1));
    for (let c = 0; c < COLUMNS; c++) {
      const add = this.addBuf[c];
      const chaseD = Math.min(Math.abs(c - (t * 2.2 * speed) % 4), 4 - Math.abs(c - (t * 2.2 * speed) % 4));
      const chase = stage >= 2 ? 0.32 * Math.exp(-chaseD * chaseD * 2) : 0;
      for (let i = 0; i < n; i++) {
        const x = (i + 0.5) / n;
        let rgb;
        if (x < e - 0.012) {
          const rel = x / Math.max(e, 0.01);
          const hue = hueTop - 45 * (1 - rel);
          const sat = 1 - 0.8 * hot * rel * rel * rel; // near white at the top late on
          const breathe = 0.85 + 0.15 * Math.sin(t * 1.6 + c);
          const ripple = stage >= 1 ? 0.28 * Math.pow(Math.max(0, Math.sin(x * 26 - t * 5 * speed + c)), 3) : 0;
          const v = ((0.2 + 0.4 * rel) * breathe + ripple + chase) * flick[c] + this.shoutFlash * 0.3 * rel + this.stageKick * 0.35;
          rgb = hsv(hue, sat, Math.min(1, v));
        } else if (x < e + 0.012) { // fill line
          const p = 0.75 + 0.25 * Math.sin(t * 14);
          rgb = hsv(hueTop, 0.3, p);
        } else {
          // above the bar: breathing glow, crackle at the line (stage 2+), live mic sparkle
          const above = x - e;
          const glow = (0.03 + 0.05 * (0.5 + 0.5 * Math.sin(t * 1.2 * speed + c * 0.8))) * Math.max(0, 1 - above * 3);
          const crackle = stage >= 2 && above < 0.05 && R() < 0.3 ? 0.55 + 0.4 * R() : 0;
          const spark = R() < lvl * 0.15 ? lvl * 0.6 : 0;
          rgb = crackle ? hsv(hueTop + 30, 0.3, crackle) : hsv(hueTop + 15, 0.65, glow + spark);
        }
        this.set(c, i, Math.min(1, rgb[0] + add[i * 3]), Math.min(1, rgb[1] + add[i * 3 + 1]), Math.min(1, rgb[2] + add[i * 3 + 2]));
      }
    }
  }

  renderProposal(state, t, g, frameScale, dt = 1 / 60) {
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
      case ArenaState.Warming: this.renderWarming(t, g, dt); this.limit(); break;
      case ArenaState.Fight: {
        const left = g.fightRemaining ?? 99;
        const warn = left < 10 ? (0.5 + 0.5 * Math.sin(t * 12)) : 0;
        // No punches: a low candle flicker at the base. More/harder punches: the fire grows
        // taller and brighter until it fills the columns.
        this.renderFx(t);
        const rg = this.rage, gain = 0.1 + 0.9 * Math.pow(rg, 0.7), wv = [0, 0, 0];
        for (let c = 0; c < COLUMNS; c++) {
          const add = this.addBuf[c], dim = this.dimBuf[c];
          this.fires[c].step(lerp(160, 55, rg), lerp(25, 170, rg), frameScale);
          for (let i = 0; i < n; i++) {
            const x = i / n;
            let [r, gg, b] = samplePalClamp(HEAT, this.fires[c].heat[i] * lerp(0.55, 0.9, rg));
            r *= gain; gg *= gain; b *= gain;
            const flick = 0.5 + 0.5 * Math.sin(t * 13 + c * 1.7) * Math.sin(t * 7.3 + c);
            const candle = (0.035 + 0.025 * flick) * Math.max(0, 1 - x * 4); // bottom quarter only
            const pulse = (0.02 + 0.13 * rg) * (0.6 + 0.4 * Math.sin(t * 8.4)) * (1 - x * 0.6); // throb grows with rage
            r = Math.max(r, candle, pulse); gg = Math.max(gg, candle * 0.35);
            const w = this.waveAt(c, i, t, wv.fill(0));
            r += w[0]; gg += w[1]; b += w[2];
            const f = this.flash * 0.8; r += f; gg += f; b += f;
            const dm = dim[i]; r *= dm; gg *= dm; b *= dm;
            r += add[i * 3]; gg += add[i * 3 + 1]; b += add[i * 3 + 2];
            if (warn && x > 0.85) { r = Math.max(r, warn); gg *= 0.3; b *= 0.3; }
            this.set(c, i, Math.min(r, 1), Math.min(gg, 1), Math.min(b, 1));
          }
        }
        this.limit();
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
    WARMING: 'energy bar in 4 stages, shout kicks',
    FIGHT: 'fire + punch signatures (jab/hook/heavy) + shout waves',
    COOLDOWN: 'embers cooling to calm blue',
  },
};
