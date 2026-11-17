// Headless end-to-end run of the simulator: fake mic, presence, shouting to
// FIGHT, fight music, punches, time-up to COOLDOWN, re-arm, YAML export, at
// desktop and phone sizes. Serve the repo root first:
//   python3 -m http.server 8765 --bind 127.0.0.1
//   PLAYWRIGHT_CORE=<path to playwright-core> CHROMIUM=/usr/bin/chromium node simulator/web/test/e2e.mjs
// Exits 1 on any problem. Never opens a window (DISPLAY/WAYLAND_DISPLAY unset).
import { createRequire } from 'module';
import { mkdirSync } from 'fs';
const require = createRequire(import.meta.url);
const { chromium } = require(process.env.PLAYWRIGHT_CORE || 'playwright-core');

const URL = process.env.SIM_URL || 'http://127.0.0.1:8765/simulator/web/index.html';
const OUT = process.env.SHOTS || 'sim-shots'; mkdirSync(OUT, { recursive: true });
const env = { ...process.env }; delete env.DISPLAY; delete env.WAYLAND_DISPLAY;
const browser = await chromium.launch({
  executablePath: process.env.CHROMIUM || undefined, headless: true, env,
  args: ['--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream', '--autoplay-policy=no-user-gesture-required', '--ozone-platform=headless'],
});
const problems = [];
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const results = [];

for (const [w, h, tag] of [[1280, 800, 'desktop'], [390, 844, 'mobile']]) {
  const ctx = await browser.newContext({ viewport: { width: w, height: h }, deviceScaleFactor: tag === 'mobile' ? 2 : 1, permissions: ['microphone'], hasTouch: tag === 'mobile' });
  const page = await ctx.newPage();
  page.on('console', (m) => { if (m.type() === 'error' || m.type() === 'warning') problems.push(`[${tag}] console.${m.type()}: ${m.text()}`); });
  page.on('pageerror', (e) => problems.push(`[${tag}] pageerror: ${e.message}`));
  await page.goto(URL);
  await page.waitForFunction(() => window.sim && window.sim.state === 'IDLE');
  const st = () => page.evaluate(() => window.sim.state);
  const waitState = async (s, ms = 15000) => { await page.waitForFunction((x) => window.sim.state === x, s, { timeout: ms }); };
  await sleep(800);
  await page.screenshot({ path: `${OUT}/${tag}-1-idle.png` });

  await page.click('#micBtn');
  await page.waitForFunction(() => window.sim.lastFeat && !window.sim.lastFeat.synthetic, null, { timeout: 8000 });
  const micMax = await page.evaluate(async () => { let mx = -200; const t0 = performance.now(); while (performance.now() - t0 < 2500) { await new Promise((r) => setTimeout(r, 4)); mx = Math.max(mx, window.sim.lastFeat.band_db); } return mx; });
  if (micMax < -60) problems.push(`[${tag}] mic frames look silent (max ${micMax})`);
  results.push(`[${tag}] fake-mic max band level over 2.5s: ${micMax.toFixed(1)} dB`);

  await page.click('label.toggle.big');
  await waitState('WARMING', 5000);
  results.push(`[${tag}] presence -> WARMING ok`);
  const shoutBtn = await page.$('#shoutBtn');
  const box = await shoutBtn.boundingBox();
  const holdShout = async (ms) => { await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2); await page.mouse.down(); await sleep(ms); await page.mouse.up(); };
  while ((await page.evaluate(() => window.sim.info.energyPercentage || 0)) < 0.45) { await holdShout(700); await sleep(100); }
  for (let i = 0; i < 40 && (await st()) === 'WARMING'; i++) await holdShout(700);
  await waitState('FIGHT', 5000);
  results.push(`[${tag}] energy full -> FIGHT ok`);
  await sleep(1200);
  const music = await page.evaluate(() => { const p = window.sim.playlist; return { source: p.source, paused: p.audio.paused, t: p.audio.currentTime }; });
  results.push(`[${tag}] fight music: source ${music.source}, paused ${music.paused}, t ${music.t.toFixed(1)}s`);
  if (music.source === 'repo' && (music.paused || music.t <= 0)) problems.push(`[${tag}] repo playlist not playing in FIGHT`);

  const sc = await (await page.$('#scene')).boundingBox();
  for (let i = 0; i < 4; i++) { await page.mouse.click(sc.x + sc.width * (0.45 + 0.1 * (i % 2)), sc.y + sc.height * 0.5); await sleep(180); }
  if (tag === 'desktop') { for (const k of ['j', 'k', 'j', 'k']) { await page.keyboard.press(k); await sleep(150); } }
  else { for (const sel of ['.btn.punch[data-power="25"]', '.btn.punch[data-power="55"]']) { await page.tap(sel); await sleep(150); } }
  await page.mouse.move(sc.x + sc.width * 0.42, sc.y + sc.height * 0.5); await page.mouse.down(); await sleep(650); await page.mouse.up();
  await sleep(70);
  await page.screenshot({ path: `${OUT}/${tag}-3-fight-hit.png` });
  const fightStats = await page.evaluate(() => ({ punches: window.sim.arena.currentSessionPunches, max: window.sim.arena.currentSessionMaxPower, hp: window.sim.game.hp }));
  results.push(`[${tag}] fight punches ${fightStats.punches} max ${fightStats.max.toFixed(0)} hp ${fightStats.hp.toFixed(0)}`);
  if (fightStats.punches < 7) problems.push(`[${tag}] expected >=7 counted punches, got ${fightStats.punches}`);

  // Effect presets: base preset, per-stage override, preview buttons, now-playing readout.
  await page.click('#fxPanel > summary');
  await page.selectOption('#ledMode', 'storm');
  await page.selectOption('#fxsel_warming', 'inferno');
  const mix = await page.evaluate(() => ['warming', 'heavy', 'fight'].map((s) => window.sim.rig.presetFor(s)).join());
  if (mix !== 'inferno,storm,storm') problems.push(`[${tag}] per-stage mix: ${mix}`);
  await page.click('button[data-preview="heavy:lightning"]'); await sleep(120);
  if (!(await page.textContent('#fxNow')).includes('Storm · LIGHTNING')) problems.push(`[${tag}] now playing misses the LIGHTNING preview: ${await page.textContent('#fxNow')}`);
  await page.click('button[data-preview="warming:2"]'); await sleep(200);
  if (!(await page.textContent('#fxNow')).includes('Inferno · Heat gauge')) problems.push(`[${tag}] now playing misses the Inferno warming preview`);
  await page.click('button[data-preview="victory"]'); await sleep(200);
  const hist = await page.textContent('#fxHistory');
  if (!/VICTORY · Storm/.test(hist) || !/HEAVY · Storm · LIGHTNING/.test(hist)) problems.push(`[${tag}] history: ${hist}`);
  results.push(`[${tag}] presets: storm base, warming from inferno, previews and history ok`);
  await sleep(500);

  // KO -> victory finale -> round 2 with more HP; the frame stays under the cap.
  await page.evaluate(() => { window.sim.rig.fx = []; window.sim.rig.preview = null; window.sim.game.hp = 1; window.sim.punch(92); });
  const vic = await page.evaluate(async () => { let mx = 0; const t0 = performance.now(); while (performance.now() - t0 < 3000) { await new Promise((r) => requestAnimationFrame(r)); mx = Math.max(mx, window.sim.rig.lastLevel); } return { mx, np: document.getElementById('fxNow').textContent, vic: !!window.sim.scene.vic }; });
  if (!vic.vic || !vic.np.includes('VICTORY')) problems.push(`[${tag}] no victory after KO: ${vic.np}`);
  if (vic.mx > 0.7201) problems.push(`[${tag}] victory frame level ${vic.mx}`);
  await page.screenshot({ path: `${OUT}/${tag}-4-victory.png` });
  await page.waitForFunction(() => window.sim.game.round === 2, null, { timeout: 9000 });
  const r2 = await page.evaluate(() => ({ st: window.sim.state, maxHp: window.sim.game.maxHp, hp: window.sim.game.hp }));
  if (r2.st !== 'FIGHT' || r2.maxHp !== Math.round(900 * 1.3)) problems.push(`[${tag}] round 2: ${JSON.stringify(r2)}`);
  results.push(`[${tag}] KO -> victory (peak level ${vic.mx.toFixed(2)}) -> round 2 with ${r2.maxHp} HP`);

  await page.evaluate(() => window.sim.skip(25)); await page.keyboard.press('l'); await sleep(1100);
  await page.evaluate(() => window.sim.skip(25)); await page.keyboard.press('l'); await sleep(1100);
  await page.evaluate(() => { for (let i = 0; i < 6; i++) { window.sim.skip(25); window.sim.arena.lastActivityAt = window.sim.arena.now(); } });
  await waitState('COOLDOWN', 5000);
  results.push(`[${tag}] FIGHT -> COOLDOWN (${await page.evaluate(() => window.sim.arena.lastEndReason)})`);
  await page.evaluate(() => window.sim.skip(61));
  await page.waitForFunction(() => ['IDLE', 'WARMING'].includes(window.sim.state), null, { timeout: 5000 });
  results.push(`[${tag}] after cooldown: ${await st()}`);

  await page.evaluate(() => { const el = document.getElementById('t_fight_duration_sec'); el.value = 120; el.dispatchEvent(new Event('input')); });
  await page.click('#exportBtn');
  const yaml = await page.inputValue('#yamlOut');
  if (!yaml.includes('arena:\n  fight_duration_sec: 120')) problems.push(`[${tag}] yaml export unexpected: ${yaml}`);
  // Effect choices persist (diffs only) across a reload.
  await page.reload(); await page.waitForFunction(() => window.sim && window.sim.state);
  const kept = await page.evaluate(() => [window.sim.rig.preset, window.sim.rig.presetFor('warming'), localStorage.getItem('sw-sim-effects-v1')]);
  if (kept[0] !== 'storm' || kept[1] !== 'inferno') problems.push(`[${tag}] effects not restored: ${kept}`);
  for (const e of await page.evaluate(() => window.sim.errors)) problems.push(`[${tag}] loop error: ${e}`);
  await ctx.close();
}

await browser.close();
console.log(results.join('\n'));
console.log(problems.length ? `PROBLEMS (${problems.length}):\n${problems.join('\n')}` : 'NO PROBLEMS');
process.exit(problems.length ? 1 : 0);
