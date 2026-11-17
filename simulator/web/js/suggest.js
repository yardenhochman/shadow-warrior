// Suggested FIGHT sections, learned from the picks already made on music.html.
//
// Each track gets a "section change" curve: 7 frequency bands every 0.25 s, compared over the
// 6 s before and after each moment (drums or bass coming in, a riff starting, a drop). The
// candidate score adds a bonus for a rise in level and for the next 24 s being loud, and is
// weighted by how close it is to where the picks usually start (median of the picks so far).
// Fitted on Yarden's first 8 picks (18.11), each hidden in turn: the first suggestion is within
// 3 s of his start on 5 of them, one of the three on 7.
const SR = 8000, HOP = 0.25, N = 2048, W = Math.round(6 / HOP);
const EDGES = [0, 60, 150, 300, 600, 1200, 2400, 4000];
const SD = 10, W_UP = 2, W_LOUD = 1, LEAD = 0.3;   // LEAD: picks sit just before the change
export const DECODE_RATE = SR;

function fft(re, im) {
  const n = re.length;
  for (let i = 1, j = 0; i < n; i++) {
    let b = n >> 1;
    for (; j & b; b >>= 1) j ^= b;
    j ^= b;
    if (i < j) { [re[i], re[j]] = [re[j], re[i]]; [im[i], im[j]] = [im[j], im[i]]; }
  }
  for (let len = 2; len <= n; len <<= 1) {
    const a = -2 * Math.PI / len, wr = Math.cos(a), wi = Math.sin(a);
    for (let i = 0; i < n; i += len) {
      let cr = 1, ci = 0;
      for (let k = 0; k < len / 2; k++) {
        const p = i + k, q = p + len / 2;
        const tr = re[q] * cr - im[q] * ci, ti = re[q] * ci + im[q] * cr;
        re[q] = re[p] - tr; im[q] = im[p] - ti; re[p] += tr; im[p] += ti;
        const nr = cr * wr - ci * wi; ci = cr * wi + ci * wr; cr = nr;
      }
    }
  }
}

// data: mono samples at DECODE_RATE. Returns per-frame novelty, level rise and loudness-ahead.
export function features(data) {
  const hop = Math.round(SR * HOP), n = Math.floor(data.length / hop), nb = EDGES.length - 1;
  const bandOf = new Int8Array(N / 2).fill(-1);
  for (let k = 0; k < N / 2; k++) {
    const f = (k * SR) / N;
    for (let b = 0; b < nb; b++) if (f >= EDGES[b] && f < EDGES[b + 1]) bandOf[k] = b;
  }
  const win = new Float32Array(hop).map((_, i) => 0.5 - 0.5 * Math.cos((2 * Math.PI * i) / (hop - 1)));
  const B = new Float32Array(n * nb), tot = new Float32Array(n);
  const re = new Float64Array(N), im = new Float64Array(N), e = new Float64Array(nb);
  for (let f = 0; f < n; f++) {
    re.fill(0); im.fill(0); e.fill(0);
    for (let i = 0; i < hop; i++) re[i] = data[f * hop + i] * win[i];
    fft(re, im);
    for (let k = 0; k < N / 2; k++) if (bandOf[k] >= 0) e[bandOf[k]] += re[k] * re[k] + im[k] * im[k];
    let s = 0;
    for (let b = 0; b < nb; b++) { B[f * nb + b] = 10 * Math.log10(e[b] + 1e-9); s += e[b]; }
    tot[f] = 10 * Math.log10(s + 1e-9);
  }
  // Prefix sums make the before/after window means O(1).
  const P = new Float64Array((n + 1) * nb), PT = new Float64Array(n + 1);
  for (let f = 0; f < n; f++) {
    for (let b = 0; b < nb; b++) P[(f + 1) * nb + b] = P[f * nb + b] + B[f * nb + b];
    PT[f + 1] = PT[f] + tot[f];
  }
  const med = [...tot].sort((a, b) => a - b)[n >> 1] || 0;
  const nov = new Float32Array(n), up = new Float32Array(n), loud = new Float32Array(n);
  for (let i = W; i < n - W; i++) {
    let d2 = 0, dm = 0;
    for (let b = 0; b < nb; b++) {
      const before = (P[i * nb + b] - P[(i - W) * nb + b]) / W, after = (P[(i + W) * nb + b] - P[i * nb + b]) / W;
      d2 += (after - before) ** 2; dm += after - before;
    }
    nov[i] = Math.sqrt(d2); up[i] = dm / nb;
    const j = Math.min(n, i + 4 * W); loud[i] = (PT[j] - PT[i]) / (j - i) - med;
  }
  // Quiet frames: the 2 s around them sit 8 dB under the track's median (a break or a bridge).
  const quiet = new Uint8Array(n), q = Math.round(1 / HOP);
  for (let i = q; i < n - q; i++) quiet[i] = (PT[i + q] - PT[i - q]) / (2 * q) < med - 8 ? 1 : 0;
  return { nov, up, loud, quiet, n, dur: n * HOP };
}

const median = (xs) => { const s = [...xs].sort((a, b) => a - b); return s.length ? s[s.length >> 1] : null; };

// picks: the other tracks' sections ({start, end, dur}). Returns up to 3 candidates, best first:
// [{ start, end }] with end null = play to the end of the track.
export function suggest(F, picks) {
  const mu = median(picks.map((p) => p.start)) ?? 44;
  const lens = picks.filter((p) => p.end != null).map((p) => p.end - p.start);
  const len = median(lens) ?? 130;
  let max = 0;
  for (let i = 0; i < F.n; i++) if (F.nov[i] > max) max = F.nov[i];
  const sc = new Float32Array(F.n);
  for (let i = 0; i < F.n; i++) {
    const t = i * HOP;
    if (t < 15 || t > F.dur - 60 || !max) continue;
    const s = F.nov[i] / max + (W_UP * Math.max(0, F.up[i])) / 10 + (W_LOUD * Math.max(-10, Math.min(3, F.loud[i]))) / 10;
    sc[i] = s * Math.exp(-0.5 * ((t - mu) / SD) ** 2);
  }
  // Candidates are real peaks: the top score within 4 s either side.
  const gap = Math.round(4 / HOP), peaks = [];
  for (let i = 0; i < F.n; i++) {
    if (!(sc[i] > 0)) continue;
    let top = true;
    for (let j = Math.max(0, i - gap); j <= Math.min(F.n - 1, i + gap) && top; j++) if (sc[j] > sc[i]) top = false;
    if (top) peaks.push(i);
  }
  return peaks.sort((x, y) => sc[y] - sc[x]).slice(0, 3).map((i) => {
    const start = Math.max(0, i * HOP - LEAD);
    return { start: +start.toFixed(2), end: endFor(F, start, len) };
  });
}

// Long tracks get an end at a section change near the usual pick length; short ones play out.
// A break of 6 s or more inside the section ends it there (if that still leaves 45 s).
function endFor(F, start, len) {
  const need = Math.round(6 / HOP);
  for (let i = Math.round((start + 45) / HOP), run = 0; i < F.n && i * HOP < start + len + 15; i++) {
    run = F.quiet[i] ? run + 1 : 0;
    if (run === need) return +((i - need + 1) * HOP).toFixed(2);
  }
  if (F.dur - start <= len + 60) return null;
  const lo = Math.round((start + len - 15) / HOP), hi = Math.round((start + len + 15) / HOP);
  let bi = lo;
  for (let i = lo; i <= hi && i < F.n; i++) if (F.nov[i] > F.nov[bi]) bi = i;
  return +(bi * HOP).toFixed(2);
}
