#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["pyserial>=3.5", "matplotlib>=3.8"]
# ///
"""Live plot of a sensor station's USB serial stream (audio levels, radar, direction).

    uv run tools/station_live.py                       # auto-pick port
    uv run tools/station_live.py -p /dev/cu.usbserial-0001 --window 60
    uv run tools/station_live.py --file capture.bin    # replay a --save capture at real time

Panels: L/R RMS (dBFS) with peak; radar moving/static distance (cm) with a
presence strip; direction angle (deg) and confidence, once the firmware sends
Direction frames (unknown frame types are skipped). Close the window to quit.
"""

from __future__ import annotations

import argparse
import threading
import time
from collections import deque

import matplotlib.pyplot as plt
from matplotlib.animation import FuncAnimation

import station_monitor as sm  # same directory


def reader_thread(args, store: dict, lock: threading.Lock, stop: threading.Event) -> None:
    src = sm.file_source(args.file) if args.file else sm.serial_source(args.port or sm.pick_port(), args.baud, None)
    t0 = time.monotonic()
    for chunk in sm.chunks(src):
        if stop.is_set():
            return
        try:
            _, msg = sm.decode_frame(chunk)
        except Exception:
            continue  # log text, unknown variant or CRC failure
        t = time.monotonic() - t0
        kind = type(msg).__name__
        with lock:
            if kind == "Audio":
                store["audio"].append((t, msg.rms_dbfs_l, msg.rms_dbfs_r, msg.peak_dbfs))
            elif kind == "Radar":
                store["radar"].append((t, msg.moving_dist_cm, msg.static_dist_cm, msg.present))
            elif kind == "Direction":
                store["dir"].append((t, getattr(msg, "angle_deg", float("nan")), getattr(msg, "confidence", 0.0)))
            elif kind == "Hello":
                store["hello"] = msg
        if args.file:
            time.sleep(0.02)  # crude replay pacing


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("-p", "--port")
    ap.add_argument("-b", "--baud", type=int, default=115200)
    ap.add_argument("--file")
    ap.add_argument("--window", type=float, default=30.0, help="seconds shown")
    args = ap.parse_args()

    store = {"audio": deque(maxlen=20000), "radar": deque(maxlen=20000), "dir": deque(maxlen=20000), "hello": None}
    lock = threading.Lock()
    stop = threading.Event()
    threading.Thread(target=reader_thread, args=(args, store, lock, stop), daemon=True).start()

    fig, (ax_a, ax_r, ax_d) = plt.subplots(3, 1, figsize=(11, 8), sharex=True)
    (l_l,) = ax_a.plot([], [], label="L rms", color="tab:blue")
    (l_r,) = ax_a.plot([], [], label="R rms", color="tab:orange")
    (l_p,) = ax_a.plot([], [], label="peak", color="gray", lw=0.7, alpha=0.7)
    ax_a.set(ylabel="dBFS", ylim=(-90, 0))
    ax_a.legend(loc="upper left", ncol=3)
    ax_a.grid(alpha=0.3)

    (r_m,) = ax_r.plot([], [], label="moving", color="tab:red")
    (r_s,) = ax_r.plot([], [], label="static", color="tab:green")
    ax_r.set(ylabel="radar cm", ylim=(0, 600))
    ax_r.legend(loc="upper left", ncol=2)
    ax_r.grid(alpha=0.3)
    present_art = []

    (d_a,) = ax_d.plot([], [], ".", label="angle", color="tab:purple")
    ax_d2 = ax_d.twinx()
    (d_c,) = ax_d2.plot([], [], "-", label="confidence", color="gray", alpha=0.6)
    ax_d.set(ylabel="angle deg", ylim=(-95, 95), xlabel="s")
    ax_d2.set(ylabel="confidence")
    ax_d.grid(alpha=0.3)
    ax_d.text(0.01, 0.9, "direction (appears when firmware sends Direction frames)", transform=ax_d.transAxes, fontsize=8, color="gray")
    fig.tight_layout()

    def update(_):
        with lock:
            audio, radar, dirs, hello = list(store["audio"]), list(store["radar"]), list(store["dir"]), store["hello"]
        now = max([s[-1][0] for s in (audio, radar, dirs) if s] or [0.0])
        lo = max(0.0, now - args.window)
        ax_d.set_xlim(lo, lo + args.window)
        if audio:
            t, l, r, p = zip(*audio)
            l_l.set_data(t, l); l_r.set_data(t, r); l_p.set_data(t, p)
        if radar:
            t, m, s, pr = zip(*radar)
            r_m.set_data(t, m); r_s.set_data(t, s)
            vis = max([*m, *s][-300:] or [0])
            ax_r.set_ylim(0, max(100, min(1000, vis * 1.3)))
            for a in present_art:
                a.remove()
            present_art.clear()
            # presence strip along the bottom: green when present
            xs = [x for x, v in zip(t, pr) if v and x >= lo]
            if xs:
                present_art.append(ax_r.plot(xs, [2] * len(xs), "|", color="tab:green", ms=6)[0])
        if dirs:
            t, a, c = zip(*dirs)
            d_a.set_data(t, a); d_c.set_data(t, c)
            ax_d2.set_ylim(0, max(1.0, max(c[-300:]) * 1.2))
        if hello is not None:
            fig.suptitle(f"station {hello.station_id}  fw {hello.fw_version}  radar {hello.radar_fw}", fontsize=9)
        return l_l, l_r, l_p, r_m, r_s, d_a, d_c

    anim = FuncAnimation(fig, update, interval=100, cache_frame_data=False)  # noqa: F841
    try:
        plt.show()
    finally:
        stop.set()


if __name__ == "__main__":
    main()
