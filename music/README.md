# Fight music

The FIGHT state plays this playlist. `playlist.tsv` lists the 18 tracks Yarden
sent Avishai in November 2025 (YouTube id, file name, title). The audio is not
committed: the repo is public and the tracks are copyrighted. `music/*.mp3` is
gitignored.

## Get the files (once per machine, needs internet)

```sh
music/fetch.sh              # into music/ (about 115 MB of MP3)
music/fetch.sh /some/dir    # or into another folder
```

Needs `yt-dlp` and `ffmpeg`. Files already present are skipped, so re-running
only fetches what is missing.

## Play offline

After the download nothing needs the network.

- **brain-rs (the installation):** it plays every `mp3`/`flac`/`wav` in
  `music.playlist_dir` (`config.yaml`, default `./music`, relative to where
  brain-rs runs, which is gitignored). Copy the files there, by USB stick or
  `scp music/*.mp3 <brain>:<brain-rs dir>/music/`, or set `playlist_dir` to this
  folder. `config.yaml` has `shuffle: true`; with `shuffle: false` the order is
  whatever the file system lists (brain-rs does not sort), not the `01-` prefixes.
- **Simulator, local:** serve the repo root (`python3 -m http.server`) and open
  `http://localhost:8000/simulator/web/`. It reads `playlist.tsv` and plays these
  files.
- **Simulator, GitHub Pages or phone:** tap **Load music** and pick the MP3s.
  The browser keeps them, so later visits play them without loading again.

## Pick the part of each track that plays

Open `simulator/web/music.html` (with the live bridge: `http://<host>:<port>/simulator/web/music.html`).
Each track shows its waveform: click to listen, mark **Start here** / **End here** or drag the
orange edges, and **Play as in game** plays the section with the game's fades. The simulator then
starts each track at its section (fade in), and at the section's end fades out and goes to the
next track's section. A track without a section plays from the beginning.

Picks are saved in the browser and, through the bridge, in `music/segments.json` (gitignored;
`{"file.mp3": {"start": s, "end": s|null}}`). **Download picks** saves the same file.
brain-rs does not read it yet.

## Adding a track

Add a line to `playlist.tsv` (tab separated), run `fetch.sh`, and copy the new
file to the brain. Your own audio files without a YouTube source can go straight
into the brain's music folder. They play, but `fetch.sh` will not know them.

Missing as of November 2026: one link (`HsLOxbLZMy8`) is gone from YouTube, and
five MP3s sent on WhatsApp in November 2025 could not be recovered.
