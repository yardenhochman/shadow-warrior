# The game on the installation Pi

The simulator game runs on the Pi itself, with no PC: `bridge.py` (strip over WLED UDP, bag over BLE,
web server on :8090) and a headless Chromium that runs the game page in kiosk mode (`?live=1&kiosk=1`).
Sound goes out through PipeWire: the paired Bluetooth speaker, else the 3.5 mm jack or HDMI.
Everything starts on boot (user services with linger).

## Install / update (from the PC or Mac)

    simulator/live/pi/deploy.sh            # shadow-warrior.local
    simulator/live/pi/deploy.sh --wait     # waits until the Pi is up, then deploys

It copies `simulator/`, `music/` (mp3s are not in git) and `brain-rs/config.yaml`, then runs `install.sh` on the
Pi (apt: chromium, pipewire, bluez, avahi; uv; user units `sw-bridge`, `sw-game`, `sw-speaker`; the `sw` command).

## Hardware

No GPIO wiring: the Pi talks to everything over the air.

| Part | Connection |
|---|---|
| Pi 3 | SD card in, micro-USB power (5 V 2.5 A). Joins the home Wi-Fi from the card. |
| LED strip | On its WLED controller (`wled-sw`, 192.168.1.203), controller on USB-C power. The Pi sends UDP. |
| Bag sensor (XIAO nRF52840 Sense) | Any USB power; BLE `ShadowWarriorPunchingBag`. A charge-only cable is enough here (data is only for flashing). |
| Speaker | Bluetooth: `sw pair-speaker` with the speaker in pairing mode, once. `sw-speaker` reconnects it. |

If the strip was off when the Pi booted, `sw restart` once it is on (the bridge starts without it).

## On the Pi

    sw status | start | stop | restart | log | pair-speaker | test-punch [power]

Settings: `~/.config/shadow-warrior/pi.env` (`WLED_HOST`, `SPEAKER_MAC`).

## Kiosk mode

Nobody clicks the page, so `?kiosk` starts the mic by itself. Without a mic (the Pi has none yet), a punch in
WARMING also counts as a shout: the first punch wakes the arena, about five more start the fight.

## Latency (PC, 23.11, fake strip)

Punch POST → LED frame out of the bridge: median 22 ms (6–58 ms; the page streams 30 fps); browser audio output
~34 ms. Not yet measured on the Pi. On top: the bag samples at 10 Hz with an EMA (`punching_bag.ino`,
`delay(100)`), so 0–100 ms before a punch is even seen, plus the BLE interval; Bluetooth speakers add
roughly 150–250 ms, so the sound lands after the light.

## Pi 3 caveats

1 GB RAM and one combined Wi-Fi/Bluetooth chip: headless Chromium renders the whole sim page, and BLE (bag) +
A2DP (speaker) + Wi-Fi (strip) share one radio, which can stutter the sound. Measure on the Pi before trusting it.
