#!/usr/bin/env python3
# /// script
# requires-python = ">=3.9"
# ///
"""Try WLED looks straight on the controller from a laptop, without the brain.

    uv run deploy/wled_try.py --wled http://10.0.0.55 --fx Breathe --pal Default --sx 40 --col 0,80,255
    uv run deploy/wled_try.py --wled http://10.0.0.55 --demo idle          # cycle labelled candidates
    uv run deploy/wled_try.py --wled http://10.0.0.55 --off

Stop the brain first (`sudo systemctl stop shadow-brain` on the station) or it will overwrite
what you set on the next arena state change. Only POSTs: an ESP32-C3 rebooted once under
4 Hz GET polling, so this tool never polls.
"""
import argparse
import json
import sys
import time
import urllib.request


def req(url, body=None, timeout=5):
    data = json.dumps(body).encode() if body is not None else None
    r = urllib.request.Request(url, data, {"content-type": "application/json"} if data else {})
    with urllib.request.urlopen(r, timeout=timeout) as resp:
        return json.load(resp)


def lookup(names, want):
    """Index of an effect/palette by exact (case-insensitive) name, else unique prefix/substring."""
    w = str(want).strip().lower()
    if w.isdigit():
        return int(w)
    lower = [n.lower() for n in names]
    if w in lower:
        return lower.index(w)
    for test in (lambda n: n.startswith(w), lambda n: w in n):
        hits = [i for i, n in enumerate(lower) if test(n)]
        if len(hits) == 1:
            return hits[0]
        if hits:
            sys.exit(f"'{want}' is ambiguous: {[names[i] for i in hits[:8]]}")
    sys.exit(f"no such effect/palette: {want}")


# Labelled candidates for --demo. Each is a dict of the same options as the CLI.
DEMOS = {
    "idle": [
        ("A  current: Breathe, Ocean palette, speed 128", dict(fx="Breathe", pal="Ocean", sx=128)),
        ("B  Breathe, deep blue, slow (sx 40)", dict(fx="Breathe", pal="Default", sx=40, col=[[0, 70, 255]])),
        ("C  Breathe, warm orange, slow (sx 40)", dict(fx="Breathe", pal="Default", sx=40, col=[[255, 90, 0]])),
        ("D  Breathe, violet, slow (sx 40)", dict(fx="Breathe", pal="Default", sx=40, col=[[150, 0, 255]])),
        ("E  Breathe, red ember, very slow (sx 25)", dict(fx="Breathe", pal="Default", sx=25, col=[[255, 25, 0]])),
        ("F  Breathe, deep blue, faster (sx 90)", dict(fx="Breathe", pal="Default", sx=90, col=[[0, 70, 255]])),
    ],
    # Fade goes fully to colour 2 (black) in one triangle wave; cycle = 65.536 s / ((sx >> 3) + 10) on WLED 0.15.4.
    "idle2": [
        ("E1  Breathe, ember, slowest (sx 0): ~5 s breath then a long dim rest", dict(fx="Breathe", pal="Default", sx=0, col=[[255, 25, 0]])),
        ("E2  Fade to black, ember, ~3.9 s cycle (sx 56)", dict(fx="Fade", pal="Default", sx=56, col=[[255, 25, 0], [0, 0, 0]])),
        ("E3  Fade to black, ember, ~4.4 s cycle (sx 40)", dict(fx="Fade", pal="Default", sx=40, col=[[255, 25, 0], [0, 0, 0]])),
        ("E4  Fade to black, ember, ~5.0 s cycle (sx 24)", dict(fx="Fade", pal="Default", sx=24, col=[[255, 25, 0], [0, 0, 0]])),
    ],
}


def apply(wled, names, o, bri=None, transition=None):
    seg = {"id": 0, "fx": lookup(names["fx"], o["fx"])}
    if "pal" in o:
        seg["pal"] = lookup(names["pal"], o["pal"])
    for k in ("sx", "ix"):
        if o.get(k) is not None:
            seg[k] = o[k]
    if o.get("col"):
        seg["col"] = o["col"]
    body = {"on": True, "seg": [seg]}
    if bri is not None:
        body["bri"] = bri
    if transition is not None:
        body["transition"] = transition
    req(f"{wled}/json/state", body)
    return seg


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--wled", required=True)
    ap.add_argument("--fx")
    ap.add_argument("--pal")
    ap.add_argument("--sx", type=int, help="effect speed 0-255")
    ap.add_argument("--ix", type=int, help="effect intensity 0-255")
    ap.add_argument("--col", action="append", help="R,G,B (repeat for up to 3 colours)")
    ap.add_argument("--bri", type=int, help="master brightness 0-255")
    ap.add_argument("--transition", type=int, help="100 ms units")
    ap.add_argument("--demo", choices=sorted(DEMOS))
    ap.add_argument("--dwell", type=float, default=9.0)
    ap.add_argument("--off", action="store_true")
    a = ap.parse_args()

    if a.off:
        req(f"{a.wled}/json/state", {"on": False})
        print("off")
        return
    names = {"fx": req(f"{a.wled}/json/eff"), "pal": req(f"{a.wled}/json/pal")}
    if a.demo:
        for label, o in DEMOS[a.demo]:
            seg = apply(a.wled, names, o, bri=a.bri if a.bri is not None else 180, transition=a.transition)
            print(f"{label}   [fx {seg['fx']}={names['fx'][seg['fx']]}]", flush=True)
            time.sleep(a.dwell)
        return
    if not a.fx:
        sys.exit("give --fx, --demo or --off")
    o = dict(fx=a.fx, pal=a.pal, sx=a.sx, ix=a.ix, col=[[int(x) for x in c.split(",")] for c in a.col] if a.col else None)
    o = {k: v for k, v in o.items() if v is not None}
    seg = apply(a.wled, names, o, bri=a.bri, transition=a.transition)
    print(f"set fx {seg['fx']} ({names['fx'][seg['fx']]})", {k: v for k, v in seg.items() if k not in ('id', 'fx')})


main()
