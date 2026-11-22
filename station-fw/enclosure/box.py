#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.10,<3.13"
# dependencies = ["cadquery>=2.4"]
# ///
"""Parametric enclosure for the sensor station (front shell + back lid).

    uv run enclosure/box.py            # writes enclosure/out/{body,lid}.stl, station.step, *.svg

Coordinates: x = width (mic A at -x, mic B at +x), y = height, z = depth.
z = 0 is the OUTSIDE of the front panel (faces the arena); the cavity opens at z = D
(the back), where the lid closes it. The body prints front-face-down, the lid
plate-down. All sizes are mm; the values marked MEASURE are assumptions, check them
against your parts and edit before printing.
"""

from __future__ import annotations

import sys
from pathlib import Path

import cadquery as cq

# ---- parts (MEASURE) ---------------------------------------------------------------
MIC_PITCH = 130.0  # centre to centre, firmware d_mm
MIC_W, MIC_H = 16.0, 13.0  # mic breakout outline (photo estimate ~15 x 12, MEASURE)
MIC_PORT_DX, MIC_PORT_DY = 0.0, 0.0  # port hole centre relative to board centre
MIC_PORT_D = 2.4  # through-hole diameter (>= the port, keep >= 2 mm)
GASKET = 1.0  # foam gasket thickness between breakout and front wall
RADAR_W, RADAR_H = 22.0, 16.0  # LD2410 board; pin header along the bottom (-y) edge
PB_W, PB_H = 70.0, 50.0  # perfboard (x by y)
PB_HOLE_INSET = 3.0  # corner hole inset on the perfboard (for the posts)
DEVKIT_L = 55.0  # devkit length, USB end flush with the perfboard right edge
USB_SLOT_W, USB_SLOT_H = 13.0, 9.0  # cut-out for the plug overmould

# ---- box ----------------------------------------------------------------------------
WALL = 2.4  # side walls
FRONT = 2.0  # front panel thickness (mic holes go through this)
RADAR_WINDOW = 1.2  # front panel thickness over the radar antenna
W_IN, H_IN, D_IN = 156.0, 56.0, 32.0  # interior (cavity) size
POST_D, SCREW_PILOT, SCREW_CLR = 7.0, 2.6, 3.2  # M3 self tapping in the body posts
LID_T, LIP_H, LIP_T = 2.4, 2.0, 1.2
STANDOFF_H, STANDOFF_D = 6.0, 5.0  # perfboard standoffs on the lid
PB_CX = 40.0  # perfboard centre x (offset right so the devkit USB end meets the wall)
USB_Z_FROM_LID = 19.2  # USB connector centre below the lid face (standoff + headers + pcb)

W, H, D = W_IN + 2 * WALL, H_IN + 2 * WALL, FRONT + D_IN
FIT = 0.2  # printing clearance per side

mic_x = (-MIC_PITCH / 2, MIC_PITCH / 2)


def box(w, h, d, x=0.0, y=0.0, z=0.0):
    return cq.Workplane("XY").box(w, h, d, centered=(True, True, False)).translate((x, y, z))


def cyl(dia, h, x=0.0, y=0.0, z=0.0):
    return cq.Workplane("XY").circle(dia / 2).extrude(h).translate((x, y, z))


def corners():
    px, py = W_IN / 2 - POST_D / 2, H_IN / 2 - POST_D / 2
    return [(sx * px, sy * py) for sx in (-1, 1) for sy in (-1, 1)]


def build_body() -> cq.Workplane:
    body = box(W, H, D).edges("|Z").fillet(3.0)
    body = body.cut(box(W_IN, H_IN, D_IN + 1, z=FRONT))

    # corner posts for the lid screws
    for x, y in corners():
        post = cyl(POST_D, D_IN, x, y, FRONT).cut(cyl(SCREW_PILOT, 14, x, y, D - 14))
        body = body.union(post)

    # mic seats: retaining frame around each breakout, port hole + outside countersink
    for x in mic_x:
        frame = box(MIC_W + 4.4, MIC_H + 4.4, GASKET + 3.0, x, 0, FRONT).cut(
            box(MIC_W + 0.4, MIC_H + 0.4, GASKET + 3.0, x, 0, FRONT)
        )
        body = body.union(frame)
        px, py = x + MIC_PORT_DX, MIC_PORT_DY
        body = body.cut(cyl(MIC_PORT_D, FRONT + 1, px, py, -0.5))
        cone = cq.Solid.makeCone(3.2, MIC_PORT_D / 2, 1.0, cq.Vector(px, py, 0), cq.Vector(0, 0, 1))
        body = body.cut(cq.Workplane("XY").add(cone))

    # radar seat: thin window in the front panel, frame with a notch for the header edge
    win = box(RADAR_W - 2, RADAR_H - 2, FRONT - RADAR_WINDOW + 0.01, 0, 0, RADAR_WINDOW)
    body = body.cut(win)
    rf = box(RADAR_W + 4.4, RADAR_H + 4.4, 4.0, 0, 0, FRONT).cut(box(RADAR_W + 0.4, RADAR_H + 0.4, 4.0, 0, 0, FRONT))
    rf = rf.cut(box(RADAR_W - 4, 6.0, 4.0, 0, -(RADAR_H / 2 + 1.5), FRONT))  # header notch
    body = body.union(rf)

    # USB slot in the +x wall
    usb_z = D - USB_Z_FROM_LID
    body = body.cut(
        cq.Workplane("YZ").rect(USB_SLOT_W, USB_SLOT_H).extrude(WALL + 2).translate((W / 2 - WALL - 1, 0, usb_z))
    )
    return body


def build_lid() -> cq.Workplane:
    lid = box(W, H, LID_T).edges("|Z").fillet(3.0)
    # inner lip that locates the lid in the cavity (printed upward, so +z here)
    ring = box(W_IN - 2 * FIT, H_IN - 2 * FIT, LIP_H, z=LID_T).cut(
        box(W_IN - 2 * FIT - 2 * LIP_T, H_IN - 2 * FIT - 2 * LIP_T, LIP_H, z=LID_T)
    )
    for x, y in corners():
        ring = ring.cut(cyl(POST_D + 1.6, LIP_H, x, y, LID_T))
    lid = lid.union(ring)
    for x, y in corners():
        lid = lid.cut(cyl(SCREW_CLR, LID_T, x, y, 0))
        cs = cq.Solid.makeCone(SCREW_CLR / 2, 3.2, 1.6, cq.Vector(x, y, LID_T - 1.6), cq.Vector(0, 0, 1))
        lid = lid.cut(cq.Workplane("XY").add(cs))
    # perfboard standoffs (corner holes) with small pilots for M2 self tapping
    for sx in (-1, 1):
        for sy in (-1, 1):
            x = PB_CX + sx * (PB_W / 2 - PB_HOLE_INSET)
            y = sy * (PB_H / 2 - PB_HOLE_INSET)
            lid = lid.union(cyl(STANDOFF_D, STANDOFF_H, x, y, LID_T).cut(cyl(1.8, STANDOFF_H, x, y, LID_T)))
    return lid


def main() -> None:
    out = Path(__file__).parent / "out"
    out.mkdir(exist_ok=True)
    body, lid = build_body(), build_lid()
    cq.exporters.export(body, str(out / "body.stl"))
    cq.exporters.export(lid, str(out / "lid.stl"))
    asm = body.union(lid.rotate((0, 0, 0), (0, 1, 0), 180).translate((0, 0, D + LID_T)))
    cq.exporters.export(asm, str(out / "station.step"))
    for name, shape, view in (("front", body, (0, 0, -1)), ("top", body, (0, -1, 0)), ("lid", lid, (0, 0, 1))):
        cq.exporters.export(shape, str(out / f"{name}.svg"), opt={"projectionDir": view, "showHidden": False, "width": 1200, "height": 500})
    bb = body.val().BoundingBox()
    print(f"body {bb.xlen:.1f} x {bb.ylen:.1f} x {bb.zlen:.1f} mm; interior {W_IN} x {H_IN} x {D_IN}")
    print(f"mic centres x = {mic_x[0]:+.1f}, {mic_x[1]:+.1f} (pitch {mic_x[1] - mic_x[0]:.1f}); outputs in {out}")


if __name__ == "__main__":
    sys.exit(main())
