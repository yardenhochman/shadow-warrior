#!/usr/bin/env bash
# Download the fight playlist (music/playlist.tsv) as MP3s into this folder, loudness-
# normalised to -14 LUFS (originals kept in raw/) so tracks don't jump in volume.
# Audio is not committed (copyright); run this locally on each machine.
# Used by the browser simulator (serve the repo root) and by brain-rs
# (copy or point music.playlist_dir at this folder). Needs yt-dlp, ffmpeg and uv.
set -euo pipefail
dir="$(cd "$(dirname "$0")" && pwd)"
out="${1:-$dir}"
raw="$out/raw"
mkdir -p "$raw"
grep -v '^#' "$dir/playlist.tsv" | while IFS=$'\t' read -r id file title; do
  [ -z "$id" ] && continue
  # older fetches kept the unnormalised download here: move it to raw/ instead of re-downloading
  if [ ! -s "$raw/$file" ] && [ -s "$out/$file" ]; then mv "$out/$file" "$raw/$file"; fi
  if [ -s "$raw/$file" ]; then echo "have  $file"; continue; fi
  echo "fetch $file  ($title)"
  yt-dlp -q --no-warnings -x --audio-format mp3 --audio-quality 2 \
    -o "$raw/${file%.mp3}.%(ext)s" -- "$id" || echo "  failed: $title" >&2
done
# Even out the loudness: plain gain to -14 LUFS (limited only where the peak would pass -1 dBTP).
echo "normalising loudness..."
uv run "$dir/../brain-rs/deploy/normalize_music.py" "$raw" --format mp3
mv -f "$raw"/normalized/*.mp3 "$out"/ && rmdir "$raw/normalized"
