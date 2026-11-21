#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.9"
# ///
"""Record a raw mono WAV (no AGC / noise suppression) for detector tuning. Needs ffmpeg.

    uv run deploy/record_clip.py yael            # -> clips/yael_1.wav, next run yael_2.wav
    uv run deploy/record_clip.py yael --list     # list input devices
    uv run deploy/record_clip.py yael -d 1       # pick input device (macOS index / ALSA name)

Enter (or Ctrl-C) stops. Prints duration, peak and clipped-sample count afterwards.
"""
import argparse
import array
import re
import signal
import subprocess
import sys
import wave
from pathlib import Path

MAC = sys.platform == "darwin"


def next_path(out_dir: Path, label: str) -> Path:
    label = re.sub(r"[^A-Za-z0-9_-]+", "_", label.strip()) or "clip"
    n = 1 + max((int(m.group(1)) for p in out_dir.glob(f"{label}_*.wav")
                 if (m := re.fullmatch(rf"{re.escape(label)}_(\d+)\.wav", p.name))), default=0)
    return out_dir / f"{label}_{n}.wav"


def report(path: Path) -> None:
    with wave.open(str(path)) as w:
        sr, ch, n = w.getframerate(), w.getnchannels(), w.getnframes()
        s = array.array("h", w.readframes(n))
    peak = max((abs(v) for v in s), default=0)
    clipped = sum(1 for v in s if abs(v) >= 32767)
    db = 20 * __import__("math").log10(max(peak, 1) / 32768)
    print(f"{path}: {n / sr:.1f}s {sr}Hz ch={ch} peak={db:.1f} dB clipped={clipped}")
    if clipped:
        print("  clipping: move mic back / lower input gain")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawTextHelpFormatter)
    ap.add_argument("label", nargs="?", default="clip")
    ap.add_argument("-d", "--device", default="1" if MAC else "default",
                    help="macOS avfoundation audio index (default 1) / ALSA device (default 'default')")
    ap.add_argument("-r", "--rate", type=int, default=48000)
    ap.add_argument("-o", "--out-dir", type=Path, default=Path(__file__).resolve().parent.parent / "clips")
    ap.add_argument("--list", action="store_true", help="list input devices and exit")
    a = ap.parse_args()

    if a.list:
        cmd = ["ffmpeg", "-hide_banner", "-f", "avfoundation", "-list_devices", "true", "-i", ""] if MAC \
            else ["arecord", "-l"]
        subprocess.run(cmd)
        return 0

    a.out_dir.mkdir(parents=True, exist_ok=True)
    path = next_path(a.out_dir, a.label)
    src = ["-f", "avfoundation", "-i", f":{a.device}"] if MAC else ["-f", "alsa", "-i", a.device]
    cmd = ["ffmpeg", "-hide_banner", "-loglevel", "error", "-y", *src,
           "-ac", "1", "-ar", str(a.rate), "-c:a", "pcm_s16le", str(path)]
    proc = subprocess.Popen(cmd, stdin=subprocess.DEVNULL)
    print(f"recording -> {path}  (Enter to stop)")
    try:
        input()
    except (KeyboardInterrupt, EOFError):
        pass
    proc.send_signal(signal.SIGINT)  # lets ffmpeg finalize the WAV header
    proc.wait()
    if not path.exists() or path.stat().st_size < 100:
        print("nothing recorded (mic permission? wrong device? try --list)", file=sys.stderr)
        return 1
    report(path)
    return 0


if __name__ == "__main__":
    sys.exit(main())
