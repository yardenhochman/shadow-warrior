#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["numpy>=1.26", "google-genai>=1.0", "python-dotenv>=1.0"]
# ///
"""Cross-check a dual recording's event labels with Gemini audio understanding. Dev-time only.

    uv run tools/gemini_label.py brain-rs/clips/dual/<rec>/ [--wav bomge.wav] [--offset 0.92]
                                 [--chunk 30] [--model gemini-3.5-flash]

Reads GEMINI_API_KEY from the repo-root .env (never printed). Sends the reference mic (bomge.wav) in
overlapping chunks, asks for a JSON list of timestamped events, writes labels_gemini.json (reference
time), and prints each labels_auto.json event next to the Gemini events overlapping it. Gemini
timestamps drift by a second or more: use it to check the event CLASS, keep onsets from the RMS labels.
"""
from __future__ import annotations

import argparse
import io
import json
import os
import sys
import time
import wave
from pathlib import Path

import numpy as np
from dotenv import load_dotenv

REPO = Path(__file__).resolve().parents[2]
PROMPT = """You are labelling a lab recording of a person doing an anger-release game: they shout at a
microphone, and also clap, knock on a table, talk loudly, and stay quiet. This clip starts at t=0.
List every distinct sound event. Reply with JSON only: a list of objects
{"t": onset seconds, "dur": seconds, "kind": one of "shout"|"clap"|"knock"|"talk"|"music"|"other",
 "intensity": 1-5 (5 = full-force angry yell), "note": short}.
A shout is a sustained loud voiced yell, not normal speech. Count each separate shout once."""


def read_wav(path: Path) -> tuple[np.ndarray, int]:
    with wave.open(str(path)) as w:
        sr, ch, n = w.getframerate(), w.getnchannels(), w.getnframes()
        x = np.frombuffer(w.readframes(n), dtype="<i2")
    if ch > 1:
        x = x.reshape(-1, ch).mean(axis=1).astype("<i2")
    return x, sr


def wav_bytes(x: np.ndarray, sr: int) -> bytes:
    b = io.BytesIO()
    with wave.open(b, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(sr)
        w.writeframes(x.astype("<i2").tobytes())
    return b.getvalue()


def ask(client, types, model: str, data: bytes) -> list[dict]:
    for attempt in range(5):
        try:
            r = client.models.generate_content(
                model=model,
                contents=[PROMPT, types.Part.from_bytes(data=data, mime_type="audio/wav")],
                config=types.GenerateContentConfig(response_mime_type="application/json", temperature=0.0),
            )
            return json.loads(r.text)
        except Exception as e:  # 429 / transient: back off
            msg = str(e)
            if any(c in msg[:40] for c in ("402", "401", "403", "400", "404")):  # billing/auth/bad request: retrying won't help
                sys.exit(f"Gemini API error, not retrying: {msg[:300]}")
            wait = 15 * (attempt + 1)
            print(f"  retry in {wait}s: {msg[:120]}", file=sys.stderr)
            time.sleep(wait)
    return []


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("rec", type=Path)
    ap.add_argument("--wav", default="bomge.wav")
    ap.add_argument("--offset", type=float, default=0.92, help="reference time - station time (for comparing with labels_auto)")
    ap.add_argument("--chunk", type=float, default=30.0)
    ap.add_argument("--overlap", type=float, default=3.0)
    ap.add_argument("--model", default="gemini-3.5-flash")
    a = ap.parse_args()

    load_dotenv(REPO / ".env")
    key = os.environ.get("GEMINI_API_KEY")
    if not key:
        sys.exit("GEMINI_API_KEY missing from repo-root .env")
    from google import genai
    from google.genai import types

    client = genai.Client(api_key=key)
    x, sr = read_wav(a.rec / a.wav)
    n_chunk, step = int(a.chunk * sr), int((a.chunk - a.overlap) * sr)
    events: list[dict] = []
    for s in range(0, max(1, len(x) - int(a.overlap * sr)), step):
        t_off = s / sr
        print(f"chunk {t_off:.0f}s ..", flush=True)
        for e in ask(client, types, a.model, wav_bytes(x[s: s + n_chunk], sr)):
            try:
                e["t"] = float(e["t"]) + t_off
                e["dur"] = float(e.get("dur", 0.3))
            except (KeyError, TypeError, ValueError):
                continue
            # overlap region: drop duplicates of an event already found within 0.8 s
            if not any(abs(o["t"] - e["t"]) < 0.8 and o.get("kind") == e.get("kind") for o in events):
                events.append(e)
    events.sort(key=lambda e: e["t"])
    (a.rec / "labels_gemini.json").write_text(json.dumps({"model": a.model, "time_base": "reference", "events": events}, indent=1))
    print(f"\n{len(events)} Gemini events -> labels_gemini.json")

    auto = a.rec / "labels_auto.json"
    if auto.exists():
        cues = json.loads(auto.read_text())["cues"]
        print(f"\n{'#':>3} {'auto':<10} {'t(ref)':>7} {'dur':>5}  gemini events within +-1.5 s")
        agree = tot = 0
        for k, l in enumerate(cues):
            t = l["t"] + a.offset
            near = [e for e in events if e["t"] - 1.5 <= t <= e["t"] + e["dur"] + 1.5]
            txt = "; ".join(f"{e['kind']}({e.get('intensity', '?')})@{e['t']:.1f}" for e in near) or "-"
            if l["kind"] in ("SHOUT", "SHOUT_WEAK"):
                tot += 1
                agree += any(e["kind"] == "shout" for e in near)
            print(f"{k:>3} {l['kind']:<10} {t:7.2f} {l.get('dur', 0):5.2f}  {txt}")
        print(f"\nauto SHOUT/SHOUT_WEAK with a Gemini shout nearby: {agree}/{tot}")
        n_g = sum(e["kind"] == "shout" for e in events)
        print(f"Gemini shout events total: {n_g}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
