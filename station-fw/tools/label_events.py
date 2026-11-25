#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["numpy>=1.26"]
# ///
"""Label the sound events of a dual recording (dual_record.py) from the reference mic's RMS peaks.

    uv run tools/label_events.py brain-rs/clips/dual/<rec>/ [--offset 0.92] [--rise-db 15]

The human's cue schedule (cues.json) is only a rough guide: people forget or miscount shouts. So the
labels come from the BOMGE audio: every burst of sound that rises `--rise-db` above the room noise
floor is one event, merged across short gaps. Each event is classified from its shape (duration, peak,
spectral centroid) and from the cue block it falls in, then written as station-time labels
(`labels_auto.json`, the cue format shout_replay reads, start_offset_sec 0) plus a printed table.

`--offset` is the lag from align_dual.py (bomge event time = station event time + offset).
Check the table by eye: the classification is a heuristic.
"""
from __future__ import annotations

import argparse
import json
import sys
import wave
from pathlib import Path

import numpy as np

HOP = 0.02  # s


def read_wav(path: Path) -> tuple[np.ndarray, int]:
    with wave.open(str(path)) as w:
        sr, ch, n = w.getframerate(), w.getnchannels(), w.getnframes()
        x = np.frombuffer(w.readframes(n), dtype="<i2").astype(np.float32) / 32768.0
    if ch > 1:
        x = x.reshape(-1, ch).mean(axis=1)
    return x, sr


def envelope(x: np.ndarray, sr: int) -> np.ndarray:
    """dBFS RMS per hop of a 150 Hz - 6 kHz band (cheap FFT-mask band-pass per hop is overkill: use diff + smoothing)."""
    # high-pass by first difference blend removes rumble and DC; good enough for event finding
    y = x - 0.97 * np.concatenate(([0.0], x[:-1]))
    n = int(sr * HOP)
    m = len(y) // n
    r = np.sqrt((y[: m * n].reshape(m, n) ** 2).mean(axis=1) + 1e-12)
    return 20 * np.log10(r)


def centroid_hz(x: np.ndarray, sr: int, t0: float, t1: float) -> float:
    seg = x[int(t0 * sr): int(t1 * sr)]
    if len(seg) < 256:
        return 0.0
    spec = np.abs(np.fft.rfft(seg * np.hanning(len(seg))))
    f = np.fft.rfftfreq(len(seg), 1 / sr)
    return float((f * spec).sum() / (spec.sum() + 1e-9))


def find_events(env: np.ndarray, rise_db: float, merge_s: float, min_s: float):
    floor = float(np.percentile(env, 20))
    thr = floor + rise_db
    on = env > thr
    ev, i, n = [], 0, len(env)
    while i < n:
        if not on[i]:
            i += 1
            continue
        j = i
        gap = 0
        last = i
        while j < n and gap * HOP <= merge_s:
            if on[j]:
                last, gap = j, 0
            else:
                gap += 1
            j += 1
        if (last - i + 1) * HOP >= min_s:
            ev.append((i, last))
        i = last + 1
    return ev, floor


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("rec", type=Path)
    ap.add_argument("--offset", type=float, default=0.92, help="bomge time - station time (align_dual.py)")
    ap.add_argument("--rise-db", type=float, default=15.0)
    ap.add_argument("--merge", type=float, default=0.25, help="merge gaps shorter than this (s)")
    ap.add_argument("--min-dur", type=float, default=0.04)
    a = ap.parse_args()

    x, sr = read_wav(a.rec / "bomge.wav")
    env = envelope(x, sr)
    events, floor = find_events(env, a.rise_db, a.merge, a.min_dur)
    cues_doc = json.loads((a.rec / "cues.json").read_text())
    start = cues_doc.get("start_offset_sec", 5)
    cues = [(c["kind"], c["t"] + start, c.get("prompt", "")) for c in cues_doc["cues"] if c["kind"] not in ("QUIET", "MOVE")]
    print(f"{len(x) / sr:.0f} s, noise floor {floor:.1f} dB, event threshold {floor + a.rise_db:.1f} dB, {len(events)} events\n")
    print(f"{'#':>3} {'t(bomge)':>8} {'dur':>5} {'peak':>6} {'cent':>5}  {'class':<6} nearest cue")

    labels, used = [], set()
    for k, (i, j) in enumerate(events, 1):
        t0, t1 = i * HOP, (j + 1) * HOP
        dur = t1 - t0
        peak = float(env[i: j + 1].max())
        cen = centroid_hz(x, sr, t0, t1)
        # nearest unused cue before the event (reaction time 0 .. 2.5 s)
        cand = [(t0 - ct, idx, kind, prompt) for idx, (kind, ct, prompt) in enumerate(cues) if -0.5 <= t0 - ct <= 2.5]
        cue = min(cand, key=lambda c: abs(c[0] - 0.5)) if cand else None
        if cue:
            used.add(cue[1])
        cue_kind = cue[2] if cue else "UNCUED"
        # shape classes: shouts are sustained and loud, claps/knocks short
        if dur < 0.18:
            shape = "CLICK"
        elif dur >= 0.35 and peak - floor >= 30:
            shape = "SHOUT"
        else:
            shape = "SOFT"
        # the cue says what the human intended, the shape says what it sounded like
        kind = "SHOUT" if (shape == "SHOUT" and cue_kind in ("SHOUT", "UNCUED")) else \
               {"CLAP": "CLAP", "KNOCK": "KNOCK", "SOFT": "SOFT"}.get(cue_kind, "SOFT" if shape == "SOFT" else cue_kind)
        if cue_kind == "SHOUT" and shape != "SHOUT":
            kind = "SHOUT_WEAK"
        print(f"{k:>3} {t0:8.2f} {dur:5.2f} {peak:6.1f} {cen:5.0f}  {kind:<10} {cue_kind}{'' if not cue else f' (+{cue[0]:.2f}s)'}")
        labels.append({"kind": kind, "t": round(t0 - a.offset, 3), "dur": round(dur, 3), "peak_db": round(peak, 1),
                       "centroid_hz": round(cen), "prompt": cue[3] if cue else ""})
    missing = [(kind, ct) for idx, (kind, ct, _) in enumerate(cues) if idx not in used]
    print(f"\ncues with no sound event: {len(missing)}: " + ", ".join(f"{k}@{t:.0f}s" for k, t in missing))
    out = {"label": cues_doc.get("label"), "start_offset_sec": 0, "source": "label_events.py", "offset_applied": a.offset,
           "cues": [{"kind": ("SHOUT" if l["kind"] == "SHOUT" else l["kind"]), "t": l["t"], **{k: v for k, v in l.items() if k not in ("kind", "t")}}
                    for l in labels]}
    (a.rec / "labels_auto.json").write_text(json.dumps(out, indent=1))
    print(f"wrote {a.rec / 'labels_auto.json'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
