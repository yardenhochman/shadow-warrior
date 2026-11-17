#!/usr/bin/env python3
# /// script
# requires-python = ">=3.9"
# ///
"""Normalise a folder of tracks to one loudness (EBU R128, two-pass loudnorm).

    uv run deploy/normalize_music.py ~/shadow-warrior-music [--lufs -14] [--tp -1.0] [--format flac|m4a]

Reads every .m4a/.mp3/.flac/.wav/.ogg/.opus/.webm in SRC and writes 16-bit FLAC (default; no second
lossy encode, works on any ffmpeg) or AAC .m4a (`--format m4a`, needs macOS's aac_at) to
SRC/normalized/ (originals untouched). Pass 1 measures, pass 2 applies a plain gain where the true-peak
limit allows (no dynamic compression; loudnorm limits only when it must), then every output is re-measured and a before/after
table is printed. The brain plays whatever is in its music dir at the level it finds it, so
this is the only place loudness is evened out.
"""
import argparse
import json
import pathlib
import re
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor

EXTS = {".m4a", ".mp3", ".flac", ".wav", ".ogg", ".opus", ".webm"}


def ffmpeg(args):
    return subprocess.run(["ffmpeg", "-hide_banner", "-nostats", *args], capture_output=True, text=True)


def measure(path: pathlib.Path, i: float, tp: float) -> dict:
    """loudnorm pass 1: measured input loudness as a dict of strings."""
    r = ffmpeg(["-i", str(path), "-af", f"loudnorm=I={i}:TP={tp}:LRA=11:print_format=json", "-f", "null", "-"])
    m = re.search(r"\{[^{}]*\"input_i\"[^{}]*\}", r.stderr, re.S)
    if not m:
        raise RuntimeError(f"cannot measure {path.name}: {r.stderr[-300:]}")
    return json.loads(m.group(0))


def integrated(path: pathlib.Path) -> tuple[float, float]:
    """(integrated LUFS, true peak dBTP) of a file, via ebur128."""
    r = ffmpeg(["-i", str(path), "-af", "ebur128=peak=true", "-f", "null", "-"])
    lufs = re.findall(r"\bI:\s+(-?[\d.]+) LUFS", r.stderr)
    peak = re.findall(r"\bPeak:\s+(-?[\d.]+) dBFS", r.stderr)
    return float(lufs[-1]), float(peak[-1])


def normalise(src: pathlib.Path, dst: pathlib.Path, i: float, tp: float, bitrate: str, fmt: str) -> None:
    m = measure(src, i, tp)
    gain = i - float(m["input_i"])
    if float(m["input_tp"]) + gain <= tp:
        # Plain gain: no limiter needed, so dynamics are untouched. (loudnorm's linear
        # mode did not reliably keep the peak under the ceiling for very peaky tracks.)
        flt = f"volume={gain:.3f}dB"
    else:
        # The gain would push the peak over the ceiling: let loudnorm limit it.
        flt = (
            f"loudnorm=I={i}:TP={tp}:LRA=11:measured_I={m['input_i']}:measured_TP={m['input_tp']}"
            f":measured_LRA={m['input_lra']}:measured_thresh={m['input_thresh']}:offset={m['target_offset']}:linear=true"
        )
    if fmt == "flac":
        codec = ["-c:a", "flac", "-sample_fmt", "s16"]
    else:
        codec = ["-ar", "44100", "-c:a", "aac_at", "-b:a", bitrate, "-movflags", "+faststart"]
    r = ffmpeg(["-y", "-i", str(src), "-vn", "-map_metadata", "0", "-af", flt, *codec, str(dst)])
    if r.returncode != 0:
        raise RuntimeError(f"encode failed for {src.name}: {r.stderr[-300:]}")


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("src", type=pathlib.Path)
    ap.add_argument("--lufs", type=float, default=-14.0, help="target integrated loudness (default -14)")
    ap.add_argument("--tp", type=float, default=-1.0, help="true-peak ceiling in dBTP (default -1.0)")
    ap.add_argument("--format", choices=["flac", "m4a"], default="flac",
                    help="output: 16-bit FLAC (default) or AAC m4a (macOS aac_at only)")
    ap.add_argument("--bitrate", default="192k", help="AAC bitrate for --format m4a")
    ap.add_argument("--jobs", type=int, default=4)
    a = ap.parse_args()

    files = sorted(p for p in a.src.iterdir() if p.suffix.lower() in EXTS and p.is_file())
    if not files:
        sys.exit(f"no audio files in {a.src}")
    out = a.src / "normalized"
    out.mkdir(exist_ok=True)

    def work(p: pathlib.Path):
        dst = out / (p.stem + "." + a.format)
        before = integrated(p)
        normalise(p, dst, a.lufs, a.tp, a.bitrate, a.format)
        return p, before, integrated(dst)

    with ThreadPoolExecutor(a.jobs) as ex:
        rows = list(ex.map(work, files))

    print(f"{'track':<40} {'before LUFS':>11} {'peak':>6} {'after LUFS':>11} {'peak':>6}")
    for p, (b, bp), (n, np_) in rows:
        print(f"{p.name[:40]:<40} {b:>11.1f} {bp:>6.1f} {n:>11.1f} {np_:>6.1f}")
    after = [n for _, _, (n, _) in rows]
    before = [b for _, (b, _), _ in rows]
    print(f"spread before: {max(before) - min(before):.1f} LU, after: {max(after) - min(after):.1f} LU "
          f"(target {a.lufs} LUFS, ceiling {a.tp} dBTP); max peak after: {max(pk for _, _, (_, pk) in rows):.1f} dBFS")


main()
