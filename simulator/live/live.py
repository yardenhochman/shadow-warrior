#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.9"
# dependencies = ["pyyaml>=6"]
# ///
"""Start/stop the live-hardware bridge (and, in brain mode, brain-rs) in the background.

    simulator/live/live.py [start] [bridge.py args...]   e.g. --wled 192.168.1.50 --no-ble
    simulator/live/live.py brain [bridge.py args...]     brain-rs + bridge in brain mode
    simulator/live/live.py stop | status | log

Needs `uv` (the shebang runs the script through it). Works on Linux and macOS.

State (PIDs + logs) lives in ${XDG_STATE_HOME:-~/.local/state}/shadow-warrior-live/.
Sim mode: stop the brain first (one BLE central; two writers fight over the strip).

brain: runs $BRAIN_BIN, else brain-rs/target/release/shadow-warrior-brain (built with
`cargo build --release` if missing), from brain-rs/ with its config.yaml, except
`wled.controllers`, which point at the bridge (BRAIN_WLED env override):
  - no --layout: one controller of 180 LEDs at 127.0.0.1:<bridge port>;
  - --layout FILE|JSON: the same controllers/strips the bridge simulates (see
    simulator/web/README.md, "Layout").
The brain API listens on $BRAIN_PORT (default 8000). Other BRAIN_* env overrides pass
through, e.g. BRAIN_AUDIO='{input=false}' (no microphone) or BRAIN_BLE='{auto_connect=false}'
(leave the bag alone).
"""
import os
import signal
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
STATE = Path(os.environ.get("XDG_STATE_HOME") or Path.home() / ".local/state") / "shadow-warrior-live"
BRIDGE_PID, BRIDGE_LOG = STATE / "bridge.pid", STATE / "bridge.log"
BRAIN_PID, BRAIN_LOG = STATE / "brain.pid", STATE / "brain.log"

sys.path.insert(0, str(HERE))


def read_pid(path):
    try:
        return int(path.read_text().strip())
    except (OSError, ValueError):
        return None


def alive(path):
    pid = read_pid(path)
    if pid is None:
        return False
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    return True


def stop(path, name):
    """SIGTERM, wait up to 15 s, then report. Removes the pid file."""
    pid = read_pid(path)
    if pid is not None and alive(path):
        os.kill(pid, signal.SIGTERM)
        for _ in range(60):
            if not alive(path):
                break
            time.sleep(0.25)
        if alive(path):
            print(f"{name} still running (PID {pid})")
            return False
        print(f"{name} stopped")
    path.unlink(missing_ok=True)
    return True


def spawn(cmd, log, pidfile, env=None, cwd=None):
    """Detached background process (own session), output to `log`."""
    with open(log, "wb") as out:
        p = subprocess.Popen(cmd, stdout=out, stderr=subprocess.STDOUT, stdin=subprocess.DEVNULL,
                             env=env, cwd=cwd, start_new_session=True)
    pidfile.write_text(str(p.pid))
    return p


def log_has(log, text):
    try:
        return text in log.read_text(errors="replace")
    except OSError:
        return False


def tail(path, n=20):
    try:
        return "".join(path.read_text(errors="replace").splitlines(keepends=True)[-n:])
    except OSError:
        return ""


def arg_value(args, name, default=None):
    """Value of `--name VALUE` or `--name=VALUE` in a bridge argument list."""
    for i, a in enumerate(args):
        if a == name and i + 1 < len(args):
            return args[i + 1]
        if a.startswith(name + "="):
            return a.split("=", 1)[1]
    return default


def start_bridge(args):
    if alive(BRIDGE_PID):
        print(f"bridge already running (PID {read_pid(BRIDGE_PID)}), log {BRIDGE_LOG}")
        return True
    spawn(["uv", "run", str(HERE / "bridge.py"), *args], BRIDGE_LOG, BRIDGE_PID)
    for _ in range(60):
        if log_has(BRIDGE_LOG, "simulator: http") or not alive(BRIDGE_PID):
            break
        time.sleep(0.5)
    if not alive(BRIDGE_PID):
        print("bridge failed to start:\n" + tail(BRIDGE_LOG))
        return False
    print(f"bridge running (PID {read_pid(BRIDGE_PID)}), log {BRIDGE_LOG}")
    return True


def toml_controllers(layout):
    """brain-rs BRAIN_WLED override for the controllers a bridge layout simulates."""
    items = []
    for c in layout.controllers:
        strips = ",".join(
            "{column=%d,leds=%d%s}" % (s.column, s.leds, ",reverse=true" if s.reverse else "") for s in c.strips
        )
        items.append('{ip="%s",num_leds=%d,strips=[%s]}' % (c.ip, c.num_leds, strips))
    return "{columns=%d,controllers=[%s]}" % (layout.columns, ",".join(items))


def cmd_start(args):
    if not start_bridge(args):
        return 1
    print(f"open http://localhost:{arg_value(args, '--port', '8090')}/")
    return 0


def cmd_brain(args):
    if alive(BRAIN_PID):
        print(f"brain already running (PID {read_pid(BRAIN_PID)})")
        return 0
    brain_dir = REPO / "brain-rs"
    binary = Path(os.environ.get("BRAIN_BIN") or brain_dir / "target/release/shadow-warrior-brain")
    if not os.access(binary, os.X_OK):
        print("building the brain (cargo build --release)...")
        if subprocess.run(["cargo", "build", "--release"], cwd=brain_dir).returncode != 0:
            return 1
    port = arg_value(args, "--port", "8090")
    brain_port = os.environ.get("BRAIN_PORT", "8000")
    env = dict(os.environ)
    if os.environ.get("BRAIN_PORT"):
        env["BRAIN_SERVER"] = "{port=%s}" % brain_port
    layout_arg = arg_value(args, "--layout")
    if layout_arg:
        from layout import Layout, LayoutError
        try:
            env["BRAIN_WLED"] = toml_controllers(Layout.load(layout_arg))
        except LayoutError as e:
            print(f"bad --layout: {e}")
            return 1
    else:
        env["BRAIN_WLED"] = '{controllers=[{ip="127.0.0.1:%s",num_leds=180}]}' % port
    if not start_bridge(["--brain", f"http://127.0.0.1:{brain_port}", *args]):
        return 1
    spawn([str(binary)], BRAIN_LOG, BRAIN_PID, env=env, cwd=brain_dir)
    for _ in range(40):
        if log_has(BRAIN_LOG, "Starting server") or not alive(BRAIN_PID):
            break
        time.sleep(0.5)
    if not alive(BRAIN_PID):
        print("brain failed to start:\n" + tail(BRAIN_LOG))
        return 1
    print(f"brain running (PID {read_pid(BRAIN_PID)}), log {BRAIN_LOG}")
    print(f"open http://localhost:{port}/")
    return 0


def cmd_stop(_args):
    ok = True
    if BRAIN_PID.exists():
        ok = stop(BRAIN_PID, "brain") and ok
    if alive(BRIDGE_PID):
        ok = stop(BRIDGE_PID, "bridge") and ok
    else:
        print("bridge not running")
        BRIDGE_PID.unlink(missing_ok=True)
    return 0 if ok else 1


def cmd_status(_args):
    if alive(BRIDGE_PID):
        print(f"bridge running (PID {read_pid(BRIDGE_PID)})")
        print(tail(BRIDGE_LOG, 5), end="")
    else:
        print("bridge not running")
    if alive(BRAIN_PID):
        print(f"brain running (PID {read_pid(BRAIN_PID)}), log {BRAIN_LOG}")
    return 0


def cmd_log(_args):
    os.execvp("tail", ["tail", "-f", str(BRIDGE_LOG)])


COMMANDS = {"start": cmd_start, "brain": cmd_brain, "stop": cmd_stop, "status": cmd_status, "log": cmd_log}


def main():
    STATE.mkdir(parents=True, exist_ok=True)
    argv = sys.argv[1:]
    cmd = argv[0] if argv and argv[0] in COMMANDS else ("start" if not argv or argv[0].startswith("-") else None)
    if cmd is None:
        print("usage: live.py [start [bridge args]|brain [bridge args]|stop|status|log]")
        return 2
    rest = argv[1:] if argv and argv[0] == cmd else argv
    return COMMANDS[cmd](rest)


if __name__ == "__main__":
    sys.exit(main())
