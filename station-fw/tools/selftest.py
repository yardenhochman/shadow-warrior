#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["pyserial>=3.5"]
# ///
"""Reset the station and show its power-on self test (mics + radar). Exit code 0 = PASS, 1 = WARN, 2 = FAIL.

    uv run tools/selftest.py            # or: just selftest

The firmware runs the test about 2 s after boot (and again every 60 s, logging only when the result
changes); this tool resets the board over the USB serial lines and prints the `selftest:` log lines.
"""

from __future__ import annotations

import argparse
import re
import sys
import time

import serial

import station_monitor as sm

ANSI = re.compile(r"\x1b\[[0-9;]*m")
CODES = {"PASS": 0, "WARN": 1, "FAIL": 2}


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("-p", "--port")
    ap.add_argument("-b", "--baud", type=int, default=115200)
    ap.add_argument("--timeout", type=float, default=15.0, help="seconds to wait for the summary")
    a = ap.parse_args()

    port = a.port or sm.pick_port()
    # Boot ROM output is 115200; the firmware then switches to the console baud (--baud) after ~100 ms.
    ser = serial.Serial(port, 115200, timeout=0.2)
    ser.dtr, ser.rts = False, True  # EN low
    time.sleep(0.1)
    ser.rts = False  # EN high: boot
    ser.reset_input_buffer()
    if a.baud != 115200:
        time.sleep(0.35)
        ser.baudrate = a.baud
    deadline = time.monotonic() + a.timeout
    buf = b""
    seen: list[str] = []
    while time.monotonic() < deadline:
        buf += ser.read(4096)
        *lines, buf = buf.split(b"\n")
        for raw in lines:
            text = ANSI.sub("", raw.decode("utf-8", "replace")).strip("\r\x00 ")
            if "selftest:" not in text:
                continue
            msg = text[text.index("selftest:"):]
            print(msg)
            seen.append(msg)
            m = re.fullmatch(r"selftest: (PASS|WARN|FAIL)", msg)
            if m:
                return CODES[m.group(1)]
    print("no self test result: wrong port/baud (build with STATION_BAUD=921600? then pass --baud 921600), or the board did not boot", file=sys.stderr)
    return 3


if __name__ == "__main__":
    sys.exit(main())
