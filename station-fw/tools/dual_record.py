#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# ///
"""Record the sensor station (serial raw audio) and the BOMGE USB mic together, with a guided routine.

    uv run tools/dual_record.py [--port /dev/cu.usbserial-0001] [--mic BOMGE] [--label tune1]

Both recorders start together on this laptop, so they share one clock. The routine prints cues
(SHOUT / CLAP / TALK / KNOCK / QUIET / MOVE) with their time offsets and stores them in
cues.json next to the WAVs, as rough labels: refine onsets from the BOMGE signal afterwards.
The routine starts with silence, then 3 claps (clock alignment between the two recordings).

Output: brain-rs/clips/dual/<label>_<time>/{station_1.wav (stereo 16 kHz), bomge.wav (mono 16 kHz),
cues.json, station_1.json}. The station needs the 921600-baud firmware (STATION_BAUD=921600 just flash).
"""
from __future__ import annotations

import argparse
import datetime as dt
import json
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent

# (kind, how many, seconds between cues, prompt)
ROUTINE = [
    ("QUIET", 1, 10, "stay quiet: room noise floor"),
    ("CLAP", 3, 1.5, "three claps near the mics: clock alignment"),
    ("QUIET", 1, 4, ""),
    ("SHOUT", 8, 4.0, "NEAR (0.5 m): shout like you are angry, about 1 s"),
    ("MOVE", 1, 10, "move to about 1.5 m"),
    ("SHOUT", 8, 4.0, "MID (1.5 m)"),
    ("MOVE", 1, 10, "move to about 3 m"),
    ("SHOUT", 8, 4.0, "FAR (3 m)"),
    ("MOVE", 1, 8, "back to about 1 m"),
    ("SOFT", 6, 4.0, "loud talking, NOT a shout"),
    ("CLAP", 6, 3.0, "claps (should not count)"),
    ("KNOCK", 6, 3.0, "knock on the table (should not count)"),
    ("SHOUT", 6, 3.0, "rapid shouts, 3 s apart, 1 m"),
    ("QUIET", 1, 6, "quiet again"),
]


def find_mic_index(name: str) -> str:
    out = subprocess.run(["ffmpeg", "-hide_banner", "-f", "avfoundation", "-list_devices", "true", "-i", ""],
                         capture_output=True, text=True).stderr
    audio = False
    for line in out.splitlines():
        if "AVFoundation audio devices" in line:
            audio = True
            continue
        if audio and name.lower() in line.lower():
            return line.split("[")[2].split("]")[0]
    sys.exit(f"audio input matching {name!r} not found:\n{out}")


def total_seconds() -> float:
    return sum(n * gap for _, n, gap, _ in ROUTINE) + 5


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--port", default="/dev/cu.usbserial-0001")
    ap.add_argument("--baud", default="921600")
    ap.add_argument("--mic", default="BOMGE", help="substring of the macOS audio input name")
    ap.add_argument("--label", default="session")
    a = ap.parse_args()

    stamp = dt.datetime.now().strftime("%Y%m%dT%H%M%S")
    out = REPO / "brain-rs" / "clips" / "dual" / f"{a.label}_{stamp}"
    out.mkdir(parents=True, exist_ok=True)
    mic = find_mic_index(a.mic)
    secs = total_seconds()
    print(f"-> {out}\nroutine {secs / 60:.1f} min; mic index {mic}. Start in 5 s.\n")

    st_name = "station"
    station = subprocess.Popen(
        ["uv", "run", str(HERE / "station_record.py"), "--port", a.port, "--baud", a.baud,
         "-t", str(int(secs)), "-o", str(out), st_name],
        stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    ff = subprocess.Popen(
        ["ffmpeg", "-y", "-loglevel", "error", "-f", "avfoundation", "-i", f":{mic}", "-ar", "16000", "-ac", "1",
         "-t", str(int(secs)), str(out / "bomge.wav")], stdin=subprocess.DEVNULL)
    time.sleep(5)  # both recorders warm up; offsets below are relative to this moment

    t0 = time.monotonic()
    cues = []
    for kind, n, gap, prompt in ROUTINE:
        if prompt:
            print(f"\n=== {prompt}")
        for i in range(n):
            t = time.monotonic() - t0
            cues.append({"kind": kind, "index": i + 1, "of": n, "t": round(t, 2), "prompt": prompt})
            if kind not in ("QUIET", "MOVE"):
                print(f"  {kind} {i + 1}/{n}  (t={t:5.1f}s)\a", flush=True)
            time.sleep(gap)
    (out / "cues.json").write_text(json.dumps({"label": a.label, "start_offset_sec": 5, "cues": cues}, indent=1))
    print("\nroutine done, finishing the recordings")
    ff.wait(timeout=30)
    text = station.communicate(timeout=60)[0]
    print(text[-600:])
    print(f"saved in {out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
