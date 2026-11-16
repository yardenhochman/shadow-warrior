# Installing a brain station

Checklist for a new brain station (Raspberry Pi 3/4 or Orange Pi Zero 3W, both
aarch64). Everything marked **verified** was done on an Orange Pi Zero 3W
(Armbian 26.11 trixie) on 2026-11-16; items marked **untested** are on the
list but have not been run on hardware yet. Track work in beads, not here.

## 0. Pick the hardware

- [ ] Board: Raspberry Pi 3/4 or Orange Pi Zero 3W. The Pi Zero W is no longer a target.
- [ ] USB Bluetooth dongle with a **Realtek RTL8761B** chip (TP-Link UB500 `2357:0604`
      verified; ASUS USB-BT500 is the same chip). The Orange Pi's onboard radio hears the
      bag at about -85 dBm at 2 m and misses half the scans; the dongle measured about
      18 dB stronger. Unknown-chipset dongles are a gamble.
- [ ] Microphone: USB audio interface (BOMGE U202 + cardioid, **untested**) or I2S MEMS.
      I2S is easy on a Raspberry Pi; on the Orange Pi Zero 3W there is no stock I2S
      overlay (see beads `shadow-warrior-c6y`).
- [ ] The board has about 1 GB RAM: build on another machine, never on the board.

## 1. Flash the OS and first boot

- [ ] Armbian (Orange Pi) or Raspberry Pi OS Lite 64-bit (Pi).
- [ ] Create the service user `warrior` (sudo group), hostname `shadow-brain`, timezone
      `Asia/Jerusalem`. Armbian can preset all of this through `.not_logged_in_yet`.
- [ ] SSH key login for your machine (`ssh-copy-id`); passwords stay for the console only.
- [ ] Passwordless sudo for the sudo group (needed by `deploy.py`):
      ```
      echo '%sudo ALL=(ALL) NOPASSWD:ALL' > /tmp/s && sudo visudo -cf /tmp/s \
        && sudo install -m 440 -o root -g root /tmp/s /etc/sudoers.d/90-sudo-nopasswd
      ```
- [ ] Make `/etc/hosts` agree with the hostname (`127.0.1.1  shadow-brain`).
- [ ] Boards have no battery clock: the time is wrong until NTP syncs (about 30 s after
      the network is up). Log timestamps before that are off by the downtime.

## 2. Network

- [ ] Wire the board with ethernet to the arena access point and give it a DHCP
      reservation. WiFi on the board is optional and unneeded.
- [ ] WLED controllers and Tasmota plugs get fixed addresses or reservations
      (`config.yaml` refers to them by IP). Access point on a fixed 2.4 GHz channel
      1, 6 or 11. See "Networking and offline operation" in the README.
- [ ] The dashboard and control API listen on `0.0.0.0:8000` with no authentication
      (decision for now: LAN only).

## 3. Bluetooth

- [ ] Plug the dongle in; `lsusb` shows the ID (verified: `2357:0604`).
- [ ] Check the firmware loaded: `dmesg | grep -i rtl_bt` must not say "not found".
      - **Raspberry Pi OS / Debian:** `sudo apt install firmware-realtek` if missing.
      - **Armbian: do NOT `apt install firmware-realtek`.** It conflicts with
        `armbian-firmware` and apt removes it, which kills the onboard WiFi and
        Bluetooth at the next reboot (happened on 2026-11-16). Instead copy the two
        files only: `apt download firmware-realtek && dpkg-deb -x firmware-realtek_*.deb /tmp/fw`,
        then `sudo install -m 644 /tmp/fw/usr/lib/firmware/rtl_bt/rtl8761bu_{fw,config}.bin /lib/firmware/rtl_bt/`.
        (The file names and destination are verified; this exact `apt download` + extract sequence was not run as written.)
        If you already ran the install: `sudo apt install armbian-firmware`, then restore
        those two files as above, and reboot.
      - Reload without rebooting: `sudo modprobe -r btusb && sudo modprobe btusb`.
- [ ] `sudo hciconfig -a` shows a Realtek controller with a real BD address.
- [ ] Select the adapter in `config.yaml` by hardware, never by `hciN` (the numbers swap
      between boots; verified): `ble.adapter: "usb:2357:0604"`. Other selectors: `usb`,
      `builtin`. A wrong value makes the brain log the available adapters.
- [ ] Leave the onboard radio for the speakers (**untested**, see `shadow-warrior-b2r`).
      Note the dongle becomes BlueZ's default controller.

## 4. Deploy the brain

On your build machine (podman machine running; `just image` once):

- [ ] `just image`
- [ ] `just deploy <host> [--ssh-opt HostKeyAlias=...]` runs `deploy/deploy.py`: it
      cross-builds a release binary, installs it with `src/web` into `/opt/shadow-brain`,
      installs and enables `shadow-brain.service`, restarts it and prints the journal.
- [ ] First install copies `config.yaml`; edit it **on the station** (`/opt/shadow-brain/config.yaml`):
      `ble.adapter`, WLED controllers, Tasmota plugs, schedule. Later deploys keep it
      (pass `--config` to overwrite).
- [ ] `systemctl is-active shadow-brain`; `sudo journalctl -u shadow-brain -f`.
- [ ] The service starts with no network and no microphone (verified); missing devices
      show up as errors under `hardware` in `GET /api/state`.
- [ ] Stop it with `sudo systemctl stop shadow-brain` (SIGTERM): the brain then
      disconnects the bag. Killing it hard leaves the bag connected in BlueZ, and a
      connected bag does not advertise.

## 5. Punching bag firmware

- [ ] XIAO nRF52840 Sense, `arduino-cli`, board `Seeeduino:nrf52:xiaonRF52840Sense`,
      libraries: Seeed Arduino LSM6DS3. `punching_bag/punching_bag.ino`.
- [ ] Compile and upload (`arduino-cli compile ...`, `arduino-cli upload -p <port> ...`).
      If the board hangs and upload fails, double-tap reset (the `XIAO-SENSE` drive appears).
- [ ] `uv run scan_ble.py scan` shows `ShadowWarriorPunchingBag`. It advertises without
      a USB host (waits at most 2 s for serial).
- [ ] The bag only sends data in fight mode and only above its threshold (default 2.0 m/s^2
      of motion after removing gravity; tuning on a real bag is **untested**).

## 6. Verify the station end to end

- [ ] **Reboot test:** `sudo reboot`; with no manual step the service is active, the
      selected adapter is the dongle, and `ble.connected` is true in `/api/state`
      (verified: within about 2 minutes of power-up).
- [ ] **Punch test:** force a long fight so you do not race the timers (a fight ends after
      5 minutes by default without punches; set 1500/1800 s):
      ```
      curl -s localhost:8000/api/arena/config            # edit fight_inactivity_timeout_sec / fight_duration_sec, POST back
      curl -XPOST localhost:8000/api/arena/state -H 'content-type: application/json' -d '{"state":"Warming"}'
      curl -XPOST localhost:8000/api/arena/state -H 'content-type: application/json' -d '{"state":"Fight"}'
      ```
      Shake the bag; the log shows `Punch detected` once per hit (350 ms refractory).
      Set the state back to `Idle` afterwards.
- [ ] **Reconnect test:** `sudo bluetoothctl disconnect <bag address>`; the brain reconnects
      in about 10-30 s and fight mode is restored. Punches must not double (verified).
- [ ] WLED: effects change with the arena state (**untested**, `shadow-warrior-2`).
- [ ] Microphone: shout detection and clip recording (**untested**, `shadow-warrior-abr`, `-c6y`).
- [ ] Speakers: pair, play the playlist, survive a reconnect (**untested**, `shadow-warrior-b2r`, `-67w`).

## Pitfalls we hit

- `pkill -f shadow-warrior-brain` over ssh matches its own shell; use a pattern like
  `pkill -f '[s]hadow-warrior-brain$'` (or `systemctl stop`).
- `bluetoothctl scan on` runs an interleaved classic inquiry; to scan LE only use
  `bluetoothctl scan le`.
- `btmgmt` exits at once when stdin is closed (background jobs); keep stdin open.
- A fight started through the API ends by itself after the inactivity timeout, which
  is easy to miss when testing by hand.
