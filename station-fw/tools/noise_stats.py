#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["pyserial>=3.5"]
# ///
"""Idle-noise statistics for the lag-0 correlation check (bead 32t).

Needs firmware built with STATION_DOA_GATE_DBFS=-200 so every frame is analysed.

    uv run tools/noise_stats.py --seconds 60 --label baseline          # live from the port
    uv run tools/noise_stats.py --file cap.bin --label baseline        # from a saved capture

Prints, over the Direction frames: how many, the share with confidence >= 10, the share that is
both confident and at lag ~0 (|lag| < 0.5 sample), median RMS, and the lag histogram. Compare runs.
"""

from __future__ import annotations

import argparse
import statistics
import time
from collections import Counter

import station_monitor as sm


def frames(args):
    src = sm.file_source(args.file) if args.file else sm.serial_source(args.port or sm.pick_port(), args.baud, None)
    t0 = time.monotonic()
    for chunk in sm.chunks(src):
        if not args.file and time.monotonic() - t0 > args.seconds:
            return
        try:
            _, msg = sm.decode_frame(chunk)
        except Exception:
            continue
        if type(msg).__name__ == "Direction":
            yield msg


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("-p", "--port")
    ap.add_argument("-b", "--baud", type=int, default=115200)
    ap.add_argument("--file")
    ap.add_argument("--seconds", type=float, default=60.0)
    ap.add_argument("--label", default="run")
    ap.add_argument("--min-conf", type=float, default=10.0)
    ap.add_argument("--skip", type=int, default=20, help="drop the first N frames (boot transient)")
    args = ap.parse_args()

    d = list(frames(args))[args.skip :]
    if not d:
        print("no Direction frames (gate not off, or nothing received)")
        return
    conf = [f.confidence for f in d]
    lag = [f.lag_samples for f in d]
    rms = [f.rms_dbfs for f in d]
    n = len(d)
    hi = [f for f in d if f.confidence >= args.min_conf]
    z = [f for f in hi if abs(f.lag_samples) < 0.5]
    print(f"[{args.label}] frames={n}  rms median {statistics.median(rms):.1f} dBFS (min {min(rms):.1f}, max {max(rms):.1f})")
    print(f"  confidence >= {args.min_conf:g}: {len(hi)} ({100 * len(hi) / n:.0f}%)   of those at lag~0: {len(z)} ({100 * len(z) / n:.0f}% of all)")
    print(f"  confidence median {statistics.median(conf):.1f}  p90 {sorted(conf)[int(0.9 * n)]:.1f}  max {max(conf):.0f}")
    h = Counter(round(x) for x in lag)
    print("  lag histogram (samples): " + "  ".join(f"{k:+d}:{v}" for k, v in sorted(h.items(), key=lambda kv: -kv[1])[:8]))


if __name__ == "__main__":
    main()
