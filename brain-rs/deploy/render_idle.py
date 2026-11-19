#!/usr/bin/env python3
# /// script
# requires-python = ">=3.9"
# ///
"""Render the idle breathing effect (WLED "Fade", ember -> black) to a video, no hardware.

    uv run deploy/render_idle.py OUT.mp4 [--sx 40] [--seconds 33]
    uv run deploy/render_idle.py OUT.mp4 --compare --period 5     # three shapes, one above the other
                                          (top: Fade triangle, middle: Sine effect cubic wave, bottom: pure sine)

Model: WLED Fade (0.15.4 FX.cpp mode_fade) is a triangle wave between colour 1 and colour 2:
counter = now_ms * ((sx >> 3) + 10), lum = triwave16(counter) >> 8, so one cycle is
65.536 s / ((sx >> 3) + 10): sx 40 -> 4.4 s, the slowest possible (sx < 8) -> 6.6 s.
Master brightness scales the result; WLED's gamma (2.8) maps the 8-bit value to LED light, and the
video converts that light back to sRGB. A real strip looks different (glare, diffusion), so judge
timing and depth, not the exact glow. Needs ffmpeg on PATH.
"""
import argparse
import math
import subprocess

W, H, FPS = 1280, 360, 30


def srgb(lin):
    return 12.92 * lin if lin <= 0.0031308 else 1.055 * lin ** (1 / 2.4) - 0.055


def level(t, cycle, shape="triangle"):
    """0..1..0 over one cycle, starting and ending at 0."""
    p = (t / cycle) % 1.0
    tri = 1 - abs(2 * p - 1)
    if shape == "triangle":
        return tri
    if shape == "cubic":  # WLED cubicwave8: ease8InOutCubic(triwave8) = smoothstep of the triangle
        return tri * tri * (3 - 2 * tri)
    return 0.5 - 0.5 * math.cos(2 * math.pi * p)  # sine


def px_for(col, lv, bri):
    out = []
    for c in col:
        v = round(c * lv * bri / 255)  # 8-bit value WLED outputs
        out.append(round(255 * srgb((v / 255) ** 2.8)))  # gamma 2.8 -> light -> sRGB
    return bytes(out)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("out")
    ap.add_argument("--sx", type=int, default=40)
    ap.add_argument("--bri", type=int, default=180)
    ap.add_argument("--seconds", type=float, default=33)
    ap.add_argument("--col", default="255,25,0")
    ap.add_argument("--compare", action="store_true", help="three bands: triangle, cubic (WLED Sine effect), sine")
    ap.add_argument("--period", type=float, help="cycle length in seconds (default: the Fade cycle for --sx)")
    a = ap.parse_args()

    col = [int(x) for x in a.col.split(",")]
    cycle = a.period or 65.536 / ((a.sx >> 3) + 10)
    strip_h = 120 if not a.compare else 80
    ff = subprocess.Popen(
        ["ffmpeg", "-y", "-loglevel", "error", "-f", "rawvideo", "-pix_fmt", "rgb24", "-s", f"{W}x{H}",
         "-r", str(FPS), "-i", "-", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-crf", "18", a.out],
        stdin=subprocess.PIPE,
    )
    n = int(a.seconds * FPS)
    bg = bytes([8, 8, 10]) * W
    shapes = ["triangle", "cubic", "sine"] if a.compare else ["triangle"]
    gap = 12
    block = strip_h * len(shapes) + gap * (len(shapes) - 1)
    top = (H - block) // 2
    for i in range(n):
        t = i / FPS
        bands = [px_for(col, level(t, cycle, sh), a.bri) * W for sh in shapes]
        bar_x = int(W * (t % cycle) / cycle)
        bar = bytearray(bg)
        bar[: 3 * bar_x] = bytes([60, 60, 60]) * bar_x
        frame = bytearray()
        for y in range(H):
            k = (y - top) // (strip_h + gap) if top <= y < top + block else -1
            in_band = k >= 0 and (y - top) - k * (strip_h + gap) < strip_h
            if in_band:
                frame += bands[k]
            elif y >= H - 12:
                frame += bar
            else:
                frame += bg
        ff.stdin.write(bytes(frame))
    ff.stdin.close()
    ff.wait()
    print(f"{a.out}: {n / FPS:.0f} s, cycle {cycle:.1f} s")


main()
