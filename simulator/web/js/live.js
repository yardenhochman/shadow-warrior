// Live hardware mode (simulator/live/bridge.py): mirror every rendered LED frame to the
// real WLED strip and take real punches from the bag. Active only when the page is
// served by the bridge with ?live=1 (the bridge's / redirects there), so the static
// GitHub Pages build never opens a socket.
import { COLUMNS } from './wled.js';

export const LIVE_REQUESTED = new URLSearchParams(location.search).has('live');

/**
 * Resample one column (Float32Array RGB 0..1, n LEDs) into `m` 8-bit LEDs by area
 * averaging (upsampling falls back to nearest), writing into out[off..].
 */
export function resampleColumn(src, n, m, out, off, reverse = false) {
  const step = n / m;
  for (let j = 0; j < m; j++) {
    const a = j * step, b = a + step;
    let r = 0, g = 0, bl = 0, w = 0;
    for (let i = Math.floor(a); i < Math.min(n, Math.ceil(b)); i++) {
      const cover = Math.min(b, i + 1) - Math.max(a, i);
      if (cover <= 0) continue;
      r += src[i * 3] * cover; g += src[i * 3 + 1] * cover; bl += src[i * 3 + 2] * cover; w += cover;
    }
    if (!w) { const i = Math.min(n - 1, Math.floor(a)); r = src[i * 3]; g = src[i * 3 + 1]; bl = src[i * 3 + 2]; w = 1; }
    const k = off + (reverse ? m - 1 - j : j) * 3;
    out[k] = Math.round(Math.min(1, Math.max(0, r / w)) * 255);
    out[k + 1] = Math.round(Math.min(1, Math.max(0, g / w)) * 255);
    out[k + 2] = Math.round(Math.min(1, Math.max(0, bl / w)) * 255);
  }
}

/**
 * Lay the four columns end to end on a strip of `count` LEDs: column 1 first, each column
 * base-first (`serpentine`: every other column runs top-down). Leftover LEDs stay dark.
 */
export function stripFrame(rig, count, serpentine, out) {
  const per = Math.floor(count / COLUMNS);
  out.fill(0);
  for (let c = 0; c < COLUMNS; c++) resampleColumn(rig.cols[c], rig.n, per, out, c * per * 3, serpentine && c % 2 === 1);
  return out;
}

export class LiveLink {
  constructor({ onPunch, onStatus, log }) {
    this.onPunch = onPunch; this.onStatus = onStatus; this.log = log;
    this.ws = null; this.status = null; this.connected = false;
    this.enabled = true; this.serpentine = false;
    this.count = 180; this.buf = new Uint8Array(this.count * 3);
    this.lastSent = 0; this.minInterval = 1000 / 35; this.sent = 0;
    this.connect();
  }

  connect() {
    const ws = new WebSocket(`${location.protocol === 'https:' ? 'wss' : 'ws'}://${location.host}/live/ws`);
    ws.binaryType = 'arraybuffer';
    this.ws = ws;
    ws.onopen = () => { this.connected = true; this.log?.('Live bridge connected: LED frames go to the real strip', 'state'); this.onStatus?.(this); };
    ws.onclose = () => {
      if (this.connected) this.log?.('Live bridge disconnected, retrying', 'ignored');
      this.connected = false; this.onStatus?.(this);
      setTimeout(() => this.connect(), 2000);
    };
    ws.onmessage = (e) => {
      let m; try { m = JSON.parse(e.data); } catch { return; }
      if (m.type === 'status') {
        const prevBag = this.status?.bag?.state;
        this.status = m;
        const leds = m.wled?.leds;
        if (leds && leds !== this.count) { this.count = leds; this.buf = new Uint8Array(leds * 3); }
        if (m.bag && m.bag.state !== prevBag && prevBag !== undefined) this.log?.(`Bag: ${m.bag.state}${m.bag.detail ? ` (${m.bag.detail})` : ''}`, 'presence');
        this.onStatus?.(this);
      } else if (m.type === 'punch') {
        this.onPunch?.(m);
      }
    };
  }

  /** Make this tab the one that drives the strip (the newest tab does by default). */
  take() { if (this.connected) this.ws.send(JSON.stringify({ type: 'take' })); }

  /** Called once per rendered frame; sends at most ~35 fps. */
  frame(rig, tms) {
    if (!this.enabled || !this.connected || this.status?.owner === false || tms - this.lastSent < this.minInterval) return;
    if (this.ws.bufferedAmount > 4 * this.buf.length) return; // socket backed up: drop, don't queue
    this.lastSent = tms;
    this.ws.send(stripFrame(rig, this.count, this.serpentine, this.buf));
    this.sent++;
  }
}
