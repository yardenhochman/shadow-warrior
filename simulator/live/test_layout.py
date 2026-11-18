#!/usr/bin/env python3
# /// script
# requires-python = ">=3.10"
# dependencies = ["aiohttp>=3.9", "bleak>=1.1.0", "pyyaml>=6"]
# ///
"""Hardware-free test of the bridge layout model: `uv run simulator/live/test_layout.py`.

Unit tests of layout.py plus two bridge subprocesses fed with UDP realtime packets: (a) four
single-strip controllers, (b) one controller hosting two strips (the second `reverse`), checking
the composed page frame (GET /live/frame) column by column. Controllers listen on distinct
loopback UDP ports via `bind`, so no 127.0.0.x aliases are needed.
"""
import json
import socket
import subprocess
import sys
import time
import unittest
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from layout import Layout, LayoutError, apply_packet  # noqa: E402

PER = 180


def free_port(udp=False):
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM if udp else socket.SOCK_STREAM)
    s.bind(("127.0.0.1", 0))
    p = s.getsockname()[1]
    s.close()
    return p


def px(col, i):
    return bytes([col + 1, i % 256, 0])


class Unit(unittest.TestCase):
    def test_spec_shapes(self):
        lay = Layout.from_spec({"wled": {"controllers": [
            {"ip": "10.0.0.1", "num_leds": 180},
            {"ip": "10.0.0.2:8090", "strips": [{"column": 1, "leds": 180}, {"column": 2, "leds": 180, "reverse": True}]}]}})
        self.assertEqual((lay.columns, lay.per, lay.count), (3, 180, 720))
        self.assertEqual(lay.controllers[1].num_leds, 360)
        self.assertEqual(lay.controllers[1].bind, ("10.0.0.2", 21324))
        self.assertEqual(lay.controllers[1].strips[1].offset, 180)

    def test_errors(self):
        for spec in ({"controllers": []},
                     {"controllers": [{"ip": "a", "num_leds": 10}, {"ip": "a", "num_leds": 10}]},  # same UDP address
                     {"controllers": [{"ip": "a", "strips": [{"column": 0, "leds": 5}], "num_leds": 9}]},
                     {"controllers": [{"ip": "a", "strips": [{"column": 1, "leds": 5}]},
                                      {"ip": "b", "strips": [{"column": 1, "leds": 5}]}]},  # column twice
                     {"columns": 1, "controllers": [{"ip": "a", "strips": [{"column": 2, "leds": 5}]}]}):  # column >= columns
            with self.assertRaises(LayoutError, msg=spec):
                Layout.from_spec(spec)

    def test_apply_packet(self):
        a = bytearray(6 * 3)
        self.assertEqual(apply_packet(a, bytes([4, 2, 0, 4]) + bytes(range(1, 7))), (2, True))  # LEDs 4,5
        self.assertEqual(a[12:18], bytes(range(1, 7)))
        self.assertEqual(apply_packet(a, bytes([4, 2, 0, 0]) + bytes(3)), (2, False))
        self.assertIsNone(apply_packet(a, b"\x09\x02"))

    def test_reverse_write(self):
        lay = Layout.from_spec({"controllers": [{"ip": "a", "strips": [{"column": 0, "leds": 3, "reverse": True}]}]})
        c = lay.controllers[0]
        c.pixels[:] = bytes([1, 1, 1, 2, 2, 2, 3, 3, 3])
        lay.write(c)
        self.assertEqual(bytes(lay.cols[0]), bytes([3, 3, 3, 2, 2, 2, 1, 1, 1]))


class Bridge:
    def __init__(self, layout):
        self.http = free_port()
        self.p = subprocess.Popen(
            [sys.executable, str(HERE / "bridge.py"), "--brain", "http://127.0.0.1:1", "--wled", "none", "--no-ble",
             "--port", str(self.http), "--layout", json.dumps(layout)],
            stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
        t0 = time.time()
        while time.time() - t0 < 60:
            try:
                self.get("/live/status")
                return
            except OSError:
                if self.p.poll() is not None:
                    raise RuntimeError("bridge exited: " + self.p.stdout.read())
                time.sleep(0.2)
        raise RuntimeError("bridge did not start")

    def get(self, path):
        with urllib.request.urlopen(f"http://127.0.0.1:{self.http}{path}", timeout=3) as r:
            return json.load(r)

    def frame(self):
        return bytes.fromhex(self.get("/live/frame")["rgb"])

    def close(self):
        self.p.terminate()
        try:
            self.p.wait(5)
        except subprocess.TimeoutExpired:
            self.p.kill()
            self.p.wait()


def send(port, data):
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    s.sendto(data, ("127.0.0.1", port))
    s.close()


def wait_seq(b, n, timeout=5):
    t0 = time.time()
    while time.time() - t0 < timeout:
        if b.get("/live/status")["brain"]["seq"] >= n:
            return
        time.sleep(0.05)
    raise AssertionError(f"bridge saw fewer than {n} frames")


class EndToEnd(unittest.TestCase):
    def test_four_single_strip_controllers(self):
        ports = [free_port(True) for _ in range(4)]
        b = Bridge({"controllers": [{"ip": "127.0.0.1", "num_leds": PER, "bind": f"127.0.0.1:{p}"} for p in ports]})
        try:
            self.assertEqual(b.get("/live/status")["wled"]["leds"], 4 * PER)
            # Controller 2 only: DRGB. Others dark.
            send(ports[2], bytes([2, 2]) + b"".join(px(2, i) for i in range(PER)))
            wait_seq(b, 1)
            f = b.frame()
            self.assertEqual(len(f), 4 * PER * 3)
            col = lambda c: f[c * PER * 3:(c + 1) * PER * 3]  # noqa: E731
            self.assertEqual(col(2), b"".join(px(2, i) for i in range(PER)))
            for c in (0, 1, 3):
                self.assertEqual(col(c), bytes(PER * 3))
            # Controller 0 via DNRGB in two chunks (0..99, 100..179); a frame completes at the end.
            send(ports[0], bytes([4, 2, 0, 0]) + b"".join(px(0, i) for i in range(100)))
            send(ports[0], bytes([4, 2, 0, 100]) + b"".join(px(0, i) for i in range(100, PER)))
            wait_seq(b, 2)
            f = b.frame()
            self.assertEqual(col(0), b"".join(px(0, i) for i in range(PER)))
            self.assertEqual(col(2), b"".join(px(2, i) for i in range(PER)))
            ctl = b.get("/live/status")["brain"]["controllers"]
            self.assertEqual([c["packets"] for c in ctl], [2, 0, 1, 0])
        finally:
            b.close()

    def test_two_strips_on_one_controller_second_reversed(self):
        p0, p1 = free_port(True), free_port(True)
        b = Bridge({"columns": 4, "controllers": [
            {"ip": "127.0.0.1", "bind": f"127.0.0.1:{p0}",
             "strips": [{"column": 0, "leds": PER}, {"column": 1, "leds": PER, "reverse": True}]},
            {"ip": "127.0.0.1", "bind": f"127.0.0.1:{p1}", "strips": [{"column": 3, "leds": PER}]}]})
        try:
            # 360-LED global array in DNRGB chunks of 200 + 160: strip A = LEDs 0..179, strip B = 180..359.
            glob = [px(0, i) for i in range(PER)] + [px(1, i) for i in range(PER)]
            send(p0, bytes([4, 2, 0, 0]) + b"".join(glob[:200]))
            send(p0, bytes([4, 2, 0, 200]) + b"".join(glob[200:]))
            wait_seq(b, 1)
            f = b.frame()
            col = lambda c: f[c * PER * 3:(c + 1) * PER * 3]  # noqa: E731
            self.assertEqual(col(0), b"".join(px(0, i) for i in range(PER)))  # bottom-first as sent
            self.assertEqual(col(1), b"".join(px(1, i) for i in reversed(range(PER))))  # flipped: LED 0 was the top
            self.assertEqual(col(2), bytes(PER * 3))
            self.assertEqual(col(3), bytes(PER * 3))
            send(p1, bytes([2, 2]) + b"".join(px(3, i) for i in range(PER)))
            wait_seq(b, 2)
            self.assertEqual(b.frame()[3 * PER * 3:], b"".join(px(3, i) for i in range(PER)))
            info = b.get("/json/info")
            self.assertEqual(info["leds"]["count"], 2 * PER)  # Host 127.0.0.1 -> first controller with that host
        finally:
            b.close()


if __name__ == "__main__":
    unittest.main(verbosity=2)
