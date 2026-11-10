// Fight playlist player: stands in for brain-rs music_manager (plays a folder of
// tracks during FIGHT). Tracks come from ../../music/playlist.tsv when the repo
// root is served locally (after music/fetch.sh), or from files the user picks.
// Each fight starts the next track (proposal; brain-rs restarts from track 1).
export class Playlist {
  constructor(sfx) {
    this.sfx = sfx; this.tracks = []; this.idx = 0; this.playing = false; this.node = null; this.fade = null;
    this.source = 'none'; this.onchange = () => {};
    this.audio = new Audio(); this.audio.preload = 'auto';
    this.audio.addEventListener('ended', () => this.skip());
    this.audio.addEventListener('error', () => { if (this.playing && this.tracks.length > 1) this.skip(); });
  }
  get available() { return this.tracks.length > 0; }
  get current() { return this.tracks[this.idx] || null; }

  async loadDefault(base = '../../music/') {
    try {
      const r = await fetch(base + 'playlist.tsv', { cache: 'no-store' });
      if (!r.ok) return false;
      const rows = (await r.text()).split('\n').filter((l) => l.trim() && !l.startsWith('#')).map((l) => l.split('\t'));
      const tracks = rows.filter((c) => c.length >= 3).map(([, file, title]) => ({ title: title.trim(), url: base + file.trim() }));
      if (!tracks.length) return false;
      const probe = await fetch(tracks[0].url, { method: 'HEAD' });
      if (!probe.ok) return false;
      this.tracks = tracks; this.idx = 0; this.source = 'repo'; this.onchange(); return true;
    } catch { return false; }
  }

  loadFiles(files) {
    const audio = [...files].filter((f) => f.type.startsWith('audio/') || /\.(mp3|ogg|opus|flac|wav|m4a)$/i.test(f.name))
      .sort((a, b) => a.name.localeCompare(b.name));
    if (!audio.length) return 0;
    this.stop(true);
    if (this.source === 'files') this.tracks.forEach((t) => URL.revokeObjectURL(t.url));
    this.tracks = audio.map((f) => ({ title: f.name.replace(/\.[^.]+$/, '').replace(/^\d+[-_ ]*/, '').replace(/[-_]+/g, ' '), url: URL.createObjectURL(f) }));
    this.idx = 0; this.source = 'files'; this.onchange(); return audio.length;
  }

  attach() {
    const ctx = this.sfx.ensure();
    if (!this.node) { this.node = ctx.createMediaElementSource(this.audio); this.node.connect(this.sfx.master); }
  }

  play() {
    if (!this.available) return false;
    this.attach(); clearInterval(this.fade);
    this.audio.src = this.current.url; this.audio.volume = 1;
    this.audio.play().catch(() => {});
    this.playing = true; this.onchange(); return true;
  }

  skip() {
    if (!this.available) return;
    this.idx = (this.idx + 1) % this.tracks.length;
    if (this.playing) this.play(); else this.onchange();
  }

  stop(now = false) {
    if (!this.playing) return;
    this.playing = false; this.idx = (this.idx + 1) % this.tracks.length; this.onchange();
    clearInterval(this.fade);
    if (now) { this.audio.pause(); return; }
    this.fade = setInterval(() => {
      this.audio.volume = Math.max(0, this.audio.volume - 0.08);
      if (this.audio.volume <= 0) { clearInterval(this.fade); this.audio.pause(); }
    }, 60);
  }
}
