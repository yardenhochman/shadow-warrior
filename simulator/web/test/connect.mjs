// Headless check of the static (GitHub Pages) build: it still runs offline with no device,
// and "Connect to device" navigates to http://<addr>:<port>/ (remembered; ?device= pre-fills).
// Serve the repo root first:
//   python3 -m http.server 8765 --bind 127.0.0.1
//   PLAYWRIGHT_CORE=<path to playwright-core> CHROMIUM=/usr/bin/chromium node simulator/web/test/connect.mjs
// Env: SIM_URL (default http://127.0.0.1:8765/simulator/web/index.html), SHOTS. Exits 1 on any problem.
import { createRequire } from 'module';
import { mkdirSync } from 'fs';
const require = createRequire(import.meta.url);
const { chromium } = require(process.env.PLAYWRIGHT_CORE || 'playwright-core');

const URL = process.env.SIM_URL || 'http://127.0.0.1:8765/simulator/web/index.html';
const OUT = process.env.SHOTS || 'sim-shots'; mkdirSync(OUT, { recursive: true });
const env = { ...process.env }; delete env.DISPLAY; delete env.WAYLAND_DISPLAY;
const browser = await chromium.launch({ executablePath: process.env.CHROMIUM || undefined, headless: true, env, args: ['--ozone-platform=headless'] });
const problems = [], results = [];
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

try {
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 900 } });
  // Stand-in devices: any http://*.test / *.lan host answers with a marker page.
  const hits = [];
  await ctx.route(/^http:\/\/[^/]+\.(test|lan)(:\d+)?\//, (route) => { hits.push(route.request().url()); route.fulfill({ contentType: 'text/html', body: '<title>device</title>device page' }); });
  const page = await ctx.newPage();
  page.on('pageerror', (e) => problems.push(`pageerror: ${e.message}`));
  page.on('console', (m) => { if (m.type() === 'error' && !/404/.test(m.text())) problems.push(`console.error: ${m.text()}`); });

  // 1. Offline sim, no device: no socket, no live panel, the JS engine runs the game.
  await page.goto(URL);
  await page.waitForFunction(() => window.sim?.state === 'IDLE', null, { timeout: 5000 });
  if (await page.evaluate(() => window.sim.live !== null || window.sim.brain !== null)) problems.push('live/brain link opened in the static build');
  if (!(await page.isHidden('#livePanel'))) problems.push('live panel visible');
  await page.click('label:has(#presenceToggle)');
  await page.waitForFunction(() => window.sim.state === 'WARMING', null, { timeout: 8000 }).catch(() => problems.push('offline sim: presence did not reach WARMING'));
  results.push(`offline sim: ${await page.evaluate(() => window.sim.state)} after presence`);

  // 2. Connect panel defaults.
  if (await page.isHidden('#connectBtn')) problems.push('Connect button hidden');
  await page.click('#connectBtn');
  if (await page.isHidden('#connectPanel')) problems.push('connect panel did not open');
  const def = await page.evaluate(() => [document.getElementById('connectAddr').value, document.getElementById('connectPort').value, document.getElementById('connectUrl').textContent]);
  if (def.join('|') !== 'shadow-warrior.local|8090|http://shadow-warrior.local:8090/') problems.push(`defaults: ${def}`);
  else results.push(`defaults: ${def[2]}`);
  await page.screenshot({ path: `${OUT}/connect-panel.png` });

  const unit = await page.evaluate(async () => {
    const { deviceUrl } = await import('./js/connect.js');
    return [deviceUrl('pi.local', 8090), deviceUrl(' http://pi.local:9000/x ', 8090), deviceUrl('', ''), deviceUrl('10.0.0.5', '8091')];
  });
  const want = ['http://pi.local:8090/', 'http://pi.local:9000/', 'http://shadow-warrior.local:8090/', 'http://10.0.0.5:8091/'];
  if (unit.join() !== want.join()) problems.push(`deviceUrl: ${unit}`);

  // 3. Connect navigates the tab to the device.
  await page.fill('#connectAddr', 'pi.test');
  await page.fill('#connectPort', '9000');
  await Promise.all([page.waitForURL('http://pi.test:9000/', { timeout: 5000 }), page.click('#connectForm button[type=submit]')]);
  results.push(`connect -> ${page.url()}`);

  // 4. Remembered on the next visit.
  await page.goto(URL);
  await page.waitForFunction(() => window.sim?.state, null, { timeout: 5000 });
  const remembered = await page.evaluate(() => [document.getElementById('connectAddr').value, document.getElementById('connectPort').value]);
  if (remembered.join(':') !== 'pi.test:9000') problems.push(`not remembered: ${remembered}`);
  else results.push(`remembered: ${remembered.join(':')}`);

  // 5. ?device= deep link pre-fills and opens the panel.
  await page.goto(`${URL}?device=box.lan:8123`);
  await page.waitForFunction(() => window.sim?.state, null, { timeout: 5000 });
  if (await page.isHidden('#connectPanel')) problems.push('?device= did not open the panel');
  const deep = await page.textContent('#connectUrl');
  if (deep !== 'http://box.lan:8123/') problems.push(`?device= url: ${deep}`);
  await Promise.all([page.waitForURL('http://box.lan:8123/', { timeout: 5000 }), page.click('#connectForm button[type=submit]')]);
  results.push(`?device=box.lan:8123 -> ${page.url()}`);

  // Phone width: no horizontal scroll with the panel open.
  await page.goto(`${URL}?device=pi.test`);
  await page.setViewportSize({ width: 390, height: 844 });
  await sleep(300);
  const sw = await page.evaluate(() => document.documentElement.scrollWidth);
  if (sw > 390) problems.push(`phone: page is ${sw}px wide`);
  await page.screenshot({ path: `${OUT}/connect-phone.png` });
  if (hits.length < 2) problems.push(`device stand-in hit ${hits.length} times`);
} catch (e) {
  problems.push(`exception: ${e.stack || e}`);
} finally {
  await browser.close();
}
for (const r of results) console.log(r);
if (problems.length) { console.log(`PROBLEMS (${problems.length}):\n${problems.join('\n')}`); process.exit(1); }
console.log('connect ok');
