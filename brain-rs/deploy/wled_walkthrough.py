#!/usr/bin/env python3
# /// script
# requires-python = ">=3.9"
# ///
"""Hardware check for a real WLED strip: walk the arena through its states and read back
what the controller is actually doing.

    uv run deploy/wled_walkthrough.py --brain http://10.0.0.28:8000 --wled http://10.0.0.55

For every state it POSTs the transition to the brain, then polls the controller until the
effect changed (reporting the latency) and prints effect/palette/brightness/colours, whether
WLED is receiving realtime UDP (and from which address), and its power estimate. A person
watches the strip alongside: this cannot see individual pixels (WLED's /json/live is empty
without a websocket), so "all N LEDs are lit" is a visual check.

Starts from whatever state the arena is in. It leaves the arena in IDLE.
"""
import argparse
import json
import time
import urllib.request

STEPS = [  # (state, what the strip should show)
    ("Cooldown", "fire effect, lava palette"),
    ("Idle", "slow breathing, ocean palette"),
    ("Warming", "moving rainbow"),
    ("Fight", "realtime UDP: all LEDs pulsing red"),
    ("Cooldown", "fire effect, lava palette"),
    ("Suspended", "strip off"),
    ("Idle", "slow breathing, ocean palette"),
]


def get(url, timeout=4):
    with urllib.request.urlopen(url, timeout=timeout) as r:
        return json.load(r)


def post(url, body, timeout=6):
    req = urllib.request.Request(url, json.dumps(body).encode(), {"content-type": "application/json"})
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return json.load(r)


def snapshot(wled, names):
    st, info = get(f"{wled}/json/state"), get(f"{wled}/json/info")
    seg = st["seg"][0]
    return {
        "on": st["on"],
        "bri": st["bri"],
        "fx": names["fx"][seg["fx"]] if seg["fx"] < len(names["fx"]) else seg["fx"],
        "pal": names["pal"][seg["pal"]] if seg["pal"] < len(names["pal"]) else seg["pal"],
        "col": seg["col"][0],
        "live": info.get("live"),
        "lip": info.get("lip"),
        "pwr_mA": info["leds"].get("pwr"),
        "leds": info["leds"].get("count"),
        "uptime": info.get("uptime"),
    }


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--brain", required=True)
    ap.add_argument("--wled", required=True)
    ap.add_argument("--dwell", type=float, default=8.0, help="seconds to stay in each state")
    ap.add_argument("--poll", type=float, default=1.5,
                    help="seconds between reads of the controller (an ESP32-C3 rebooted under 4 Hz polling)")
    a = ap.parse_args()

    names = {"fx": get(f"{a.wled}/json/eff"), "pal": get(f"{a.wled}/json/pal")}
    print(f"controller: {get(a.wled + '/json/info')['leds']['count']} LEDs, "
          f"{len(names['fx'])} effects; brain state now: {get(a.brain + '/api/state')['data']['arena']['current_state']}")
    print(f"{'state':<10} {'brain':<10} {'latency':>8}  effect / palette / bri / colour / live (from) / power / uptime")
    last_up = get(a.wled + "/json/info").get("uptime", 0)
    for state, expect in STEPS:
        before = snapshot(a.wled, names)
        t0 = time.time()
        try:
            r = post(f"{a.brain}/api/arena/state", {"state": state})
        except Exception as e:  # noqa: BLE001
            print(f"{state:<10} POST failed: {e}")
            continue
        if not r.get("success"):
            print(f"{state:<10} refused by brain: {r.get('error')}")
            continue
        latency = None
        snap = before
        while time.time() - t0 < a.dwell:
            time.sleep(a.poll)
            try:
                snap = snapshot(a.wled, names)
            except Exception as e:  # noqa: BLE001  (controller rebooting: keep going)
                print(f"{'':<10} {'':<10} {'':>8}  !! controller did not answer ({e.__class__.__name__}) at +{time.time() - t0:.1f}s")
                continue
            if snap["uptime"] is not None and snap["uptime"] < last_up:
                print(f"{'':<10} {'':<10} {'':>8}  !! REBOOT detected: uptime {last_up}s -> {snap['uptime']}s at +{time.time() - t0:.1f}s in {state}")
            last_up = snap["uptime"] or last_up
            if latency is None and (snap["fx"], snap["on"], snap["live"]) != (before["fx"], before["on"], before["live"]):
                latency = time.time() - t0
        brain = get(f"{a.brain}/api/state")["data"]["arena"]["current_state"]
        lat = f"{latency:.1f}s" if latency is not None else "same"
        live = f"live from {snap['lip']}" if snap["live"] else "no realtime"
        print(f"{state:<10} {brain:<10} {lat:>8}  {snap['fx']} / {snap['pal']} / bri {snap['bri']} / "
              f"{snap['col']} / {live} / {snap['pwr_mA']} mA / up {snap['uptime']}s / on={snap['on']}   [expect: {expect}]")


main()
