// Headless check of the LED effect presets: every preset x stage is fired through the preview
// buttons' code path, its frame mean level is measured (must stay <= 0.72 in WARMING/FIGHT),
// punch hues must stay out of the shout teal band, and the victory sound must peak below 0 dBFS.
// Writes contact sheets (LED columns per preset/stage, the victory sequence on the LEDs and in the
// arena, overlays minimal vs full, the effects panel at desktop and phone widths) to SHOTS.
// Serve the repo root first:
//   python3 -m http.server 8765 --bind 127.0.0.1
//   PLAYWRIGHT_CORE=<path to playwright-core> CHROMIUM=/usr/bin/chromium node simulator/web/test/effects.mjs
// Exits 1 on any problem. Never opens a window (DISPLAY/WAYLAND_DISPLAY unset).
import { createRequire } from 'module';
import { mkdirSync, writeFileSync } from 'fs';
const require = createRequire(import.meta.url);
const { chromium } = require(process.env.PLAYWRIGHT_CORE || 'playwright-core');

const URL = process.env.SIM_URL || 'http://127.0.0.1:8765/simulator/web/index.html';
const OUT = process.env.SHOTS || 'sim-shots'; mkdirSync(OUT, { recursive: true });
const env = { ...process.env }; delete env.DISPLAY; delete env.WAYLAND_DISPLAY;
const browser = await chromium.launch({ executablePath: process.env.CHROMIUM || undefined, headless: true, env, args: ['--ozone-platform=headless', '--autoplay-policy=no-user-gesture-required'] });
const problems = [], results = [];
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const ctx = await browser.newContext({ viewport: { width: 1280, height: 900 } });
const page = await ctx.newPage();
page.on('pageerror', (e) => problems.push(`pageerror: ${e.message}`));
page.on('console', (m) => { if (m.type() === 'error' && !/404/.test(m.text())) problems.push(`console.error: ${m.text()}`); });
await page.goto(URL);
await page.waitForFunction(() => window.sim?.state === 'IDLE');
await page.click('#fxPanel > summary');

// 1. Punch hues never in the shout band (150..200), for every preset and colour shift.
const teal = await page.evaluate(async () => {
  const { PRESET_IDS, hitTable } = await import('./js/presets.js');
  const bad = [];
  for (const id of PRESET_IDS) {
    const tb = hitTable(id);
    for (const tier of ['jab', 'hook', 'heavy']) for (const kind of Object.keys(tb.kinds[tier])) for (let sh = -180; sh <= 180; sh += 5) for (let r = 0; r < 4; r++) {
      const h = window.sim.hitPicker.make(tier, kind, { table: tb, hueShift: sh }).hue;
      if (h >= 150 && h <= 200) bad.push(`${id} ${tier} ${kind} shift ${sh}: ${h}`);
    }
  }
  return bad;
});
if (teal.length) problems.push(`punch hues in the shout band: ${teal.slice(0, 5).join('; ')}`);
else results.push('punch hues: none in 150-200 deg for any preset, kind or colour shift');

// 2. Victory sound: render offline and measure the peak.
const peak = await page.evaluate(async () => {
  const { buildVictory } = await import('./js/sfx.js');
  const c = new OfflineAudioContext(2, 44100 * 7, 44100);
  buildVictory(c, c.destination, 0.05);
  const buf = await c.startRendering();
  let p = 0; for (let ch = 0; ch < 2; ch++) for (const v of buf.getChannelData(ch)) p = Math.max(p, Math.abs(v));
  return 20 * Math.log10(p);
});
results.push(`victory sound peak: ${peak.toFixed(2)} dBFS`);
if (!(peak < 0)) problems.push(`victory sound peaks at ${peak.toFixed(2)} dBFS`);

// 3. Levels and LED snapshots per preset x stage.
// The sim must draw what a real column has: one 3 m strip at 60 LEDs/m = 180 LEDs.
const ledsPerColumn = await page.evaluate(() => window.sim.rig.n);
if (ledsPerColumn !== 180) problems.push(`LEDs per column: ${ledsPerColumn}, want 180 (3 m x 60/m)`);
const presets = await page.evaluate(async () => (await import('./js/presets.js')).PRESET_IDS);
const kindsOf = (id) => page.evaluate(async (p) => { const { PRESETS } = await import('./js/presets.js'); const o = {}; for (const t of ['jab', 'hook', 'heavy']) o[t] = PRESETS[p][t].kinds ? Object.keys(PRESETS[p][t].kinds) : []; return o; }, id);
/** Fire a preview and sample every frame: mean / peak level and snapshots at the given ages. */
const run = (stage, arg, dur, snaps, setup) => page.evaluate(async ([stage, arg, dur, snaps, setup]) => {
  const { rig } = window.sim;
  rig.fx = []; rig.preview = null; rig.rage = 0;
  window.sim.previewStage(stage, arg ?? undefined);
  if (setup === 'calm') rig.rage = 0;
  const t0 = performance.now(), levels = [], out = [];
  let k = 0;
  while (performance.now() - t0 < dur * 1000) {
    await new Promise((r) => requestAnimationFrame(r));
    const a = (performance.now() - t0) / 1000;
    if (setup === 'calm') rig.rage = 0; else if (setup === 'rage') rig.rage = 0.8;
    levels.push(rig.lastLevel);
    while (k < snaps.length && a >= snaps[k]) { out.push({ a: +a.toFixed(2), cols: rig.cols.map((c) => Array.from(c, (v) => Math.round(v * 255))) }); k++; }
  }
  rig.preview = null;
  return { mean: levels.reduce((x, y) => x + y, 0) / levels.length, peak: Math.max(...levels), frames: levels.length, snaps: out };
}, [stage, arg, dur, snaps, setup]);

const sheet = {}, table = [], vicSheet = {};
for (const id of presets) {
  await page.selectOption('#ledMode', id);
  const kinds = await kindsOf(id);
  const plan = [
    ['idle', 'idle', null, 2.5, [1.5]],
    ...[0, 1, 2, 3].map((i) => [`warm ${i + 1}`, 'warming', i, 2.2, [1.4]]),
    ['open', 'open', null, 2.0, [0.2, 0.55]],
    ['fight calm', 'fight', null, 1.6, [1.2], 'calm'],
    ['fight rage', 'fight', null, 1.6, [1.2], 'rage'],
    ...kinds.jab.map((k) => [`jab ${k}`, 'jab', k, 0.6, [0.08]]),
    ...kinds.hook.map((k) => [`hook ${k}`, 'hook', k, 0.8, [0.15]]),
    ...kinds.heavy.map((k) => [`heavy ${k}`, 'heavy', k, 1.5, [0.3]]),
    ['shout', 'shout', null, 1.3, [0.3]],
  ];
  sheet[id] = [];
  for (const [label, stage, arg, dur, snaps, setup] of plan) {
    const r = await run(stage, arg, dur, snaps, setup);
    for (const s of r.snaps) sheet[id].push({ label: snaps.length > 1 ? `${label} ${s.a}s` : label, cols: s.cols });
    table.push({ preset: id, stage: label, mean: r.mean, peak: r.peak });
    if (r.peak > 0.7201) problems.push(`${id} ${label}: frame level ${r.peak.toFixed(3)} over the cap`);
  }
  const v = await run('victory', null, 7.0, [0.12, 0.6, 1.0, 1.5, 2.0, 2.35, 2.5, 2.8, 3.5, 4.5, 5.5, 6.3]);
  vicSheet[id] = v.snaps.map((s) => ({ label: `${s.a}s`, cols: s.cols }));
  table.push({ preset: id, stage: 'victory', mean: v.mean, peak: v.peak });
  if (v.peak > 0.7201) problems.push(`${id} victory: frame level ${v.peak.toFixed(3)} over the cap`);
}
await page.selectOption('#ledMode', 'proposal');
// level table (markdown) next to the sheets
const stagesOrder = [...new Set(table.map((r) => r.stage.replace(/^(jab|hook|heavy) .*/, '$1')))];
const agg = {};
for (const r of table) { const st = r.stage.replace(/^(jab|hook|heavy) .*/, '$1'); const k = `${r.preset}|${st}`; (agg[k] ||= []).push(r); }
const md = ['| stage | ' + presets.join(' | ') + ' |', '|---|' + presets.map(() => '---|').join('')];
for (const st of stagesOrder) md.push(`| ${st} | ` + presets.map((p) => { const rs = agg[`${p}|${st}`]; if (!rs) return '-'; const m = rs.reduce((a, r) => a + r.mean, 0) / rs.length, pk = Math.max(...rs.map((r) => r.peak)); return `${m.toFixed(2)} / ${pk.toFixed(2)}`; }).join(' | ') + ' |');
writeFileSync(`${OUT}/levels.md`, `Frame mean level, mean / peak over the effect (cap 0.72)\n\n${md.join('\n')}\n`);
results.push(`levels table: ${OUT}/levels.md`);

// 4. LED contact sheets, drawn in the page.
const drawSheet = async (data, file, title, colW = 5, ledH = 2) => {
  const p2 = await ctx.newPage();
  await p2.setContent('<body style="margin:0;background:#0b0b10"><canvas id=c></canvas></body>');
  const size = await p2.evaluate(([data, title, colW, ledH]) => {
    const rows = Object.entries(data), n = rows[0][1][0].cols[0].length / 3, tileW = 4 * colW + 3 * 2 + 14, tileH = n * ledH + 34, labelW = 120;
    const maxT = Math.max(...rows.map(([, t]) => t.length));
    const c = document.getElementById('c'); c.width = labelW + maxT * tileW + 10; c.height = 40 + rows.length * tileH;
    const g = c.getContext('2d'); g.fillStyle = '#0b0b10'; g.fillRect(0, 0, c.width, c.height);
    g.fillStyle = '#ddd'; g.font = '700 16px system-ui'; g.fillText(title, 10, 24);
    rows.forEach(([name, tiles], r) => {
      const y0 = 40 + r * tileH;
      g.fillStyle = '#ffcf5a'; g.font = '700 13px system-ui'; g.fillText(name, 8, y0 + tileH / 2);
      tiles.forEach((tile, j) => {
        const x0 = labelW + j * tileW;
        g.fillStyle = '#000'; g.fillRect(x0 - 3, y0, 4 * colW + 12, n * ledH + 6);
        tile.cols.forEach((col, ci) => { for (let i = 0; i < n; i++) { g.fillStyle = `rgb(${col[i * 3]},${col[i * 3 + 1]},${col[i * 3 + 2]})`; g.fillRect(x0 + ci * (colW + 2), y0 + 3 + (n - 1 - i) * ledH, colW, ledH - (ledH > 2 ? 1 : 0)); } });
        g.save(); g.translate(x0 + 2, y0 + n * ledH + 10); g.fillStyle = '#9a98b0'; g.font = '10px system-ui'; g.rotate(0.5); g.fillText(tile.label, 0, 0); g.restore();
      });
    });
    return [c.width, c.height];
  }, [data, title, colW, ledH]);
  await p2.setViewportSize({ width: size[0], height: size[1] });
  await (await p2.$('#c')).screenshot({ path: `${OUT}/${file}` });
  await p2.close();
  results.push(`sheet: ${OUT}/${file}`);
};
await drawSheet(sheet, 'sheet-presets.png', `LED columns per preset x stage (bottom LED at the bottom; 4 columns, ${ledsPerColumn} sim LEDs each)`);
await drawSheet(vicSheet, 'sheet-victory-leds.png', 'Victory finale on the LEDs per preset (age in s; climax at 2.4 s)', 7, 3);

// 5. Arena: overlays minimal vs full (FIGHT shout and a heavy), then a real KO -> victory -> round 2.
const shots = [];
await page.addStyleTag({ content: '.topbar { position: static !important; }' });
const grab = async (label) => { await page.evaluate(() => window.scrollTo(0, 0)); shots.push({ label, b64: (await (await page.$('#scene')).screenshot()).toString('base64') }); };
await page.evaluate(() => window.sim.setSensor(true));
await page.waitForFunction(() => window.sim.state === 'WARMING', null, { timeout: 8000 });
await page.evaluate(() => { const a = window.sim.arena; a.energyBar.current = a.config.warming_energy_threshold - 0.1; });
await page.evaluate(() => window.sim.shoutFor(450));
await page.waitForFunction(() => window.sim.state === 'FIGHT', null, { timeout: 8000 });
await sleep(1500);
for (const mode of ['full', 'minimal']) {
  await page.selectOption('#overlayMode', mode);
  await page.evaluate(() => window.sim.shoutFor(450)); await sleep(650); await grab(`${mode}: shout in FIGHT`);
  await sleep(900);
  await page.evaluate(() => window.sim.previewStage('heavy', 'nova')); await sleep(330); await grab(`${mode}: SUPERNOVA heavy`);
  await sleep(1200);
}
const before = shots.splice(0);
const vic = [];
await page.evaluate(() => { window.sim.game.hp = 1; window.sim.punch(92); });
const tK = Date.now();
for (const at of [0.15, 0.7, 1.5, 2.45, 3.3, 4.6, 6.0, 6.9, 7.8]) { await sleep(Math.max(0, at * 1000 - (Date.now() - tK))); await grab(`${at}s`); }
vic.push(...shots.splice(0));
const g = await page.evaluate(() => ({ round: window.sim.game.round, maxHp: window.sim.game.maxHp, hp: window.sim.game.hp, st: window.sim.game.lastVictory }));
results.push(`KO -> victory -> round ${g.round}, ${g.maxHp} HP (stats ${JSON.stringify(g.st)})`);
if (g.round !== 2 || g.maxHp !== Math.round(900 * 1.3)) problems.push(`after victory: round ${g.round}, max HP ${g.maxHp}`);
const imgSheet = async (imgs, file, title, w) => {
  const p2 = await ctx.newPage(); await p2.setViewportSize({ width: w, height: 600 });
  await p2.setContent(`<body style="margin:0;padding:10px;background:#0b0b10;color:#ddd;font:13px system-ui"><h3 style="margin:4px 0 10px">${title}</h3><div style="display:flex;flex-wrap:wrap;gap:8px">${imgs.map((s) => `<figure style="margin:0;width:${Math.floor((w - 40) / 3)}px"><img style="width:100%;border-radius:6px" src="data:image/png;base64,${s.b64}"><figcaption>${s.label}</figcaption></figure>`).join('')}</div></body>`);
  await p2.screenshot({ path: `${OUT}/${file}`, fullPage: true }); await p2.close();
  results.push(`sheet: ${OUT}/${file}`);
};
await imgSheet(before, 'sheet-overlays.png', 'Screen overlays: full (old look, before) vs minimal (new default, after)', 1300);
await imgSheet(vic, 'sheet-victory-arena.png', 'KO -> victory finale -> round 2 rises (time after the KO punch)', 1300);

// 6. The effects panel at desktop and phone widths.
for (const [w, h, tag] of [[1280, 900, 'desktop'], [390, 844, 'phone']]) {
  const c2 = await browser.newContext({ viewport: { width: w, height: h }, deviceScaleFactor: tag === 'phone' ? 2 : 1 });
  const p3 = await c2.newPage();
  await p3.goto(URL); await p3.waitForFunction(() => window.sim?.state === 'IDLE');
  await p3.addStyleTag({ content: '.topbar { position: static !important; }' });
  await p3.click('#fxPanel > summary');
  await p3.selectOption('#fxsel_warming', 'inferno');
  await p3.click('.fxStage[data-stage=heavy] .fxTune > summary');
  await p3.click('button[data-preview="heavy:thunder"]'); await sleep(250);
  await (await p3.$('.panel.strips')).screenshot({ path: `${OUT}/panel-${tag}.png` });
  const over = await p3.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
  if (over > 0) problems.push(`[${tag}] page scrolls sideways by ${over}px`);
  results.push(`panel: ${OUT}/panel-${tag}.png`);
  await p3.evaluate(() => window.sim.previewStage('victory')); await sleep(3200);
  await p3.evaluate(() => window.scrollTo(0, 0));
  await (await p3.$('#scene')).screenshot({ path: `${OUT}/victory-${tag}.png` });
  results.push(`victory at 3.2 s: ${OUT}/victory-${tag}.png`);
  await c2.close();
}

for (const e of await page.evaluate(() => window.sim.errors)) problems.push(`loop error: ${e}`);
await browser.close();
console.log(results.join('\n'));
console.log(md.join('\n'));
console.log(problems.length ? `PROBLEMS (${problems.length}):\n${problems.join('\n')}` : 'NO PROBLEMS');
process.exit(problems.length ? 1 : 0);
