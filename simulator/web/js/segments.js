// Music sections: the part of each fight track that plays in FIGHT, picked on music.html.
// Keyed by file name: { "07-pantera-walk.mp3": { start: 62.5, end: 180 } } (seconds; end
// optional = to the end). Kept in this browser (localStorage) and, when the page is served
// by the live bridge, in music/segments.json next to the tracks (gitignored, like the MP3s).
const KEY = 'sw.musicSegments';

export function loadLocal() {
  try { return JSON.parse(localStorage.getItem(KEY) || '{}') || {}; } catch { return {}; }
}

export function saveLocal(segs) {
  try { localStorage.setItem(KEY, JSON.stringify(segs)); return true; } catch { return false; }
}

// Bridge file first, this browser's picks on top (they are the newest edits).
export async function loadSegments() {
  let file = {};
  try {
    const r = await fetch('/live/music-segments', { cache: 'no-store' });
    if (r.ok) file = await r.json();
  } catch {}
  return { ...file, ...loadLocal() };
}

export async function saveToBridge(segs) {
  try {
    const r = await fetch('/live/music-segments', {
      method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(segs),
    });
    return r.ok;
  } catch { return false; }
}

export const fileOf = (url) => decodeURIComponent(url.split('/').pop().split('?')[0]);
