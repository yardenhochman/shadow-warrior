#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Send a command frame to a station over Wi-Fi (UDP port 4211).

    uv run tools/udp_cmd.py 192.168.1.50 start     # StartAudio (keep-alive: repeat every second)
    uv run tools/udp_cmd.py 192.168.1.50 stop
"""

from __future__ import annotations

import argparse
import socket

import station_monitor as sm

CMDS = {"start": 0, "stop": 1}  # station-proto Command variant order


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("host")
    ap.add_argument("cmd", choices=CMDS)
    ap.add_argument("--port", type=int, default=4211)
    a = ap.parse_args()
    frame = sm.encode_command(1, CMDS[a.cmd])
    socket.socket(socket.AF_INET, socket.SOCK_DGRAM).sendto(frame, (a.host, a.port))
    print(f"sent {a.cmd} to {a.host}:{a.port} ({len(frame)} bytes)")


if __name__ == "__main__":
    main()
