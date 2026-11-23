#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.10"
# dependencies = []
# ///
"""Match shouts found by the brain's detector with the radar distance and the mic-array angle.

Record with the radar powered and facing the person:

    cd station-fw && just record shout_walk         # writes <name>.wav and <name>.events.jsonl
    uv run tools/shout_vs_radar.py ../brain-rs/clips/station/shout_walk_1.wav

For each shout event (detections less than --gap s apart) it prints the detector margin over
the noise floor, the radar moving/static distance around that moment, and the median
direction angle from the mic array. Then it groups the shouts by radar distance.
"""

from __future__ import annotations

import argparse
import json
import statistics as st
import subprocess
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]


def run_eval(wav: Path) -> list[dict]:
    out = Path(tempfile.mkdtemp()) / "eval.json"
    subprocess.run(["just", "eval", str(wav), "--json", str(out)], cwd=REPO / "brain-rs", check=True,
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    reports = json.loads(out.read_text())[0]["reports"]
    return reports[2]["detections"] if len(reports) >= 3 else reports[-1]["detections"]  # [left, right, mix]


def events_of(path: Path) -> list[dict]:
    return [json.loads(l) for l in path.read_text().splitlines() if l.strip()]


def cluster(dets: list[dict], gap: float) -> list[list[dict]]:
    out: list[list[dict]] = []
    for d in dets:
        if out and d["t"] - out[-1][-1]["t"] < gap:
            out[-1].append(d)
        else:
            out.append([d])
    return out


def med(xs):
    return round(st.median(xs), 1) if xs else None


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("wav", type=Path)
    ap.add_argument("--events", type=Path, help="events.jsonl (default: next to the wav)")
    ap.add_argument("--gap", type=float, default=1.0, help="seconds apart that make a new shout (default 1)")
    ap.add_argument("--window", type=float, default=1.0, help="seconds either side of a shout to read the radar")
    ap.add_argument("--min-conf", type=float, default=10.0)
    a = ap.parse_args()

    ev = events_of(a.events or a.wav.with_suffix(".events.jsonl"))
    radar = [e for e in ev if e["type"] == "radar"]
    direc = [e for e in ev if e["type"] == "direction" and e["confidence"] >= a.min_conf]
    if not radar:
        print("no radar frames in the events file: is the radar powered and wired?")
    shouts = cluster(run_eval(a.wav), a.gap)
    print(f"{a.wav.name}: {len(shouts)} shout events, {len(radar)} radar frames, {len(direc)} confident direction frames\n")
    print(" #   t(s)  n  margin  level | radar moving/static cm  present | angle deg (n)")
    rows = []
    for i, s in enumerate(shouts, 1):
        t0, t1 = s[0]["t"] - a.window, s[-1]["t"] + a.window
        r = [e for e in radar if t0 <= e["t"] <= t1 and e["present"]]
        mv = med([e["moving_dist_cm"] for e in r if e["moving_energy"] > 0])
        sd = med([e["static_dist_cm"] for e in r if e["static_energy"] > 0])
        d = [e for e in direc if s[0]["t"] - 0.2 <= e["t"] <= s[-1]["t"] + 0.5]
        ang = med([e["angle_deg"] for e in d])
        margin = max(x["db_above_floor"] for x in s)
        level = max(x["level_db"] for x in s)
        dist = sd if sd is not None else mv
        rows.append((dist, margin))
        print(f"{i:2d} {s[0]['t']:6.1f} {len(s):2d} {margin:6.1f} {level:6.1f} | {str(mv):>8}/{str(sd):<8} {len(r):4d}f    | {ang} ({len(d)})")

    print("\nby radar distance (static, else moving), 50 cm bins:")
    bins: dict[int, list[float]] = {}
    for dist, m in rows:
        if dist is not None:
            bins.setdefault(int(dist // 50) * 50, []).append(m)
    for b in sorted(bins):
        print(f"  {b:4d}-{b + 49:<4d} cm: {len(bins[b]):2d} shouts, margin median {st.median(bins[b]):5.1f} dB, min {min(bins[b]):5.1f}")


if __name__ == "__main__":
    main()
