#!/usr/bin/env bash
# Start/stop the live-hardware bridge in the background.
#   simulator/live/live.sh [start] [bridge.py args...]   e.g. --wled 192.168.1.50 --no-ble
#   simulator/live/live.sh brain [bridge.py args...]     brain-rs + bridge in brain mode, e.g. --wled 192.168.1.50
#   simulator/live/live.sh stop | status | log
# State (PIDs + logs) in ${XDG_STATE_HOME:-~/.local/state}/shadow-warrior-live/.
# Sim mode: stop the brain first (one BLE central; two writers fight over the strip).
# brain: runs brain-rs/target/release/shadow-warrior-brain (built with cargo if missing; or
# $BRAIN_BIN) from brain-rs/ with its config.yaml, except wled.controllers, which point at the
# bridge (BRAIN_WLED env override). The brain API listens on $BRAIN_PORT (default 8000).
# Other BRAIN_* env overrides pass through, e.g. BRAIN_AUDIO='{input=false}' (no microphone)
# or BRAIN_BLE='{auto_connect=false}' (leave the bag alone).
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
state="${XDG_STATE_HOME:-$HOME/.local/state}/shadow-warrior-live"
pidf="$state/bridge.pid" logf="$state/bridge.log"
bpidf="$state/brain.pid" blogf="$state/brain.log"
mkdir -p "$state"
alive() { [ -s "$1" ] && kill -0 "$(cat "$1")" 2>/dev/null; }
running() { alive "$pidf"; }
port() { local p=8090 prev=""; for a in "$@"; do [ "$prev" = --port ] && p="$a"; prev="$a"; done; echo "$p"; }
stop_pid() { # pidfile name
  if alive "$1"; then
    kill "$(cat "$1")"; for _ in $(seq 60); do alive "$1" || break; sleep 0.25; done
    alive "$1" && { echo "$2 still running (PID $(cat "$1"))"; return 1; }
    echo "$2 stopped"
  fi
  rm -f "$1"
}
start_bridge() {
  if running; then echo "bridge already running (PID $(cat "$pidf")), log $logf"; return 0; fi
  setsid nohup uv run "$here/bridge.py" "$@" >"$logf" 2>&1 </dev/null &
  echo $! >"$pidf"
  for _ in $(seq 60); do grep -q "simulator: http" "$logf" 2>/dev/null && break; running || break; sleep 0.5; done
  running || { echo "bridge failed to start:"; tail -20 "$logf"; exit 1; }
  echo "bridge running (PID $(cat "$pidf")), log $logf"
}

cmd="${1:-start}"; [ $# -gt 0 ] && shift
case "$cmd" in
  start)
    start_bridge "$@"
    echo "open http://localhost:$(port "$@")/"
    ;;
  brain)
    if alive "$bpidf"; then echo "brain already running (PID $(cat "$bpidf"))"; exit 0; fi
    bin="${BRAIN_BIN:-$repo/brain-rs/target/release/shadow-warrior-brain}"
    if [ ! -x "$bin" ]; then
      echo "building the brain (cargo build --release)..."
      (cd "$repo/brain-rs" && cargo build --release)
    fi
    p="$(port "$@")" bp="${BRAIN_PORT:-8000}"
    [ -n "${BRAIN_PORT:-}" ] && export BRAIN_SERVER="{port=$BRAIN_PORT}"
    start_bridge --brain "http://127.0.0.1:$bp" "$@"
    # sh records its own PID, then execs the brain (setsid may fork, so $! is not the brain).
    (cd "$repo/brain-rs" || exit 1; BRAIN_WLED="{controllers=[{ip=\"127.0.0.1:$p\",num_leds=180}]}" \
      setsid nohup sh -c 'echo $$ >"$0"; exec "$1"' "$bpidf" "$bin" >"$blogf" 2>&1 </dev/null &)
    sleep 0.5
    for _ in $(seq 40); do grep -q "Starting server" "$blogf" 2>/dev/null && break; alive "$bpidf" || break; sleep 0.5; done
    alive "$bpidf" || { echo "brain failed to start:"; tail -20 "$blogf"; exit 1; }
    echo "brain running (PID $(cat "$bpidf")), log $blogf"
    echo "open http://localhost:$p/"
    ;;
  stop)
    [ -s "$bpidf" ] && stop_pid "$bpidf" brain
    if running; then stop_pid "$pidf" bridge; else echo "bridge not running"; rm -f "$pidf"; fi
    ;;
  status)
    if running; then echo "bridge running (PID $(cat "$pidf"))"; tail -5 "$logf"; else echo "bridge not running"; fi
    alive "$bpidf" && echo "brain running (PID $(cat "$bpidf")), log $blogf"
    true
    ;;
  log) exec tail -f "$logf" ;;
  *) echo "usage: $0 [start [bridge args]|brain [bridge args]|stop|status|log]"; exit 2 ;;
esac
