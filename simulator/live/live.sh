#!/usr/bin/env bash
# Start/stop the live-hardware bridge in the background.
#   simulator/live/live.sh [start] [bridge.py args...]   e.g. --wled 192.168.1.203 --no-ble
#   simulator/live/live.sh stop | status | log
# State (PID + log) in ${XDG_STATE_HOME:-~/.local/state}/shadow-warrior-live/.
# Stop the brain first (one BLE central; two writers fight over the strip).
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
state="${XDG_STATE_HOME:-$HOME/.local/state}/shadow-warrior-live"
pidf="$state/bridge.pid" logf="$state/bridge.log"
mkdir -p "$state"
running() { [ -s "$pidf" ] && kill -0 "$(cat "$pidf")" 2>/dev/null; }
port() { local p=8090 prev=""; for a in "$@"; do [ "$prev" = --port ] && p="$a"; prev="$a"; done; echo "$p"; }

cmd="${1:-start}"; [ $# -gt 0 ] && shift
case "$cmd" in
  start)
    if running; then echo "already running (PID $(cat "$pidf")), log $logf"; exit 0; fi
    setsid nohup uv run "$here/bridge.py" "$@" >"$logf" 2>&1 </dev/null &
    echo $! >"$pidf"
    for _ in $(seq 60); do grep -q "simulator: http" "$logf" 2>/dev/null && break; running || break; sleep 0.5; done
    running || { echo "bridge failed to start:"; tail -20 "$logf"; exit 1; }
    echo "running (PID $(cat "$pidf")), log $logf"
    echo "open http://localhost:$(port "$@")/"
    ;;
  stop)
    if running; then kill "$(cat "$pidf")"; for _ in $(seq 20); do running || break; sleep 0.25; done; echo stopped; else echo "not running"; fi
    rm -f "$pidf"
    ;;
  status) if running; then echo "running (PID $(cat "$pidf"))"; tail -5 "$logf"; else echo "not running"; fi ;;
  log) exec tail -f "$logf" ;;
  *) echo "usage: $0 [start [bridge args]|stop|status|log]"; exit 2 ;;
esac
