#!/usr/bin/env python3
# /// script
# requires-python = ">=3.9"
# ///
"""Render the idle breathing effect (WLED "Fade", ember -> black) to a video, no hardware.

    uv run deploy/render_idle.py OUT.mp4 [--sx 40] [--seconds 33]

Model: WLED Fade is a triangle wave between colour 1 and colour 2, cycle ~ 65.5 s / (1 + sx/8).
Master brightness scales the result; WLED's gamma (2.8) maps the 8-bit value to LED light, and the
video converts that light back to sRGB. A real strip looks different (glare, diffusion), so judge
timing and depth, not the exact glow. Needs ffmpeg on PATH.
"""
import argparse
import subprocess

W, H, FPS = 1280, 360, 30


def srgb(lin):
    return 12.92 * lin if lin <= 0.0031308 else 1.055 * lin ** (1 / 2.4) - 0.055


def level(t, cycle):
    """Triangle 0..1..0 over one cycle."""
    p = (t / cycle) % 1.0
    return 1 - abs(2 * p - 1)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("out")
    ap.add_argument("--sx", type=int, default=40)
    ap.add_argument("--bri", type=int, default=180)
    ap.add_argument("--seconds", type=float, default=33)
    ap.add_argument("--col", default="255,25,0")
    a = ap.parse_args()

    col = [int(x) for x in a.col.split(",")]
    cycle = 65.5 / (1 + a.sx / 8)
    strip_h = 120
    top = (H - strip_h) // 2
    ff = subprocess.Popen(
        ["ffmpeg", "-y", "-loglevel", "error", "-f", "rawvideo", "-pix_fmt", "rgb24", "-s", f"{W}x{H}",
         "-r", str(FPS), "-i", "-", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-crf", "18", a.out],
        stdin=subprocess.PIPE,
    )
    n = int(a.seconds * FPS)
    bg = bytes([8, 8, 10]) * W
    for i in range(n):
        t = i / FPS
        lv = level(t, cycle)
        px = []
        for c in col:
            v = round(c * lv * a.bri / 255)  # 8-bit value WLED outputs
            px.append(round(255 * srgb((v / 255) ** 2.8)))  # gamma 2.8 -> light -> sRGB
        strip = bytes(px) * W
        bar_x = int(W * (t % cycle) / cycle)
        bar = bytearray(bg)
        bar[: 3 * bar_x] = bytes([60, 60, 60]) * bar_x
        frame = bytearray()
        for y in range(H):
            if top <= y < top + strip_h:
                frame += strip
            elif y >= H - 12:
                frame += bar
            else:
                frame += bg
        ff.stdin.write(bytes(frame))
    ff.stdin.close()
    ff.wait()
    print(f"{a.out}: {n / FPS:.0f} s, cycle {cycle:.1f} s")


main()
