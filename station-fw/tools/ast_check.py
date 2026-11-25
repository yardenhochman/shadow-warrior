#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11,<3.14"
# dependencies = ["numpy>=1.26", "torch>=2.2", "transformers>=4.40"]
# ///
"""Proof-check the DSP shout stack with AST (AudioSet Audio Spectrogram Transformer). Offline only.

    uv run tools/ast_check.py brain-rs/clips/dual/<rec>/ [--wav station_1.wav] [--labels labels_auto.json]
                              [--offset 0.0] [--normalize] [--cache ~/code/audio-models]

Same report as yamnet_check.py: per labelled event, the strongest shout-like AudioSet class score
(sigmoid probability) in 1.5 s windows (hop 0.5 s) overlapping the event, next to Speech/Clapping/Knock/
Music, then AUC of SHOUT vs the rest. Model: MIT/ast-finetuned-audioset-10-10-0.4593 (~350 MB, Hugging
Face), cached under --cache (default ~/code/audio-models, outside the repo).
"""
from __future__ import annotations

import argparse
import json
import os
import sys
import wave
from pathlib import Path

import numpy as np

MODEL = "MIT/ast-finetuned-audioset-10-10-0.4593"
WIN_S, HOP_S = 1.5, 0.5
SHOUTY = ["Shout", "Yell", "Screaming", "Bellow", "Battle cry", "Whoop", "Children shouting"]
COMPARE = ["Speech", "Clapping", "Knock", "Music", "Hands", "Thump, thud", "Silence"]


def read_wav(path: Path) -> tuple[np.ndarray, int]:
    with wave.open(str(path)) as w:
        sr, ch, n = w.getframerate(), w.getnchannels(), w.getnframes()
        x = np.frombuffer(w.readframes(n), dtype="<i2").astype(np.float32) / 32768.0
    if ch > 1:
        x = x.reshape(-1, ch).mean(axis=1)
    return x, sr


def auc(pos: list[float], neg: list[float]) -> float:
    if not pos or not neg:
        return float("nan")
    p, n = np.array(pos), np.array(neg)
    return float(((p[:, None] > n[None, :]).mean() + 0.5 * (p[:, None] == n[None, :]).mean()))


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("rec", type=Path)
    ap.add_argument("--wav", default="station_1.wav")
    ap.add_argument("--labels", default="labels_auto.json")
    ap.add_argument("--offset", type=float, default=0.0)
    ap.add_argument("--normalize", action="store_true")
    ap.add_argument("--cache", type=Path, default=Path.home() / "code" / "audio-models")
    a = ap.parse_args()

    a.cache.mkdir(parents=True, exist_ok=True)
    os.environ["HF_HOME"] = str(a.cache)
    import torch
    from transformers import ASTFeatureExtractor, ASTForAudioClassification

    fe = ASTFeatureExtractor.from_pretrained(MODEL, cache_dir=a.cache)
    model = ASTForAudioClassification.from_pretrained(MODEL, cache_dir=a.cache).eval()
    id2label = {int(k): v for k, v in model.config.id2label.items()}
    idx = {v: k for k, v in id2label.items()}
    shouty = [idx[n] for n in SHOUTY if n in idx]
    compare = {n: idx[n] for n in COMPARE if n in idx}

    x, sr = read_wav(a.rec / a.wav)
    if sr != 16000:
        sys.exit(f"need 16 kHz audio, got {sr}")
    if a.normalize:
        ref = float(np.percentile(np.abs(x), 99.9))
        x = np.clip(x * (0.5 / max(ref, 1e-4)), -1.0, 1.0)
        print(f"normalised: gain {20 * np.log10(0.5 / max(ref, 1e-4)):.1f} dB")

    win, hop = int(WIN_S * sr), int(HOP_S * sr)
    starts = list(range(0, max(1, len(x) - win), hop))
    scores = []
    with torch.no_grad():
        for s in starts:
            w = x[s: s + win]
            inp = fe(w, sampling_rate=sr, return_tensors="pt")
            scores.append(torch.sigmoid(model(**inp).logits)[0].numpy())
    sc = np.array(scores)
    t_start = np.array(starts) / sr
    shout_score = sc[:, shouty].max(axis=1)
    labels = json.loads((a.rec / a.labels).read_text())["cues"]

    print(f"{len(x) / sr:.0f} s, {len(starts)} windows, shouty classes: {[id2label[i] for i in shouty]}\n")
    print(f"{'#':>3} {'kind':<10} {'t':>7} {'dur':>5} {'shouty':>6}  " + " ".join(f"{n[:7]:>7}" for n in compare))
    groups: dict[str, list[float]] = {}
    for k, l in enumerate(labels):
        t0 = l["t"] + a.offset
        t1 = t0 + max(l.get("dur", 0.3), 0.3)
        sel = [i for i, s in enumerate(t_start) if s < t1 and s + WIN_S > t0 + 0.1]
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
    print(f"\nSHOUT vs rest separation: AUC {auc(pos, neg):.2f}  (1.0 perfect, 0.5 none)")
    for thr in (0.05, 0.1, 0.2, 0.3, 0.5):
        print(f"  threshold {thr:.2f}: shouts kept {sum(s >= thr for s in pos)}/{len(pos)}, others passed {sum(s >= thr for s in neg)}/{len(neg)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
