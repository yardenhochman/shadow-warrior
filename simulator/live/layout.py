#!/usr/bin/env python3
# /// script
# requires-python = ">=3.10"
# dependencies = ["pyyaml>=6"]
# ///
"""Column/strip layout model of the bridge, the same shape as brain-rs `wled:` (config.rs).

    wled:
      columns: 4                         # optional; default = highest column + 1
      controllers:
        - {ip: "127.0.0.1", num_leds: 180}          # one strip = column 0 (column = list index)
        - ip: "127.0.0.2"                           # two strips, columns 1 and 2
          strips:                                   # consecutive in LED order
            - {column: 1, leds: 180}
            - {column: 2, leds: 180, reverse: true} # LED 0 at the top

Bridge-only extras per controller (ignored by brain-rs): `bind: "host:port"` (UDP listen address
instead of <ip host>:21324) and `forward: "host"` (real WLED that gets this controller's packets).

The canonical page frame is four columns (columns beyond the 4th are not shown, missing ones are dark), each `per` LEDs bottom-first (per = longest
strip; shorter strips and columns without data are dark), serpentine off.
"""
import json
import sys
from dataclasses import dataclass, field
from pathlib import Path

REALTIME_PORT = 21324
DNRGB_MAX = 489
PAGE_COLUMNS = 4  # the page (web/js/wled.js COLUMNS) shows exactly four columns


class LayoutError(ValueError):
    pass


def split_host_port(addr: str):
    """'host' or 'host:port' -> (host, port|None). Same rule as brain-rs realtime_addr."""
    host, sep, port = addr.rpartition(":")
    if sep and ":" not in host and port.isdigit():
        return host, int(port)
    return addr, None


@dataclass
class Strip:
    column: int
    leds: int
    reverse: bool = False
    offset: int = 0  # first LED in the controller's pixel array


@dataclass
class Controller:
    ip: str
    num_leds: int
    strips: list
    bind: tuple  # UDP listen (host, port)
    forward: str = None
    pixels: bytearray = field(default_factory=bytearray)
    last_udp: float = 0.0
    rt_timeout: float = 2.0
    packets: int = 0

    @property
    def http_host(self):
        return split_host_port(self.ip)[0]


class Layout:
    def __init__(self, controllers, columns):
        self.controllers = controllers
        self.columns = columns
        self.strip_leds = {}
        for c in controllers:
            for s in c.strips:
                self.strip_leds[s.column] = s.leds
        self.per = max(self.strip_leds.values(), default=0)
        self.cols = [bytearray(self.per * 3) for _ in range(columns)]

    @property
    def count(self):
        """LEDs in the canonical page frame."""
        return PAGE_COLUMNS * self.per

    # ---- building
    @classmethod
    def from_spec(cls, spec, udp_port=REALTIME_PORT):
        if not isinstance(spec, dict):
            raise LayoutError("layout must be an object")
        spec = spec.get("wled", spec)
        raw = spec.get("controllers")
        if not isinstance(raw, list) or not raw:
            raise LayoutError("layout needs a non-empty `controllers` list")
        ctrls, seen = [], set()
        for i, rc in enumerate(raw):
            ip = str(rc.get("ip") or "")
            if not ip:
                raise LayoutError(f"controller {i}: ip is required")
            strips, off = [], 0
            if rc.get("strips"):
                for rs in rc["strips"]:
                    s = Strip(int(rs["column"]), int(rs["leds"]), bool(rs.get("reverse", False)), off)
                    strips.append(s)
                    off += s.leds
                num = int(rc.get("num_leds") or 0)
                if num and num != off:
                    raise LayoutError(f"controller {ip}: num_leds {num} != sum of its strips ({off})")
            else:
                num = int(rc.get("num_leds") or 0)
                if num <= 0:
                    raise LayoutError(f"controller {ip}: num_leds (or strips) is required")
                strips, off = [Strip(i, num, False, 0)], num
            for s in strips:
                if s.leds <= 0:
                    raise LayoutError(f"controller {ip}: strip of column {s.column} has 0 leds")
                if s.column in seen:
                    raise LayoutError(f"column {s.column} appears twice")
                seen.add(s.column)
            if rc.get("bind"):
                h, p = split_host_port(str(rc["bind"]))
                bind = (h, p or udp_port)
            else:
                bind = (split_host_port(ip)[0], udp_port)
            ctrls.append(Controller(ip, off, strips, bind, rc.get("forward") or None, bytearray(off * 3)))
        used = max(seen) + 1
        columns = int(spec.get("columns") or 0) or used
        if columns < used:
            raise LayoutError(f"columns {columns} < highest column + 1 ({used})")
        binds = [c.bind for c in ctrls]
        if len(set(binds)) != len(binds):
            raise LayoutError("two controllers share one UDP address (give each its own ip, or `bind`): " + str(binds))
        return cls(ctrls, columns)

    @classmethod
    def load(cls, arg, udp_port=REALTIME_PORT):
        """`arg`: inline JSON/YAML object text or a path to a YAML/JSON file (a brain
        config.yaml works: its `wled:` section is used)."""
        text = arg.strip()
        if not text.startswith("{"):
            text = Path(arg).read_text()
        try:
            spec = json.loads(text)
        except ValueError:
            import yaml
            spec = yaml.safe_load(text)
        return cls.from_spec(spec, udp_port)

    # ---- running
    def controller_for_host(self, host):
        for c in self.controllers:
            if host in (c.http_host, c.bind[0]):
                return c
        return None

    def write(self, ctrl: Controller):
        """Cut the controller's pixel array into its strips into the column buffers."""
        for s in ctrl.strips:
            seg = ctrl.pixels[s.offset * 3:(s.offset + s.leds) * 3]
            if s.reverse:  # LED 0 is the top: flip to bottom-first
                seg = b"".join(seg[i:i + 3] for i in range(len(seg) - 3, -1, -3))
            col = self.cols[s.column]
            col[:len(seg)] = seg
            col[len(seg):] = bytes(len(col) - len(seg))

    def frame(self, now=None, expire=True):
        """Canonical page frame: columns in order, each `per` LEDs bottom-first. With `now`,
        controllers whose realtime timeout has passed show dark."""
        if now is not None and expire:
            for c in self.controllers:
                if now - c.last_udp >= c.rt_timeout:
                    for s in c.strips:
                        self.cols[s.column][:] = bytes(self.per * 3)
        cols = self.cols[:PAGE_COLUMNS] + [bytearray(self.per * 3)] * (PAGE_COLUMNS - self.columns)
        return b"".join(bytes(col) for col in cols)


def apply_packet(pixels: bytearray, data: bytes):
    """Apply a WLED realtime UDP packet to a global pixel array. Returns None when ignored,
    else (timeout byte, frame_complete). DRGB/DRGBW/WARLS cover the frame in one packet; a
    DNRGB frame is complete when a packet reaches the array end."""
    if len(data) < 2:
        return None
    proto, t = data[0], data[1]
    n = len(pixels) // 3
    if proto == 2:  # DRGB
        px, start = data[2:], 0
    elif proto == 4 and len(data) >= 4:  # DNRGB
        px, start = data[4:], (data[2] << 8) | data[3]
    elif proto == 3:  # DRGBW: drop white
        w = data[2:]
        px, start = b"".join(w[i:i + 3] for i in range(0, len(w) - 3, 4)), 0
    elif proto == 1:  # WARLS: index, r, g, b
        for i in range(2, len(data) - 3, 4):
            k = data[i]
            if k < n:
                pixels[k * 3:k * 3 + 3] = data[i + 1:i + 4]
        px, start = b"", None
    else:
        return None
    if start is not None:
        m = min(len(px) // 3, max(0, n - start))
        pixels[start * 3:(start + m) * 3] = px[:m * 3]
    complete = start is None or proto != 4 or start + len(px) // 3 >= n
    return t, complete


if __name__ == "__main__":
    lay = Layout.load(sys.argv[1])
    print(f"{lay.columns} columns x {lay.per} LEDs = {lay.count}")
    for c in lay.controllers:
        print(c.ip, "udp", c.bind, [(s.column, s.leds, s.reverse) for s in c.strips])
