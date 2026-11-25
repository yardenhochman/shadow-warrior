#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11,<3.14"
# dependencies = ["numpy>=1.26", "ai-edge-litert>=1.0"]
# ///
"""Proof-check the DSP shout stack with YAMNet (offline, never part of the game).

    uv run tools/yamnet_check.py brain-rs/clips/dual/<rec>/ --model yamnet.tflite --classmap yamnet_class_map.csv
                                 [--wav station_1.wav|bomge.wav] [--labels labels_auto.json] [--offset 0.0]

Runs YAMNet (0.975 s windows, hop 0.48 s) over the recording and, for each labelled event, reports the
strongest "shout-like" class score (Shout, Yell, Screaming, Bellow, Battle cry, Whoop, Children shouting)
in the windows that overlap the event, next to the scores of Speech, Clapping, Knock and Music. Then it
compares event groups (SHOUT vs the rest): if YAMNet separates them clearly better than the DSP features
do, it is worth a second gate. The labels are in STATION time (labels_auto.json from label_events.py);
for bomge.wav pass --offset 0.92 (bomge time = station time + offset).

Model: the TF Hub lite YAMNet classification model (yamnet.tflite, float32, input 15600 samples at 16 kHz);
class map: yamnet_class_map.csv (index,mid,display_name) from tensorflow/models.
"""
from __future__ import annotations

import argparse
import csv
import json
import sys
import wave
from pathlib import Path

import numpy as np

WIN = 15600  # samples, 0.975 s
HOP = 7680   # 0.48 s
SHOUTY = ["Shout", "Yell", "Screaming", "Bellow", "Battle cry", "Whoop", "Children shouting"]
COMPARE = ["Speech", "Clapping", "Knock", "Music", "Hands", "Thump, thud", "Silence"]


def read_wav(path: Path) -> tuple[np.ndarray, int]:
    with wave.open(str(path)) as w:
        sr, ch, n = w.getframerate(), w.getnchannels(), w.getnframes()
        x = np.frombuffer(w.readframes(n), dtype="<i2").astype(np.float32) / 32768.0
    if ch > 1:
        x = x.reshape(-1, ch).mean(axis=1)
    return x, sr


def load_classes(path: Path) -> list[str]:
    with path.open() as f:
        rows = list(csv.reader(f))
    return [r[2] for r in rows[1:]]


def run_yamnet(model: Path, x: np.ndarray):
    from ai_edge_litert.interpreter import Interpreter

    it = Interpreter(model_path=str(model))
    inp = it.get_input_details()[0]
    it.resize_tensor_input(inp["index"], [WIN]) if tuple(inp["shape"]) != (WIN,) else None
    it.allocate_tensors()
    inp = it.get_input_details()[0]
    out = it.get_output_details()[0]
    starts, scores = [], []
    for s in range(0, max(1, len(x) - WIN), HOP):
        w = x[s: s + WIN]
        if len(w) < WIN:
            w = np.pad(w, (0, WIN - len(w)))
        it.set_tensor(inp["index"], w.astype(np.float32))
        it.invoke()
        scores.append(np.array(it.get_tensor(out["index"])).reshape(-1))
        starts.append(s / 16000.0)
    return np.array(starts), np.array(scores)


def auc(pos: list[float], neg: list[float]) -> float:
    if not pos or not neg:
        return float("nan")
    p, n = np.array(pos), np.array(neg)
    return float(((p[:, None] > n[None, :]).mean() + 0.5 * (p[:, None] == n[None, :]).mean()))


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("rec", type=Path)
    ap.add_argument("--model", type=Path, required=True)
    ap.add_argument("--classmap", type=Path, required=True)
    ap.add_argument("--wav", default="station_1.wav")
    ap.add_argument("--labels", default="labels_auto.json")
    ap.add_argument("--offset", type=float, default=0.0, help="added to label times (use 0.92 with bomge.wav)")
    ap.add_argument("--normalize", action="store_true", help="scale the whole file so its 99.9th percentile is 0.5")
    a = ap.parse_args()

    classes = load_classes(a.classmap)
    idx = {n: i for i, n in enumerate(classes)}
    shouty = [idx[n] for n in SHOUTY if n in idx]
    compare = {n: idx[n] for n in COMPARE if n in idx}
    x, sr = read_wav(a.rec / a.wav)
    if sr != 16000:
        sys.exit(f"need 16 kHz audio, got {sr}")
    if a.normalize:
        # recordings are quiet (shout peaks about -40 dBFS); YAMNet expects normal-loudness audio
        ref = float(np.percentile(np.abs(x), 99.9))
        x = np.clip(x * (0.5 / max(ref, 1e-4)), -1.0, 1.0)
        print(f"normalised: gain {20 * np.log10(0.5 / max(ref, 1e-4)):.1f} dB")
    starts, sc = run_yamnet(a.model, x)
    shout_score = sc[:, shouty].max(axis=1)
    labels = json.loads((a.rec / a.labels).read_text())["cues"]

    print(f"{len(x) / sr:.0f} s, {len(starts)} windows, shouty classes: {[classes[i] for i in shouty]}\n")
    print(f"{'#':>3} {'kind':<10} {'t':>7} {'dur':>5} {'shouty':>6}  " + " ".join(f"{n[:7]:>7}" for n in compare))
    groups: dict[str, list[float]] = {}
    for k, l in enumerate(labels):
        t0 = l["t"] + a.offset
        t1 = t0 + max(l.get("dur", 0.3), 0.3)
        # windows overlapping the event by at least half of the event
        sel = [i for i, s in enumerate(starts) if s < t1 and s + WIN / 16000 > t0 + 0.1]
        if not sel:
            continue
        best = float(shout_score[sel].max())
        row = {n: float(sc[sel, i].max()) for n, i in compare.items()}
        groups.setdefault(l["kind"], []).append(best)
        print(f"{k:>3} {l['kind']:<10} {t0:7.2f} {l.get('dur', 0):5.2f} {best:6.2f}  " + " ".join(f"{row[n]:7.2f}" for n in compare))

    print("\nshouty score per group (max over overlapping windows):")
    for g, v in sorted(groups.items()):
        print(f"  {g:<10} n={len(v):<3} mean {np.mean(v):.2f}  median {np.median(v):.2f}  min {np.min(v):.2f}  max {np.max(v):.2f}")
    pos = groups.get("SHOUT", [])
    neg = [s for g, v in groups.items() if g not in ("SHOUT", "SHOUT_WEAK") for s in v]
    print(f"\nSHOUT vs (CLAP, KNOCK, SOFT, ...) separation: AUC {auc(pos, neg):.2f}  (1.0 perfect, 0.5 none)")
    for thr in (0.1, 0.2, 0.3, 0.5):
        tp = sum(s >= thr for s in pos)
        fp = sum(s >= thr for s in neg)
        print(f"  threshold {thr:.1f}: shouts kept {tp}/{len(pos)}, others passed {fp}/{len(neg)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
