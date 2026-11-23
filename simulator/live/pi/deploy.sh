#!/bin/bash
# Copy the game to the installation Pi and (re)install it. Run on the PC/Mac:
#   simulator/live/pi/deploy.sh [host]        (default shadow-warrior.local)
#   simulator/live/pi/deploy.sh --wait [host] waits until the Pi answers SSH, then deploys
# Needs the Pi's SSH key (the SD card was written with ~/.ssh/pi_agent_ed25519.pub).
set -euo pipefail
WAIT=0; [ "${1:-}" = --wait ] && { WAIT=1; shift; }
HOST="${1:-shadow-warrior.local}"
KEY="${SW_PI_KEY:-$HOME/.ssh/pi_agent_ed25519}"
SSH=(ssh -i "$KEY" -o StrictHostKeyChecking=accept-new -o ConnectTimeout=5 -o BatchMode=yes)
REPO="$(cd "$(dirname "$0")/../../.." && pwd)"
if [ "$WAIT" = 1 ]; then
  echo "waiting for $HOST ..."
  until "${SSH[@]}" "yarden@$HOST" true 2>/dev/null; do sleep 20; done
fi
echo "copying the game to $HOST"
rsync -a --delete -e "${SSH[*]}" \
  --include='/simulator/***' --include='/music/***' --include='/brain-rs/' --include='/brain-rs/config.yaml' \
  --exclude='__pycache__' --exclude='*' "$REPO/" "yarden@$HOST:shadow-warrior/"
"${SSH[@]}" "yarden@$HOST" 'bash ~/shadow-warrior/simulator/live/pi/install.sh'
