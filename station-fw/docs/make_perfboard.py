#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.10"
# dependencies = []
# ///
"""Hole-by-hole perfboard layout for the sensor station: the single source of truth.

    uv run docs/make_perfboard.py        # validate, then write docs/perfboard.svg and docs/PERFBOARD.md

Parts, pin nets, solder bridges, bare links and insulated wires are data below. Every run
validates the layout (one net per hole, every lead in a hole of its net, every connection inside
one net, every net connected, antenna keep-out, body overlaps, mounting-hole clearance, GPIO
assignment, the build sheet's own continuity/isolation checks) and exits non-zero on an error.

Board: PY-5CM*7CM single-sided, isolated round pads, 24 columns x 18 rows at 2.54 mm. Hole
(col,row): col 1..24 = silkscreen letters A..X, row 1..18 = silkscreen 01..18. The drawing is the
COMPONENT side (column A on the left). The silkscreen and the copper are on the other side, where
the board is mirrored left-right (A on the right); the letters identify the same holes on both sides.
"""
from __future__ import annotations

import math
import sys
from dataclasses import dataclass, field
from html import escape
from pathlib import Path

HERE = Path(__file__).parent
COLS, ROWS, PITCH = 24, 18, 2.54
BOARD_W, BOARD_H = 70.0, 50.0
X0 = (BOARD_W - (COLS - 1) * PITCH) / 2  # mm from the left edge to col 1
Y0 = (BOARD_H - (ROWS - 1) * PITCH) / 2  # mm from the top edge to row 1
G_LEFT, G_TOP = 1 - X0 / PITCH, 1 - Y0 / PITCH  # board edges in grid units
G_RIGHT, G_BOTTOM = COLS + X0 / PITCH, ROWS + Y0 / PITCH
MOUNT_IN = 3.0  # mounting hole centres, mm in from each edge (MEASURE)
MOUNT_R = 1.0  # clearance radius in pitches (5 mm standoff under the board, M2 head on top)
MOUNTS = [(1 + (x - X0) / PITCH, 1 + (y - Y0) / PITCH) for x in (MOUNT_IN, BOARD_W - MOUNT_IN) for y in (MOUNT_IN, BOARD_H - MOUNT_IN)]
KEEPOUT = (0.5, 2.5, 5.5, 16.5)  # antenna keep-out, grid rect (c0, r0, c1, r1): cols 1-5, rows 3-16
LOW_MAX_MM = 6.0  # max part height under the devkit


def hole(c: int, r: int) -> str:
    """silkscreen name of a hole, e.g. (14, 15) -> N15."""
    return f"{chr(64 + c)}{r:02d}"


Hole = tuple[int, int]

# ---- nets -------------------------------------------------------------------------------------
NET_COL = {
    "3V3": "#991b1b", "VMIC": "#ef4444", "5V": "#ea580c", "GND": "#374151",
    "BCLK": "#1e3a8a", "LRCL": "#0891b2", "DOUT": "#2563eb", "RADAR_TX": "#0369a1", "RADAR_RX": "#38bdf8",
}
NET_DESC = {
    "3V3": "devkit 3V3 (unfiltered), filter input only",
    "VMIC": "filtered mic supply (after R3a||R3b = 50 ohm)",
    "5V": "devkit 5V (USB) to the radar VCC",
    "GND": "ground, one star at S16",
    "BCLK": "I2S bit clock, GPIO26",
    "LRCL": "I2S word select, GPIO25",
    "DOUT": "I2S data from both mics, GPIO33",
    "RADAR_TX": "radar TX -> GPIO16 (ESP RX)",
    "RADAR_RX": "GPIO17 (ESP TX) -> radar RX",
}

# ---- devkit (ESP32-DevKitC clone, WROOM-32D, 38 pins, USB at the right) -------------------------
DK_TOP = "GND 23 22 TX0 RX0 21 GND 19 18 5 17 16 4 0 2 15 D1 D0 CLK".split()  # row 4, cols 6..24
DK_BOT = "3V3 EN VP VN 34 35 32 33 25 26 27 14 12 GND 13 D2 D3 CMD 5V".split()  # row 15, cols 6..24
DK_ROW_T, DK_ROW_B, DK_COL0 = 4, 15, 6
# what the firmware expects (README.md pin table); checked against the devkit lead nets below
GPIO_EXPECT = {"B:3V3": "3V3", "B:33": "DOUT", "B:25": "LRCL", "B:26": "BCLK", "B:GND": "GND", "B:5V": "5V", "T:16": "RADAR_TX", "T:17": "RADAR_RX"}
DK_PCB = (5.15, 3.3, 24.85, 15.7)  # devkit PCB outline (50 x 31.5 mm), grid rect
DK_ANT = (2.8, 6.0, 5.15, 13.0)  # module antenna overhang, grid rect


@dataclass
class Part:
    ref: str
    kind: str  # devkit, socket, resistor, ceramic, electrolytic, header
    value: str
    leads: dict[str, Hole]  # pin -> hole
    nets: dict[str, str | None]  # pin -> net (None: not connected on the perfboard)
    body: tuple  # ("rect", c0, r0, c1, r1) or ("circle", c, r, radius), component side, grid units
    height: float  # mm above the board
    label: str = ""
    pin_names: dict[str, str] = field(default_factory=dict)
    plus: str | None = None  # electrolytic + lead


def rect_body(c0, r0, c1, r1):
    return ("rect", c0, r0, c1, r1)


def devkit() -> Part:
    leads, nets, names = {}, {}, {}
    for row, side, pins in ((DK_ROW_T, "T", DK_TOP), (DK_ROW_B, "B", DK_BOT)):
        for i, p in enumerate(pins):
            key = f"{side}{i + 1}"
            leads[key] = (DK_COL0 + i, row)
            names[key] = p
            nets[key] = None
    used = {("B", "3V3"): "3V3", ("B", "33"): "DOUT", ("B", "25"): "LRCL", ("B", "26"): "BCLK", ("B", "GND"): "GND",
            ("B", "5V"): "5V", ("T", "16"): "RADAR_TX", ("T", "17"): "RADAR_RX"}
    for key, p in names.items():
        nets[key] = used.get((key[0], p))
    return Part("U1", "devkit", "ESP32-DevKitC WROOM-32D", leads, nets, rect_body(*DK_PCB), 0.0, "U1 devkit", names)


MIC_PINS = {1: "VMIC", 2: "GND", 3: "BCLK", 4: "LRCL", 5: "DOUT", 6: "GND"}  # cable pin -> net
MIC_PIN1_COL = 17  # pin k sits in column MIC_PIN1_COL + 1 - k (pin 1 on the right, toward the USB end)
MIC_ROWS = {"A": 17, "B": 18}  # one 1x6 housing per row
RADAR_PINS = [("TX", "RADAR_TX"), ("RX", "RADAR_RX"), ("OUT", None), ("GND", "GND"), ("VCC", "5V")]  # LD2410C board order
RADAR_COL0, RADAR_ROW = 15, 1

PARTS: list[Part] = [
    devkit(),
    Part("R3a", "resistor", "100 ohm", {"1": (6, 17), "2": (10, 17)}, {"1": "3V3", "2": "VMIC"}, rect_body(6.76, 16.52, 9.24, 17.48), 2.5, "R3a 100"),
    Part("R3b", "resistor", "100 ohm", {"1": (6, 18), "2": (10, 18)}, {"1": "3V3", "2": "VMIC"}, rect_body(6.76, 17.52, 9.24, 18.48), 2.5, "R3b 100"),
    Part("C2", "ceramic", "100 nF", {"1": (18, 18), "2": (18, 17)}, {"1": "VMIC", "2": "GND"}, rect_body(17.6, 16.7, 18.4, 18.3), 5.0, "C2"),
    Part("C1", "electrolytic", "10 uF >= 6.3 V (5 mm can)", {"+": (20, 18), "-": (20, 17)}, {"+": "VMIC", "-": "GND"}, ("circle", 20, 17.5, 0.98), 11.0, "C1", plus="+"),
    Part("TP1", "header", "1x2 male pin header", {"1": (22, 17), "2": (22, 18)}, {"1": "GND", "2": "VMIC"}, rect_body(21.5, 16.5, 22.5, 18.5), 8.5, "TP1", {"1": "GND", "2": "VMIC"}),
    Part("J1", "header", "2x6 male pin header (two 1x6 Dupont housings)",
         {f"{row}{k}": (MIC_PIN1_COL + 1 - k, r) for row, r in MIC_ROWS.items() for k in MIC_PINS},
         {f"{row}{k}": MIC_PINS[k] for row in MIC_ROWS for k in MIC_PINS},
         rect_body(MIC_PIN1_COL - 5.5, 16.5, MIC_PIN1_COL + 0.5, 18.5), 8.5, "J1 MIC",
         {f"{row}{k}": f"{k} {MIC_PINS[k]}" for row in MIC_ROWS for k in MIC_PINS}),
    Part("J2", "header", "1x5 male pin header (radar cable, OUT not used)",
         {str(i + 1): (RADAR_COL0 + i, RADAR_ROW) for i in range(5)},
         {str(i + 1): n for i, (_, n) in enumerate(RADAR_PINS)},
         rect_body(RADAR_COL0 - 0.5, 0.5, RADAR_COL0 + 4.5, 1.5), 8.5, "J2 RADAR",
         {str(i + 1): p for i, (p, _) in enumerate(RADAR_PINS)}),
    Part("C4", "ceramic", "100 nF", {"1": (20, 1), "2": (20, 2)}, {"1": "5V", "2": "GND"}, rect_body(19.6, 0.7, 20.4, 2.3), 5.0, "C4"),
    Part("C3", "electrolytic", "470 uF >= 10 V (8 mm can max)", {"+": (22, 1), "-": (22, 2)}, {"+": "5V", "-": "GND"}, ("circle", 22, 1.5, 1.57), 12.0, "C3", plus="+"),
]
# the female sockets are their own bodies (the devkit PCB above them is checked as a height envelope)
SOCKETS = [("X1", DK_ROW_T), ("X2", DK_ROW_B)]


@dataclass
class Conn:
    kind: str  # bridge (solder blob, adjacent pads), link (bare tinned wire, straight), wire (insulated)
    net: str
    pts: list[tuple[float, float]]  # grid points; first and last are holes
    ref: str = ""
    route: str = ""  # words for the build sheet
    label_at: tuple[float, float] | None = None


def bridge(a: Hole, b: Hole, net: str) -> Conn:
    return Conn("bridge", net, [a, b])


def link(a: Hole, b: Hole, net: str, ref: str, route: str = "", label_at=None) -> Conn:
    return Conn("link", net, [a, b], ref, route, label_at)


def wire(ref: str, net: str, pts, route: str, label_at=None) -> Conn:
    return Conn("wire", net, list(pts), ref, route, label_at)


CONNS: list[Conn] = [
    # star ground: devkit GND pin S15 - star S16 - GND rail S17
    bridge((19, 15), (19, 16), "GND"),
    bridge((19, 16), (19, 17), "GND"),
    # mic header: each column's two pads carry the same net
    *[bridge((MIC_PIN1_COL + 1 - k, 17), (MIC_PIN1_COL + 1 - k, 18), MIC_PINS[k]) for k in MIC_PINS],
    # R3a || R3b
    bridge((6, 17), (6, 18), "3V3"),
    bridge((10, 17), (10, 18), "VMIC"),
    # rails beside the mic header
    link((17, 18), (22, 18), "VMIC", "L1", "row 18, Q18 to V18 (J1 pin 1, C2, C1 +, TP1 VMIC)", (21, 18.45)),
    link((18, 17), (22, 17), "GND", "L2", "row 17, R17 to V17 (C2, star bridge, C1 -, TP1 GND)", (21, 16.55)),
    link((19, 1), (23, 1), "5V", "L3", "row 1, S01 to W01 (J2 VCC, C4, C3 +, end of W8)", (21, 0.55)),
    # insulated wires (copper side)
    wire("W1", "3V3", [(6, 15), (6, 17)], "straight down column F over F16", (6.45, 16)),
    wire("W2", "VMIC", [(10, 18), (10, 18.95), (19, 18.95), (19, 18)], "from J18 down into the bottom margin, along the edge under J1, up into S18", (14.5, 18.95)),
    wire("W3", "GND", [(12, 18), (12, 18.5), (16, 18.5), (16, 18)], "L18 down into the margin, along under J1 (inside W2), up into P18", (14, 18.5)),
    wire("W4", "GND", [(16, 17), (16, 16), (19, 16)], "P17 up to P16, right along row 16 to the star S16", (17.5, 16)),
    wire("W5", "DOUT", [(13, 15), (13, 17)], "straight down column M over M16"),
    wire("W6", "LRCL", [(14, 15), (14, 17)], "straight down column N over N16"),
    wire("W7", "BCLK", [(15, 15), (15, 17)], "straight down column O over O16"),
    wire("W8", "5V", [(24, 15), (24.6, 15), (24.6, 2.5), (23, 2.5), (23, 1)],
         "X15 out to the right edge (outside column X), up the edge, in between rows 2 and 3, up into W01", (24.6, 9.5)),
    wire("W9", "GND", [(18, 1), (18, 2), (20, 2)], "R01 down to R02, right to T02 (radar GND to C4)", (19, 2)),
    wire("W10", "GND", [(20, 2), (22, 2)], "T02 right to V02 (C4 to C3 -)", (21, 2)),
    wire("W11", "GND", [(22, 2), (22, 3), (23.5, 3), (23.5, 16), (19, 16)],
         "V02 down to V03, right to between W and X, down between the header pins W04/X04 and W15/X15, left along row 16 to the star S16", (23.5, 9.5)),
    wire("W12", "RADAR_RX", [(16, 1), (16, 4)], "straight down column P (J2 RX to GPIO17)"),
    wire("W13", "RADAR_TX", [(15, 1), (15, 2.5), (17, 2.5), (17, 4)], "O01 down, right between rows 2 and 3 (crosses W12), down into Q04 (J2 TX to GPIO16)"),
]

# ---- build steps: (title, parts, connections, instructions, beep pairs, must-not-beep pairs) ----------------------
STEPS = [
    ("Devkit sockets", ["X1", "X2"], [],
     "Plug both 19-pin female headers onto the devkit, put them through rows 04 and 15, columns F..X "
     "(F04 and F15 are pin 1 at the antenna end, X04/X15 at the USB end). Check from the component side that the "
     "USB end is toward column X. Solder the four end pins, unplug the devkit (heat), then solder the rest.",
     [], [((19, 15), (18, 15)), ((19, 15), (20, 15)), ((24, 15), (23, 15)), ((6, 15), (7, 15)), ((16, 4), (17, 4))]),
    ("Star ground", [], ["bridge:GND:(19, 15)", "bridge:GND:(19, 16)"],
     "Solder blobs S15-S16 and S16-S17. S16 is the star: later W4 (mics) and W11 (radar) end here. Keep R16/T16 clean.",
     [((19, 15), (19, 17))], [((19, 16), (18, 15)), ((19, 16), (20, 15)), ((19, 16), (18, 16)), ((19, 16), (20, 16))]),
    ("Mic supply filter", ["R3a", "R3b"], ["bridge:3V3:(6, 17)", "bridge:VMIC:(10, 17)", "W1"],
     "R3a F17-J17 and R3b F18-J18 lying flat. Bridge F17-F18 and J17-J18. Wire W1 F15 to F17 (the devkit 3V3 pin).",
     [((6, 15), (6, 18))], [((6, 15), (7, 15)), ((6, 15), (19, 15))]),
    ("Mic header and I2S", ["J1"], [f"bridge:{MIC_PINS[k]}:({MIC_PIN1_COL + 1 - k}, 17)" for k in MIC_PINS] + ["W5", "W6", "W7"],
     "J1 (2x6 male) in L17..Q18, short pins on the copper side. Bridge each column L..Q from row 17 to row 18 and "
     "nothing sideways. Wires W5 M15-M17 (GPIO33 DOUT), W6 N15-N17 (GPIO25 LRCL), W7 O15-O17 (GPIO26 BCLK).",
     [((13, 15), (13, 18)), ((14, 15), (14, 18)), ((15, 15), (15, 18))],
     [((12, 17), (13, 17)), ((13, 17), (14, 17)), ((14, 17), (15, 17)), ((15, 17), (16, 17)), ((16, 17), (17, 17)),
      ((12, 18), (13, 18)), ((16, 18), (17, 18)), ((13, 15), (14, 15)), ((14, 15), (15, 15))]),
    ("Mic decoupling, rails, test pins", ["C2", "C1", "TP1"], ["L1", "L2", "W2", "W3", "W4"],
     "C2 R17/R18, C1 T17 (-) / T18 (+): the stripe (-) toward row 17. TP1 (2 pins) V17 GND / V18 VMIC. "
     "Bare links L1 along row 18 Q18..V18 and L2 along row 17 R17..V17: lay them straight, solder at every pad, "
     "then check the gap between the two rows along its whole length. Wires W2 (VMIC, outer margin), W3 (GND, inner margin), W4.",
     [((10, 18), (17, 17)), ((10, 18), (22, 18)), ((12, 17), (19, 15)), ((16, 17), (19, 16)), ((22, 17), (19, 15))],
     [((22, 17), (22, 18)), ((17, 17), (18, 17)), ((19, 17), (19, 18)), ((6, 15), (19, 15))]),
    ("Radar connector, bulk cap, 5 V", ["J2", "C4", "C3"], ["L3", "W8", "W9", "W10", "W11"],
     "J2 (1x5 male) O01..S01, pin 1 (TX) at O01. C4 T01/T02. C3 V01 (+) / V02 (-): stripe toward row 02. "
     "Bare link L3 along row 01 S01..W01. W8 from the devkit 5V pin X15 along the right edge (outside the pads) to W01. "
     "W9 R01-T02, W10 T02-V02, W11 V02 to the star S16 (between the header pins W/X). Radar GND has its own wire to the star.",
     [((24, 15), (19, 1)), ((24, 15), (22, 1)), ((18, 1), (19, 16)), ((22, 2), (19, 15))],
     [((19, 1), (18, 1)), ((20, 1), (20, 2)), ((22, 1), (22, 2)), ((24, 15), (19, 15))]),
    ("Radar UART", [], ["W12", "W13"],
     "W12 P01 to P04 (J2 RX to GPIO17), W13 O01 to Q04 (J2 TX to GPIO16). They cross once: both insulated.",
     [((16, 1), (16, 4)), ((15, 1), (17, 4))],
     [((15, 1), (16, 1)), ((16, 1), (17, 1)), ((16, 4), (17, 4)), ((15, 1), (16, 4))]),
]
FINAL_ISOLATION = [  # power nets pairwise, devkit NOT plugged in, no mics/radar
    ("3V3", (6, 15), "GND", (19, 15), "open (no beep)"),
    ("VMIC", (22, 18), "GND", (22, 17), "starts low, climbs (C1 charging), ends > 10 kohm; never a steady 0"),
    ("5V", (24, 15), "GND", (19, 15), "starts low, climbs (C3 470 uF charges slowly), ends > 10 kohm"),
    ("3V3", (6, 15), "VMIC", (22, 18), "50 ohm (R3a and R3b in parallel); 100 ohm means one is open"),
    ("3V3", (6, 15), "5V", (24, 15), "open"),
    ("VMIC", (22, 18), "5V", (24, 15), "open"),
    ("BCLK", (15, 15), "GND", (19, 15), "open"),
    ("LRCL", (14, 15), "GND", (19, 15), "open"),
    ("DOUT", (13, 15), "GND", (19, 15), "open"),
    ("BCLK", (15, 15), "LRCL", (14, 15), "open"),
    ("LRCL", (14, 15), "DOUT", (13, 15), "open"),
    ("RADAR_TX", (17, 4), "RADAR_RX", (16, 4), "open"),
    ("RADAR_TX", (17, 4), "GND", (19, 15), "open"),
    ("RADAR_RX", (16, 4), "5V", (24, 15), "open"),
]


# ==== validation ====================================================================================
class Report:
    def __init__(self):
        self.errors: list[str] = []
        self.warnings: list[str] = []
        self.info: list[str] = []

    def err(self, s):
        self.errors.append(s)


def body_bbox(b):
    if b[0] == "rect":
        return b[1:]
    _, c, r, rad = b
    return (c - rad, r - rad, c + rad, r + rad)


def bodies_overlap(a, b) -> bool:
    if a[0] == "circle" and b[0] == "circle":
        return math.dist(a[1:3], b[1:3]) < a[3] + b[3]
    if a[0] == "circle" or b[0] == "circle":
        circ, rect = (a, b) if a[0] == "circle" else (b, a)
        _, cx, cy, rad = circ
        x0, y0, x1, y1 = rect[1:]
        nx, ny = min(max(cx, x0), x1), min(max(cy, y0), y1)
        return math.dist((cx, cy), (nx, ny)) < rad
    ax0, ay0, ax1, ay1 = a[1:]
    bx0, by0, bx1, by1 = b[1:]
    return ax0 < bx1 and bx0 < ax1 and ay0 < by1 and by0 < ay1


def point_in_body(p, b) -> bool:
    if b[0] == "circle":
        return math.dist(p, b[1:3]) < b[3]
    x0, y0, x1, y1 = b[1:]
    return x0 < p[0] < x1 and y0 < p[1] < y1


def seg_rect(p, q, rect) -> bool:
    """axis-aligned segment p-q touches the rect (c0, r0, c1, r1)."""
    x0, y0, x1, y1 = rect
    return min(p[0], q[0]) <= x1 and max(p[0], q[0]) >= x0 and min(p[1], q[1]) <= y1 and max(p[1], q[1]) >= y0


def seg_point_dist(p, q, c) -> float:
    (px, py), (qx, qy) = p, q
    dx, dy = qx - px, qy - py
    t = 0.0 if dx == dy == 0 else max(0.0, min(1.0, ((c[0] - px) * dx + (c[1] - py) * dy) / (dx * dx + dy * dy)))
    return math.dist((px + t * dx, py + t * dy), c)


def segs(pts):
    return list(zip(pts, pts[1:]))


def holes_on_seg(p, q) -> list[Hole]:
    """grid holes a straight axis-aligned segment passes over (endpoints included when on grid)."""
    out = []
    if p[0] == q[0] and float(p[0]).is_integer():
        for r in range(math.ceil(min(p[1], q[1])), math.floor(max(p[1], q[1])) + 1):
            out.append((int(p[0]), r))
    elif p[1] == q[1] and float(p[1]).is_integer():
        for c in range(math.ceil(min(p[0], q[0])), math.floor(max(p[0], q[0])) + 1):
            out.append((c, int(p[1])))
    return out


def conn_key(c: Conn) -> str:
    return c.ref if c.ref else f"bridge:{c.net}:{tuple(c.pts[0])}"


def validate():
    rep = Report()
    lead_at: dict[Hole, tuple[str, str]] = {}  # hole -> (part, pin)
    net_at: dict[Hole, str | None] = {}
    nc_at: dict[Hole, str] = {}
    parts = {p.ref: p for p in PARTS}
    for p in PARTS:
        for pin, h in p.leads.items():
            c, r = h
            if not (1 <= c <= COLS and 1 <= r <= ROWS):
                rep.err(f"{p.ref}.{pin} at {h} is off the grid")
            if h in lead_at:
                rep.err(f"hole {hole(*h)} has two leads: {lead_at[h]} and {(p.ref, pin)}")
            lead_at[h] = (p.ref, pin)
            n = p.nets[pin]
            if n is None:
                nc_at[h] = f"{p.ref}.{p.pin_names.get(pin, pin)}"
            else:
                if n not in NET_COL:
                    rep.err(f"{p.ref}.{pin}: unknown net {n}")
                net_at[h] = n

    # GPIO assignment against the firmware pin table
    dk = parts["U1"]
    for key, want in GPIO_EXPECT.items():
        side, name = key.split(":")
        hits = [pin for pin, nm in dk.pin_names.items() if pin[0] == side and nm == name]
        if len(hits) != 1:
            rep.err(f"devkit pin {key} not unique: {hits}")
            continue
        if dk.nets[hits[0]] != want:
            rep.err(f"devkit {key} at {hole(*dk.leads[hits[0]])} has net {dk.nets[hits[0]]}, firmware expects {want}")
    used_dk = {f"{pin[0]}:{dk.pin_names[pin]}" for pin, n in dk.nets.items() if n}
    if used_dk != set(GPIO_EXPECT):
        rep.err(f"devkit pins used {sorted(used_dk)} != expected {sorted(GPIO_EXPECT)}")
    if dk.leads["B8"] != (13, 15) or dk.leads["T12"] != (17, 4) or dk.leads["B19"] != (24, 15):
        rep.err("devkit pin positions moved (GPIO33 must be M15, GPIO16 Q04, 5V X15)")

    # connections
    uf: dict[Hole, Hole] = {}

    def find(h):
        uf.setdefault(h, h)
        while uf[h] != h:
            uf[h] = uf[uf[h]]
            h = uf[h]
        return h

    def union(a, b):
        uf[find(a)] = find(b)

    conn_pads: dict[Hole, set[str]] = {}
    for cn in CONNS:
        key = conn_key(cn)
        ends = [tuple(cn.pts[0]), tuple(cn.pts[-1])]
        for h in ends:
            if not all(float(v).is_integer() for v in h):
                rep.err(f"{key}: endpoint {h} is not a hole")
        ends = [(int(a), int(b)) for a, b in ends]
        if cn.kind == "bridge":
            (a, b) = ends
            if abs(a[0] - b[0]) + abs(a[1] - b[1]) != 1:
                rep.err(f"bridge {hole(*a)}-{hole(*b)} is not between adjacent pads")
            pads = ends
        elif cn.kind == "link":
            (a, b) = ends
            if a[0] != b[0] and a[1] != b[1]:
                rep.err(f"link {key} is not straight")
            pads = holes_on_seg(a, b)
        else:
            pads = ends
            for p, q in segs(cn.pts):
                if p[0] != q[0] and p[1] != q[1]:
                    rep.err(f"wire {key}: segment {p}-{q} is not orthogonal")
        for h in pads:
            if h in nc_at:
                rep.err(f"{key} ({cn.net}) touches {hole(*h)}, a not-connected pin ({nc_at[h]})")
            have = net_at.get(h)
            if have is not None and have != cn.net:
                rep.err(f"{key} ({cn.net}) touches {hole(*h)} which is net {have}")
            net_at[h] = cn.net
            conn_pads.setdefault(h, set()).add(key)
        for h in pads[1:]:
            union(pads[0], h)

    # every hole of a net connected
    for n in NET_COL:
        hs = [h for h, v in net_at.items() if v == n]
        roots = {find(h) for h in hs}
        if len(roots) > 1:
            groups: dict[Hole, list[str]] = {}
            for h in hs:
                groups.setdefault(find(h), []).append(hole(*h))
            rep.err(f"net {n} is split into {len(roots)} pieces: " + " | ".join(" ".join(sorted(g)) for g in groups.values()))
        if not hs:
            rep.err(f"net {n} has no holes")

    # adjacent used pads of different nets (never bridged: bridges/links were checked above)
    used = {**{h: v for h, v in net_at.items()}, **{h: f"nc:{v}" for h, v in nc_at.items()}}
    adj_other, adj_inherent = [], []
    for h, n in sorted(used.items()):
        for d in ((1, 0), (0, 1)):
            g = (h[0] + d[0], h[1] + d[1])
            if g in used and used[g] != n and not (n.startswith("nc:") and used[g].startswith("nc:")):
                same_part = h in lead_at and g in lead_at and lead_at[h][0] == lead_at[g][0]
                (adj_inherent if same_part else adj_other).append((h, n, g, used[g]))

    # keep-out
    for h in used:
        if KEEPOUT[0] <= h[0] <= KEEPOUT[2] and KEEPOUT[1] <= h[1] <= KEEPOUT[3]:
            rep.err(f"hole {hole(*h)} used inside the antenna keep-out")
    for cn in CONNS:
        for p, q in segs(cn.pts):
            if seg_rect(p, q, KEEPOUT):
                rep.err(f"{conn_key(cn)} runs through the antenna keep-out")
            for x, y in (p, q):
                if not (G_LEFT + 0.2 < x < G_RIGHT - 0.2 and G_TOP + 0.2 < y < G_BOTTOM - 0.2):
                    rep.err(f"{conn_key(cn)} point {(x, y)} is off the board")
            for m in MOUNTS:
                if seg_point_dist(p, q, m) < MOUNT_R + 0.1:
                    rep.err(f"{conn_key(cn)} passes the mounting hole at {m[0]:.1f},{m[1]:.1f}")

    # bodies
    bodies = [(p.ref, p.body, p.height) for p in PARTS if p.kind != "devkit"]
    bodies += [(ref, rect_body(DK_COL0 - 0.5, row - 0.5, DK_COL0 + 18.5, row + 0.5), 8.5) for ref, row in SOCKETS]
    for i, (ra, ba, _) in enumerate(bodies):
        for rb, bb, _ in bodies[i + 1:]:
            if bodies_overlap(ba, bb):
                rep.err(f"bodies overlap: {ra} and {rb}")
        x0, y0, x1, y1 = body_bbox(ba)
        if x0 < G_LEFT or y0 < G_TOP or x1 > G_RIGHT or y1 > G_BOTTOM:
            rep.err(f"{ra} body hangs over the board edge")
        for m in MOUNTS:
            if bodies_overlap(ba, ("circle", m[0], m[1], 0.9)):
                rep.err(f"{ra} body is on the mounting hole at {m[0]:.1f},{m[1]:.1f}")
        if bodies_overlap(ba, rect_body(*KEEPOUT)):
            rep.err(f"{ra} body in the antenna keep-out")
    for ref, b, hgt in bodies:
        if ref in ("X1", "X2"):
            continue
        if bodies_overlap(b, rect_body(*DK_PCB)) and hgt > LOW_MAX_MM:
            rep.err(f"{ref} ({hgt} mm) is under the devkit PCB (max {LOW_MAX_MM} mm)")
    for h, (ref, pin) in lead_at.items():
        for rb, bb, _ in bodies:
            if rb != ref and not (ref == "U1" and rb in ("X1", "X2")) and point_in_body(h, bb):
                rep.err(f"{ref}.{pin} lead {hole(*h)} is under {rb}")

    # wires: crossings and pads passed over
    wires = [c for c in CONNS if c.kind != "bridge"]
    crossings = []
    for i, a in enumerate(wires):
        for b in wires[i + 1:]:
            for p, q in segs(a.pts):
                for s, t in segs(b.pts):
                    hv = (p[0] == q[0]) != (s[0] == t[0])
                    if not hv:
                        continue
                    (vx, vy0, vy1), (hy, hx0, hx1) = ((p[0], min(p[1], q[1]), max(p[1], q[1])), (s[1], min(s[0], t[0]), max(s[0], t[0]))) if p[0] == q[0] else \
                        ((s[0], min(s[1], t[1]), max(s[1], t[1])), (p[1], min(p[0], q[0]), max(p[0], q[0])))
                    if hx0 < vx < hx1 and vy0 < hy < vy1:
                        if a.kind == "link" or b.kind == "link":
                            rep.err(f"bare link crossed: {a.ref} x {b.ref}")
                        crossings.append((a.ref, b.ref, (vx, hy)))
    passed = []
    for cn in CONNS:
        if cn.kind != "wire":
            continue
        for p, q in segs(cn.pts):
            for h in holes_on_seg(p, q):
                if h in (tuple(cn.pts[0]), tuple(cn.pts[-1])):
                    continue
                if h in used:
                    passed.append((cn.ref, h, used[h]))

    # build sheet checks must agree with the layout
    def connected(a, b):
        return a in net_at and b in net_at and net_at[a] == net_at[b] and find(a) == find(b)

    done_conns, done_parts = set(), {"U1"}
    all_keys = {conn_key(c) for c in CONNS}
    for title, sparts, sconns, _, beep, nobeep in STEPS:
        done_parts |= set(sparts)
        for k in sconns:
            if k not in all_keys:
                rep.err(f"step '{title}': unknown connection {k}")
            done_conns.add(k)
        for a, b in beep:
            if not connected(a, b):
                rep.err(f"step '{title}': {hole(*a)}-{hole(*b)} should beep but they are not one net")
        for a, b in nobeep:
            if connected(a, b):
                rep.err(f"step '{title}': {hole(*a)}-{hole(*b)} must not beep but they are one net")
    missing = all_keys - done_conns
    if missing:
        rep.err(f"connections not in any build step: {sorted(missing)}")
    missing_p = {p.ref for p in PARTS} | {s for s, _ in SOCKETS} - done_parts
    missing_p = ({p.ref for p in PARTS} | {s for s, _ in SOCKETS}) - done_parts
    if missing_p:
        rep.err(f"parts not in any build step: {sorted(missing_p)}")
    for na, ha, nb, hb, _ in FINAL_ISOLATION:
        if net_at.get(ha) != na or net_at.get(hb) != nb:
            rep.err(f"final check {hole(*ha)}={net_at.get(ha)} / {hole(*hb)}={net_at.get(hb)}, expected {na}/{nb}")

    rep.info.append(f"{len(PARTS) + len(SOCKETS)} parts, {sum(1 for c in CONNS if c.kind == 'bridge')} bridges, "
                    f"{sum(1 for c in CONNS if c.kind == 'link')} bare links, {sum(1 for c in CONNS if c.kind == 'wire')} insulated wires")
    rep.info.append(f"{len(used)} holes used ({len(nc_at)} of them not-connected socket/header pins), nets connected: {', '.join(NET_COL)}")
    for a, b, (x, y) in crossings:
        rep.info.append(f"insulated wires cross: {a} x {b} near ({x:g},{y:g})")
    for ref, h, n in passed:
        rep.warnings.append(f"{ref} passes over used pad {hole(*h)} ({n}) - insulated, keep it off the solder")
    for h, n, g, m in adj_other:
        rep.warnings.append(f"adjacent pads of different nets (not bridged): {hole(*h)} {n} | {hole(*g)} {m}")
    return rep, dict(net_at=net_at, nc_at=nc_at, lead_at=lead_at, adj_other=adj_other, adj_inherent=adj_inherent,
                     crossings=crossings, conn_pads=conn_pads)


# ==== SVG ==============================================================================================
S = 12.5  # px per mm
PX = PITCH * S
BX, BY = 46, 118
W_SVG = 1400
INK, FAINT = "#1f2937", "#6b7280"


def gx(c):
    return BX + (X0 + (c - 1) * PITCH) * S


def gy(r):
    return BY + (Y0 + (r - 1) * PITCH) * S


class Svg:
    def __init__(self):
        self.o: list[str] = []

    def add(self, s):
        self.o.append(s)

    def text(self, x, y, s, size=11, anchor="start", c=INK, bold=False, rot=None, halo=False, italic=False):
        attrs = f'x="{x:.1f}" y="{y:.1f}" font-size="{size}" text-anchor="{anchor}" fill="{c}"'
        if bold:
            attrs += ' font-weight="bold"'
        if italic:
            attrs += ' font-style="italic"'
        if rot is not None:
            attrs += f' transform="rotate({rot} {x:.1f} {y:.1f})"'
        if halo:
            attrs += ' stroke="#ffffff" stroke-width="3" paint-order="stroke" stroke-linejoin="round"'
        self.add(f"<text {attrs}>{escape(s)}</text>")

    def line(self, x1, y1, x2, y2, c=INK, w=1.5, dash=None, cap="round", op=1.0):
        d = f' stroke-dasharray="{dash}"' if dash else ""
        self.add(f'<line x1="{x1:.1f}" y1="{y1:.1f}" x2="{x2:.1f}" y2="{y2:.1f}" stroke="{c}" stroke-width="{w}" stroke-linecap="{cap}" opacity="{op}"{d}/>')

    def poly(self, pts, c=INK, w=1.5, dash=None, op=1.0):
        d = f' stroke-dasharray="{dash}"' if dash else ""
        p = " ".join(f"{x:.1f},{y:.1f}" for x, y in pts)
        self.add(f'<polyline points="{p}" fill="none" stroke="{c}" stroke-width="{w}" stroke-linejoin="round" stroke-linecap="round" opacity="{op}"{d}/>')

    def rect(self, x, y, w, h, fill="none", c=INK, sw=1.2, r=2, op=1.0, dash=None):
        d = f' stroke-dasharray="{dash}"' if dash else ""
        self.add(f'<rect x="{x:.1f}" y="{y:.1f}" width="{w:.1f}" height="{h:.1f}" rx="{r}" fill="{fill}" fill-opacity="{op}" stroke="{c}" stroke-width="{sw}"{d}/>')

    def circle(self, x, y, r, fill="none", c=INK, sw=1.2, op=1.0):
        self.add(f'<circle cx="{x:.1f}" cy="{y:.1f}" r="{r:.1f}" fill="{fill}" fill-opacity="{op}" stroke="{c}" stroke-width="{sw}"/>')


def grect(svg, c0, r0, c1, r1, **kw):
    svg.rect(gx(c0), gy(r0), gx(c1) - gx(c0), gy(r1) - gy(r0), **kw)


def render(info) -> str:
    net_at, nc_at = info["net_at"], info["nc_at"]
    sv = Svg()
    sv.add('<defs><pattern id="hatch" width="8" height="8" patternUnits="userSpaceOnUse" patternTransform="rotate(45)">'
           '<line x1="0" y1="0" x2="0" y2="8" stroke="#f59e0b" stroke-width="2" opacity="0.45"/></pattern></defs>')
    # title
    sv.text(BX, 34, "Sensor station perfboard: hole-by-hole layout", 20, bold=True)
    sv.text(BX, 56, "PY-5CM*7CM, 24 x 18 isolated pads, 2.54 mm.  Seen from the COMPONENT side (column A on the left, row 01 at the top).", 12, c=FAINT)
    sv.text(BX, 73, "Generated by docs/make_perfboard.py (validated); build steps and wire list in docs/PERFBOARD.md.", 12, c=FAINT)

    # board
    bw, bh = BOARD_W * S, BOARD_H * S
    sv.rect(BX, BY, bw, bh, "#f3ead8", "#8a7556", 1.6, 6)
    for mx, my in MOUNTS:
        sv.circle(gx(mx), gy(my), 1.4 * S, "#ffffff", "#8a7556", 1.4)
        sv.circle(gx(mx), gy(my), MOUNT_R * PX, "none", "#c4b59b", 1, 1)
    # keep-out
    k0, k1, k2, k3 = KEEPOUT
    grect(sv, k0, k1, k2, k3, fill="url(#hatch)", c="#f59e0b", sw=1.2, r=3)

    # column / row labels
    for c in range(1, COLS + 1):
        x = gx(c)
        sv.text(x, BY - 24, str(c), 10, "middle", FAINT)
        sv.text(x, BY - 9, chr(64 + c), 11, "middle", INK, bold=True)
        sv.text(x, BY + bh + 80, str(c), 10, "middle", FAINT)
        sv.text(x, BY + bh + 94, chr(64 + c), 11, "middle", INK, bold=True)
    for r in range(1, ROWS + 1):
        sv.text(BX - 8, gy(r) + 4, f"{r:02d}", 10, "end", INK, bold=True)
        sv.text(BX + bw + 8, gy(r) + 4, f"{r:02d}", 10, "start", INK, bold=True)

    # devkit outline and antenna
    grect(sv, *DK_PCB, fill="#0f766e", c="#0f766e", sw=1.2, r=4, op=0.06, dash="6 4")
    grect(sv, *DK_ANT, fill="#0f766e", c="#0f766e", sw=1, r=2, op=0.10, dash="3 3")
    for ref, row in SOCKETS:
        grect(sv, DK_COL0 - 0.5, row - 0.5, DK_COL0 + 18.5, row + 0.5, fill="#111827", c="#111827", sw=1, r=2, op=0.10)

    # pads
    pad_r, hole_r = 0.85 * S, 0.45 * S
    for c in range(1, COLS + 1):
        for r in range(1, ROWS + 1):
            x, y = gx(c), gy(r)
            n = net_at.get((c, r))
            if n:
                sv.circle(x, y, pad_r, NET_COL[n], NET_COL[n], 1.2, 0.35)
            elif (c, r) in nc_at:
                sv.circle(x, y, pad_r, "#d1d5db", "#9ca3af", 1, 0.6)
            else:
                sv.circle(x, y, pad_r, "none", "#d8c3a0", 0.9)
            sv.circle(x, y, hole_r, "#ffffff", "#a8977c", 0.8)

    # texts over the pads: keep-out, devkit
    kx = gx(k0) + 14
    sv.text(kx, gy(9.5), "ANTENNA KEEP-OUT", 11, "middle", "#b45309", bold=True, rot=-90, halo=True)
    sv.text(kx + 17, gy(9.5), "cols A-E, rows 03-16: no copper, wires or parts", 9, "middle", "#b45309", rot=-90, halo=True)
    sv.text(gx(DK_ANT[2]) - 6, gy(9.5), "WROOM antenna overhang", 9, "middle", "#0f766e", rot=-90, halo=True)
    sv.text(gx(15), gy(8.5) + 4, "U1  ESP32-DevKitC (WROOM-32D) on two 19-pin female sockets", 12, "middle", "#0f766e", bold=True, halo=True)
    sv.text(gx(15), gy(9.5) + 4, "antenna end at the left (over the keep-out), micro-USB at the right (column X)", 10, "middle", "#0f766e", halo=True)
    sv.text(gx(15), gy(10.5) + 4, "rows 05-14 under the devkit stay empty (low parts up to 6 mm would fit)", 10, "middle", "#0f766e", italic=True, halo=True)
    sv.text(gx(25.55), gy(9.5), "USB end", 10, "middle", "#0f766e", bold=True, rot=-90, halo=True)

    # devkit pin names (vertical, into the devkit area)
    dk = PARTS[0]
    for pin, (c, r) in dk.leads.items():
        name = dk.pin_names[pin]
        n = dk.nets[pin]
        lab = name if not name.isdigit() else f"IO{name}"
        if n:
            lab = {"RADAR_TX": f"{lab} RX<-radar", "RADAR_RX": f"{lab} TX->radar"}.get(n, f"{lab} {n}" if lab != n else lab)
        col = NET_COL[n] if n else "#9ca3af"
        if r == DK_ROW_T:
            sv.text(gx(c) + 3.5, gy(r) + 15, lab, 9, "start", col, bold=bool(n), rot=90, halo=bool(n))
        else:
            sv.text(gx(c) + 3.5, gy(r) - 15, lab, 9, "end", col, bold=bool(n), rot=90, halo=bool(n))

    # parts (component side)
    for p in PARTS[1:]:
        draw_part(sv, p)

    # connections (copper side)
    for cn in CONNS:
        col = NET_COL[cn.net]
        pts = [(gx(c), gy(r)) for c, r in cn.pts]
        if cn.kind == "bridge":
            sv.poly(pts, col, 11, op=0.55)
            sv.poly(pts, "#c0c0c0", 4, op=0.9)
        elif cn.kind == "link":
            sv.poly(pts, col, 6.5, op=0.75)
            sv.poly(pts, "#e5e7eb", 2.2)
        else:
            sv.poly(pts, "#ffffff", 5.5, op=0.85)
            sv.poly(pts, col, 3, dash="7 4")
    for cn in CONNS:
        if cn.kind == "bridge":
            continue
        if cn.label_at is not None:
            lx, ly = cn.label_at
        else:
            (a, b) = max(segs(cn.pts), key=lambda s: abs(s[0][0] - s[1][0]) + abs(s[0][1] - s[1][1]))
            lx, ly = (a[0] + b[0]) / 2, (a[1] + b[1]) / 2
            if a[0] == b[0] and float(ly).is_integer():
                ly += 0.5  # between pads
        sv.text(gx(lx), gy(ly) + 3.5, cn.ref, 9, "middle", NET_COL[cn.net], bold=True, halo=True)

    # star marker
    sx, sy = gx(19), gy(16)
    sv.circle(sx, sy, pad_r + 4, "none", NET_COL["GND"], 2)
    sv.text(sx + 22, sy - 12, "STAR", 9, "middle", NET_COL["GND"], bold=True, halo=True)

    # labels outside the board: J1 pins below, J2 pins above
    yb = BY + bh
    sv.text(gx(MIC_PIN1_COL - 5.5) - 6, yb + 22, "J1 pin", 10, "end", INK, bold=True)
    sv.text(gx(MIC_PIN1_COL - 5.5) - 6, yb + 38, "net", 10, "end", FAINT)
    sv.text(gx(MIC_PIN1_COL - 5.5) - 6, yb + 54, "row 17 = mic A, row 18 = mic B", 9, "end", FAINT)
    for k, n in MIC_PINS.items():
        x = gx(MIC_PIN1_COL + 1 - k)
        sv.text(x, yb + 22, str(k), 11, "middle", INK, bold=True)
        sv.text(x, yb + 38, n, 9, "middle", NET_COL[n], bold=True)
    sv.text(gx(MIC_PIN1_COL) + 18, yb + 22, "<- pin 1", 10, "start", INK, bold=True)
    sv.text(gx(22), yb + 38, "TP1: V17 GND, V18 VMIC", 9, "middle", FAINT)
    yt = gy(1) - 18
    for i, (pn, n) in enumerate(RADAR_PINS):
        sv.text(gx(RADAR_COL0 + i), yt, pn, 9, "middle", NET_COL[n] if n else FAINT, bold=bool(n), halo=True)
    sv.text(gx(RADAR_COL0) - 18, gy(1) + 4, "J2 radar  pin 1 ->", 10, "end", INK, bold=True, halo=True)
    sv.text(gx(RADAR_COL0) - 18, gy(1) + 18, "(LD2410C order, OUT unused)", 9, "end", FAINT, halo=True)

    # ---- right panel ------------------------------------------------------------------------------
    x0 = BX + bw + 46
    y = BY - 4
    sv.text(x0, y, "Read this first", 14, bold=True)
    notes = [
        "This is the COMPONENT side. On the copper side",
        "(where you solder) the board is MIRRORED left-right:",
        "column A is on the RIGHT there. The letters A..X and",
        "rows 01..18 are printed on the copper side, so find",
        "holes by name (e.g. N15), not by position.",
        "",
        "Bridges, links and wires are all on the COPPER side.",
        "Parts sit on this side, leads through the holes.",
        "Never bridge two pads of different nets.",
    ]
    for i, s in enumerate(notes):
        sv.text(x0, y + 20 + i * 15, s, 11, c=INK if s else FAINT)
    y += 20 + len(notes) * 15 + 18

    sv.text(x0, y, "Legend", 14, bold=True)
    y += 16
    items = [
        ("pad", "unused pad (copper on the far side)"),
        ("padnet", "pad used by a net (net colour)"),
        ("padnc", "socket/header pin not connected"),
        ("bridge", "solder bridge, adjacent pads only"),
        ("link", "bare tinned wire, soldered at every pad"),
        ("wire", "insulated wire (30 AWG wire-wrap), copper side"),
        ("body", "part body on the component side"),
        ("plus", "electrolytic: + lead, stripe = minus"),
        ("pin1", "square = pin 1 of a header"),
    ]
    for kind, s in items:
        cx, cy = x0 + 16, y + 4
        if kind == "pad":
            sv.circle(cx, cy, pad_r, "none", "#d8c3a0", 0.9)
            sv.circle(cx, cy, hole_r, "#fff", "#a8977c", 0.8)
        elif kind == "padnet":
            sv.circle(cx, cy, pad_r, NET_COL["VMIC"], NET_COL["VMIC"], 1.2, 0.35)
            sv.circle(cx, cy, hole_r, "#fff", "#a8977c", 0.8)
        elif kind == "padnc":
            sv.circle(cx, cy, pad_r, "#d1d5db", "#9ca3af", 1, 0.6)
            sv.circle(cx, cy, hole_r, "#fff", "#a8977c", 0.8)
        elif kind == "bridge":
            sv.line(cx - 14, cy, cx + 14, cy, NET_COL["GND"], 11, op=0.55)
            sv.line(cx - 14, cy, cx + 14, cy, "#c0c0c0", 4, op=0.9)
        elif kind == "link":
            sv.line(cx - 14, cy, cx + 14, cy, NET_COL["5V"], 6.5, op=0.75)
            sv.line(cx - 14, cy, cx + 14, cy, "#e5e7eb", 2.2)
        elif kind == "wire":
            sv.line(cx - 14, cy, cx + 14, cy, NET_COL["BCLK"], 3, dash="7 4")
        elif kind == "body":
            sv.rect(cx - 13, cy - 6, 26, 12, "#e9d5a7", "#7c6a46", 1, 3, 0.85)
        elif kind == "plus":
            sv.circle(cx, cy, 9, "#dbeafe", "#1e40af", 1.2, 0.75)
            sv.text(cx - 3, cy + 4, "+", 11, "middle", "#1e40af", bold=True)
        elif kind == "pin1":
            sv.rect(cx - 7, cy - 7, 14, 14, "none", INK, 1.6, 0)
        sv.text(x0 + 40, y + 8, s, 11)
        y += 22
    y += 22

    sv.text(x0, y, "Nets", 14, bold=True)
    y += 16
    for n, col in NET_COL.items():
        sv.line(x0 + 2, y, x0 + 30, y, col, 3, dash="7 4")
        sv.text(x0 + 40, y + 4, n, 11, c=col, bold=True)
        sv.text(x0 + 118, y + 4, NET_DESC[n], 10, c=FAINT)
        y += 17
    y += 12

    sv.text(x0, y, "Key holes", 14, bold=True)
    y += 18
    key = [
        "3V3 F15, IO33 M15, IO25 N15, IO26 O15",
        "GND S15, 5V X15, IO17 P04, IO16 Q04",
        "S16 = star: W4 (mics), W11 (radar), devkit GND",
        "C1: + T18, - T17.   C3: + V01, - V02",
    ]
    for s in key:
        sv.text(x0, y, s, 11)
        y += 15
    y += 10
    sv.text(x0, y, "Not fitted: R1/R2 33 ohm series (BCLK/LRCL),", 10, c=FAINT)
    sv.text(x0, y + 14, "radar OUT (J2 pin 3 / GPIO27), TP for 5V (use C3).", 10, c=FAINT)

    h = max(BY + bh + 110, y + 40)
    return (f'<svg xmlns="http://www.w3.org/2000/svg" width="{W_SVG}" height="{h:.0f}" viewBox="0 0 {W_SVG} {h:.0f}" '
            f'font-family="ui-monospace, Menlo, Consolas, monospace">'
            f'<rect width="{W_SVG}" height="{h:.0f}" fill="#ffffff"/>' + "".join(sv.o) + "</svg>")


def draw_part(sv: Svg, p: Part):
    b = p.body
    if p.kind == "resistor":
        (c1, r1), (c2, r2) = p.leads["1"], p.leads["2"]
        sv.line(gx(c1), gy(r1), gx(c2), gy(r2), "#6b7280", 1.6)
        grect(sv, *b[1:], fill="#e9d5a7", c="#7c6a46", sw=1.1, r=6, op=0.95)
        x0, x1 = gx(b[1]), gx(b[3])
        for f, col in ((0.18, "#8b4513"), (0.30, "#000000"), (0.42, "#8b4513")):  # brown black brown = 100
            sv.line(x0 + (x1 - x0) * f, gy(b[2]) + 1, x0 + (x1 - x0) * f, gy(b[4]) - 1, col, 2.5, cap="butt")
        sv.text((x0 + x1) / 2 + 16, gy((b[2] + b[4]) / 2) + 4, p.label, 9, "middle", INK, bold=True)
    elif p.kind == "ceramic":
        grect(sv, *b[1:], fill="#f5b041", c="#9a6a12", sw=1.1, r=5, op=0.8)
        cx, cy = gx((b[1] + b[3]) / 2), gy((b[2] + b[4]) / 2)
        sv.text(cx + 3, cy, p.label, 9, "middle", INK, bold=True, rot=90)
    elif p.kind == "electrolytic":
        _, c, r, rad = b
        sv.circle(gx(c), gy(r), rad * PX, "#dbeafe", "#1e40af", 1.4, 0.75)
        (pc, pr), (mc, mr) = p.leads["+"], p.leads["-"]
        # minus stripe: a band on the minus side
        dx, dy = mc - pc, mr - pr
        cxm, cym = gx(c + dx * rad * 0.62), gy(r + dy * rad * 0.62)
        if dx == 0:
            sv.line(gx(c) - rad * PX * 0.75, cym, gx(c) + rad * PX * 0.75, cym, "#1e40af", 5, cap="butt", op=0.6)
        else:
            sv.line(cxm, gy(r) - rad * PX * 0.75, cxm, gy(r) + rad * PX * 0.75, "#1e40af", 5, cap="butt", op=0.6)
        px_, py_ = gx(pc) + (12 if dx == 0 else 0), gy(pr) + (4 if dx == 0 else -12)
        sv.text(px_ + 4, py_ + 2, "+", 14, "middle", "#b91c1c", bold=True, halo=True)
        sv.text(gx(mc) + 14, gy(mr) + 5, "-", 14, "middle", "#1e40af", bold=True, halo=True)
        sv.text(gx(c) - (rad * PX * 0.35 if p.ref == "C3" else 0) - 22, gy(r) + 4, p.label, 10, "middle", "#1e3a8a", bold=True, halo=True)
    elif p.kind == "header":
        grect(sv, *b[1:], fill="#111827", c="#111827", sw=1.2, r=2, op=0.12)
        first = {"J1": ["A1", "B1"], "J2": ["1"], "TP1": ["1"]}[p.ref]
        for pin in first:
            c, r = p.leads[pin]
            sv.rect(gx(c) - 11, gy(r) - 11, 22, 22, "none", INK, 1.8, 0)
        if p.ref == "TP1":
            sv.text(gx(22) + 20, gy(17.5) + 4, "TP1", 9, "middle", INK, bold=True, rot=90, halo=True)


# ==== markdown ===========================================================================================
def wire_len_mm(cn: Conn) -> float:
    return sum(math.dist(p, q) for p, q in segs(cn.pts)) * PITCH


def render_md(rep: Report, info) -> str:
    net_at = info["net_at"]
    L: list[str] = []
    a = L.append
    a("# Sensor station perfboard: build sheet")
    a("")
    a("Generated by `docs/make_perfboard.py` (the single source of truth: edit the data there and re-run")
    a("`uv run docs/make_perfboard.py`, never this file). Drawing: [`perfboard.svg`](perfboard.svg). Schematic:")
    a("[`wiring.svg`](wiring.svg). Netlist and parts background: [`../HARDWARE.md`](../HARDWARE.md).")
    a("")
    a("## Board and conventions")
    a("")
    a("- PY-5CM*7CM single-sided perfboard, 24 columns x 18 rows of isolated round pads at 2.54 mm, four")
    a("  mounting holes about 3 mm in from the corners (they sit outside the pad grid at the left and right ends).")
    a("- A hole is named by the silkscreen: column letter + row, e.g. **N15** = column 14, row 15. Tables also")
    a("  give (col,row) with col 1..24 = A..X.")
    a("- The drawing shows the **component side** (column A on the left, row 01 at the top). The silkscreen and the")
    a("  copper are on the **other side**, where the board is **mirrored left-right** (X..A from left to right in")
    a("  your photo). Rows do not change. Always find holes by their printed name.")
    a("- All bridges, bare links and insulated wires are on the **copper side**. Parts and the devkit sockets are")
    a("  on the component side.")
    a("- Connection types: **bridge** = solder blob between two ADJACENT pads of the same net; **link** = bare tinned")
    a("  wire (a cut resistor lead) laid straight on the pads and soldered at every pad it crosses; **wire** = insulated")
    a("  30 AWG wire-wrap (or thin hookup) wire, stripped 2-3 mm at each end. GND: the two GND rails (L2, and the")
    a("  bridges S15-S16-S17) are bare; every longer GND run (W3, W4, W9, W10, W11) is insulated.")
    a("- Antenna keep-out: columns A-E, rows 03-16 stay empty (the WROOM antenna overhangs there).")
    a("- Nothing is under the devkit (rows 05-14). Tall parts (C1, C3, headers) are only in rows 01-02 and 17-18.")
    a("")
    a("## Bill of materials")
    a("")
    a("| Ref | Part | Value / type | Holes | Height |")
    a("|---|---|---|---|---|")
    a("| U1 | ESP32-DevKitC clone, WROOM-32D, 38 pin, micro-USB | plugs into X1/X2 | rows 04 and 15, F..X | - |")
    a("| X1, X2 | 1x19 female header (2.54 mm) | devkit sockets, 11 holes apart (27.94 mm) | F04..X04, F15..X15 | 8.5 mm |")
    for p in PARTS[1:]:
        holes = ", ".join(f"{pin}={hole(*h)}" for pin, h in p.leads.items()) if p.ref != "J1" else "L17..Q18"
        kind = {"resistor": "resistor 1/4 W, lying flat, leads 4 holes apart (10.16 mm)", "ceramic": "ceramic, 2.54 mm leads", "electrolytic": "electrolytic, radial",
                "header": "pin header"}[p.kind]
        a(f"| {p.ref} | {kind} | {p.value} | {holes} | {p.height:g} mm |")
    a("| - | 30 AWG wire-wrap wire (insulated) | about 30 cm | W1..W13 | - |")
    a("| - | bare tinned wire (cut resistor leads) | 3 pieces | L1..L3 | - |")
    a("| - | 2x 1x6 Dupont housings + crimps (mic cables), 1x 1x5 (radar cable) | | J1, J2 | - |")
    a("")
    a("Not fitted: R1/R2 (33 ohm series on BCLK/LRCL; breadboard tests with long wires worked without them; if ever")
    a("needed, put them in the cable at the board end). R3 = 22 ohm in HARDWARE.md v1 is replaced by R3a || R3b = 2x")
    a("100 ohm = 50 ohm (fc with C1 about 320 Hz; both mics together draw about 1-1.5 mA, so the drop is under 0.1 V).")
    a("Radar OUT (J2 pin 3) and GPIO27 are not connected. No separate 5 V test point: measure 5 V on the C3 leads")
    a("V01/V02. The README suggests a 100 kohm pull-down on the shared DOUT line: there is no room for it next to")
    a("M15..M18 (an upright resistor would hit the devkit PCB), so enable the GPIO33 internal pull-down in firmware")
    a("instead, or solder a 100 kohm from SD to GND on one mic breakout if a scope shows DOUT floating.")
    a("")
    a("## Hole map per part")
    a("")
    for p in PARTS:
        if p.ref == "U1":
            a("**U1 devkit** (component side, USB toward column X). Used pins in bold:")
            a("")
            a("| Row | " + " | ".join(chr(64 + c) for c in range(DK_COL0, DK_COL0 + 19)) + " |")
            a("|---|" + "---|" * 19)
            for row, side in ((DK_ROW_T, "T"), (DK_ROW_B, "B")):
                cells = []
                for i in range(19):
                    key = f"{side}{i + 1}"
                    nm, n = p.pin_names[key], p.nets[key]
                    cells.append(f"**{nm}**" if n else nm)
                a(f"| {row:02d} | " + " | ".join(cells) + " |")
            a("")
            a("| Devkit pin | Hole | (col,row) | Net |")
            a("|---|---|---|---|")
            for key, n in p.nets.items():
                if n:
                    a(f"| {'GPIO' + p.pin_names[key] if p.pin_names[key].isdigit() else p.pin_names[key]} | {hole(*p.leads[key])} | {p.leads[key]} | {n} |")
            a("")
            continue
        a(f"**{p.ref}** {p.value}")
        a("")
        a("| Pin | Hole | (col,row) | Net |")
        a("|---|---|---|---|")
        for pin, h in p.leads.items():
            nm = p.pin_names.get(pin, pin)
            if p.ref == "J1":
                nm = f"mic {pin[0]} pin {nm}"
            a(f"| {nm} | {hole(*h)} | {h} | {p.nets[pin] or 'not connected'} |")
        a("")
    a("J1: pin 1 (VMIC) is column Q, on the right (USB end); pin 6 (GND) is column L. Both rows carry the same nets")
    a("column by column; plug both mic cables with pin 1 toward column Q. Row 17 = mic A (SEL to GND, left), row 18 = mic B.")
    a("")
    a("## Connections")
    a("")
    a("| Ref | Type | Net | From | To | Route (copper side) | Path | Cut |")
    a("|---|---|---|---|---|---|---|---|")
    for cn in CONNS:
        f, t = tuple(cn.pts[0]), tuple(cn.pts[-1])
        if cn.kind == "bridge":
            a(f"| - | bridge | {cn.net} | {hole(*f)} | {hole(*t)} | solder blob | - | - |")
        elif cn.kind == "link":
            ln = wire_len_mm(cn)
            a(f"| {cn.ref} | bare link | {cn.net} | {hole(*f)} | {hole(*t)} | {cn.route} | {ln:.0f} mm | {ln + 6:.0f} mm |")
        else:
            ln = wire_len_mm(cn)
            a(f"| {cn.ref} | insulated | {cn.net} | {hole(*f)} | {hole(*t)} | {cn.route} | {ln:.0f} mm | {ln + 10:.0f} mm |")
    a("")
    a("Cut = path + 10 mm for insulated wires (strip 2-3 mm each end), + 6 mm for bare links.")
    a("")
    a("## Order of assembly (measure after every step, devkit NOT plugged in)")
    a("")
    a("Meter on continuity (beep). \"Beep\" pairs must beep; \"no beep\" pairs must not. Hole names are printed on")
    a("the copper side, where you probe.")
    a("")
    for i, (title, sparts, sconns, text, beep, nobeep) in enumerate(STEPS, 1):
        a(f"### {i}. {title}")
        a("")
        a(text)
        a("")
        if beep:
            a("- Beep: " + ", ".join(f"{hole(*x)}-{hole(*y)} ({net_at[x]})" for x, y in beep))
        if nobeep:
            a("- No beep: " + ", ".join(f"{hole(*x)}-{hole(*y)}" for x, y in nobeep))
        if i == 5:
            a("- C1 polarity: the stripe (minus) is on T17, the GND rail; the long lead (+) in T18, the VMIC rail.")
        if i == 6:
            a("- C3 polarity: the stripe (minus) is on V02 (GND); + in V01 on the 5 V link. A reversed 470 uF can vent.")
        a("")
    a("### 8. Final checks before any USB")
    a("")
    a("Devkit unplugged, no mic or radar cable. Ohms range for the resistor line, continuity for the rest:")
    a("")
    a("| Between | Holes | Expect |")
    a("|---|---|---|")
    for na, ha, nb, hb, exp in FINAL_ISOLATION:
        a(f"| {na} - {nb} | {hole(*ha)} - {hole(*hb)} | {exp} |")
    a("")
    a("Also look at the copper side under a light: every bridge between the right two pads, nothing between the J1")
    a("columns sideways, nothing between rows 17 and 18 along L1/L2, no whiskers at the header pins next to W11 (W/X).")
    a("")
    a("### 9. First power-up")
    a("")
    a("1. Plug the devkit in (USB toward column X, check pin F15 = 3V3 on the devkit silkscreen). No mic, no radar.")
    a("   USB from a current-limited port or a USB meter if you have one: idle draw about 40-80 mA.")
    a("2. Measure DC to GND (S15 or V17): F15 3.3 V, V18 (TP1 VMIC) 3.2-3.3 V, X15 / V01 4.7-5.1 V.")
    a("3. Unplug USB. Plug the radar cable (TX to O01, pin 1). Power: 5 V at V01 still >= 4.7 V; radar LED / UART")
    a("   frames in the firmware log.")
    a("4. Unplug USB. Plug both mic cables (pin 1 to column Q). Power: VMIC (V18) still about 3.25 V; run the")
    a("   firmware level check (README) and tap each mic: A must show in the left slot, B in the right.")
    a("")
    a("## Validation report")
    a("")
    a("```")
    a(report_text(rep))
    a("```")
    a("")
    a("Adjacent pads of different nets that are part of the same part (normal lead spacing, listed for inspection):")
    a("")
    inh = sorted({f"{hole(*h)}/{hole(*g)}" for h, n, g, m in info["adj_inherent"] if not (n.startswith("nc:") or m.startswith("nc:"))})
    a(", ".join(inh) + ".")
    a("")
    return "\n".join(L)


def report_text(rep: Report) -> str:
    lines = ["perfboard validation: " + ("OK" if not rep.errors else f"{len(rep.errors)} ERROR(S)")]
    lines += [f"  info: {s}" for s in rep.info]
    lines += [f"  warn: {s}" for s in rep.warnings]
    lines += [f"  ERROR: {s}" for s in rep.errors]
    return "\n".join(lines)


def main() -> int:
    rep, info = validate()
    print(report_text(rep))
    if rep.errors:
        print("not writing outputs", file=sys.stderr)
        return 1
    (HERE / "perfboard.svg").write_text(render(info))
    (HERE / "PERFBOARD.md").write_text(render_md(rep, info))
    print("wrote", HERE / "perfboard.svg", "and", HERE / "PERFBOARD.md")
    return 0


if __name__ == "__main__":
    sys.exit(main())
