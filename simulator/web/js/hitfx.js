// Punch "signatures": every punch gets a named effect for its tier, picked with variety
// (never the same one twice in a row per tier). The LEDs (wled.js), the arena (scene.js)
// and the sound (sfx.js) all read the same signature, so one hit looks and sounds as one.
// Tiers follow the punch buttons: jab < 45 <= hook < 75 <= heavy (Jab 25, Hook 55, Heavy 90).

export const HEAVY_AT = 75, HOOK_AT = 45;

/** kind -> { hue (deg), label (big floater for heavies), weight (pick chance; heavies scale with escalation) } */
export const SIGNATURES = {
  heavy: {
    thunder: { hue: 228, label: 'THUNDER', big: true },   // electric blue/violet lightning strikes, column by column
    gold: { hue: 44, label: 'GOLD RUSH' },                  // gold spark fountain that rises and falls back
    shatter: { hue: 350, label: 'SHATTER', big: true },     // crimson flash, then the columns break into falling shards
    strobe: { hue: 300, label: 'STROBE' },                  // colour strobe pops, inner pair / outer pair alternating
    split: { hue: 100, label: 'SPLIT' },                    // acid-green fronts split up and down, inner columns first
    nova: { hue: 28, label: 'SUPERNOVA', big: true },       // blackout beat, then a rainbow-fringed explosion
  },
  hook: {
    sweep: {},  // band sweeps across the columns in the hook's direction
    ring: {},   // coloured ring expands up and down from the impact height
    double: {}, // two-beat pop: inner columns, then outer
  },
  jab: {
    tick: {},  // short coloured spark on one column
    flick: {}, // a dot flicks up one column
    twin: {},  // two tiny sparks on the inner pair
  },
};
// Rotating accent hues for jabs and hooks (magenta, gold, orange, violet, lime, red).
// Teal/aqua (150..200) is kept free for shouts, so a shout never looks like a punch.
const ACCENTS = [305, 48, 18, 268, 100, 355];
export const SHOUT_HUE = 172;

export const tierOf = (power) => (power >= HEAVY_AT ? 'heavy' : power >= HOOK_AT ? 'hook' : 'jab');

export class HitPicker {
  constructor(rand = Math.random) { this.rand = rand; this.last = {}; this.accent = 0; this.jabCol = 1; }

  /**
   * @param {number} power 1..100
   * @param {object} o { combo, rage (0..1), dir (+1 hit from the left), variety (false = classic) }
   */
  pick(power, { combo = 1, rage = 0, dir = 1, variety = true } = {}) {
    const tier = tierOf(power);
    const esc = Math.min(1, Math.max(0, 0.6 * Math.min(1, (combo - 1) / 5) + 0.4 * rage));
    if (!variety) return { tier, kind: 'classic', hue: 35, esc, dir, label: null };
    const kinds = Object.keys(SIGNATURES[tier]).filter((k) => k !== this.last[tier]);
    // Escalation: deep into a combo / high rage the biggest heavies come up more often.
    const w = kinds.map((k) => (SIGNATURES[tier][k].big ? 1 + 2 * esc : 1));
    let r = this.rand() * w.reduce((a, b) => a + b, 0), kind = kinds[kinds.length - 1];
    for (let i = 0; i < kinds.length; i++) { r -= w[i]; if (r < 0) { kind = kinds[i]; break; } }
    this.last[tier] = kind;
    let hue = SIGNATURES[tier][kind].hue ?? 0;
    if (tier !== 'heavy') {
      // Jab/hook colours rotate through the accents, never the same twice in a row.
      this.accent = (this.accent + 1 + Math.floor(this.rand() * (ACCENTS.length - 1))) % ACCENTS.length;
      hue = ACCENTS[this.accent];
    }
    let col = null;
    if (tier === 'jab') { col = (this.jabCol + 1 + Math.floor(this.rand() * 3)) % 4; this.jabCol = col; }
    return { tier, kind, hue: ((hue % 360) + 360) % 360, esc, dir, col, label: SIGNATURES[tier][kind].label || null };
  }
}

/** hsv (deg, 0..1, 0..1) -> [r, g, b] 0..1 */
export function hsv(h, s, v) {
  h = ((h % 360) + 360) % 360;
  const f = (n) => { const k = (n + h / 60) % 6; return v - v * s * Math.max(Math.min(k, 4 - k, 1), 0); };
  return [f(5), f(3), f(1)];
}
export const css = (h, s = 0.85, v = 1, a = 1) => { const [r, g, b] = hsv(h, s, v); return `rgba(${r * 255 | 0},${g * 255 | 0},${b * 255 | 0},${a})`; };
