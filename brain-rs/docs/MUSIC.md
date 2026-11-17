# Arena music

The brain plays a shuffled playlist from `music.playlist_dir` (default `./music`, i.e.
`/opt/shadow-brain/music` on a station) while a fight is running. It loads the folder at
startup, so restart the service after changing it. Formats are limited by `music.formats`
(`mp3`, `flac`, `wav`, `m4a` by default).

## The playlist

YouTube playlist "Shadow Warrior" (18 tracks, about 1 h 20 min):
<https://www.youtube.com/playlist?list=PLDwQ9Bbo4Ol8>

The video IDs pin the exact tracks, in case the playlist is edited later.

| # | Track | Length | Video ID |
|--:|-------|-------:|----------|
| 01 | Doom OST - Rip & Tear | 4:18 | `vyA1z2A-lhU` |
| 02 | Heavy Violence - Naruto OST 3 | 3:13 | `-qtqXYrit54` |
| 03 | Naruto Soundtrack - The Raising Fighting Spirit | 1:39 | `sNjtroPFQ1s` |
| 04 | Need to Be Strong | 3:07 | `_KMBjIjbqvA` |
| 05 | Hamrer Hippyer | 14:17 | `gGVH0qY-JGw` |
| 06 | Carpenter Brut - Turbo Killer | 3:29 | `wy9r2qeouiQ` |
| 07 | Walk (2010 Remaster) | 5:16 | `XNjKN0_wx3s` |
| 08 | Limp Bizkit - Break Stuff (Official Music Video) | 2:48 | `ZpUYjpKg9KY` |
| 09 | Godsmack - I Stand Alone | 4:42 | `OYjZK_6i37M` |
| 10 | Daft Punk - Derezzed (Tron soundtrack) | 4:11 | `F4eccPBFEjE` |
| 11 | Bjorth - Drums of Midgard | 3:05 | `GClPj0RmU6s` |
| 12 | At Doom's Gate | 1:11 | `Lp-TdtDYGAA` |
| 13 | Cyberpunk 2077 OST - The Rebel Path (Cello Version) | 4:21 | `AGsjA1pXajk` |
| 14 | Rob Zombie - Dragula | 3:48 | `EqQuihD0hoI` |
| 15 | Rage Against The Machine - Killing In The Name | 5:14 | `bWXazVhlyxQ` |
| 16 | Master of Puppets (Remastered) | 8:36 | `E0ozmU9cJDg` |
| 17 | Techno Syndrome (Mortal Kombat) - The Immortals | 4:46 | `EAwWPadFsOA` |
| 18 | Massive Stunner | 1:58 | `2W2sDBrwxW4` |

**Licensing:** these are commercial recordings. Playing them in a public installation can
need a licence (performing-rights organisation); downloading from YouTube generally
breaches its terms. Treat this list as the working set for development and swap in music
you may play publicly before an event.

## Rebuilding the library

On a build machine (macOS here), not on the station; the files are not in git.

1. Download the native AAC audio, no re-encoding (`m4a`; the brain cannot decode the Opus/webm
   streams):
   ```
   yt-dlp -f "bestaudio[acodec^=mp4a]" --restrict-filenames --no-overwrites --ignore-errors \
     -o "%(playlist_index)02d_%(title).80s.%(ext)s" "https://www.youtube.com/playlist?list=PLDwQ9Bbo4Ol8"
   ```
   Files used so far: `~/shadow-warrior-music/` (AAC 44.1 kHz stereo, 128 kbps, 74 MB).
2. Even out the loudness (the raw tracks span -15.1 to -3.4 LUFS and two clip):
   ```
   uv run deploy/normalize_music.py ~/shadow-warrior-music
   ```
   Writes `normalized/*.m4a` at -14 LUFS (spread 0.1 LU, peaks below 0 dBFS, AAC 192k, 111 MB).
3. Copy to the station and restart:
   ```
   rsync -a --delete -e "ssh ..." ~/shadow-warrior-music/normalized/ warrior@<station>:/opt/shadow-brain/music/
   ssh warrior@<station> sudo systemctl restart shadow-brain
   ```

At -14 LUFS the music is quieter than the raw tracks (most were -7 to -10), so raise
`music.volume` and the speaker's own volume as needed.
