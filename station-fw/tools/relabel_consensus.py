#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# ///
"""Relabel labels_auto.json by model consensus (labels_<model>.json from gemini_label.py). Dev-time only.

    uv run tools/relabel_consensus.py brain-rs/clips/dual/<rec>/ [--models labels_gemini.json labels_qwen_qwen3.8-omni-flash.json]
                                      [--offset 0.92] [--window 1.5]

For every event in labels_auto.json (station time; model events are in reference time = station + offset):
  * every model hears a shout within +-window  -> SHOUT   (also upgrades CLAP/SOFT/UNCUED mislabels)
  * SHOUT/SHOUT_WEAK/UNCUED, not every model agrees -> UNCERTAIN (kept out of the hit/miss score)
  * anything else keeps its kind
Writes labels_v2.json (same cue format, `prev_kind` and `votes` recorded per event) and prints the changes.
"""
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("rec", type=Path)
    ap.add_argument("--models", nargs="+", default=["labels_gemini.json", "labels_qwen_qwen3.8-omni-flash.json"])
    ap.add_argument("--offset", type=float, default=0.92)
    ap.add_argument("--window", type=float, default=1.5)
    ap.add_argument("--out", default="labels_v2.json")
    a = ap.parse_args()

    models = {m: json.loads((a.rec / m).read_text())["events"] for m in a.models}
    doc = json.loads((a.rec / "labels_auto.json").read_text())
    counts: dict[str, int] = {}
    for k, c in enumerate(doc["cues"]):
        t = c["t"] + a.offset
        votes = {m: any(e.get("kind") == "shout" and e["t"] - a.window <= t <= e["t"] + e.get("dur", 0.3) + a.window for e in ev)
                 for m, ev in models.items()}
        n = sum(votes.values())
        prev = c["kind"]
        if n == len(models):
            c["kind"] = "SHOUT"
        elif prev in ("SHOUT", "SHOUT_WEAK", "UNCUED"):
            c["kind"] = "UNCERTAIN"
        c["prev_kind"], c["votes"] = prev, n
        counts[c["kind"]] = counts.get(c["kind"], 0) + 1
        if c["kind"] != prev:
            print(f"{k:>3} t={c['t']:7.2f} {prev:<10} -> {c['kind']:<9} ({n}/{len(models)} models hear a shout)")
    doc["source"] = "relabel_consensus.py"
    (a.rec / a.out).write_text(json.dumps(doc, indent=1))
    print(f"\n{counts} -> {a.out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
