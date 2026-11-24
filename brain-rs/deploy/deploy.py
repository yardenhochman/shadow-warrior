#!/usr/bin/env python3
# /// script
# requires-python = ">=3.9"
# ///
"""Build brain-rs for aarch64 and install it on a station as a systemd service.

    uv run deploy/deploy.py shadow-brain.local
    uv run deploy/deploy.py 10.0.0.28 --ssh-opt HostKeyAlias=10.0.0.27 --config

Layout on the station: /opt/shadow-brain/{shadow-warrior-brain,config.yaml,src/web,
shadow_warrior.db,...}, run by the unit deploy/shadow-brain.service. The station
keeps its own config.yaml (adapter, WLED addresses...): it is only pushed on the
first install or with --config.
"""
import argparse
import pathlib
import shlex
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
BIN = ROOT / "target/aarch64-unknown-linux-gnu/release/shadow-warrior-brain"
STAGE = ".shadow-brain-stage"  # in the ssh user's home


def run(cmd, **kw):
    print("+", " ".join(shlex.quote(c) for c in cmd), flush=True)
    return subprocess.run(cmd, check=True, **kw)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("host")
    ap.add_argument("--user", default="warrior", help="ssh login and service user (default: warrior)")
    ap.add_argument("--dir", default="/opt/shadow-brain")
    ap.add_argument("--ssh-opt", action="append", default=[], help="extra ssh -o option, e.g. HostKeyAlias=...")
    ap.add_argument("--image", default="brain-build", help="podman build image (see `just image`)")
    ap.add_argument("--no-build", action="store_true", help="deploy the existing release binary")
    ap.add_argument("--config", action="store_true", help="also overwrite the station's config.yaml")
    ap.add_argument("--no-restart", action="store_true")
    a = ap.parse_args()

    target = f"{a.user}@{a.host}"
    ssh_opts = ["-o", "BatchMode=yes"] + [x for o in a.ssh_opt for x in ("-o", o)]
    ssh = ["ssh", *ssh_opts, target]
    rsh = "ssh " + " ".join(shlex.quote(o) for o in ssh_opts)

    if not a.no_build:
        run(["podman", "run", "--rm", "-v", f"{ROOT.parent}:/project", "-w", "/project/brain-rs", a.image,
             "cargo", "build", "--release", "--target", "aarch64-unknown-linux-gnu"])
    if not BIN.exists():
        sys.exit(f"missing {BIN}; build first")

    uid = subprocess.run(ssh + [f"id -u {shlex.quote(a.user)}"], check=True, capture_output=True, text=True).stdout.strip()
    unit = (ROOT / "deploy/shadow-brain.service").read_text().replace("@USER@", a.user).replace("@UID@", uid)
    run(ssh + [f"mkdir -p ~/{STAGE}/src"])
    run(ssh + [f"cat > ~/{STAGE}/shadow-brain.service"], input=unit, text=True)
    run(["rsync", "-az", "-e", rsh, str(BIN), str(ROOT / "config.yaml"), f"{target}:{STAGE}/"])
    run(["rsync", "-az", "--delete", "-e", rsh, str(ROOT / "src/web"), f"{target}:{STAGE}/src/"])

    d, u = shlex.quote(a.dir), shlex.quote(a.user)
    steps = [
        f"sudo install -d -o {u} -g {u} {d} {d}/src",
        f"sudo systemctl stop shadow-brain 2>/dev/null || true",
        f"sudo install -m 755 -o {u} -g {u} ~/{STAGE}/shadow-warrior-brain {d}/",
        f"sudo rsync -a --delete --chown={u}:{u} ~/{STAGE}/src/web {d}/src/",
        (f"sudo install -m 644 -o {u} -g {u} ~/{STAGE}/config.yaml {d}/config.yaml" if a.config else
         f"[ -f {d}/config.yaml ] || sudo install -m 644 -o {u} -g {u} ~/{STAGE}/config.yaml {d}/config.yaml"),
        f"sudo install -m 644 ~/{STAGE}/shadow-brain.service /etc/systemd/system/shadow-brain.service",
        "sudo systemctl daemon-reload",
        "sudo systemctl enable shadow-brain",
    ]
    if not a.no_restart:
        steps.append("sudo systemctl restart shadow-brain")
    run(ssh + [" && ".join(steps)])

    if not a.no_restart:
        run(ssh + ["sleep 8; systemctl is-active shadow-brain; "
                   "sudo journalctl -u shadow-brain -n 8 --no-pager -o cat | sed 's/\\x1b\\[[0-9;]*m//g' | cut -c1-200"])


main()
