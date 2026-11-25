#!/usr/bin/env python3
# /// script
# requires-python = ">=3.9"
# ///
"""Set a WLED controller's gamma (a one-off controller setting, not a brain setting).

    uv run deploy/wled_set_gamma.py 10.0.0.35            # 1.8, what the arena uses
    uv run deploy/wled_set_gamma.py 10.0.0.35 --gamma 2.8

WLED's default 2.8 makes the dark end of the idle breath steppy; 1.8 is smoother at night.
Sends the whole `gc` object: sending only `col` did not take. Reads it back afterwards.
"""
import argparse
import json
import sys
import urllib.request


def req(url, body=None, timeout=5):
    data = json.dumps(body).encode() if body is not None else None
    r = urllib.request.Request(url, data, {"content-type": "application/json"} if data else {})
    with urllib.request.urlopen(r, timeout=timeout) as resp:
        return json.load(resp)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("host", help="controller address, e.g. 10.0.0.35")
    ap.add_argument("--gamma", type=float, default=1.8)
    a = ap.parse_args()
    base = a.host if a.host.startswith("http") else f"http://{a.host}"
    v = a.gamma
    req(f"{base}/json/cfg", {"light": {"gc": {"bri": 1, "col": v, "val": v}}})
    gc = req(f"{base}/json/cfg")["light"]["gc"]
    print("gc:", gc)
    if abs(gc.get("col", 0) - v) > 0.05 or abs(gc.get("val", 0) - v) > 0.05:
        sys.exit(f"gamma did not take (wanted {v})")


main()
