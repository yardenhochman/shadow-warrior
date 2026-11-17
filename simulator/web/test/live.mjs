// Headless check of live hardware mode against a running bridge (simulator/live/bridge.py):
// the page connects, streams strip frames at ~35 fps that match the rendered LEDs, the
// bridge forwards them to WLED, and a bridge punch message lands as a counted FIGHT punch.
//   uv run simulator/live/bridge.py --port 8090 &
//   PLAYWRIGHT_CORE=<path to playwright-core> CHROMIUM=/usr/bin/chromium node simulator/web/test/live.mjs
// Env: LIVE_URL (default http://127.0.0.1:8090/), WLED_HOST (optional: also GET its
// /json/info once and require live: true), SHOTS (screenshot dir). Exits 1 on any problem.
import { createRequire } from 'module';
import { mkdirSync } from 'fs';
const require = createRequire(import.meta.url);
const { chromium } = require(process.env.PLAYWRIGHT_CORE || 'playwright-core');

const BASE = (process.env.LIVE_URL || 'http://127.0.0.1:8090/').replace(/\/?$/, '/');
const OUT = process.env.SHOTS || 'sim-shots'; mkdirSync(OUT, { recursive: true });
const env = { ...process.env }; delete env.DISPLAY; delete env.WAYLAND_DISPLAY;
const browser = await chromium.launch({ executablePath: process.env.CHROMIUM || undefined, headless: true, env, args: ['--ozone-platform=headless', '--autoplay-policy=no-user-gesture-required'] });
const problems = [], results = [];
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const status = async () => (await fetch(`${BASE}live/status`)).json();

const page = await (await browser.newContext({ viewport: { width: 1280, height: 900 } })).newPage();
page.on('console', (m) => { if (m.type() === 'error') problems.push(`console.error: ${m.text()}`); });
page.on('pageerror', (e) => problems.push(`pageerror: ${e.message}`));
await page.goto(BASE); // bridge redirects to index.html?live=1
await page.waitForFunction(() => window.sim?.live?.connected, null, { timeout: 8000 });
if (await page.isHidden('#livePanel')) problems.push('live panel hidden');

// Resampler: 90 LEDs per column -> 45, area average; serpentine reverses odd columns.
const unit = await page.evaluate(async () => {
  const { resampleColumn, stripFrame } = await import('./js/live.js');
  const src = new Float32Array(90 * 3); for (let i = 0; i < 90; i++) src[i * 3] = i < 45 ? 1 : 0;
  const out = new Uint8Array(45 * 3); resampleColumn(src, 90, 45, out, 0);
  const rig = { n: 4, cols: [0, 1, 2, 3].map((c) => Float32Array.from({ length: 12 }, (_, k) => (k % 3 === 0 ? (c + 1) / 4 * (k / 3 + 1) / 4 : 0))) };
  const s = stripFrame(rig, 8, true, new Uint8Array(24));
  return { half: [out[0], out[21 * 3], out[23 * 3], out[44 * 3]], serp: [s[0], s[3], s[6], s[9]] };
});
if (unit.half.join() !== '255,255,0,0') problems.push(`resample edge wrong: ${unit.half}`);
if (unit.serp[2] <= unit.serp[3]) problems.push(`serpentine column 2 not reversed: ${unit.serp}`);
results.push(`resampler ok: ${unit.half} serpentine ${unit.serp}`);

// Frames: count what the bridge receives over 3 s and compare one sent frame with the rig.
await page.evaluate(() => {
  const l = window.sim.live, send = l.ws.send.bind(l.ws);
  l.ws.send = (buf) => { window.__lastFrame = Array.from(buf); window.__lastCols = window.sim.rig.cols.map((c) => Array.from(c)); send(buf); };
});
await page.evaluate(() => window.sim.setSensor(true));
await page.waitForFunction(() => window.sim.state === 'WARMING', null, { timeout: 8000 });
await page.evaluate(() => window.sim.sm.transition('FIGHT'));
const s0 = await status(); await sleep(3000); const s1 = await status();
const fps = (s1.wled.frames - s0.wled.frames) / 3;
results.push(`bridge received ${fps.toFixed(1)} frames/s, streaming=${s1.wled.streaming}, WLED ${s1.wled.host} ${s1.wled.leds} LEDs`);
if (fps < 25 || fps > 45) problems.push(`frame rate ${fps.toFixed(1)} outside 25-45`);
if (!s1.wled.streaming) problems.push('bridge not streaming');
const cmp = await page.evaluate(() => {
  const f = window.__lastFrame, cols = window.__lastCols, per = f.length / 3 / 4;
  let maxErr = 0, lit = 0;
  for (let c = 0; c < 4; c++) for (let j = 0; j < per; j++) for (let k = 0; k < 3; k++) {
    const n = cols[c].length / 3, a = j * n / per, b = a + n / per; let s = 0;
    for (let i = Math.floor(a); i < Math.ceil(b); i++) s += cols[c][i * 3 + k];
    const want = Math.round(Math.min(1, s / (Math.ceil(b) - Math.floor(a))) * 255), got = f[(c * per + j) * 3 + k];
    maxErr = Math.max(maxErr, Math.abs(want - got)); if (got > 20) lit++;
  }
  return { len: f.length, maxErr, lit };
});
results.push(`sent frame ${cmp.len} bytes, max diff vs rendered LEDs ${cmp.maxErr}, ${cmp.lit} lit channels`);
if (cmp.len !== s1.wled.leds * 3) problems.push(`frame length ${cmp.len} != ${s1.wled.leds} x 3`);
if (cmp.maxErr > 1) problems.push(`sent frame differs from rendered LEDs by ${cmp.maxErr}`);
if (cmp.lit < 10) problems.push('FIGHT frame looks black');

if (process.env.WLED_HOST) {
  const info = await (await fetch(`http://${process.env.WLED_HOST}/json/info`)).json();
  results.push(`WLED /json/info: live=${info.live} lm="${info.lm}" lip=${info.lip}`);
  if (!info.live) problems.push('WLED not in realtime mode');
}

// Punch round trip through the bridge (same message a bag punch produces).
const p0 = await page.evaluate(() => window.sim.arena.currentSessionPunches);
const r = await (await fetch(`${BASE}live/punch?power=80`, { method: 'POST' })).json();
await page.waitForFunction((n) => window.sim.arena.currentSessionPunches > n, p0, { timeout: 3000 }).catch(() => {});
const p1 = await page.evaluate(() => ({ n: window.sim.arena.currentSessionPunches, max: window.sim.arena.currentSessionMaxPower }));
results.push(`bridge punch (clients ${r.clients}): session punches ${p0} -> ${p1.n}, max power ${p1.max.toFixed(0)}`);
if (p1.n !== p0 + 1 || Math.round(p1.max) !== 80) problems.push('bridge punch not counted as an 80-power FIGHT punch');
await sleep(150);
await page.screenshot({ path: `${OUT}/live-fight.png` });
for (const e of await page.evaluate(() => window.sim.errors)) problems.push(`loop error: ${e}`);
results.push(`status line: ${await page.textContent('#wledStatus')} | ${await page.textContent('#bagStatus')}`);
await browser.close();
console.log(results.join('\n'));
console.log(problems.length ? `PROBLEMS (${problems.length}):\n${problems.join('\n')}` : 'NO PROBLEMS');
process.exit(problems.length ? 1 : 0);
