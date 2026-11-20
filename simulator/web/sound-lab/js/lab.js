// Sound Lab page: builds every section from the sound modules, renders each sound offline
// (the exact buffer you hear is the one measured), draws spectrograms, runs the demos and the
// taunt recorder. window.lab exposes measureAll() for the headless check.
import { SR, renderOffline, measure, wavBytes, zipBlob, download } from './dsp.js';
import { FAMILIES, TIER_POWER, tierOf, buildImpact } from './impacts.js';
import { AMBIENCE, SURPRISES } from './ambience.js';
import { FIGHT_STARTS } from './fightstart.js';
import { BREAKS, BREAK_DUR, buildBreak, BreakRule } from './breakstuff.js';
import { PRESETS, demonize, decodeMono, trim } from './demon.js';
import { demoTemple, demoBreak, renderDemo } from './demos.js';

const $ = (s, el = document) => el.querySelector(s);
const h = (tag, attrs = {}, ...kids) => {
  const e = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) { if (k === 'class') e.className = v; else if (k.startsWith('on')) e.addEventListener(k.slice(2), v); else if (v !== false && v != null) e.setAttribute(k, v === true ? '' : v); }
  for (const k of kids.flat()) if (k != null) e.append(k.nodeType ? k : document.createTextNode(k));
  return e;
};
const LETTERS = 'ABCDEFGHIJKLMNOP';

// ---- playback --------------------------------------------------------------
let ctx = null, master = null; const live = new Set();
function audio() {
  if (!ctx) { ctx = new AudioContext(); master = ctx.createGain(); master.gain.value = 0.9; master.connect(ctx.destination); }
  if (ctx.state === 'suspended') ctx.resume();
  return ctx;
}
function playBuffer(buf, onend) {
  const c = audio(), s = c.createBufferSource(); s.buffer = buf; s.connect(master); s.start();
  live.add(s); s.onended = () => { live.delete(s); onend && onend(); }; return s;
}
function stopAll() { for (const s of live) { try { s.stop(); } catch { /* already stopped */ } } live.clear(); document.querySelectorAll('.playing').forEach((b) => b.classList.remove('playing')); }

const voiceCache = new Map();
async function voiceBuffer(file) {
  if (!voiceCache.has(file)) voiceCache.set(file, fetch(file).then((r) => { if (!r.ok) throw new Error(`${file} ${r.status}`); return r.arrayBuffer(); })
    .then((ab) => new OfflineAudioContext(1, 1, SR).decodeAudioData(ab)));
  return voiceCache.get(file);
}

// ---- measurement registry (for the loudness table) -------------------------
const registry = []; // {section, name, get: () => Promise<AudioBuffer>, kind}
const reg = (section, name, get, kind = 'synth') => registry.push({ section, name, get, kind });

// ---- spectrogram -------------------------------------------------------------
function fft(re, im) {
  const n = re.length;
  for (let i = 1, j = 0; i < n; i++) { let bit = n >> 1; for (; j & bit; bit >>= 1) j ^= bit; j ^= bit; if (i < j) { [re[i], re[j]] = [re[j], re[i]]; [im[i], im[j]] = [im[j], im[i]]; } }
  for (let len = 2; len <= n; len <<= 1) {
    const a = -2 * Math.PI / len, wr = Math.cos(a), wi = Math.sin(a);
    for (let i = 0; i < n; i += len) { let cr = 1, ci = 0; for (let k = 0; k < len / 2; k++) { const ur = re[i + k], ui = im[i + k], vr = re[i + k + len / 2] * cr - im[i + k + len / 2] * ci, vi = re[i + k + len / 2] * ci + im[i + k + len / 2] * cr; re[i + k] = ur + vr; im[i + k] = ui + vi; re[i + k + len / 2] = ur - vr; im[i + k + len / 2] = ui - vi; const t = cr * wr - ci * wi; ci = cr * wi + ci * wr; cr = t; } }
  }
}
function drawSpectrogram(cv, buf) {
  const dpr = Math.min(2, window.devicePixelRatio || 1), W = Math.max(60, cv.clientWidth), H = cv.clientHeight || 56;
  cv.width = W * dpr; cv.height = H * dpr; const g = cv.getContext('2d'); g.scale(dpr, dpr);
  const x = buf.getChannelData(0), N = 1024, cols = Math.min(W, 400), img = g.createImageData(cols, H);
  const st = getComputedStyle(document.documentElement), hot = st.getPropertyValue('--spec-hot').trim() || '#ffb347';
  const [hr, hg, hb] = hot.match(/\w\w/g).map((v) => parseInt(v, 16));
  const fmin = 40, fmax = 16000, re = new Float32Array(N), im = new Float32Array(N), bins = new Float32Array(H);
  for (let col = 0; col < cols; col++) {
    const start = Math.floor(col / cols * Math.max(1, x.length - N)); re.fill(0); im.fill(0);
    for (let i = 0; i < N; i++) re[i] = (x[start + i] || 0) * (0.5 - 0.5 * Math.cos(2 * Math.PI * i / N));
    fft(re, im);
    for (let y = 0; y < H; y++) { const f = fmin * Math.pow(fmax / fmin, 1 - y / H), k = Math.min(N / 2 - 1, Math.round(f / buf.sampleRate * N)); const m = Math.hypot(re[k], im[k]) / N * 4; bins[y] = Math.max(0, Math.min(1, (20 * Math.log10(m + 1e-9) + 80) / 72)); }
    for (let y = 0; y < H; y++) { const v = bins[y], o = (y * cols + col) * 4; img.data[o] = hr * v; img.data[o + 1] = hg * v * v; img.data[o + 2] = hb * v * v * v + 40 * v; img.data[o + 3] = 40 + 215 * v; }
  }
  const tmp = document.createElement('canvas'); tmp.width = cols; tmp.height = H; tmp.getContext('2d').putImageData(img, 0, 0);
  g.imageSmoothingEnabled = true; g.drawImage(tmp, 0, 0, W, H);
}
const drawQueue = []; let drawing = false;
const io = new IntersectionObserver((es) => es.forEach((e) => { if (e.isIntersecting) { io.unobserve(e.target); drawQueue.push(e.target); pump(); } }), { rootMargin: '300px' });
async function pump() {
  if (drawing) return; drawing = true;
  while (drawQueue.length) { const cv = drawQueue.shift(); try { const b = await cv._get(); drawSpectrogram(cv, b); cv.dataset.drawn = '1'; const m = measure(b); const tag = cv.parentElement.querySelector('.meter'); if (tag) tag.textContent = `${m.lufs.toFixed(1)} LUFS · peak ${m.tp.toFixed(1)} dBTP · ${b.duration.toFixed(1)} s`; } catch (err) { console.error(err); } await new Promise((r) => setTimeout(r, 0)); }
  drawing = false;
}
function specCanvas(get) { const cv = h('canvas', { class: 'spec', 'aria-hidden': 'true' }); cv._get = get; io.observe(cv); return cv; }

// ---- generic building blocks -------------------------------------------------
function playBtn(label, getBuf, { cls = '' } = {}) {
  const b = h('button', { class: `play ${cls}`, type: 'button', 'aria-label': `Play ${label}` }, h('span', { class: 'tri', 'aria-hidden': 'true' }), label);
  b.addEventListener('click', async () => {
    if (b.classList.contains('playing')) { stopAll(); return; }
    audio(); b.classList.add('busy');
    try { const buf = await getBuf(); b.classList.remove('busy'); b.classList.add('playing'); playBuffer(buf, () => b.classList.remove('playing')); }
    catch (e) { b.classList.remove('busy'); console.error(e); }
  });
  return b;
}
const memo = (fn) => { let p = null; return () => (p ||= fn()); };
const synth = (dur, build) => memo(() => renderOffline(dur, build));

function card({ letter, name, desc, when, leds, get, section, extra }) {
  reg(section, `${letter ? letter + '. ' : ''}${name}`, get);
  return h('article', { class: 'card' },
    h('div', { class: 'card-head' }, letter ? h('span', { class: 'letter' }, letter) : null, h('h3', {}, name)),
    h('p', { class: 'desc' }, desc),
    when ? h('p', { class: 'when' }, h('b', {}, 'Plays: '), when) : null,
    leds ? h('p', { class: 'when leds' }, h('b', {}, 'LEDs: '), leds) : null,
    h('div', { class: 'card-play' }, playBtn('Play', get), extra || null),
    specCanvas(get), h('p', { class: 'meter' }, ''));
}
function section(id, title, intro, ...kids) {
  return h('section', { id, class: 'sec' }, h('h2', {}, title), intro ? h('p', { class: 'intro' }, intro) : null, ...kids);
}

// ---- sections ------------------------------------------------------------------
function fightStartSection() {
  const cur = { letter: 'A', name: 'Current (sfx.js arena-opening)', dur: 2.9,
    desc: 'The arena-opening sound the simulator plays today, for comparison: noise riser, sub boom, gong, war drums, crowd.',
    leds: 'As now: the LED burst at 0.35 s.',
    build: async (c, d, t) => { const m = await import('../../js/sfx.js'); m.buildArenaOpen(c, d, t); } };
  const items = [cur, ...FIGHT_STARTS];
  return section('fight-start', 'Fight start', 'The moment WARMING turns into FIGHT. Pick by letter. A is the current sound; B onward are new, all WebAudio so they drop into sfx.js in place of buildArenaOpen(). Ring bells are G to J; K to O are the rough family (dented bell, steel pipe, fire-alarm bell, blown speaker, oil drum).',
    h('div', { class: 'grid' }, items.map((f, i) => card({ letter: LETTERS[i], name: f.name, desc: f.desc, leds: f.leds, section: 'Fight start', get: synth(f.dur + 0.3, (c, d, t) => f.build(c, d, t)) }))));
}

function laughSection(man) {
  return section('laughs', 'Evil laughs', 'Six rolling shadow laughs, all built from offline TTS through original processing chains (no samples). The note under each says when it would play.',
    h('div', { class: 'grid' }, man.laugh.map((l, i) => card({ letter: LETTERS[i], name: l.name, desc: l.desc, when: l.when, section: 'Evil laughs', get: () => voiceBuffer(l.file) }))));
}

function demoSection() {
  const box = (title, desc, fn) => {
    const cap = h('ol', { class: 'captions' }), st = h('span', { class: 'status' }, '');
    let job = null, raf = 0;
    const go = async (reroll) => {
      stopAll(); cancelAnimationFrame(raf); st.textContent = 'rendering...';
      try {
        if (!job || reroll) job = renderDemo(fn, reroll ? Math.floor(Math.random() * 1e6) : 1);
        const { buffer, ev } = await job; st.textContent = `${buffer.duration.toFixed(0)} s`;
        cap.replaceChildren(...ev.map((e) => h('li', { class: e.dim ? 'dim' : '' }, h('span', { class: 'ts' }, e.t.toFixed(1).padStart(4, ' ')), ' ', e.text)));
        const c = audio(), t0 = c.currentTime; playBuffer(buffer);
        const lis = [...cap.children];
        const tick = () => { const t = c.currentTime - t0; lis.forEach((li, i) => li.classList.toggle('now', ev[i].t <= t && (i === ev.length - 1 || ev[i + 1].t > t))); if (t < buffer.duration) raf = requestAnimationFrame(tick); };
        tick();
      } catch (e) { st.textContent = 'failed'; console.error(e); }
    };
    reg('Mix demos', title, async () => (await (job ||= renderDemo(fn, 1))).buffer);
    return h('article', { class: 'card demo' }, h('h3', {}, title), h('p', { class: 'desc' }, desc),
      h('div', { class: 'card-play' }, h('button', { class: 'play big', type: 'button', onclick: () => go(false) }, h('span', { class: 'tri' }), 'Play demo'),
        fn === demoBreak ? h('button', { class: 'ghost', type: 'button', onclick: () => go(true) }, 'Reroll the dice') : null, st), cap);
  };
  return section('demos', 'Mix demos', 'Scripted mini-fights with the sounds together. The captions follow along.',
    h('div', { class: 'grid two' },
      box('Demon temple (about 30 s)', 'Recommendation 1 in action: Hebrew butler-demon greeting, singing-bowl charge, demon-monk announcer, floodlight fight start, Leather & Sand hits with Cinematic heavies, crowd, heartbeat, bullet-time K.O., victory.', demoTemple),
      box('Break Stuff mode (about 23 s)', 'A stand-in rock groove (the song is not bundled). Every punch goes through BreakRule; the captions show which ones broke something and why. Reroll for a different roll of the dice.', demoBreak)));
}

const CHAR = {
  monk: { name: 'Demon monk', desc: 'A meditation teacher\'s calm, slow voice, with a sub-octave growl rising underneath until each line ends possessed. Temple reverb.', why: 'Calm menace is scarier than shouting, and it is a character nobody else has. The calm-to-growl arc mirrors the game: still breath, then violence.' },
  announcer: { name: 'Arena barker', desc: 'Big, compressed, slightly overdriven hype voice with a stadium slapback.', why: 'Instantly readable "this is a fighting game" energy; short lines cut through music. The safe crowd-pleaser.' },
  choir: { name: 'Shadow choir', desc: 'The shadow itself: three of "you" whispering in unison at different pitches, with a reversed-reverb inhale before every line.', why: 'The opponent talks, not a narrator, so the shadow becomes a person. The reverse swell means every line starts with the room breathing in.' },
  arcade: { name: 'Bonus: 1987 cabinet', desc: 'A bit-crushed, band-limited arcade machine that has been left on for forty years.', why: 'Retro joke option, a few lines only. Great for a party mode.' },
};
function announcerSection(man) {
  const blocks = ['monk', 'announcer', 'choir', 'arcade'].map((k) => {
    const rows = man[k].map((v) => { const get = () => voiceBuffer(v.file); reg(`Announcer: ${CHAR[k].name}`, v.id, get, 'clip');
      return h('li', { class: 'line' }, playBtn('', get, { cls: 'mini' }), h('span', { class: 'cat' }, v.cat), h('span', { class: 'txt' }, `"${v.text}"`)); });
    return h('article', { class: 'card voice' }, h('h3', {}, CHAR[k].name), h('p', { class: 'desc' }, CHAR[k].desc), h('p', { class: 'why' }, h('b', {}, 'Why it fits: '), CHAR[k].why),
      specCanvas(() => voiceBuffer(man[k][0].file)), h('p', { class: 'meter' }, ''), h('ul', { class: 'lines' }, rows));
  });
  return section('announcer', 'Announcer', 'Original lines, voiced with Piper (offline open-source TTS) and processed with ffmpeg. All clips are mono mp3 at about -16 LUFS, true peak under -1 dBTP.', h('div', { class: 'grid two' }, blocks));
}

// ---- Hebrew taunts + recorder ----------------------------------------------------
const DB = 'sound-lab-takes';
const idb = (mode, fn) => new Promise((res, rej) => {
  let req; try { req = indexedDB.open(DB, 1); } catch (e) { rej(e); return; }
  req.onupgradeneeded = () => req.result.createObjectStore('takes', { keyPath: 'id' });
  req.onerror = () => rej(req.error);
  req.onsuccess = () => { const tx = req.result.transaction('takes', mode), st = tx.objectStore('takes'), out = fn(st); tx.oncomplete = () => res(out && out.result !== undefined ? out.result : out); tx.onerror = () => rej(tx.error); };
});
const extOf = (type) => (/ogg/.test(type) ? 'ogg' : /mp4|aac|m4a/.test(type) ? 'm4a' : /wav/.test(type) ? 'wav' : /mpeg/.test(type) ? 'mp3' : 'webm');

function tauntSection(man) {
  const lines = man.hebrew, takes = new Map(lines.map((l) => [l.id, []])), chosen = {};
  let stream = null, rec = null, recLine = null, preset = 'butler';
  const presetSel = h('select', { id: 'preset', 'aria-label': 'Demon preset', onchange: (e) => { preset = e.target.value; } }, Object.entries(PRESETS).map(([k, p]) => h('option', { value: k }, `${p.name}: ${p.desc}`)));
  const note = h('p', { class: 'status' }, '');

  async function addTake(lineId, blob, persist = true, id = `${lineId}-${Date.now()}`) {
    const t = { id, line: lineId, blob, demon: {} }; takes.get(lineId).push(t);
    if (persist) idb('readwrite', (st) => st.put({ id, line: lineId, blob, type: blob.type })).catch(() => {});
    if (!chosen[lineId]) chosen[lineId] = id;
    renderTakes(lineId);
  }
  async function rawBuf(t) { if (!t.raw) { const x = trim(await decodeMono(t.blob)); const b = new AudioBuffer({ length: Math.max(1, x.length), sampleRate: SR, numberOfChannels: 1 }); b.copyToChannel(x, 0); t.raw = b; } return t.raw; }
  async function demonBuf(t, p) { if (!t.demon[p]) t.demon[p] = demonize((await rawBuf(t)).getChannelData(0), p); return t.demon[p]; }

  const takeLists = {};
  function renderTakes(lineId) {
    const ul = takeLists[lineId], list = takes.get(lineId);
    ul.replaceChildren(...list.map((t, i) => h('li', { class: 'take' },
      h('label', { class: 'choose' }, h('input', { type: 'radio', name: `ch-${lineId}`, checked: chosen[lineId] === t.id, onchange: () => { chosen[lineId] = t.id; } }), `Take ${i + 1}`),
      playBtn('Raw', () => rawBuf(t), { cls: 'mini' }), playBtn('Demon', () => demonBuf(t, preset), { cls: 'mini hot' }),
      h('button', { class: 'ghost', type: 'button', onclick: async () => download(new Blob([wavBytes((await demonBuf(t, preset)).getChannelData(0), SR)], { type: 'audio/wav' }), `${lineId}-take${i + 1}-${preset}.wav`) }, 'WAV'),
      h('button', { class: 'ghost', type: 'button', onclick: () => download(t.blob, `${lineId}-take${i + 1}-raw.${extOf(t.blob.type)}`) }, 'Raw file'),
      h('button', { class: 'ghost', type: 'button', 'aria-label': 'Delete take', onclick: () => { list.splice(list.indexOf(t), 1); if (chosen[lineId] === t.id) chosen[lineId] = list[0]?.id; idb('readwrite', (st) => st.delete(t.id)).catch(() => {}); renderTakes(lineId); } }, 'Delete'))));
  }
  async function toggleRec(lineId, btn) {
    if (rec) { rec.stop(); return; }
    try {
      stream ||= await navigator.mediaDevices.getUserMedia({ audio: { echoCancellation: false, noiseSuppression: false, autoGainControl: true } });
      const chunks = []; rec = new MediaRecorder(stream); recLine = lineId;
      rec.ondataavailable = (e) => e.data.size && chunks.push(e.data);
      rec.onstop = () => { const b = new Blob(chunks, { type: rec.mimeType || 'audio/webm' }); rec = null; recLine = null; btn.classList.remove('rec-on'); btn.lastChild.textContent = 'Record'; addTake(lineId, b); };
      rec.start(); btn.classList.add('rec-on'); btn.lastChild.textContent = 'Stop';
      note.textContent = '';
    } catch (e) { note.textContent = `Microphone unavailable (${e.name || e}). Use "Add file" to load a recording instead.`; }
  }
  const byTrigger = {};
  for (const l of lines) (byTrigger[l.cat] ||= []).push(l);
  const groups = Object.entries(byTrigger).map(([trig, ls]) => h('div', { class: 'tgroup' }, h('h3', {}, trig), h('ul', { class: 'lines he' }, ls.map((l) => {
    const get = () => voiceBuffer(l.file); reg('Hebrew taunts (TTS placeholder)', l.id, get, 'clip');
    const ul = h('ul', { class: 'takes' }); takeLists[l.id] = ul;
    const file = h('input', { type: 'file', accept: 'audio/*', class: 'visually-hidden', onchange: (e) => { for (const f of e.target.files) addTake(l.id, f); e.target.value = ''; } });
    return h('li', { class: 'line tline' },
      h('div', { class: 'tl-top' }, playBtn('TTS', get, { cls: 'mini' }), h('span', { class: 'txt', lang: 'he', dir: 'rtl' }, l.text)),
      h('p', { class: 'gloss' }, l.gloss),
      h('div', { class: 'tl-act' }, h('button', { class: 'rec', type: 'button', onclick: (e) => toggleRec(l.id, e.currentTarget) }, h('span', { class: 'dot', 'aria-hidden': 'true' }), 'Record'),
        h('label', { class: 'ghost filebtn' }, 'Add file', file)), ul);
  }))));
  const allBtn = h('button', { class: 'play', type: 'button', onclick: async () => {
    const files = [], txt = [];
    for (const l of lines) { const t = takes.get(l.id).find((x) => x.id === chosen[l.id]); if (!t) continue;
      files.push({ name: `demon-${preset}/${l.id}.wav`, data: wavBytes((await demonBuf(t, preset)).getChannelData(0), SR) });
      files.push({ name: `raw/${l.id}.${extOf(t.blob.type)}`, data: new Uint8Array(await t.blob.arrayBuffer()) }); txt.push(`${l.id}\t${l.cat}\t${l.text}`); }
    if (!files.length) { note.textContent = 'No takes yet: record or add a file first.'; return; }
    files.push({ name: 'lines.tsv', data: new TextEncoder().encode(txt.join('\n') + '\n') });
    download(zipBlob(files), `shadow-taunts-${preset}.zip`);
  } }, 'Download all chosen (zip)');
  const script = h('button', { class: 'ghost', type: 'button', onclick: () => download(new Blob([lines.map((l) => `${l.id}  [${l.cat}]\n${l.text}\n(${l.gloss})\n`).join('\n')], { type: 'text/plain;charset=utf-8' }), 'shadow-taunts-script.txt') }, 'Script for friends (.txt)');
  idb('readonly', (st) => st.getAll()).then((rows) => (rows || []).forEach((r) => takes.has(r.line) && addTake(r.line, r.blob, false, r.id))).catch(() => {});
  window.lab_addTake = addTake; // test hook
  return section('taunts', 'Shadow taunts (Hebrew)', null,
    h('div', { class: 'callout' },
      h('p', {}, h('b', {}, 'The plan: '), 'these lines get recorded by Yarden and friends, then turned into the demon voice. The TTS clips (Phonikud diacritizer + Phonikud Piper voice, offline, through the same demon chain) are placeholders so the timing and tone can be judged now.'),
      h('p', {}, h('b', {}, 'Tone: '), 'a butler-demon. Perfect manners on the surface, disgusting underneath. Gender-balanced: plural or impersonal, or the gender confusion is the joke.'),
      h('p', {}, h('b', {}, 'How to record: '), 'Record (or Add file) per line, as many takes as you like. Raw plays the take as recorded; Demon plays it through the chosen preset. Pick a take per line and download them all. Everything stays in this browser (takes are kept in local storage); nothing is uploaded. Batch route for a folder of WAVs: ', h('code', {}, 'tools/demonize.py'), '.'),
      h('div', { class: 'card-play' }, h('label', { for: 'preset' }, 'Demon preset'), presetSel), h('div', { class: 'card-play' }, allBtn, script), note),
    groups);
}

function impactSection() {
  const fams = FAMILIES.map((f) => {
    const cells = ['jab', 'hook', 'heavy'].map((tier) => h('div', { class: 'tier' }, h('span', { class: 'tname' }, `${tier} ${TIER_POWER[tier]}`),
      [1, 2, 3].map((seed) => { const get = synth(2.2, (c, d, t) => buildImpact(f.id, c, d, t + 0.15, TIER_POWER[tier] + (seed - 2) * 4, seed)); reg(`Impacts: ${f.name}`, `${tier} ${LETTERS[seed - 1]}`, get); return playBtn(LETTERS[seed - 1], get, { cls: 'mini' }); })));
    const val = h('output', {}, '60 (hook)'), slider = h('input', { type: 'range', min: 1, max: 100, value: 60, 'aria-label': `${f.name} power`, oninput: (e) => { val.textContent = `${e.target.value} (${tierOf(+e.target.value)})`; } });
    const hit = playBtn('Hit', () => renderOffline(2.2, (c, d, t) => buildImpact(f.id, c, d, t + 0.15, +slider.value, Math.floor(Math.random() * 1e5))), { cls: 'mini hot' });
    const ramp = synth(5.2, (c, d, t) => { for (let i = 0; i < 10; i++) buildImpact(f.id, c, d, t + 0.15 + i * 0.36, 10 + i * 10, i + 1); });
    reg(`Impacts: ${f.name}`, 'ramp 10 to 100', ramp);
    return h('article', { class: 'card' }, h('h3', {}, f.name), h('p', { class: 'desc' }, f.desc), h('p', { class: 'why' }, h('b', {}, 'Why it fits: '), f.why),
      h('div', { class: 'tiers' }, cells),
      h('div', { class: 'power' }, h('label', {}, 'Power ', val), slider, hit, playBtn('Ramp 10 to 100', ramp, { cls: 'mini' })),
      specCanvas(ramp), h('p', { class: 'meter' }, ''));
  });
  return section('impacts', 'Bag impacts', 'Four families, each scaling continuously with power across the simulator\'s tiers (jab under 45, hook 45 to 74, heavy 75 and up). A, B, C are alternates so repeats never sound robotic. The slider renders a fresh random alternate at any power; the spectrogram shows the 10-to-100 ramp.', h('div', { class: 'grid two' }, fams));
}

function ambienceSection() {
  const mk = (list, sec) => list.map((a, i) => card({ name: a.name, desc: a.desc, when: a.when, section: sec, get: synth(a.dur + 0.2, (c, d, t) => a.build(c, d, t)) }));
  return [section('ambience', 'Ambience and extras', 'Game-state sounds around the punches.', h('div', { class: 'grid' }, mk(AMBIENCE, 'Ambience'))),
    section('surprises', 'Surprises', 'Ideas nobody asked for. Each one ties a sound to something the game already knows.', h('div', { class: 'grid' }, mk(SURPRISES, 'Surprises')))];
}

function breakSection() {
  const sizes = { small: 'Small: jabs', medium: 'Medium: hooks', big: 'Big: heavies' };
  const groups = Object.entries(sizes).map(([size, label]) => h('div', { class: 'tgroup' }, h('h3', {}, label), h('div', { class: 'grid' },
    BREAKS.filter((b) => b.size === size).map((b) => {
      const gets = Array.from({ length: b.alts }, (_, a) => synth(BREAK_DUR[size] + 0.3, (c, d, t) => buildBreak(b.id, c, d, t + 0.05, a, 1)));
      gets.forEach((g, a) => reg(`Break Stuff: ${size}`, `${b.name} ${LETTERS[a]}`, g));
      return h('article', { class: 'card small' }, h('h3', {}, b.name), h('div', { class: 'card-play' }, gets.map((g, a) => playBtn(LETTERS[a], g, { cls: 'mini' }))), specCanvas(gets[0]), h('p', { class: 'meter' }, ''));
    }))));
  const sim = h('pre', { class: 'code' }), simBtn = h('button', { class: 'ghost', type: 'button', onclick: () => {
    const r = new BreakRule(), out = []; let n = 0, t = 0; const tally = { small: 0, medium: 0, big: 0 }, cnt = { jab: 0, hook: 0, heavy: 0 };
    for (let i = 0; i < 400; i++) { t += 0.4 + Math.random() * 1.2; const p = Math.round(Math.min(100, Math.max(5, 20 + Math.random() * 80))); cnt[tierOf(p)]++; const b = r.decide(p, { now: t, combo: 1 + (i % 7) }); if (b) { n++; tally[b.size]++; if (out.length < 8) out.push(`${t.toFixed(1)}s  power ${p}  -> ${b.kind} (${b.reason})`); } }
    sim.textContent = `400 random punches over ${t.toFixed(0)} s: ${n} breaks (small ${tally.small}, medium ${tally.medium}, big ${tally.big}); punches: jab ${cnt.jab}, hook ${cnt.hook}, heavy ${cnt.heavy}\n` + out.join('\n');
  } }, 'Simulate 400 punches');
  return section('break-stuff', 'Break Stuff mode', null,
    h('div', { class: 'callout' },
      h('p', {}, 'When the fight track is Limp Bizkit\'s "Break Stuff", some punches also smash something, sized by power. All synthesized here, nothing sampled.'),
      h('p', {}, h('b', {}, 'Trigger rule '), h('code', {}, 'BreakRule.decide(power, {now, combo})'), ' in ', h('code', {}, 'sound-lab/js/breakstuff.js'), ': chance by tier rising with power inside the tier (jab about 3 to 7 %, hook 14 to 26 %, heavy 40 to 60 %); always on a new max-power hit (hook or above) or a combo milestone (every 5); at most one break per 2 s; never the same object twice in a row; breakage goes to a bus at -6 dB so it sits under the music.'),
      h('p', {}, h('b', {}, 'Detecting the track: '), h('code', {}, 'isBreakStuffTrack(playlist.current)'), ' matches /break\\W*stuff/i against the title and URL that music.js keeps for the current track ("08-limp-bizkit-break-stuff.mp3" becomes the title "limp bizkit break stuff").'),
      h('div', { class: 'card-play' }, simBtn), sim),
    groups);
}

function recSection() {
  return section('picks', 'Top 3 recommendations', null, h('ol', { class: 'picks' },
    h('li', {}, h('b', {}, 'Demon temple. '), 'Demon monk announcer + Hebrew butler-demon taunts (recorded by you and friends) + Leather & Sand for jabs and hooks with Cinematic Boom heavies + dojo wind and singing-bowl charge + fight start B or C + Abyss rumble laugh + bullet-time K.O. The most original direction: calm menace, a real-feeling bag, one coherent world. Hear it in the first mix demo.'),
    h('li', {}, h('b', {}, 'Arcade arena. '), 'Arena barker + Anime Crack (matches the LED signatures) + crowd swell and combo ladder + ring bell G or J + tape-stop K.O. + Villain crescendo laugh. Loud, readable, party-friendly; the easiest to enjoy for first-time players.'),
    h('li', {}, h('b', {}, 'Haunted mirror. '), 'Shadow choir as the opponent\'s voice + Taiko Dojo impacts + shadow breath between punches + heartbeat at low HP + Legion laugh when it rises again + fight start F (Vacuum). Eerie and theatrical; best in a dark room.')));
}

function tableSection() {
  const tb = h('tbody'), st = h('span', { class: 'status' }, '');
  const run = async () => {
    st.textContent = 'measuring...'; const rows = await window.lab.measureAll(); tb.replaceChildren(...rows.map((r) => h('tr', { class: r.tp > -1 ? 'warn' : '' }, h('td', {}, r.section), h('td', {}, r.name), h('td', {}, r.dur.toFixed(2)), h('td', {}, r.lufs.toFixed(1)), h('td', {}, r.peak.toFixed(1)), h('td', {}, r.tp.toFixed(1)))));
    const worst = rows.reduce((a, r) => Math.max(a, r.tp), -99); st.textContent = `${rows.length} sounds; highest true peak ${worst.toFixed(1)} dBTP`;
  };
  return section('loudness', 'Loudness check', 'Every sound on this page rendered offline at 48 kHz and measured here: integrated loudness (BS.1770, gated; clips under 0.4 s padded), sample peak and 4x-oversampled true peak. Rows over -1 dBTP are highlighted.',
    h('div', { class: 'card-play' }, h('button', { class: 'play', type: 'button', id: 'measure', onclick: run }, 'Measure everything'), st),
    h('div', { class: 'tablewrap' }, h('table', {}, h('thead', {}, h('tr', {}, ['Section', 'Sound', 's', 'LUFS', 'Peak dBFS', 'True peak dBTP'].map((x) => h('th', {}, x)))), tb)));
}

// ---- boot ------------------------------------------------------------------------------
async function boot() {
  const man = await (await fetch('voice/manifest.json')).json();
  const main = $('#main');
  main.append(recSection(), fightStartSection(), laughSection(man), demoSection(), announcerSection(man), tauntSection(man), impactSection(), ...ambienceSection(), breakSection(), tableSection());
  $('#stop').addEventListener('click', stopAll);
  document.addEventListener('keydown', (e) => { if (e.key === 'Escape') stopAll(); });
  window.lab = {
    ready: true, registry, specState: () => ({ queued: drawQueue.length, drawing }),
    async measureAll() {
      const rows = [];
      for (const r of registry) { const b = await r.get(); const m = measure(b); rows.push({ section: r.section, name: r.name, kind: r.kind, dur: b.duration, ...m }); await new Promise((x) => setTimeout(x, 0)); }
      return rows;
    },
  };
}
boot().catch((e) => { console.error(e); const m = document.getElementById('main'); if (m) m.prepend(Object.assign(document.createElement('p'), { className: 'callout', textContent: `Sound Lab failed to load: ${e.message}` })); });
