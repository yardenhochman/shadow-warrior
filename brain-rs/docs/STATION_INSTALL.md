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
- [ ] Leave the onboard radio for the speakers unless the speaker sits far from the
      board: the onboard radio is the weaker one (see section 0), and which adapter
      suits the speakers depends on your layout (open question in `shadow-warrior-b2r`).
      BlueZ's default controller can be either; check with `bluetoothctl list`.

## 3b. Bluetooth speakers (verified with a UE BOOM 4 on 2026-11-17)

BlueZ alone cannot play A2DP audio: without an audio server the pairing succeeds but
the log says `a2dp-sink profile connect failed: Protocol not available`.

- [ ] Install PipeWire (check first: `sudo apt-get install -s ...` must show **0 removals**):
      ```
      sudo apt-get install pipewire wireplumber libspa-0.2-bluetooth pipewire-alsa pipewire-pulse pulseaudio-utils
      ```
- [ ] PipeWire is a per-user service and the station has no login: enable lingering for the
      service user, and tell WirePlumber not to wait for a login seat before enabling Bluetooth:
      ```
      sudo loginctl enable-linger warrior
      mkdir -p ~/.config/wireplumber/wireplumber.conf.d
      cat > ~/.config/wireplumber/wireplumber.conf.d/10-bluez-headless.conf <<'EOF'
      wireplumber.profiles = {
        main = {
          monitor.bluez.seat-monitoring = disabled
        }
      }
      EOF
      export XDG_RUNTIME_DIR=/run/user/$(id -u)      # the uid is NOT necessarily 1000 (it was 1001)
      systemctl --user enable --now pipewire.socket pipewire-pulse.socket wireplumber.service
      ```
      `RTKit` warnings in `wpctl status` are harmless.
- [ ] Put the speaker in pairing mode, then (onboard controller shown; use `select` for another):
      `sudo bluetoothctl --timeout 18 scan on` to find the address, then in one session
      `agent NoInputNoOutput`, `default-agent`, `pair <addr>`, `trust <addr>`, `connect <addr>`.
      Pairing prints `Pairing successful`; `trust` makes it reconnect by itself.
- [ ] If the profile still says "Protocol not available" (the link came up before PipeWire
      registered its A2DP endpoints), connect the profile directly:
      `sudo busctl call org.bluez /org/bluez/hciN/dev_AA_BB_CC_DD_EE_FF org.bluez.Device1 ConnectProfile s 0000110b-0000-1000-8000-00805f9b34fb`.
      `bluetoothctl connect` can hang; wrap it in `timeout`.
- [ ] `XDG_RUNTIME_DIR=/run/user/<uid> wpctl status` lists the speaker under Audio > Sinks
      (default sink `*`). Play a test: `pw-play tone.wav` and `aplay tone.wav` (the ALSA
      `default` device is routed to PipeWire by `pipewire-alsa`).
- [ ] The brain's unit already sets `XDG_RUNTIME_DIR` and starts after `user@<uid>.service`
      (`deploy.py` fills in the uid). Not yet verified: the playlist playing through the
      speaker from the brain, and surviving a speaker power-cycle (`shadow-warrior-67w`).
- **Warning: with no microphone the brain "listens" to its own music.** Once PipeWire is
  installed the ALSA default also exposes a capture device, so the audio manager starts
  "successfully" without a mic, but PipeWire links that capture stream to the speaker's
  *output monitor* (`wpctl status` shows `input_FL < BOOM 4:monitor_FL`). Shout detection and
  presence would then react to the playlist, and the health page shows audio as connected.
  A real microphone must be the default source (see `shadow-warrior-abr`, `-c6y`); tracked in
  the beads issue about the monitor capture.

## 3c. Music

- [ ] Download on a build machine, audio only and **without re-encoding**: YouTube's native
      AAC stream is `m4a`, which the brain decodes (Opus/webm it cannot):
      ```
      yt-dlp -f "bestaudio[acodec^=mp4a]" --restrict-filenames --no-overwrites --ignore-errors \
        -o "%(playlist_index)02d_%(title).80s.%(ext)s" "<playlist url>"
      ```
- [ ] `rsync` them to `/opt/shadow-brain/music/` and make sure `music.formats` lists the
      extension (`m4a` is in the repo default; mp3, flac and wav also work). The brain loads the
      playlist at startup, so restart the service. Verified: 18 `m4a` tracks load and the
      brain starts playing when a fight begins.
- [ ] Tracks come out at very different loudness (measured -15.1 to -3.4 LUFS, two peak above
      0 dBFS); decide on normalisation before an event. Only use music you may play publicly.

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
- [ ] Speakers: pairing and a PipeWire sink verified (section 3b); the brain playing its playlist and surviving a speaker reconnect are **untested** (`shadow-warrior-b2r`, `-67w`).

## Pitfalls we hit

- `pkill -f shadow-warrior-brain` over ssh matches its own shell; use a pattern like
  `pkill -f '[s]hadow-warrior-brain$'` (or `systemctl stop`).
- `bluetoothctl scan on` runs an interleaved classic inquiry; to scan LE only use
  `bluetoothctl scan le`.
- `btmgmt` exits at once when stdin is closed (background jobs); keep stdin open.
- `apt-get -qq` hides removals: always run `apt-get install -s ...` and read every `Remv` line first.
- A fight started through the API ends by itself after the inactivity timeout, which
  is easy to miss when testing by hand.
