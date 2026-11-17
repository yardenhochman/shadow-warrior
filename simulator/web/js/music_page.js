// music.html: pick the section of each fight track that plays in FIGHT (see segments.js).
import { loadSegments, saveLocal, saveToBridge } from './segments.js';

const BASE = '../../music/';
const FADE_IN = 0.8, FADE_OUT = 1.5;
const $ = (s, el = document) => el.querySelector(s);
const status = $('#status');
const audio = new Audio(); audio.preload = 'auto';

let segs = {};
let cards = [];
let cur = null;          // card whose track is loaded in `audio`
let gameMode = false;    // playing "as in game": fades and stop/loop at the end
let saveTimer = null;

const fmt = (t) => {
  if (t == null || !isFinite(t)) return '–';
  const m = Math.floor(t / 60), s = t - m * 60;
  return `${m}:${s.toFixed(1).padStart(4, '0')}`;
};

function setStatus(text, ok = false) { status.textContent = text; status.classList.toggle('ok', ok); }

function save() {
  saveLocal(segs);
  setStatus('Saving…');
  clearTimeout(saveTimer);
  saveTimer = setTimeout(async () => {
    const ok = await saveToBridge(segs);
    setStatus(ok ? 'Saved (game uses it)' : 'Saved in this browser', true);
  }, 300);
}

async function loadTracks() {
  const r = await fetch(BASE + 'playlist.tsv', { cache: 'no-store' });
  if (!r.ok) throw new Error('playlist.tsv not found');
  return (await r.text()).split('\n').filter((l) => l.trim() && !l.startsWith('#'))
    .map((l) => l.split('\t')).filter((c) => c.length >= 3)
    .map(([, file, title]) => ({ file: file.trim(), title: title.trim(), url: BASE + file.trim() }));
}

function makeCard(track, i) {
  const el = document.createElement('section');
  el.className = 'card';
  el.innerHTML = `
    <div class="top"><span class="num">${String(i + 1).padStart(2, '0')}</span>
      <span class="title"></span><span class="chip"></span></div>
    <canvas></canvas>
    <div class="controls">
      <button data-a="play">Play</button>
      <button data-a="start">Start here</button>
      <button data-a="end">End here</button>
      <button data-a="game" class="primary">Play as in game</button>
      <button data-a="loop">Loop section</button>
      <button data-a="reset">Whole track</button>
      <span class="time"></span>
    </div>`;
  $('.title', el).textContent = track.title;
  const c = { track, el, canvas: $('canvas', el), peaks: null, dur: null, pos: 0, loop: false, missing: false, drag: null };
  el.addEventListener('click', (e) => { const a = e.target.dataset?.a; if (a) action(c, a); });
  bindCanvas(c);
  return c;
}

const segOf = (c) => segs[c.track.file] || null;
const startOf = (c) => segOf(c)?.start || 0;
const endOf = (c) => segOf(c)?.end ?? c.dur;

function setSeg(c, start, end) {
  if (!start && end == null) delete segs[c.track.file];
  else segs[c.track.file] = { start: Math.max(0, +start.toFixed(2)), end: end == null ? null : +end.toFixed(2) };
  update(c); save();
}

function load(c) {
  if (cur !== c) {
    if (cur) cur.el.classList.remove('playing');
    cur = c; audio.src = c.track.url; c.el.classList.add('playing');
  }
}

function playFrom(c, t, asGame = false) {
  load(c);
  gameMode = asGame;
  const go = () => { audio.currentTime = t; audio.volume = asGame ? 0 : 1; audio.play().catch(() => {}); };
  if (audio.readyState >= 1) go(); else audio.addEventListener('loadedmetadata', go, { once: true });
}

function action(c, a) {
  const here = cur === c ? audio.currentTime : c.pos;
  const s = segOf(c);
  if (a === 'play') {
    if (cur === c && !audio.paused) audio.pause(); else playFrom(c, here);
  } else if (a === 'start') {
    const end = s?.end != null && s.end > here + 3 ? s.end : null;
    setSeg(c, here, end);
  } else if (a === 'end') {
    if (here <= startOf(c) + 3) { setStatus('End must be at least 3 s after the start'); return; }
    setSeg(c, startOf(c), here);
  } else if (a === 'game') {
    playFrom(c, startOf(c), true);
  } else if (a === 'loop') {
    c.loop = !c.loop; update(c);
  } else if (a === 'reset') {
    setSeg(c, 0, null);
  }
}

// Canvas: click to seek and play, drag the section edges.
function bindCanvas(c) {
  const tAt = (e) => {
    const r = c.canvas.getBoundingClientRect();
    return Math.max(0, Math.min(1, (e.clientX - r.left) / r.width)) * (c.dur || 0);
  };
  const xOf = (t) => (t / c.dur) * c.canvas.getBoundingClientRect().width;
  c.canvas.addEventListener('pointerdown', (e) => {
    if (!c.dur) return;
    const r = c.canvas.getBoundingClientRect(), x = e.clientX - r.left, s = segOf(c);
    if (s) {
      if (Math.abs(x - xOf(startOf(c))) < 12) c.drag = 'start';
      else if (s.end != null && Math.abs(x - xOf(s.end)) < 12) c.drag = 'end';
    }
    if (c.drag) { c.canvas.setPointerCapture(e.pointerId); return; }
    c.pos = tAt(e); playFrom(c, c.pos);
  });
  c.canvas.addEventListener('pointermove', (e) => {
    if (!c.drag) return;
    const t = tAt(e), s = segOf(c);
    if (c.drag === 'start') s.start = Math.max(0, Math.min(t, (s.end ?? c.dur) - 3));
    else s.end = Math.min(c.dur, Math.max(t, s.start + 3));
    update(c);
  });
  const done = () => { if (c.drag) { c.drag = null; setSeg(c, segOf(c).start, segOf(c).end); } };
  c.canvas.addEventListener('pointerup', done);
  c.canvas.addEventListener('pointercancel', done);
}

function update(c) {
  const s = segOf(c), chip = $('.chip', c.el);
  chip.classList.toggle('set', !!s);
  chip.textContent = c.missing ? 'file missing' : s ? `${fmt(s.start)} – ${s.end != null ? fmt(s.end) : 'end'}` : 'Whole track';
  $('[data-a=loop]', c.el).classList.toggle('on', c.loop);
  $('[data-a=play]', c.el).textContent = cur === c && !audio.paused ? 'Pause' : 'Play';
  const t = cur === c ? audio.currentTime : c.pos;
  $('.time', c.el).textContent = c.dur ? `${fmt(t)} / ${fmt(c.dur)}` : '';
  draw(c);
}

function draw(c) {
  const cv = c.canvas, dpr = window.devicePixelRatio || 1;
  const w = cv.clientWidth, h = cv.clientHeight;
  if (cv.width !== Math.round(w * dpr)) { cv.width = Math.round(w * dpr); cv.height = Math.round(h * dpr); }
  const g = cv.getContext('2d'); g.setTransform(dpr, 0, 0, dpr, 0, 0); g.clearRect(0, 0, w, h);
  const css = getComputedStyle(document.documentElement), v = (n) => css.getPropertyValue(n).trim();
  if (!c.dur) {
    g.fillStyle = v('--muted'); g.font = '12px system-ui';
    g.fillText(c.missing ? 'Not downloaded: run music/fetch.sh' : 'Loading waveform…', 10, h / 2 + 4); return;
  }
  const s = segOf(c), a = startOf(c), b = endOf(c), X = (t) => (t / c.dur) * w;
  if (s) { g.fillStyle = v('--sel'); g.fillRect(X(a), 0, X(b) - X(a), h); }
  if (c.peaks) {
    const n = c.peaks.length;
    for (let i = 0; i < n; i++) {
      const t = (i / n) * c.dur, x = (i / n) * w, ph = Math.max(1, c.peaks[i] * (h - 6));
      g.fillStyle = s && t >= a && t <= b ? v('--wave-sel') : v('--wave');
      g.fillRect(x, (h - ph) / 2, Math.max(1, w / n - 0.5), ph);
    }
  }
  if (s) {
    g.fillStyle = v('--accent');
    g.fillRect(X(a) - 1.5, 0, 3, h);
    if (s.end != null) g.fillRect(X(b) - 1.5, 0, 3, h);
  }
  const t = cur === c ? audio.currentTime : c.pos;
  if (t > 0) { g.fillStyle = v('--head'); g.fillRect(X(t) - 0.5, 0, 1, h); }
}

// Waveforms: decode one track at a time, at a low sample rate (enough for an envelope).
const queue = [];
let busy = false;
async function pump() {
  if (busy || !queue.length) return;
  busy = true;
  const c = queue.shift();
  try {
    const r = await fetch(c.track.url);
    if (!r.ok) throw new Error(String(r.status));
    const buf = await new OfflineAudioContext(1, 1, 3000).decodeAudioData(await r.arrayBuffer());
    const data = buf.getChannelData(0), n = 400, step = Math.floor(data.length / n), peaks = new Float32Array(n);
    let max = 0;
    for (let i = 0; i < n; i++) {
      let p = 0;
      for (let j = i * step, e = j + step; j < e; j++) { const x = Math.abs(data[j]); if (x > p) p = x; }
      peaks[i] = p; if (p > max) max = p;
    }
    if (max > 0) for (let i = 0; i < n; i++) peaks[i] /= max;
    c.peaks = peaks; c.dur = buf.duration;
  } catch { c.missing = true; }
  update(c);
  busy = false; pump();
}

function tick() {
  if (cur && !audio.paused) {
    const c = cur, s = segOf(c), t = audio.currentTime, end = endOf(c);
    if (gameMode) {
      const a = startOf(c);
      if (t < a + FADE_IN) audio.volume = Math.max(0, Math.min(1, (t - a) / FADE_IN));
      else if (s && end && t > end - FADE_OUT) audio.volume = Math.max(0, (end - t) / FADE_OUT);
      else audio.volume = 1;
    }
    if (s && end && t >= end) {
      if (c.loop) playFrom(c, startOf(c), gameMode); else { audio.pause(); audio.volume = 1; }
    }
    c.pos = t; update(c);
  }
  requestAnimationFrame(tick);
}

audio.addEventListener('play', () => cur && update(cur));
audio.addEventListener('pause', () => cur && update(cur));

$('#export').addEventListener('click', () => {
  const a = document.createElement('a');
  a.href = URL.createObjectURL(new Blob([JSON.stringify(segs, null, 1)], { type: 'application/json' }));
  a.download = 'segments.json'; a.click();
});
window.addEventListener('resize', () => cards.forEach(draw));

(async () => {
  try {
    const [tracks, s] = await Promise.all([loadTracks(), loadSegments()]);
    segs = s;
    const list = $('#list');
    cards = tracks.map(makeCard);
    cards.forEach((c) => { list.append(c.el); update(c); });
    const io = new IntersectionObserver((es) => es.forEach((e) => {
      if (!e.isIntersecting) return;
      const c = cards.find((k) => k.el === e.target);
      io.unobserve(e.target); queue.push(c); pump();
    }), { rootMargin: '300px' });
    cards.forEach((c) => io.observe(c.el));
    const n = Object.keys(segs).length;
    setStatus(n ? `${n} sections picked` : 'No sections yet', !!n);
    window.__musicPage = { cards, segs: () => segs };
  } catch (e) {
    setStatus('Music not found: serve the repo with the bridge and run music/fetch.sh');
  }
  requestAnimationFrame(tick);
})();
