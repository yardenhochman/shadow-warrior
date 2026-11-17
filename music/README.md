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

## Adding a track

Add a line to `playlist.tsv` (tab separated), run `fetch.sh`, and copy the new
file to the brain. Your own audio files without a YouTube source can go straight
into the brain's music folder. They play, but `fetch.sh` will not know them.

Missing as of November 2026: one link (`HsLOxbLZMy8`) is gone from YouTube, and
five MP3s sent on WhatsApp in November 2025 could not be recovered.
