#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["pyserial>=3.5", "numpy>=1.26"]
# ///
"""Record raw audio from a sensor station (2x ICS-43434) to a 16 kHz stereo 16-bit WAV.

The station must run a build with STATION_BAUD=921600 (`STATION_BAUD=921600 just flash`).
This tool sends StartAudio (and repeats it every second as a keep-alive; the station stops
by itself 5 s after the last one), writes every AudioChunk to the WAV, and sends StopAudio
at the end. The audio is the I2S stream decimated 48 -> 16 kHz behind a linear-phase
anti-alias FIR and rounded to 16 bits: no AGC, no noise suppression, no other filtering.

    uv run tools/station_record.py shout              # -> brain-rs/clips/station/shout_1.wav
    uv run tools/station_record.py shout --mono       # + shout_1_mono.wav ((L+R)/2)
    uv run tools/station_record.py shout --split      # + shout_1_L.wav, shout_1_R.wav
    uv run tools/station_record.py idle -t 30         # stop after 30 s instead of Enter

Enter (or Ctrl-C) stops. Prints duration, per-channel peak, clipped counts, dropped chunks
and the idle noise floor, and writes <name>.json next to the WAV (station id, firmware,
baud, counts, noise floor). If the station dropped chunks (UART could not keep up), the
gap is filled with silence so the WAV stays in real time; the count is reported.
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import re
import sys
import threading
import time
import wave
from pathlib import Path

import numpy as np

import station_monitor as sm  # same directory

REPO = Path(__file__).resolve().parents[2]
DEFAULT_OUT = REPO / "brain-rs" / "clips" / "station"
CHUNK_FRAMES = 160
KEEPALIVE_S = 1.0
FIRST_CHUNK_TIMEOUT_S = 4.0


def next_path(out_dir: Path, label: str) -> Path:
    label = re.sub(r"[^A-Za-z0-9_-]+", "_", label.strip()) or "clip"
    n = 1 + max((int(m.group(1)) for p in out_dir.glob(f"{label}_*.wav")
                 if (m := re.fullmatch(rf"{re.escape(label)}_(\d+)\.wav", p.name))), default=0)
    return out_dir / f"{label}_{n}.wav"


def dbfs(x: float) -> float:
    return round(20 * np.log10(max(float(x), 1e-9) / 32768), 1)


class Recorder:
    """Reader thread: splits the serial stream, collects AudioChunks into a PCM buffer."""

    def __init__(self, ser, verbose: bool) -> None:
        self.ser = ser
        self.verbose = verbose
        self.pcm = bytearray()  # interleaved LE i16, stereo
        self.lock = threading.Lock()
        self.stop = threading.Event()
        self.first_chunk = threading.Event()
        self.hello: sm.Hello | None = None
        self.rate = 16000
        self.next_frame: int | None = None
        self.chunks = 0
        self.dropped_frames = 0
        self.restarts = 0
        self.seq_gaps = 0
        self.bad = 0
        self.clipped_reported = 0
        self.last_seq: int | None = None
        self.t_first: float | None = None
        self.t_last: float | None = None
        self.station_log: list[str] = []
        self.kinds: dict[str, int] = {}
        self.arrivals: list[tuple[float, int]] = []  # (host time, frames so far) per chunk

    def run(self) -> None:
        def src():
            while not self.stop.is_set():
                data = self.ser.read(8192)
                if data:
                    yield data

        for chunk in sm.chunks(src()):
            try:
                seq, msg = sm.decode_frame(chunk)
            except Exception:
                if sm.looks_binary(chunk):
                    self.bad += 1
                else:
                    self.text(chunk.decode("utf-8", "replace"))
                continue
            if self.last_seq is not None and seq != (self.last_seq + 1) & 0xFFFF:
                self.seq_gaps += 1
            self.last_seq = seq
            if isinstance(msg, sm.Hello):
                self.hello = msg
            elif isinstance(msg, sm.AudioChunk):
                self.audio(msg)
            if self.t_first is not None:
                k = type(msg).__name__.lower()
                self.kinds[k] = self.kinds.get(k, 0) + 1

    def text(self, s: str) -> None:
        for line in s.splitlines():
            line = line.strip()
            if not line:
                continue
            if any(k in line for k in ("stream:", "stats:")) or line.startswith(("W (", "E (")):
                self.station_log.append(line)
                print(f"  station: {line}", file=sys.stderr)
            elif self.verbose:
                print(f"  station: {line}", file=sys.stderr)

    def audio(self, c: sm.AudioChunk) -> None:
        now = time.monotonic()
        if c.channels != 2:
            raise SystemExit(f"unexpected channel count {c.channels}")
        with self.lock:
            self.rate = c.rate_hz
            if self.next_frame is not None and c.frame != self.next_frame:
                if c.frame > self.next_frame:
                    gap = c.frame - self.next_frame
                    self.dropped_frames += gap
                    self.pcm += bytes(gap * 4)  # silence keeps the WAV in real time
                else:  # stream restarted (station reset, or keep-alive lapsed)
                    self.restarts += 1
            self.pcm += c.samples
            self.next_frame = c.frame + c.frames
            self.chunks += 1
            self.arrivals.append((now, len(self.pcm) // 4))
            self.clipped_reported += c.clipped
            if self.t_first is None:
                self.t_first = now - c.frames / c.rate_hz
                # Opening the port resets this board; its ROM log at 115200 baud reads as
                # binary noise. Count damage only from the first chunk on.
                self.bad = 0
                self.seq_gaps = 0
            self.t_last = now
        self.first_chunk.set()


def write_wav(path: Path, pcm: np.ndarray, rate: int) -> None:
    with wave.open(str(path), "wb") as w:
        w.setnchannels(pcm.shape[1] if pcm.ndim == 2 else 1)
        w.setsampwidth(2)
        w.setframerate(rate)
        w.writeframes(pcm.astype("<i2").tobytes())


def stats(x: np.ndarray, rate: int) -> dict:
    """Per-channel peak, clipping, DC and the noise floor (10th percentile of 100 ms AC RMS)."""
    win = rate // 10
    out: dict = {}
    for c, name in enumerate("LR"):
        ch = x[:, c].astype(np.float64)
        n = len(ch) // win
        if n:
            w = ch[: n * win].reshape(n, win)
            rms = np.sqrt(((w - w.mean(axis=1, keepdims=True)) ** 2).mean(axis=1))
            floor, median, loud = (dbfs(v) for v in np.percentile(rms, [10, 50, 100]))
        else:
            floor = median = loud = None
        out[name] = {
            "peak_dbfs": dbfs(np.abs(ch).max(initial=0)),
            "clipped": int(((x[:, c] >= 32767) | (x[:, c] <= -32768)).sum()),
            "dc": round(float(ch.mean()), 1) if len(ch) else 0.0,
            "noise_floor_dbfs": floor,
            "median_100ms_dbfs": median,
            "loudest_100ms_dbfs": loud,
        }
    return out


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("label", nargs="?", default="clip")
    ap.add_argument("-p", "--port", help="serial port (default: first USB serial)")
    ap.add_argument("-b", "--baud", type=int, default=921600, help="station console baud (default 921600)")
    ap.add_argument("-o", "--out-dir", type=Path, default=DEFAULT_OUT)
    ap.add_argument("-t", "--duration", type=float, help="stop after this many seconds (default: Enter)")
    ap.add_argument("--mono", action="store_true", help="also write <name>_mono.wav, (L+R)/2")
    ap.add_argument("--split", action="store_true", help="also write <name>_L.wav and <name>_R.wav")
    ap.add_argument("-v", "--verbose", action="store_true", help="echo all station log text")
    a = ap.parse_args()

    port = a.port or sm.pick_port()
    ser = sm.open_serial(port, a.baud, timeout=0.05)
    rec = Recorder(ser, a.verbose)
    reader = threading.Thread(target=rec.run, daemon=True)
    reader.start()

    cmd_seq = 0

    def command(cmd: int) -> None:
        nonlocal cmd_seq
        cmd_seq += 1
        ser.write(sm.encode_command(cmd_seq, cmd))
        ser.flush()

    keep = threading.Event()

    def keepalive() -> None:
        while not keep.wait(KEEPALIVE_S):
            command(sm.CMD_START_AUDIO)

    started = dt.datetime.now().astimezone()
    command(sm.CMD_START_AUDIO)
    threading.Thread(target=keepalive, daemon=True).start()
    if not rec.first_chunk.wait(FIRST_CHUNK_TIMEOUT_S):
        keep.set()
        rec.stop.set()
        hint = " ".join(rec.station_log[-3:])
        print(f"no audio from {port} at {a.baud} baud within {FIRST_CHUNK_TIMEOUT_S:.0f}s. "
              f"Is the station built with STATION_BAUD={a.baud}? {hint}", file=sys.stderr)
        return 1

    a.out_dir.mkdir(parents=True, exist_ok=True)
    path = next_path(a.out_dir, a.label)
    print(f"recording -> {path}  ({'%.0f s' % a.duration if a.duration else 'Enter to stop'})")
    try:
        if a.duration:
            time.sleep(a.duration)
        else:
            input()
    except (KeyboardInterrupt, EOFError):
        pass
    keep.set()
    command(sm.CMD_STOP_AUDIO)
    time.sleep(0.3)  # let in-flight chunks arrive
    rec.stop.set()
    reader.join(timeout=1.0)
    ser.close()

    with rec.lock:
        x = np.frombuffer(bytes(rec.pcm), dtype="<i2").reshape(-1, 2)
        rate = rec.rate
    write_wav(path, x, rate)
    extra = []
    if a.mono:
        p = path.with_name(path.stem + "_mono.wav")
        write_wav(p, ((x[:, 0].astype(np.int32) + x[:, 1]) // 2).astype(np.int16), rate)
        extra.append(p)
    if a.split:
        for c, name in enumerate("LR"):
            p = path.with_name(f"{path.stem}_{name}.wav")
            write_wav(p, x[:, c].copy(), rate)
            extra.append(p)

    dur = len(x) / rate
    # Station sample rate as seen by the host clock: slope of frames vs arrival time.
    # Robust to USB/UART burst jitter, unlike first-to-last arrival.
    measured_rate = None
    if len(rec.arrivals) > 100:
        t, f = (np.array(v, dtype=np.float64) for v in zip(*rec.arrivals))
        measured_rate = round(float(np.polyfit(t - t[0], f, 1)[0]), 1)
    wall = (rec.t_last - rec.t_first) if rec.t_first is not None and rec.t_last is not None else 0.0
    st = stats(x, rate)
    h = rec.hello
    dropped_chunks = rec.dropped_frames / CHUNK_FRAMES
    meta = {
        "file": path.name,
        "label": a.label,
        "started_at": started.isoformat(timespec="seconds"),
        "station_id": h.station_id if h else None,
        "fw_version": h.fw_version if h else None,
        "proto_version": h.proto_version if h else None,
        "radar_fw": h.radar_fw if h else None,
        "port": port,
        "baud": a.baud,
        "rate_hz": rate,
        "channels": 2,
        "channel_order": "L (mic SEL=GND), R (mic SEL=3V3)",
        "processing": "ICS-43434 24-bit @48 kHz -> linear-phase FIR (191 taps, flat to 7 kHz, "
                      ">=64 dB from 8 kHz) -> decimate 3 -> round to 16-bit. No AGC, NS or DC removal.",
        "duration_s": round(dur, 3),
        "wall_s": round(wall, 3),
        "measured_rate_hz": measured_rate,
        "frames": len(x),
        "chunks": rec.chunks,
        "dropped_frames": rec.dropped_frames,
        "dropped_chunks": dropped_chunks,
        "seq_gaps": rec.seq_gaps,
        "bad_frames": rec.bad,
        "stream_restarts": rec.restarts,
        "clipped_reported_by_station": rec.clipped_reported,
        "frame_counts": rec.kinds,
        "channels_stats": st,
        "noise_floor_dbfs": [st["L"]["noise_floor_dbfs"], st["R"]["noise_floor_dbfs"]],
        "noise_floor_note": "10th percentile of 100 ms AC RMS over the clip, dBFS (16-bit = 24-bit full scale)",
        "station_log": rec.station_log[-20:],
    }
    side = path.with_suffix(".json")
    side.write_text(json.dumps(meta, indent=2) + "\n")

    drift = (dur / wall - 1) * 100 if wall > 0 else float("nan")
    print(f"{path}: {dur:.2f}s ({wall:.2f}s wall, {drift:+.2f}%) {rate}Hz stereo"
          + (f", measured rate {measured_rate} Hz ({(measured_rate / rate - 1) * 100:+.2f}%)" if measured_rate else "")
          + f", {rec.chunks} chunks, dropped {dropped_chunks:g} chunks, seq gaps {rec.seq_gaps}, bad frames {rec.bad}"
          + (f", stream restarts {rec.restarts}" if rec.restarts else ""))
    print("  frames: " + ", ".join(f"{k} {v}" for k, v in sorted(rec.kinds.items())))
    for name in "LR":
        s = st[name]
        print(f"  {name}: peak {s['peak_dbfs']} dBFS  clipped {s['clipped']}  noise floor {s['noise_floor_dbfs']} dBFS"
              f"  median {s['median_100ms_dbfs']}  loudest 100ms {s['loudest_100ms_dbfs']}  dc {s['dc']}")
    if any(st[c]["clipped"] for c in "LR"):
        print("  clipping: move back from the mics")
    for p in [*extra, side]:
        print(f"  + {p}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
