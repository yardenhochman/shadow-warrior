#!/usr/bin/env bash
# Download the fight playlist (music/playlist.tsv) as MP3s into this folder.
# Audio is not committed (copyright); run this locally on each machine.
# Used by the browser simulator (serve the repo root) and by brain-rs
# (copy or point music.playlist_dir at this folder). Needs yt-dlp + ffmpeg.
set -euo pipefail
dir="$(cd "$(dirname "$0")" && pwd)"
out="${1:-$dir}"
mkdir -p "$out"
grep -v '^#' "$dir/playlist.tsv" | while IFS=$'\t' read -r id file title; do
  [ -z "$id" ] && continue
  if [ -s "$out/$file" ]; then echo "have  $file"; continue; fi
  echo "fetch $file  ($title)"
  yt-dlp -q --no-warnings -x --audio-format mp3 --audio-quality 2 \
    -o "$out/${file%.mp3}.%(ext)s" -- "$id" || echo "  failed: $title" >&2
done
