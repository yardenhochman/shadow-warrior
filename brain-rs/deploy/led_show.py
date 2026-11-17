#!/usr/bin/env python3
# /// script
# requires-python = ">=3.9"
# dependencies = ["numpy"]
# ///
"""Stream the arena light effects to a real WLED strip from a laptop, without the brain.

    uv run deploy/led_show.py --wled 192.168.1.203                 # full demo loop
    uv run deploy/led_show.py --wled 192.168.1.203 --scene shadow  # one scene, looped
    uv run deploy/led_show.py --wled 192.168.1.203 --layout columns:4 --serpentine
    uv run deploy/led_show.py --list

Effects are the browser simulator's "sim proposal" set (simulator/web/js/wled.js) ported to
numpy, plus new scenes for the arena: the shadow that punches push back, a KO supernova and
per-column variants for the arc of columns. Punches and shouts are simulated.

Frames go out as WLED UDP realtime (DNRGB, port 21324), so the controller is never polled
(an ESP32-C3 rebooted under 4 Hz GETs). When the show stops, WLED returns to its own effect
after the realtime timeout (2 s). Stop the brain first, or the two fight over the strip.

Layouts map the strip to virtual columns, index 0 = base:
  mirror     (default) one strip lying flat: the bag is the middle, both halves grow outwards
  line       the whole strip is one column from LED 0 upwards
  columns:K  K equal segments, one per Perspex column (add --serpentine if every other
             segment runs top-down)
"""
import argparse
import math
import random
import socket
import sys
import time

import numpy as np

FPS = 40
DT = 1.0 / FPS


# ------------------------------------------------------------------ colour helpers
def hsv(h, s, v):
    """Vectorised HSV (h in degrees, s/v 0..1, broadcastable) -> (..., 3) RGB in 0..1."""
    h = np.mod(np.asarray(h, dtype=np.float32), 360.0)
    s = np.asarray(s, dtype=np.float32)
    v = np.asarray(v, dtype=np.float32)
    out = []
    for n in (5, 3, 1):
        k = np.mod(n + h / 60.0, 6.0)
        out.append(v - v * s * np.clip(np.minimum(np.minimum(k, 4 - k), 1), 0, None))
    return np.stack(np.broadcast_arrays(*out), axis=-1)


def palette(*hexes):
    return np.array([[int(h[i:i + 2], 16) / 255 for i in (1, 3, 5)] for h in hexes], np.float32)


def sample(pal, x):
    """Clamped palette lookup; x any shape -> (..., 3)."""
    x = np.clip(np.asarray(x, np.float32), 0, 1) * (len(pal) - 1)
    i = np.minimum(x.astype(int), len(pal) - 2)
    t = (x - i)[..., None]
    return pal[i] * (1 - t) + pal[i + 1] * t


HEAT = palette('#000000', '#330000', '#990000', '#ff2200', '#ff7700', '#ffcc33', '#ffffcc')
LAVA_HEAT = palette('#000000', '#3a0000', '#800000', '#c01000', '#ff4000', '#ffa500', '#fff0c0')
SHADOW = palette('#000000', '#08001a', '#1c0040', '#3a0070', '#7a00ff')
GOLD = palette('#000000', '#401000', '#c04000', '#ff9020', '#ffe0a0')


# ------------------------------------------------------------------ geometry
class Layout:
    """Virtual grid (C columns x L LEDs, base first) -> physical strip indices."""

    def __init__(self, n, spec, serpentine=False):
        self.n = n
        if spec == "mirror":
            half = n // 2
            left = np.arange(half - 1, -1, -1)            # middle -> LED 0
            right = np.arange(n - half, n)                # middle -> last LED
            self.cols = [left, right]
            self.order = [0, 0]                           # both halves equally close to the bag
        elif spec == "line":
            self.cols = [np.arange(n)]
            self.order = [0]
        elif spec.startswith("columns:"):
            k = int(spec.split(":")[1])
            size = n // k
            self.cols = []
            for c in range(k):
                idx = np.arange(c * size, (c + 1) * size)
                self.cols.append(idx[::-1] if serpentine and c % 2 else idx)
            mid = (k - 1) / 2
            self.order = [abs(c - mid) - (0.5 if k % 2 == 0 else 0) for c in range(k)]
        else:
            sys.exit(f"unknown layout {spec}")
        self.C = len(self.cols)
        self.L = min(len(c) for c in self.cols)
        self.y = np.linspace(0, 1, self.L, dtype=np.float32)[None, :].repeat(self.C, 0)
        self.ring = np.array(self.order, np.float32)[:, None]  # 0 = nearest the bag

    def to_strip(self, grid):
        out = np.zeros((self.n, 3), np.float32)
        for c, idx in enumerate(self.cols):
            out[idx[:self.L]] = grid[c]
        return out


# ------------------------------------------------------------------ building blocks
class Fire:
    """Fire2012 per column (WLED "Fire 2012" / FastLED), as in the simulator."""

    def __init__(self, C, L):
        self.h = np.zeros((C, L), np.float32)

    def step(self, cooling, sparking):
        h = self.h
        C, L = h.shape
        h -= np.random.rand(C, L).astype(np.float32) * ((cooling * 10) / L + 2) / 255 * 1.5
        np.clip(h, 0, 1, out=h)
        for k in range(L - 1, 1, -1):
            h[:, k] = (h[:, k - 1] + h[:, k - 2] * 2) / 3
        for c in range(C):
            if random.random() * 255 < sparking * 1.5:
                y = random.randrange(min(7, L))
                h[c, y] = min(1.0, h[c, y] + (160 + random.random() * 95) / 255)


class Waves:
    """Punch shockwaves travelling from the bag up/out, inner columns first."""

    def __init__(self):
        self.w = []

    def add(self, t, p):
        self.w.append((t, p))
        self.w = [x for x in self.w if t - x[0] < 2.5][-24:]

    def field(self, lay, t):
        v = np.zeros((lay.C, lay.L), np.float32)
        for t0, p in self.w:
            age = t - t0 - lay.ring * 0.07
            front = age * (1.2 + p * 2.2)
            width = 0.06 + p * 0.16
            d = front - lay.y
            hit = (age >= 0) & (d >= 0) & (d < width * 3)
            v += np.where(hit, p * np.exp(-np.clip(d, 0, None) / width) * np.exp(-np.clip(age, 0, None) * 1.6), 0)
        return v


class Puncher:
    """Simulated player: bursts of punches with rests; returns powers 0..1 for this frame."""

    def __init__(self, rate=1.6):
        self.rate = rate
        self.next = 0.6
        self.burst = 0

    def tick(self, t):
        if t < self.next:
            return None
        if self.burst <= 0:
            self.burst = random.randint(2, 6)
        self.burst -= 1
        gap = random.uniform(0.18, 0.4) if self.burst else random.uniform(0.9, 2.2) / self.rate
        self.next = t + gap
        p = random.uniform(0.3, 0.75)
        if self.burst == 0 and random.random() < 0.55:
            p = random.uniform(0.8, 1.0)  # finisher
        return p


# ------------------------------------------------------------------ scenes
# Each scene: class with duration and render(t, lay) -> (C, L, 3) in 0..1.
class Idle:
    """Sim IDLE: slow violet breathing with a wave rolling across the arc."""
    name, duration = "idle", 20

    def render(self, t, lay):
        c = np.arange(lay.C, dtype=np.float32)[:, None] + lay.ring
        breath = 0.08 + 0.32 * (0.5 + 0.5 * np.sin(t * 0.9 - c * 0.7))
        roll = 0.5 + 0.5 * np.sin(t * 1.4 - lay.y * 6 - c * 1.1)
        return hsv(250 - 25 * lay.y + 10 * math.sin(t * 0.2), 0.75, breath * (0.55 + 0.45 * roll))


class Warming:
    """Sim WARMING: shouting fills an energy bar that heats from blue to white; shout = flash."""
    name, duration = "warming", 16

    def __init__(self):
        self.energy, self.flash, self.next_shout, self.shouting = 0.0, 0.0, 0.8, 0.0

    def render(self, t, lay):
        if t >= self.next_shout:                       # a shout every ~1.5 s, 0.5-1.2 s long
            self.shouting = random.uniform(0.5, 1.2)
            self.flash = 1.0
            self.next_shout = t + self.shouting + random.uniform(0.4, 1.2)
        if self.shouting > 0:
            self.shouting -= DT
            self.energy = min(1.0, self.energy + DT * 0.16)
        else:
            self.energy = max(0.0, self.energy - DT * 0.02)
        if t > self.duration - 1.5:
            self.energy = min(1.0, self.energy + DT * 0.5)
        self.flash *= 0.05 ** DT
        y, top = lay.y, self.energy
        hue = 230 + 150 * np.minimum(y * 1.15, 1)
        sat = np.where(y > 0.9, 1 - (y - 0.9) * 8, 1)
        shimmer = 0.75 + 0.25 * np.sin(t * 10 + np.arange(lay.L)[None, :] * 0.9)
        bar = hsv(hue, sat, np.minimum(1, (0.5 + 0.5 * y) * shimmer + self.flash * 0.6))
        lvl = 1.0 if self.shouting > 0 else 0.0
        sparkle = (np.random.rand(lay.C, lay.L) < lvl * 0.12) * 0.6
        above = hsv(260, 0.6, 0.03 + 0.03 * math.sin(t * 2) + sparkle)
        head = np.abs(y - top) < 1.5 / lay.L
        out = np.where((y < top)[..., None], bar, above)
        out[head] = 0.6 + 0.4 * math.sin(t * 14)
        return out


class Fight:
    """Sim FIGHT (the proposal): fire + a shockwave per punch, full flash on hard hits,
    red warning at the tips in the last 10 s."""
    name, duration = "fight", 40

    def __init__(self):
        self.fire, self.waves, self.puncher, self.flash = None, Waves(), Puncher(), 0.0

    def render(self, t, lay):
        if self.fire is None:
            self.fire = Fire(lay.C, lay.L)
        p = self.puncher.tick(t)
        if p is not None:
            self.waves.add(t, p)
            if p > 0.75:
                self.flash = max(self.flash, p)
        self.fire.step(70, 110)
        rgb = sample(HEAT, self.fire.h * 0.85)
        pulse = 0.12 + 0.08 * math.sin(t * 8.4)
        rgb[..., 0] = np.maximum(rgb[..., 0], pulse * (1 - lay.y * 0.6))
        w = self.waves.field(lay, t)[..., None]
        rgb += w * np.array([1.0, 0.75, 0.45], np.float32)
        rgb += self.flash * 0.8
        self.flash *= 0.04 ** DT
        left = self.duration - t
        if left < 10:
            warn = 0.5 + 0.5 * math.sin(t * 12)
            tip = lay.y > 0.85
            rgb[tip, 0] = np.maximum(rgb[tip, 0], warn)
            rgb[tip, 1:] *= 0.3
        return rgb


class Shadow:
    """New: the shadow creeps in from the tips towards the bag and eats the light; every punch
    blasts it back. Keep punching and it is driven off the strip, then a victory burst."""
    name, duration = "shadow", 30

    def __init__(self):
        self.front, self.waves, self.puncher, self.won = 1.0, Waves(), Puncher(1.3), None

    def render(self, t, lay):
        p = self.puncher.tick(t) if self.won is None else None
        if p is not None:
            self.waves.add(t, p)
            self.front = min(1.05, self.front + p * (0.14 if t > 16 else 0.05))
        if self.won is None:
            self.front -= DT * (0.14 if t < 16 else 0.03)   # creeps in; later the player wins
            self.front = max(self.front, 0.12)
            if t > 16 and self.front >= 1.04:
                self.won = t
        y = lay.y
        # the player's light: warm glow from the bag
        light = sample(GOLD, (0.55 + 0.2 * np.sin(t * 3 - y * 9)) * (1 - y * 0.4))
        # the shadow beyond the front, with a flickering violet edge
        edge = np.clip(1 - np.abs(y - self.front) * lay.L / 4, 0, 1) * (0.6 + 0.4 * np.random.rand(lay.C, lay.L))
        dark = sample(SHADOW, 0.15 + 0.15 * np.sin(t * 2 + y * 20))
        out = np.where((y > self.front)[..., None], dark, light)
        out += edge[..., None] * np.array([0.5, 0.0, 1.0], np.float32)
        out += self.waves.field(lay, t)[..., None] * np.array([1.0, 0.9, 0.7], np.float32)
        if self.won is not None:
            a = t - self.won
            out += max(0.0, 1 - a * 0.8) * 0.9
            out = out * min(1.0, 0.4 + a * 0.3)
        return out


class Supernova:
    """New: the KO. A white blast from the bag, then sparks fall back like embers."""
    name, duration = "ko", 6

    def __init__(self):
        self.sparks = None

    def render(self, t, lay):
        if self.sparks is None:
            n = lay.C * 14
            self.sparks = dict(c=np.random.randint(0, lay.C, n), y=np.zeros(n),
                               v=np.random.uniform(0.6, 1.6, n), hue=np.random.uniform(0, 60, n))
        out = np.zeros((lay.C, lay.L, 3), np.float32)
        ring = np.exp(-((lay.y - t * 1.8) * 9) ** 2) * max(0.0, 1 - t / 1.5)
        out += ring[..., None] * np.array([1.0, 0.95, 0.85], np.float32)
        if t < 0.15:
            out += 1.0
        s = self.sparks
        s["v"] -= DT * 1.1           # gravity back towards the bag
        s["y"] = np.clip(s["y"] + s["v"] * DT, 0, 1)
        fade = max(0.0, 1 - t / self.duration)
        idx = np.minimum((s["y"] * (lay.L - 1)).astype(int), lay.L - 1)
        col = hsv(s["hue"], 0.9, fade)
        for k in range(len(idx)):
            if s["y"][k] > 0.01:
                out[s["c"][k], idx[k]] += col[k]
        return out


class Cooldown:
    """Sim COOLDOWN: embers cooling over the period and settling to calm blue."""
    name, duration = "cooldown", 16

    def __init__(self):
        self.fire = None

    def render(self, t, lay):
        if self.fire is None:
            self.fire = Fire(lay.C, lay.L)
            self.fire.h[:] = np.random.rand(lay.C, lay.L) * 0.7
        p = min(1.0, t / self.duration)
        self.fire.step(90 + 130 * p, 90 - 80 * p)
        rgb = sample(LAVA_HEAT, self.fire.h * (1 - p * 0.7))
        c = np.arange(lay.C, dtype=np.float32)[:, None]
        calm = p * (0.06 + 0.05 * np.sin(t * 0.8 + c)) * (1 - lay.y * 0.5)
        rgb[..., 1] += calm * 0.3
        rgb[..., 2] += calm
        return rgb


class Thunder:
    """New, for the combo: every hit throws a bolt up a random column; a long combo
    lights the whole arc storm-white."""
    name, duration = "thunder", 20

    def __init__(self):
        self.puncher, self.bolts, self.combo, self.last = Puncher(1.8), [], 0, -9

    def render(self, t, lay):
        p = self.puncher.tick(t)
        if p is not None:
            self.combo = self.combo + 1 if t - self.last < 1.0 else 1
            self.last = t
            self.bolts.append(dict(t0=t, c=random.randrange(lay.C), p=p,
                                   jag=np.random.rand(lay.L).astype(np.float32)))
        self.bolts = [b for b in self.bolts if t - b["t0"] < 0.6]
        base = hsv(215, 0.8, 0.04 + 0.02 * np.sin(t * 0.7 + lay.y * 4))
        out = np.array(base, np.float32)
        for b in self.bolts:
            a = t - b["t0"]
            reach = min(1.0, a * 14) * (0.5 + 0.5 * b["p"])
            flick = 1.0 if (a * 40) % 3 < 2 else 0.35
            on = (lay.y[b["c"]] < reach) & (b["jag"] > 0.25)
            out[b["c"], on] += np.array([0.75, 0.85, 1.0]) * flick * max(0.0, 1 - a / 0.6)
        storm = min(1.0, max(0, self.combo - 3) / 5) * max(0.0, 1 - (t - self.last) * 2)
        out += storm * 0.5 * np.array([0.7, 0.8, 1.0], np.float32)
        return out


SCENES = {s.name: s for s in (Idle, Warming, Fight, Supernova, Cooldown, Shadow, Thunder)}
PLAYLIST = ["idle", "warming", "fight", "ko", "cooldown", "shadow", "thunder"]


# ------------------------------------------------------------------ output
class Wled:
    def __init__(self, host, n, bri, gamma):
        self.addr = (host, 21324)
        self.sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        self.n, self.bri, self.gamma = n, bri, gamma

    def send(self, strip):
        px = (np.clip(strip, 0, 1) ** self.gamma * self.bri * 255).astype(np.uint8)
        per = 489  # DNRGB carries at most 489 LEDs per packet
        for start in range(0, self.n, per):
            chunk = px[start:start + per]
            hdr = bytes([4, 2, start >> 8, start & 0xff])  # DNRGB, 2 s timeout, start index
            self.sock.sendto(hdr + chunk.tobytes(), self.addr)

    def blank(self):
        self.send(np.zeros((self.n, 3), np.float32))


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--wled", help="controller host or IP")
    ap.add_argument("--leds", type=int, default=180)
    ap.add_argument("--layout", default="mirror")
    ap.add_argument("--serpentine", action="store_true")
    ap.add_argument("--scene", choices=list(SCENES), help="loop one scene instead of the playlist")
    ap.add_argument("--brightness", type=float, default=0.7, help="0..1 master scale (WLED ABL still applies)")
    ap.add_argument("--gamma", type=float, default=2.2)
    ap.add_argument("--minutes", type=float, default=0, help="stop after this long (0 = until Ctrl-C)")
    ap.add_argument("--list", action="store_true")
    a = ap.parse_args()
    if a.list:
        for name, s in SCENES.items():
            print(f"{name:9} {s.duration:3}s  {s.__doc__.split(chr(10))[0]}")
        return
    if not a.wled:
        ap.error("--wled is required")
    lay = Layout(a.leds, a.layout, a.serpentine)
    out = Wled(a.wled, a.leds, a.brightness, a.gamma)
    order = [a.scene] if a.scene else PLAYLIST
    stop_at = time.monotonic() + a.minutes * 60 if a.minutes else float("inf")
    i = 0
    try:
        while time.monotonic() < stop_at:
            scene = SCENES[order[i % len(order)]]()
            print(f"{time.strftime('%H:%M:%S')} {scene.name}", flush=True)
            t0 = time.monotonic()
            nxt = t0
            while (t := time.monotonic() - t0) < scene.duration and time.monotonic() < stop_at:
                out.send(lay.to_strip(scene.render(t, lay)))
                nxt += DT
                time.sleep(max(0.0, nxt - time.monotonic()))
            i += 1
    except KeyboardInterrupt:
        pass
    finally:
        out.blank()


if __name__ == "__main__":
    main()
