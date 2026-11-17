// Headless check of brain mode: a real brain-rs drives the page through the bridge.
//   brain-rs (wled.controllers -> 127.0.0.1:8090) + uv run simulator/live/bridge.py --brain http://127.0.0.1:8000 [--wled HOST]
//   PLAYWRIGHT_CORE=<path to playwright-core> CHROMIUM=/usr/bin/chromium node simulator/web/test/brain.mjs
// The page's presence toggle and SHOUT button drive the brain from IDLE into FIGHT; then the
// page's LED columns must equal the frames the brain sent over UDP (bridge /live/frame), a
// page punch must be counted by the brain, and the state buttons must move it to COOLDOWN.
// Env: LIVE_URL (default http://127.0.0.1:8090/), WLED_HOST (also require its /json/info
// live: true, lm UDP while the brain streams), SHOTS (screenshot dir). Exits 1 on any problem.
// Leaves the brain in IDLE with presence forced off.
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
const api = async (path, body) => (await fetch(`${BASE}brain${path}`, body ? { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) } : {})).json();
const brainState = async () => (await api('/api/state')).data;

try {
  const page = await (await browser.newContext({ viewport: { width: 1280, height: 900 } })).newPage();
  page.on('console', (m) => { if (m.type() === 'error') problems.push(`console.error: ${m.text()}`); });
  page.on('pageerror', (e) => problems.push(`pageerror: ${e.message}`));
  await page.goto(BASE); // bridge in brain mode redirects to index.html?brain=1
  if (!page.url().includes('brain=1')) problems.push(`not redirected to brain mode: ${page.url()}`);
  await page.waitForFunction(() => window.sim?.live?.connected && window.sim?.brain?.ok === true && window.sim.brain.tunables, null, { timeout: 10000 });
  for (const id of ['#detectorPanel', '#tunablesPanel']) if (!(await page.isHidden(id))) problems.push(`${id} visible in brain mode`);
  if (await page.isHidden('#brainStates')) problems.push('state buttons hidden');

  // Start from IDLE with presence off (the brain may be mid-game from an earlier run).
  await api('/api/arena/presence', { detected: false });
  let st = await brainState();
  if (st.arena.current_state !== 'Idle') {
    for (const s of ['Idle']) await api('/api/arena/state', { state: s });
    await sleep(300); st = await brainState();
  }
  if (st.arena.current_state === 'Suspended') { await api('/api/arena/state', { state: 'Idle' }); st = await brainState(); }
  results.push(`brain start state: ${st.arena.current_state}`);
  const sessions0 = st.statistics.total_sessions;

  // IDLE: WLED renders its own effect; with a strip the page shows WLED's live view.
  if (process.env.WLED_HOST) {
    await page.waitForFunction(() => window.sim.brainFrame.source === 'wled', null, { timeout: 8000 }).catch(() => problems.push('no WLED live view frames in IDLE'));
    results.push(`IDLE frames from: ${await page.evaluate(() => window.sim.brainFrame.source)}`);
  }

  // Presence toggle (page) -> brain WARMING.
  await page.click('label:has(#presenceToggle)');
  await page.waitForFunction(() => window.sim.state === 'WARMING' || window.sim.info.currentState === 'WARMING', null, { timeout: 5000 }).catch(() => {});
  st = await brainState();
  if (st.arena.current_state !== 'Warming') problems.push(`presence toggle: brain is ${st.arena.current_state}, want Warming`);
  else results.push('presence toggle -> brain WARMING');

  // Hold SHOUT (page) until the brain reaches FIGHT.
  const box = await page.locator('#shoutBtn').boundingBox();
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.down();
  const t0 = Date.now();
  while (Date.now() - t0 < 12000 && (await brainState()).arena.current_state !== 'Fight') await sleep(200);
  await page.mouse.up();
  st = await brainState();
  if (st.arena.current_state !== 'Fight') problems.push(`shouting: brain is ${st.arena.current_state} after ${(Date.now() - t0) / 1000}s, want Fight`);
  else results.push(`SHOUT held ${((Date.now() - t0) / 1000).toFixed(1)}s -> brain FIGHT`);
  await page.waitForFunction(() => window.sim.info.currentState === 'FIGHT', null, { timeout: 3000 }).catch(() => problems.push('page did not show FIGHT'));

  // FIGHT is realtime: page LED columns == the brain's UDP frames (inverse of the 4 x 45 layout).
  await page.waitForFunction(() => window.sim.brainFrame.source === 'brain', null, { timeout: 5000 }).catch(() => problems.push('no brain frames in FIGHT'));
  let compared = 0, mismatched = 0, lit = 0;
  for (let i = 0; i < 10; i++) {
    const snap = await page.evaluate(() => {
      const { rig, brainFrame } = window.sim, per = rig.n, out = [];
      for (let c = 0; c < 4; c++) for (let j = 0; j < per; j++) for (let k = 0; k < 3; k++) out.push(Math.round(rig.cols[c][j * 3 + k] * 255));
      return { seq: brainFrame.seq, source: brainFrame.source, serp: window.sim.live.serpentine, per, out };
    });
    const r = await fetch(`${BASE}live/frame?seq=${snap.seq}`);
    if (r.ok) {
      const f = await r.json();
      const want = Buffer.from(f.rgb, 'hex');
      // Rebuild the strip order from the columns (base first; serpentine reverses odd columns).
      const got = new Uint8Array(want.length);
      for (let c = 0; c < 4; c++) for (let j = 0; j < snap.per; j++) {
        const s = (c * snap.per + (snap.serp && c % 2 ? snap.per - 1 - j : j)) * 3, d = (c * snap.per + j) * 3;
        for (let k = 0; k < 3; k++) got[s + k] = snap.out[d + k];
      }
      compared++;
      if (f.source !== 'brain' || Buffer.compare(Buffer.from(got), want) !== 0) mismatched++;
      if (want.some((b) => b > 0)) lit++;
    }
    await sleep(120);
  }
  if (compared < 5 || mismatched) problems.push(`LED columns vs brain frames: ${compared} compared, ${mismatched} differ`);
  else results.push(`LED columns == brain UDP frames: ${compared}/${compared} identical (${lit} lit), ${await page.evaluate(() => window.sim.rig.n)} LEDs per column`);
  const bs = (await (await fetch(`${BASE}live/status`)).json()).brain;
  results.push(`bridge: realtime ${bs.realtime}, ${bs.udp_fps} fps from ${bs.udp_from}, ${bs.udp_packets} packets`);
  if (!bs.realtime || bs.udp_fps < 15) problems.push(`bridge realtime ${bs.realtime} at ${bs.udp_fps} fps`);

  if (process.env.WLED_HOST) {
    const info = await (await fetch(`http://${process.env.WLED_HOST}/json/info`)).json();
    results.push(`WLED ${process.env.WLED_HOST}: live ${info.live}, lm ${info.lm}, lip ${info.lip}`);
    if (info.live !== true || info.lm !== 'UDP') problems.push(`WLED not in UDP realtime: live ${info.live} lm ${info.lm}`);
  }

  // A page punch is counted by the brain and comes back as PunchDetected.
  const p0 = (await brainState()).statistics.current_session_punches;
  await page.click('.btn.punch.heavy');
  await sleep(600);
  const p1 = (await brainState()).statistics.current_session_punches;
  if (p1 !== p0 + 1) problems.push(`page punch: brain punches ${p0} -> ${p1}`);
  else results.push(`page punch counted by brain: ${p0} -> ${p1}`);
  if ((await page.evaluate(() => window.sim.game.punches)) < 1) problems.push('punch not shown on the page');

  await page.screenshot({ path: `${OUT}/brain-fight.png` });

  // State buttons: FIGHT -> COOLDOWN, then IDLE; presence off.
  await page.click('#brainStates button[data-s="COOLDOWN"]');
  await page.waitForFunction(() => window.sim.info.currentState === 'COOLDOWN', null, { timeout: 4000 }).catch(() => {});
  st = await brainState();
  if (st.arena.current_state !== 'Cooldown') problems.push(`COOL button: brain is ${st.arena.current_state}`);
  else results.push(`COOL button -> brain COOLDOWN (sessions ${sessions0} -> ${st.statistics.total_sessions})`);
  await sleep(1500);
  await page.screenshot({ path: `${OUT}/brain-cooldown.png` });
  await page.click('label:has(#presenceToggle)'); // presence off
  await page.click('#brainStates button[data-s="IDLE"]');
  await sleep(500);
  st = await brainState();
  if (st.arena.current_state !== 'Idle') problems.push(`IDLE button: brain is ${st.arena.current_state}`);
  else results.push('IDLE button -> brain IDLE');

  // Phone width: no horizontal scroll.
  await page.setViewportSize({ width: 390, height: 844 });
  await sleep(400);
  const sw = await page.evaluate(() => document.documentElement.scrollWidth);
  if (sw > 390) problems.push(`phone: page is ${sw}px wide`);
  await page.screenshot({ path: `${OUT}/brain-phone.png`, fullPage: true });
} catch (e) {
  problems.push(`exception: ${e.stack || e}`);
} finally {
  await browser.close();
}
for (const r of results) console.log(r);
if (problems.length) { console.log(`PROBLEMS (${problems.length}):\n${problems.join('\n')}`); process.exit(1); }
console.log('brain mode ok');
