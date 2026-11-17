// Headless check of the Sound Lab: page errors, offline render + loudness of every sound,
// recorder with Chromium's FAKE microphone only, screenshots at desktop and phone width.
// Audio output is muted and pointed at no device; no window opens.
//   (repo root) python3 -m http.server 8817 --bind 127.0.0.1
//   PLAYWRIGHT_CORE=<path> CHROMIUM=/usr/bin/chromium node simulator/web/sound-lab/tools/check.mjs
import { createRequire } from 'module';
import { mkdirSync, writeFileSync } from 'fs';
const require = createRequire(import.meta.url);
const { chromium } = require(process.env.PLAYWRIGHT_CORE || 'playwright-core');
const URL = process.env.LAB_URL || 'http://127.0.0.1:8817/simulator/web/sound-lab/';
const OUT = process.env.SHOTS || 'lab-shots'; mkdirSync(OUT, { recursive: true });
const env = { ...process.env, PULSE_SERVER: 'unix:/nonexistent', PIPEWIRE_REMOTE: 'none' }; delete env.DISPLAY; delete env.WAYLAND_DISPLAY;
const browser = await chromium.launch({ executablePath: process.env.CHROMIUM || undefined, headless: true, env,
  args: ['--mute-audio', '--disable-audio-output', '--use-fake-device-for-media-stream', '--use-fake-ui-for-media-stream', '--autoplay-policy=no-user-gesture-required', '--ozone-platform=headless'] });
const problems = []; let rows = null;
for (const [w, hgt, tag] of [[1280, 900, 'desktop'], [390, 844, 'phone']]) {
  const ctx = await browser.newContext({ viewport: { width: w, height: hgt }, deviceScaleFactor: tag === 'phone' ? 2 : 1, permissions: ['microphone'], hasTouch: tag === 'phone' });
  const page = await ctx.newPage();
  page.on('console', (m) => { if (m.type() === 'error' || m.type() === 'warning') problems.push(`[${tag}] console.${m.type()}: ${m.text()}`); });
  page.on('pageerror', (e) => problems.push(`[${tag}] pageerror: ${e.message}`));
  page.on('requestfailed', (r) => problems.push(`[${tag}] requestfailed: ${r.url()}`));
  await page.goto(URL); await page.waitForFunction(() => window.lab && window.lab.ready, null, { timeout: 20000 });
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
  if (overflow > 1) problems.push(`[${tag}] horizontal overflow ${overflow}px`);
  if (tag === 'desktop') {
    const t0 = Date.now();
    rows = await page.evaluate(() => window.lab.measureAll());
    console.log(`measured ${rows.length} sounds in ${((Date.now() - t0) / 1000).toFixed(1)} s`);
    // recorder: fake mic, 2 s take, then demon-process it with each preset
    const rec = page.locator('.tline').first().locator('button.rec');
    await rec.click(); await page.waitForTimeout(2200); await rec.click();
    await page.waitForSelector('.tline .take', { timeout: 5000 });
    for (const p of ['butler', 'abyss', 'imp']) {
      await page.selectOption('#preset', p);
      await page.locator('.tline .take').first().locator('button.play.hot').click();
      await page.waitForFunction(() => document.querySelector('.tline .take button.play.hot.playing'), null, { timeout: 20000 });
      await page.locator('#stop').click();
    }
    const dl = page.waitForEvent('download', { timeout: 20000 });
    await page.getByText('Download all chosen (zip)').click();
    const d = await dl; console.log('zip download:', d.suggestedFilename());
    // play the two demos through the real buttons (muted output)
    for (const b of await page.locator('.demo button.play').all()) { await b.click(); await page.waitForTimeout(300); }
    await page.waitForFunction(() => [...document.querySelectorAll('.demo .status')].every((s) => /\d+ s/.test(s.textContent)), null, { timeout: 60000 });
    await page.locator('#stop').click();
  }
  // scroll through so every spectrogram draws, then screenshot
  await page.evaluate(async () => { for (let y = 0; y < document.body.scrollHeight; y += 500) { window.scrollTo({ top: y, behavior: 'instant' }); await new Promise((r) => setTimeout(r, 120)); } window.scrollTo({ top: 0, behavior: 'instant' }); });
  try { await page.waitForFunction(() => [...document.querySelectorAll('canvas.spec')].every((c) => c.dataset.drawn), null, { timeout: 90000 }); }
  catch { problems.push(`[${tag}] spectrograms not drawn ${JSON.stringify(await page.evaluate(() => window.lab.specState()))}: ` + await page.evaluate(() => [...document.querySelectorAll('canvas.spec')].filter((c) => !c.dataset.drawn).map((c) => c.closest('article')?.querySelector('h3')?.textContent).join(', '))); }
  await page.screenshot({ path: `${OUT}/${tag}-top.png` });
  await page.screenshot({ path: `${OUT}/${tag}-full.png`, fullPage: true });
  await ctx.close();
}
await browser.close();
writeFileSync(`${OUT}/loudness.json`, JSON.stringify(rows, null, 1));
const over = rows.filter((r) => r.tp > -1 || r.peak > 0);
console.log(`highest true peak: ${Math.max(...rows.map((r) => r.tp)).toFixed(2)} dBTP; rows over -1 dBTP: ${over.length}`);
over.forEach((r) => console.log('  OVER', r.section, r.name, r.tp.toFixed(2)));
if (problems.length) { console.log('PROBLEMS:\n' + problems.join('\n')); process.exit(1); }
console.log('OK');
