#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["pyserial>=3.5"]
# ///
"""Read a sensor station's USB serial stream: decode station-proto frames, pass log text through.

The stream mixes log lines with frames written as 0x00 <COBS> 0x00. Each
COBS body decodes to postcard(Envelope) + CRC-32 LE (zlib.crc32). Anything
that is not a valid frame is printed as text (unless --quiet-text).

    uv run tools/station_monitor.py                         # auto-pick port
    uv run tools/station_monitor.py -p /dev/cu.usbserial-0001 --json
    uv run tools/station_monitor.py --csv levels.csv --quiet-text
    uv run tools/station_monitor.py --only direction --csv doa.csv  # direction rows instead
    uv run tools/station_monitor.py --file capture.bin      # decode a saved capture
    uv run tools/station_monitor.py --save capture.bin      # tee raw bytes to a file

Exit with Ctrl-C; a summary (frames per type, CRC failures, seq gaps) is printed.
"""

from __future__ import annotations

import argparse
import csv
import json
import struct
import sys
import time
import zlib
from collections import Counter
from dataclasses import asdict, dataclass
from typing import BinaryIO, Iterator

# ---- COBS -------------------------------------------------------------------


def cobs_decode(data: bytes) -> bytes:
    out = bytearray()
    i = 0
    n = len(data)
    while i < n:
        code = data[i]
        if code == 0:
            raise ValueError("zero in COBS data")
        i += 1
        end = i + code - 1
        if end > n:
            raise ValueError("COBS overrun")
        out += data[i:end]
        i = end
        if code < 0xFF and i < n:
            out.append(0)
    return bytes(out)


# ---- postcard ---------------------------------------------------------------


class Reader:
    def __init__(self, b: bytes) -> None:
        self.b = b
        self.i = 0

    def u8(self) -> int:
        v = self.b[self.i]
        self.i += 1
        return v

    def varint(self) -> int:
        shift = 0
        v = 0
        while True:
            byte = self.u8()
            v |= (byte & 0x7F) << shift
            if not byte & 0x80:
                return v
            shift += 7
            if shift > 63:
                raise ValueError("varint too long")

    def bool(self) -> bool:
        v = self.u8()
        if v > 1:
            raise ValueError("bad bool")
        return v == 1

    def f32(self) -> float:
        (v,) = struct.unpack_from("<f", self.b, self.i)
        self.i += 4
        return v

    def string(self) -> str:
        n = self.varint()
        s = self.b[self.i : self.i + n].decode()
        self.i += n
        return s

    def done(self) -> bool:
        return self.i == len(self.b)


@dataclass
class Hello:
    proto_version: int
    station_id: str
    fw_version: str
    radar_fw: str | None


@dataclass
class Radar:
    present: bool
    moving_dist_cm: int
    static_dist_cm: int
    moving_energy: int
    static_energy: int
    state: int


@dataclass
class Audio:
    rms_dbfs_l: float
    rms_dbfs_r: float
    peak_dbfs: float
    frames: int


@dataclass
class Direction:
    frame: int
    lag_samples: float
    angle_deg: float
    confidence: float
    rms_dbfs: float


CSV_COLUMNS = {
    "audio": ["rms_dbfs_l", "rms_dbfs_r", "peak_dbfs", "frames"],
    "direction": ["frame", "lag_samples", "angle_deg", "confidence", "rms_dbfs"],
}


def decode_envelope(body: bytes) -> tuple[int, object]:
    r = Reader(body)
    seq = r.varint()
    variant = r.varint()
    msg: object
    if variant == 0:
        pv = r.u8()
        sid = r.varint()
        fw = r.string()
        radar = r.string() if r.u8() == 1 else None
        msg = Hello(pv, f"{sid:08x}", fw, radar)
    elif variant == 1:
        msg = Radar(r.bool(), r.varint(), r.varint(), r.u8(), r.u8(), r.u8())
    elif variant == 2:
        msg = Audio(r.f32(), r.f32(), r.f32(), r.varint())
    elif variant == 3:
        msg = Direction(r.varint(), r.f32(), r.f32(), r.f32(), r.f32())
    else:
        raise ValueError(f"unknown variant {variant}")
    if not r.done():
        raise ValueError("trailing bytes")
    return seq, msg


def decode_frame(chunk: bytes) -> tuple[int, object]:
    raw = cobs_decode(chunk)
    if len(raw) < 5:
        raise ValueError("short")
    body, crc = raw[:-4], int.from_bytes(raw[-4:], "little")
    if zlib.crc32(body) != crc:
        raise ValueError("crc")
    return decode_envelope(body)


# ---- stream -----------------------------------------------------------------


def chunks(src: Iterator[bytes]) -> Iterator[bytes]:
    buf = bytearray()
    for data in src:
        buf += data
        while True:
            i = buf.find(0)
            if i < 0:
                break
            if i:
                yield bytes(buf[:i])
            del buf[: i + 1]
        if len(buf) > 1 << 16:  # runaway text without delimiter
            yield bytes(buf)
            buf.clear()


def serial_source(port: str, baud: int, save: BinaryIO | None) -> Iterator[bytes]:
    import serial  # pyserial

    # dsrdtr/rtscts off and DTR/RTS low so opening the port does not reset the ESP32.
    with serial.Serial() as s:
        s.port = port
        s.baudrate = baud
        s.timeout = 0.1
        s.dtr = False
        s.rts = False
        s.open()
        while True:
            data = s.read(4096)
            if data:
                if save:
                    save.write(data)
                    save.flush()
                yield data


def file_source(path: str) -> Iterator[bytes]:
    with open(path, "rb") as f:
        while data := f.read(65536):
            yield data


def pick_port() -> str:
    from serial.tools import list_ports

    ports = [p.device for p in list_ports.comports()]
    for pat in ("usbserial", "SLAB", "usbmodem", "ttyUSB", "ttyACM"):
        for p in ports:
            if pat in p and not p.startswith("/dev/tty."):
                return p
    sys.exit(f"no USB serial port found (have: {ports}); pass --port")


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("-p", "--port", help="serial port (default: first USB serial)")
    ap.add_argument("-b", "--baud", type=int, default=115200)
    ap.add_argument("--file", help="decode a raw capture instead of a port")
    ap.add_argument("--save", help="also write raw bytes to this file")
    ap.add_argument("--json", action="store_true", help="print frames as JSON lines")
    ap.add_argument(
        "--csv", help="append rows to this CSV: audio levels, or directions with --only direction (t, seq, fields...)"
    )
    ap.add_argument("--quiet-text", action="store_true", help="do not echo log text")
    ap.add_argument("--only", choices=["hello", "radar", "audio", "direction"], help="print only this frame type")
    ap.add_argument("--check", action="store_true", help="exit 1 if any frame failed to decode (for tests)")
    args = ap.parse_args()
    csv_kind = "direction" if args.only == "direction" else "audio"

    save = open(args.save, "ab") if args.save else None
    src = file_source(args.file) if args.file else serial_source(args.port or pick_port(), args.baud, save)
    csv_w = None
    if args.csv:
        f = open(args.csv, "a", newline="")
        csv_w = csv.writer(f)
        if f.tell() == 0:
            csv_w.writerow(["t", "seq", *CSV_COLUMNS[csv_kind]])

    counts: Counter[str] = Counter()
    last_seq: int | None = None
    gaps = 0
    t0 = time.monotonic()
    try:
        for chunk in chunks(src):
            try:
                seq, msg = decode_frame(chunk)
            except Exception as e:  # not a frame: text, or a damaged frame
                # Looks binary (no printable text) -> count as a bad frame.
                if not any(32 <= c < 127 for c in chunk[:8]) and len(chunk) < 200:
                    counts[f"bad:{e}"] += 1
                elif not args.quiet_text:
                    sys.stdout.write(chunk.decode("utf-8", "replace"))
                    sys.stdout.flush()
                continue
            kind = type(msg).__name__.lower()
            counts[kind] += 1
            if last_seq is not None and seq != (last_seq + 1) & 0xFFFF:
                gaps += 1
            last_seq = seq
            if csv_w and kind == csv_kind:
                d = asdict(msg)
                csv_w.writerow([f"{time.monotonic() - t0:.3f}", seq, *(d[c] for c in CSV_COLUMNS[csv_kind])])
            if args.only and kind != args.only:
                continue
            if args.json:
                print(json.dumps({"seq": seq, "type": kind, **asdict(msg)}), flush=True)
            else:
                print(f"<< #{seq:5d} {msg}", flush=True)
    except KeyboardInterrupt:
        pass
    finally:
        print(f"\n-- frames: {dict(counts)}  seq gaps: {gaps}", file=sys.stderr)
    bad = sum(v for k, v in counts.items() if k.startswith("bad:"))
    if args.check and bad:
        sys.exit(f"--check: {bad} undecodable frame(s)")


if __name__ == "__main__":
    main()
