#!/bin/bash
# Runs on the Pi (deploy.sh calls it). Idempotent.
# The game runs on the Pi itself: bridge.py (WLED UDP + bag BLE + web server on :8090) and a
# headless Chromium that runs the simulator page in kiosk mode, sound out through PipeWire
# (Bluetooth speaker, else the 3.5 mm jack / HDMI). Starts on boot.
set -euo pipefail
D="$HOME/shadow-warrior/simulator/live/pi"
export DEBIAN_FRONTEND=noninteractive
need=()
for p in chromium pipewire pipewire-pulse wireplumber libspa-0.2-bluetooth bluez avahi-daemon libnss-mdns pulseaudio-utils curl; do
  dpkg -s "$p" >/dev/null 2>&1 || need+=("$p")
done
if [ "${#need[@]}" -gt 0 ]; then
  sudo apt-get update -q && sudo apt-get install -y -q "${need[@]}"
fi
[ -x "$HOME/.local/bin/uv" ] || curl -LsSf https://astral.sh/uv/install.sh | sh
sudo loginctl enable-linger "$USER"          # user services (PipeWire, the game) run without a login
mkdir -p ~/.config/systemd/user ~/.config/shadow-warrior ~/.local/bin
[ -f ~/.config/shadow-warrior/pi.env ] || cp "$D/pi.env.example" ~/.config/shadow-warrior/pi.env
cp "$D"/sw-*.service ~/.config/systemd/user/
install -m 755 "$D/sw" ~/.local/bin/sw
grep -q 'local/bin' ~/.profile 2>/dev/null || echo 'PATH="$HOME/.local/bin:$PATH"' >> ~/.profile
"$HOME/.local/bin/uv" run --quiet --script "$HOME/shadow-warrior/simulator/live/bridge.py" --help >/dev/null  # warm the venv
export XDG_RUNTIME_DIR="/run/user/$(id -u)"
systemctl --user daemon-reload
systemctl --user enable --now pipewire pipewire-pulse wireplumber sw-bridge sw-game sw-speaker
systemctl --user restart sw-bridge sw-game
sleep 5
~/.local/bin/sw status
