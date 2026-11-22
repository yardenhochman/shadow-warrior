#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.10"
# dependencies = []
# ///
"""Generate docs/wiring.svg, the perfboard schematic (see HARDWARE.md for the netlist)."""
from html import escape
from pathlib import Path

W, H = 1000, 760
o: list[str] = []
INK, SIG, PWR, GRN = "#1f2937", "#2563eb", "#dc2626", "#6b7280"


def add(s):
    o.append(s)


def line(x1, y1, x2, y2, c=INK, w=1.6, dash=None):
    d = f' stroke-dasharray="{dash}"' if dash else ""
    add(f'<line x1="{x1}" y1="{y1}" x2="{x2}" y2="{y2}" stroke="{c}" stroke-width="{w}"{d}/>')


def poly(pts, c=INK, w=1.6):
    add(f'<polyline points="{" ".join(f"{x},{y}" for x, y in pts)}" fill="none" stroke="{c}" stroke-width="{w}"/>')


def text(x, y, s, size=12, anchor="start", c=INK, bold=False):
    b = ' font-weight="bold"' if bold else ""
    add(f'<text x="{x}" y="{y}" font-size="{size}" text-anchor="{anchor}" fill="{c}"{b}>{escape(s)}</text>')


def rect(x, y, w, h, fill="none", c=INK, sw=1.6, r=3):
    add(f'<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="{r}" fill="{fill}" stroke="{c}" stroke-width="{sw}"/>')


def dot(x, y, c=INK):
    add(f'<circle cx="{x}" cy="{y}" r="3.4" fill="{c}"/>')


def gnd(x, y, d=1):
    """ground symbol starting at (x, y), growing in direction d (+1 down, -1 up)."""
    line(x, y, x, y + 8 * d, GRN)
    for i, hw in enumerate((10, 6, 2)):
        line(x - hw, y + (8 + i * 4) * d, x + hw, y + (8 + i * 4) * d, GRN)


def resistor_h(x, y, label, val):
    rect(x, y - 8, 44, 16, "var(--bg)")
    text(x + 22, y - 13, f"{label} {val}", 11, "middle")


def cap(x, y, d, ref, val, side=1):
    """capacitor hanging from the node (x, y) in direction d (+1 down, -1 up), ends in a ground symbol."""
    line(x, y, x, y + 12 * d, PWR)
    line(x - 9, y + 12 * d, x + 9, y + 12 * d)
    line(x - 9, y + 18 * d, x + 9, y + 18 * d)
    line(x, y + 18 * d, x, y + 28 * d, GRN)
    gnd(x, y + 28 * d, d)
    text(x + 14 * side, y + 14 * d + 4, ref, 11, "start" if side > 0 else "end")
    text(x + 14 * side, y + 14 * d + 4 + 13 * (1 if d > 0 else -1), val, 10, "start" if side > 0 else "end", GRN)


# ---- title -----------------------------------------------------------------------
text(30, 34, "Sensor station perfboard (5 x 7 cm): wiring", 18, bold=True)
text(30, 54, "Nets and parts: HARDWARE.md. All GND symbols meet at ONE star pad; the radar GND has its own wire to it.", 12, c=GRN)

# ---- ESP32 devkit ------------------------------------------------------------------
ex0, ex1, ey0, ey1 = 440, 640, 70, 600
rect(ex0, ey0, ex1 - ex0, ey1 - ey0, "#f3f4f6")
text((ex0 + ex1) / 2, 100, "ESP32-WROOM devkit", 14, "middle", bold=True)
text((ex0 + ex1) / 2, 118, "38-pin, USB at the bottom", 11, "middle", GRN)
left = {"3V3": 140, "GPIO33": 230, "GPIO25": 270, "GPIO26": 310, "GPIO27": 350, "GND": 530, "5V": 570}

for n, y in left.items():
    line(ex0 - 12, y, ex0, y)
    text(ex0 + 8, y + 4, n, 11)
text(ex0 + 8, 372, "optional: radar OUT (unused)", 10, c=GRN)

# ---- mic connectors ----------------------------------------------------------------
pin_names = ["1 VMIC", "2 GND", "3 BCLK", "4 LRCL", "5 DOUT", "6 GND"]
rails = {"GND": 110, "VMIC": 140, "BCLK": 170, "LRCL": 200, "DOUT": 230}
pin_rail = ["VMIC", "GND", "BCLK", "LRCL", "DOUT", "GND"]
rail_col = {"GND": GRN, "VMIC": PWR, "BCLK": SIG, "LRCL": SIG, "DOUT": SIG}
conns = {"J_MIC_A": 170, "J_MIC_B": 380}
for name, y0 in conns.items():
    rect(20, y0 - 12, 64, 6 * 22 + 6, "#f3f4f6")
    text(52, y0 - 18, name, 12, "middle", bold=True)
    for i, (pn, rn) in enumerate(zip(pin_names, pin_rail)):
        y = y0 + i * 22
        text(26, y + 4, pn, 10)
        line(84, y, rails[rn], y, rail_col[rn])
        dot(rails[rn], y, rail_col[rn])
text(30, 74, "Mics: A has SEL to GND (left), B has SEL to 3V (right); 100 nF 3V/GND at each mic; cables <= 10 cm.", 12, c=GRN)

# rails (vertical)
top = {"GND": 170, "VMIC": 140, "BCLK": 170, "LRCL": 170, "DOUT": 170}
bot = {"GND": 560, "VMIC": 502, "BCLK": 502, "LRCL": 502, "DOUT": 502}
for rn, x in rails.items():
    line(x, top[rn], x, bot[rn], rail_col[rn], 2.0)
    text(x, 158 if rn != "VMIC" else 128, rn, 10, "middle", rail_col[rn])
gnd(rails["GND"], 560)
text(rails["GND"] + 16, 580, "star GND pad", 10, "middle", GRN)

# ---- 3V3 filter: devkit 3V3 -> R3 -> VMIC rail ----------------------------------------
y = left["3V3"]
poly([(ex0 - 12, y), (404, y)], PWR)
resistor_h(360, y, "R3", "22 ohm")
poly([(360, y), (140, y)], PWR)
dot(140, y, PWR)
cap(320, y, -1, "C1", "10 uF", -1)
dot(320, y, PWR)
cap(270, y, -1, "C2", "100 nF", -1)
dot(270, y, PWR)
text(330, 190, "mic supply RC filter, fc ~ 700 Hz", 10, "middle", GRN)

# ---- I2S signals from devkit -----------------------------------------------------------
for pin, rail, val, ref in (("GPIO33", "DOUT", None, None), ("GPIO25", "LRCL", "33 ohm", "R2"), ("GPIO26", "BCLK", "33 ohm", "R1")):
    y = left[pin]
    x_end = rails[rail]
    poly([(ex0 - 12, y), (x_end, y)], SIG)
    dot(x_end, y, SIG)
    if val:
        resistor_h(330, y, ref, val)
text(ex0 - 20, left["GPIO33"] - 8, "data in (both mics, tri-state)", 10, "end", GRN)

# GND pin of devkit to star pad
y = left["GND"]
poly([(ex0 - 12, y), (110, y)], GRN)
dot(110, y, GRN)

# ---- radar ---------------------------------------------------------------------------------
rx0, ry0 = 790, 360
rect(rx0, ry0, 100, 160, "#f3f4f6")
text(rx0 + 50, ry0 - 8, "J_RADAR (LD2410)", 12, "middle", bold=True)
pins = ["TX", "RX", "OUT", "GND", "VCC"]  # board silkscreen order
py = {n: ry0 + 30 + i * 26 for i, n in enumerate(pins)}
for n in pins:
    line(rx0 - 20, py[n], rx0, py[n], SIG if n in ("TX", "RX") else (PWR if n == "VCC" else GRN))
    text(rx0 + 8, py[n] + 4, n, 11)
# UART (crossed: radar TX -> ESP RX GPIO16, ESP TX GPIO17 -> radar RX)
right = {"GPIO16": py["TX"], "GPIO17": py["RX"]}
for n, y in right.items():
    line(ex1, y, ex1 + 12, y)
    text(ex1 - 8, y + 4, n, 11, "end")
    poly([(ex1 + 12, y), (rx0 - 20, y)], SIG)
text(ex1 + 24, py["TX"] - 6, "TX -> GPIO16 (RX)", 10, c=SIG)
text(ex1 + 24, py["RX"] + 16, "GPIO17 (TX) -> RX", 10, c=SIG)
text(rx0 - 26, py["OUT"] + 4, "OUT: n/c", 10, "end", GRN)
# 5 V trunk from the devkit 5V pin, along the bottom, to VCC
yt = 640
poly([(ex0 - 12, left["5V"]), (400, left["5V"]), (400, yt), (745, yt), (745, py["VCC"]), (rx0 - 20, py["VCC"])], PWR)
text(570, yt - 8, "5 V (devkit 5V pin) to radar VCC", 10, "middle", PWR)
cap(600, yt, 1, "C3", "100 uF", 1)
dot(600, yt, PWR)
cap(660, yt, 1, "C4", "100 nF", 1)
dot(660, yt, PWR)
# radar GND: its own wire to the star pad
poly([(rx0 - 20, py["GND"]), (725, py["GND"]), (725, 725), (110, 725), (110, 590)], GRN)
text(430, 742, "radar GND: its own wire back to the star pad", 10, "middle", GRN)

svg = (
    f'<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}" viewBox="0 0 {W} {H}" '
    f'font-family="ui-monospace, Menlo, monospace">'
    f'<style>:root{{--bg:#fff}}</style><rect width="{W}" height="{H}" fill="#ffffff"/>' + "".join(o) + "</svg>"
)
out = Path(__file__).parent / "wiring.svg"
out.write_text(svg)
print("wrote", out)
