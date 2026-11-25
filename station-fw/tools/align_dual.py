#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["numpy>=1.26"]
# ///
"""Time offset between station_1.wav and bomge.wav from the first claps of a dual recording.

    uv run station-fw/tools/align_dual.py brain-rs/clips/dual/<rec>/            # prints the offset
    uv run station-fw/tools/align_dual.py <rec>/ --write                        # + bomge_aligned.wav
    uv run station-fw/tools/align_dual.py a.wav b.wav --claps 11.2,13.5,15.9    # explicit clap times
    uv run station-fw/tools/align_dual.py --selftest

Cross-correlates the 10 ms RMS envelopes of both files around the first 3 CLAP cues of cues.json
(t about 10-17 s, shifted by start_offset_sec). Prints per-clap lags and their median:
    offset = (time of an event in bomge.wav) - (time of the same event in station_1.wav)
bomge_aligned.wav is bomge.wav shifted by -offset (head trimmed, or zero padded when offset < 0),
so it lines up with station_1.wav and the cues.
"""

from __future__ import annotations

import argparse
import json
import statistics
import sys
import wave
from pathlib import Path

import numpy as np

HOP_SEC = 0.01
HALF_WINDOW = 1.5  # seconds of envelope around each clap used as the template
MAX_LAG = 4.0  # largest |offset| searched, seconds


def read_wav(path: Path) -> tuple[int, int, np.ndarray]:
    """(rate, channels, int16 frames shaped (n, channels))."""
    with wave.open(str(path), "rb") as w:
        if w.getsampwidth() != 2:
            raise SystemExit(f"{path}: only 16-bit PCM is supported")
        ch, rate = w.getnchannels(), w.getframerate()
        raw = np.frombuffer(w.readframes(w.getnframes()), dtype="<i2")
    return rate, ch, raw.reshape(-1, ch)


def write_wav(path: Path, rate: int, frames: np.ndarray) -> None:
    with wave.open(str(path), "wb") as w:
        w.setnchannels(frames.shape[1])
        w.setsampwidth(2)
        w.setframerate(rate)
        w.writeframes(frames.astype("<i2").tobytes())


def to_mono(frames: np.ndarray) -> np.ndarray:
    """(l+r)/2 in [-1, 1], like the brain."""
    return frames.astype(np.float64).mean(axis=1) / 32768.0


def envelope(mono: np.ndarray, rate: int) -> np.ndarray:
    """RMS per HOP_SEC, DC removed."""
    hop = max(1, int(round(rate * HOP_SEC)))
    n = len(mono) // hop
    x = mono[: n * hop].reshape(n, hop)
    x = x - x.mean(axis=1, keepdims=True)
    return np.sqrt((x**2).mean(axis=1))


def _norm(v: np.ndarray) -> np.ndarray:
    v = v - v.mean()
    s = np.linalg.norm(v)
    return v / s if s > 0 else v


def clap_lag(env_a: np.ndarray, env_b: np.ndarray, t: float) -> tuple[float, float]:
    """Lag (seconds, b minus a) of the clap near `t` and the correlation strength (0..1)."""
    i0 = max(0, int((t - HALF_WINDOW) / HOP_SEC))
    i1 = min(len(env_a), int((t + HALF_WINDOW) / HOP_SEC))
    tmpl = _norm(env_a[i0:i1])
    if len(tmpl) < 10:
        raise ValueError(f"clap at {t:.2f}s is outside the station recording")
    max_l = int(MAX_LAG / HOP_SEC)
    best, best_lag = -2.0, 0
    for lag in range(-max_l, max_l + 1):
        s = i0 + lag
        if s < 0 or s + len(tmpl) > len(env_b):
            continue
        seg = env_b[s : s + len(tmpl)]
        c = float(np.dot(tmpl, _norm(seg)))
        if c > best:
            best, best_lag = c, lag
    return best_lag * HOP_SEC, best


def find_offset(env_a: np.ndarray, env_b: np.ndarray, claps: list[float]) -> tuple[float, list[tuple[float, float, float]]]:
    """Median lag over `claps`; also returns (clap t, lag, strength) per clap."""
    per = []
    for t in claps:
        lag, strength = clap_lag(env_a, env_b, t)
        per.append((t, lag, strength))
    return statistics.median(l for _, l, _ in per), per


def clap_times(cues_path: Path, n: int = 3) -> list[float]:
    d = json.loads(cues_path.read_text())
    shift = float(d.get("start_offset_sec", 0.0))
    ts = sorted(float(c["t"]) for c in d.get("cues", []) if str(c.get("kind", "")).upper() == "CLAP")
    return [t + shift for t in ts[:n]]


def shift_frames(frames: np.ndarray, rate: int, offset: float) -> np.ndarray:
    """Move bomge by -offset: drop the first `offset` s, or prepend silence when negative."""
    n = int(round(abs(offset) * rate))
    if offset >= 0:
        return frames[n:]
    return np.vstack([np.zeros((n, frames.shape[1]), dtype=frames.dtype), frames])


def selftest() -> None:
    rate, dur, true_off = 16000, 30.0, 0.73
    rng = np.random.default_rng(1)
    claps = [11.0, 13.4, 16.1]

    def render(shift: float) -> np.ndarray:
        x = rng.normal(0, 0.003, int(rate * dur))
        for c in claps:
            s = int((c + shift) * rate)
            n = int(0.05 * rate)
            x[s : s + n] += rng.normal(0, 0.5, n) * np.exp(-np.arange(n) / (0.01 * rate))
        return (x * 32767).astype(np.int16).reshape(-1, 1)

    a, b = render(0.0), render(true_off)
    off, per = find_offset(envelope(to_mono(a), rate), envelope(to_mono(b), rate), claps)
    assert abs(off - true_off) <= HOP_SEC + 1e-9, off
    assert all(s > 0.5 for _, _, s in per), per
    # negative offsets and aligned output
    off2, _ = find_offset(envelope(to_mono(b), rate), envelope(to_mono(a), rate), claps)
    assert abs(off2 + true_off) <= HOP_SEC + 1e-9, off2
    aligned = shift_frames(b, rate, off)
    e = envelope(to_mono(aligned), rate)
    assert abs(find_offset(envelope(to_mono(a), rate), e, claps)[0]) <= HOP_SEC + 1e-9
    assert len(shift_frames(a, rate, -0.5)) == len(a) + rate // 2
    # cues.json parsing: start_offset_sec shifts clap times
    import tempfile

    with tempfile.TemporaryDirectory() as d:
        p = Path(d) / "cues.json"
        p.write_text(json.dumps({"start_offset_sec": 1.0, "cues": [{"kind": "SHOUT", "t": 5}, {"kind": "CLAP", "t": 10}, {"kind": "CLAP", "t": 12}]}))
        assert clap_times(p) == [11.0, 13.0]
    print("selftest ok")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("paths", nargs="*", help="recording dir (station_1.wav, bomge.wav, cues.json) or station.wav bomge.wav")
    ap.add_argument("--cues", type=Path, help="cues.json (default: <dir>/cues.json)")
    ap.add_argument("--claps", help="comma separated clap times in station-file seconds (overrides cues)")
    ap.add_argument("--write", nargs="?", const="", metavar="OUT", help="write the aligned bomge wav (default <dir>/bomge_aligned.wav)")
    ap.add_argument("--selftest", action="store_true")
    a = ap.parse_args()
    if a.selftest:
        selftest()
        return 0
    if len(a.paths) == 1 and Path(a.paths[0]).is_dir():
        d = Path(a.paths[0])
        station, bomge, cues = d / "station_1.wav", d / "bomge.wav", a.cues or d / "cues.json"
    elif len(a.paths) == 2:
        station, bomge = Path(a.paths[0]), Path(a.paths[1])
        cues = a.cues or station.parent / "cues.json"
    else:
        ap.error("give a recording directory or two wav files")
    claps = [float(x) for x in a.claps.split(",")] if a.claps else clap_times(cues)
    if not claps:
        raise SystemExit("no CLAP cues found; pass --claps t1,t2,t3")
    ra, _, fa = read_wav(station)
    rb, _, fb = read_wav(bomge)
    off, per = find_offset(envelope(to_mono(fa), ra), envelope(to_mono(fb), rb), claps)
    for t, lag, s in per:
        print(f"clap near {t:6.2f}s: lag {lag:+.3f}s  corr {s:.2f}")
    print(f"offset {off:+.3f} s  (bomge event time = station event time + offset)")
    if min(s for _, _, s in per) < 0.5:
        print("warning: weak correlation on some claps; check --claps", file=sys.stderr)
    if a.write is not None:
        out = Path(a.write) if a.write else bomge.parent / "bomge_aligned.wav"
        write_wav(out, rb, shift_frames(fb, rb, off))
        print(f"wrote {out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
