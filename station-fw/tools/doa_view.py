#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["pyserial>=3.5", "matplotlib>=3.8"]
# ///
"""Direction-of-arrival viewer for a sensor station (GCC-PHAT `Direction` frames).

Live, from the serial port (or a raw capture made with --save / station_monitor.py --save):

    uv run tools/doa_view.py                          # live gauge per sound event
    uv run tools/doa_view.py --save run1.bin          # same, and keep the raw bytes
    uv run tools/doa_view.py --file run1.bin --png run1.png   # angle vs time + histogram
    uv run tools/doa_view.py --file cal0.bin --calibrate      # broadside bias

The station sends one Direction per 21 ms frame above its level gate. A clap or a
click gives a short run of frames; consecutive frames (gap <= --gap frames) are
grouped into one *event*, reported by its most confident frame. Frames below
--min-conf are ignored (uncorrelated noise scores about 4, rarely above 8).

At the end (Ctrl-C or end of file) a summary prints: an angle histogram of the
events, and the median bias-corrected lag (what --calibrate is for).

Angles: 0 = broadside, positive = toward the R mic (SEL = 3V3), negative =
toward the L mic (SEL = GND). Two mics cannot tell front from back, so a source
behind the array reads the same as its mirror image in front.
"""

from __future__ import annotations

import argparse
import statistics
import sys
from dataclasses import dataclass, field

from station_monitor import Direction, chunks, decode_frame, file_source, pick_port, serial_source

FRAME_SECONDS = 1024 / 48_000


@dataclass
class Event:
    frames: list[Direction] = field(default_factory=list)

    @property
    def best(self) -> Direction:
        return max(self.frames, key=lambda d: d.confidence)

    @property
    def t(self) -> float:
        return self.frames[0].frame * FRAME_SECONDS


def gauge(angle: float, width: int = 37) -> str:
    """-90 ... +90 as a text bar with the broadside mark in the middle."""
    pos = round((angle + 90) / 180 * (width - 1))
    cells = ["-"] * width
    cells[width // 2] = "|"
    cells[max(0, min(width - 1, pos))] = "#"
    return "L [" + "".join(cells) + "] R"


def print_event(ev: Event) -> None:
    b = ev.best
    spread = max(d.angle_deg for d in ev.frames) - min(d.angle_deg for d in ev.frames)
    print(
        f"t={ev.t:8.2f}s  {gauge(b.angle_deg)}  {b.angle_deg:+6.1f} deg  lag {b.lag_samples:+6.2f}  "
        f"conf {b.confidence:6.1f}  {b.rms_dbfs:6.1f} dBFS  frames {len(ev.frames):2d} spread {spread:4.1f}",
        flush=True,
    )


def histogram(events: list[Event], step: int) -> None:
    if not events:
        print("no events above --min-conf")
        return
    bins: dict[int, int] = {}
    for ev in events:
        a = ev.best.angle_deg
        k = int((a + 90 + step / 2) // step) * step - 90
        bins[k] = bins.get(k, 0) + 1
    top = max(bins.values())
    print(f"\nangle histogram ({len(events)} events, {step} deg bins, event = best frame):")
    for k in range(-90, 91, step):
        n = bins.get(k, 0)
        if n or -90 <= k <= 90:
            print(f"  {k:+4d} deg | {'#' * round(40 * n / top):<40s} {n}")


def summary(events: list[Event], step: int, calibrate: bool) -> None:
    histogram(events, step)
    if not events:
        return
    lags = [ev.best.lag_samples for ev in events]
    angles = [ev.best.angle_deg for ev in events]
    med = statistics.median(lags)
    print(
        f"\nevents {len(events)}: angle median {statistics.median(angles):+.1f} deg "
        f"(min {min(angles):+.1f}, max {max(angles):+.1f}); lag median {med:+.3f} samples"
        + (f", stdev {statistics.stdev(lags):.3f}" if len(lags) > 1 else "")
    )
    if calibrate:
        print(
            "\ncalibration (source at 0 deg / broadside): the station already subtracts its current\n"
            f"STATION_DOA_BIAS. New bias = current bias {med:+.3f}. With the default bias 0:\n"
            f"    STATION_DOA_BIAS={med:.3f} just flash"
        )


def plot(events: list[Event], frames: list[Direction], path: str, step: int) -> None:
    import matplotlib

    matplotlib.use("Agg")
    import matplotlib.pyplot as plt

    fig, (ax1, ax2) = plt.subplots(1, 2, figsize=(13, 5), gridspec_kw={"width_ratios": [3, 1]})
    if frames:
        sc = ax1.scatter(
            [d.frame * FRAME_SECONDS for d in frames],
            [d.angle_deg for d in frames],
            c=[d.confidence for d in frames],
            s=8,
            cmap="viridis",
            vmin=0,
            vmax=max(50.0, max(d.confidence for d in frames) * 0.5),
        )
        fig.colorbar(sc, ax=ax1, label="confidence")
    ax1.scatter([ev.t for ev in events], [ev.best.angle_deg for ev in events], marker="x", c="red", s=30, label="event")
    ax1.set(xlabel="time since boot (s)", ylabel="angle (deg, + = R mic side)", ylim=(-95, 95))
    ax1.set_yticks(range(-90, 91, 15))
    ax1.grid(alpha=0.3)
    ax1.legend(loc="upper right")
    ax2.hist([ev.best.angle_deg for ev in events], bins=range(-90 - step // 2, 91 + step, step), orientation="horizontal")
    ax2.set(xlabel="events", ylim=(-95, 95))
    ax2.set_yticks(range(-90, 91, 15))
    ax2.grid(alpha=0.3)
    fig.tight_layout()
    fig.savefig(path, dpi=120)
    print(f"wrote {path}")


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("-p", "--port", help="serial port (default: first USB serial)")
    ap.add_argument("-b", "--baud", type=int, default=115200)
    ap.add_argument("--file", help="read a raw capture instead of the port")
    ap.add_argument("--save", help="append raw serial bytes to this file (replay with --file)")
    ap.add_argument("--min-conf", type=float, default=10.0, help="ignore frames below this confidence (default 10)")
    ap.add_argument("--gap", type=int, default=3, help="frames apart that still count as one event (default 3)")
    ap.add_argument("--frames", action="store_true", help="print every frame, not only one line per event")
    ap.add_argument("--bins", type=int, default=5, help="histogram bin width in degrees (default 5)")
    ap.add_argument("--calibrate", action="store_true", help="print the broadside bias to configure")
    ap.add_argument("--png", help="write angle-vs-time and histogram plot to this PNG at the end")
    args = ap.parse_args()

    save = open(args.save, "ab") if args.save else None
    src = file_source(args.file) if args.file else serial_source(args.port or pick_port(), args.baud, save)

    events: list[Event] = []
    frames: list[Direction] = []
    cur: Event | None = None
    try:
        for chunk in chunks(src):
            try:
                _, msg = decode_frame(chunk)
            except Exception:
                continue
            if not isinstance(msg, Direction) or msg.confidence < args.min_conf:
                continue
            frames.append(msg)
            if args.frames:
                print(f"  frame {msg.frame:7d}  {gauge(msg.angle_deg)}  {msg.angle_deg:+6.1f}  conf {msg.confidence:6.1f}")
            if cur and msg.frame - cur.frames[-1].frame <= args.gap:
                cur.frames.append(msg)
                continue
            if cur:
                print_event(cur)
            cur = Event([msg])
            events.append(cur)
    except KeyboardInterrupt:
        pass
    if cur:
        print_event(cur)
    summary(events, args.bins, args.calibrate)
    if args.png:
        plot(events, frames, args.png, args.bins)


if __name__ == "__main__":
    sys.exit(main())
