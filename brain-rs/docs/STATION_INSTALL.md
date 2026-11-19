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
- [ ] WLED controllers (one per column, section 3d) and Tasmota plugs get fixed addresses or reservations
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
      (`deploy.py` fills in the uid). The brain plays through PipeWire's ALSA default and
      PipeWire moves the stream onto the speaker by itself when its sink appears.
- [ ] **Reconnect supervision** (verified 2026-11-17): add the speaker to the station's
      `config.yaml`, restart the service:
      ```
      speaker:
        address: "10:94:97:40:6F:95"
        adapter: builtin        # the radio it is paired on (same selectors as ble.adapter)
        check_interval_sec: 10
        max_backoff_sec: 60
      ```
      Without it the station does NOT recover: after a drop PipeWire silently moves the music to
      the board's unused analog output, and the speaker can come back at link level without its
      A2DP profile, so the music plays into nothing until someone reconnects it by hand. With it,
      every `check_interval_sec` the brain reconnects a dropped speaker and re-requests the A2DP
      profile (retrying "operation in progress"), backing off up to `max_backoff_sec` while it
      keeps failing, and reports `hardware.health.speaker` plus `Speaker*` events.
      Measured: BlueZ-level drop -> audio back after about 9 s; real power-off for 55 s, then on ->
      lost detected within ~3 s, audio back about 10 s after the speaker came up (the backoff
      during a long outage does not delay it), no manual step.
- **Warning: with no microphone the brain "listens" to its own music.** Once PipeWire is
  installed the ALSA default also exposes a capture device, so the audio manager starts
  "successfully" without a mic, but PipeWire links that capture stream to the speaker's
  *output monitor* (`wpctl status` shows `input_FL < BOOM 4:monitor_FL`). Shout detection and
  presence would then react to the playlist, and the health page shows audio as connected.
  A real microphone must be the default source (see `shadow-warrior-abr`, `-c6y`).
  - [ ] Check after every audio change: `XDG_RUNTIME_DIR=/run/user/<uid> wpctl status`. Under
        Audio > Streams the brain's capture stream must be linked to the microphone, never to
        `<speaker>:monitor_*`. Under Audio > Sources the microphone must be the default (`*`);
        if it is not, `wpctl set-default <id>` (the id is in `wpctl status`).
  - [ ] **Untested candidate** (not run on the board yet; verify with the check above, and
        remove it if it changes nothing): stop PipeWire from capturing a sink monitor for
        applications that did not ask for one, in
        `~/.config/pipewire/pipewire-pulse.conf.d/10-no-monitor-capture.conf` and
        `~/.config/pipewire/client.conf.d/10-no-monitor-capture.conf`:
        ```
        stream.properties = {
          stream.capture.sink = false
        }
        ```
        then `systemctl --user restart pipewire pipewire-pulse wireplumber`. With no
        microphone present the capture stream should then stay unlinked instead of falling
        back to the monitor.
  - **Brain guard** (`audio.reject_monitor`, default `true`; `services/audio_source.rs`): at
    start the brain refuses a capture whose device name looks like a monitor/loopback
    ("monitor", "loopback", "stereo mix"), and for a generic alias (`default`, `pipewire`,
    "Default Audio Device") it asks `pactl` (2 s timeout, skipped if `pactl` is missing)
    whether the default source is a monitor. It then tries another named non-monitor input
    device; if none exists it does not open the stream, so no shout detection runs, and
    publishes an audio error: `hardware.audio` in `/api/state` shows not connected with
    `last_error` "start: no real microphone: capture is a sink monitor (...)". The rest of
    the brain keeps running. Recovery needs a restart of the service after plugging in a mic
    (there is no capture retry loop). If `pactl` is unavailable and the name is a generic
    alias, the guard cannot tell and capture starts, so keep the `wpctl` check above.
    **Not verified on the Orange Pi yet** (`shadow-warrior-25j` follow-up).

## 3c. Music

The current playlist, the exact tracks and the rebuild steps are in [MUSIC.md](MUSIC.md).

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
- [ ] Normalise before copying: `uv run deploy/normalize_music.py <dir> [--lufs -14] [--tp -1]`
      writes `<dir>/normalized/*.m4a` (originals untouched; a plain gain per track, `loudnorm`
      only where the gain would push the peak over the ceiling) and prints a before/after table.
      Verified on the 18-track playlist: spread 11.7 LU -> 0.1 LU at -14 LUFS, peaks all below
      0 dBFS (raw downloads ranged -15.1 to -3.4 LUFS and two clipped). This costs one lossy
      generation (re-encoded as AAC 192k with `aac_at`, macOS only; edit the encoder elsewhere).
      The result is quieter overall than the raw tracks, so raise `music.volume` as needed.
- [ ] Only use music you may play publicly.

## 3d. LED strips (WLED)

The arena has 4-5 columns, each one 3 m strip at 60 LEDs/m = **180 LEDs**, normally one WLED
controller per column. A controller may host 2-4 strips (its LED outputs concatenated, so
`num_leds = 180 x strips`). Design and rendering: [LEDS.md](LEDS.md). Only idle and cooldown use
built-in WLED effects; warming and fight are UDP realtime from the brain.

On each controller (WLED web UI):

- [ ] Fixed address or DHCP reservation; note it for `config.yaml`.
- [ ] LED preferences: strip type and colour order of your LEDs, **LED count 180 per strip**
      (several outputs for a multi-strip controller, in the column order you will put in
      `strips`), "Maximum current" matching the PSU (the one verified controller runs 1000 mA;
      the strip's PSU rating is **unknown**, check it before raising the limit).
- [ ] Sync / realtime: "Receive UDP realtime" on (default), port 21324. The brain sends a frame
      every tick, so the 2 s realtime timeout never expires while it runs.
- [ ] Version: effect and palette ids differ between WLED builds. The brain resolves names
      against the controller (`/json/eff`, `/json/pal`) and the fallback table is for 0.15.4
      (verified on an ESP32-C3). Use exact names in `config.yaml` (`"Fade"`, `"Fire 2012"`, `"Lava"`).
- [ ] An ESP32-C3 rebooted under 4 Hz GET polling: do not poll it hard. The brain polls
      `/json/info` every 1.5 s at most and only to read the LED count and detect reboots.

In `/opt/shadow-brain/config.yaml` on the station:

```yaml
wled:
  columns: 4                                       # optional, default = highest column + 1
  controllers:
    - {ip: "10.0.0.55", num_leds: 180}             # one strip = column 0 (column = list index)
    - ip: "10.0.0.56"                              # one controller hosting columns 1 and 2
      strips:                                      # consecutive in LED order; num_leds optional (= sum)
        - {column: 1, leds: 180}
        - {column: 2, leds: 180, reverse: true}    # wired top-down (LED 0 at the top)
```

- Column 0 is the outermost on one side; the bag hangs in the **centre**, so the middle columns
  are nearest to it (punch shockwaves reach them first).
- Invalid layouts (duplicate column, strips not summing to `num_leds`) stop the brain at start
  with a clear error. A multi-strip controller keeps its configured layout even if WLED reports
  another LED count (warning in the log).
- Put every controller in `config.yaml`. One found by discovery is added on the lowest free
  column with 180 LEDs (never a taken column; ignored if all `columns` are taken), which is not
  your real layout.
- The idle/cooldown/warming/fight looks live under `wled.effects`. Defaults equal the chosen
  looks, so on a station with an old config **delete the whole `wled.effects` block** (or copy
  it from the repo `config.yaml`) rather than keeping stale values. `deploy.py --config` would
  overwrite everything else on the station too, so edit by hand.

Check the strips before trusting the brain:

- [ ] `uv run deploy/wled_try.py --wled http://<ip> --fx Fade --sx 40 --col 255,25,0 --col 0,0,0`
      shows the idle breathing (stop the brain first; this tool never polls).
- [ ] Without any hardware, `simulator/live/live.py brain --wled none --layout <file>` shows the
      real brain's frames in the browser; `simulator/live/layouts/four-columns.yaml` is a ready
      layout (see `simulator/web/README.md`, "Layout").

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
- [ ] WLED, per state (**untested on the real strips**, `shadow-warrior-2`; the realtime scenes
      were checked only in the browser simulator): Idle = slow ember breathing (built-in),
      Warming = a bar rising from the bottom of every column as shouts add energy, with a
      glow at its head, Fight = dim ember with a shockwave per punch (inner columns first),
      Cooldown = fire (built-in), Suspended = off. Run
      `uv run deploy/wled_walkthrough.py --brain http://<station>:8000 --wled http://<ip>` (single
      controller) and watch every column; a reverse-wired strip must also grow upwards.
- [ ] Microphone: shout detection and clip recording (**untested**, `shadow-warrior-abr`, `-c6y`).
- [ ] Speakers: pairing and a PipeWire sink verified (section 3b); the playlist plays through the brain and the speaker reconnects after a drop or power-cycle (section 3b); A2DP + BLE contention on one radio is **untested** (`shadow-warrior-b2r`).

## Pitfalls we hit

- `pkill -f shadow-warrior-brain` over ssh matches its own shell; use a pattern like
  `pkill -f '[s]hadow-warrior-brain$'` (or `systemctl stop`).
- `bluetoothctl scan on` runs an interleaved classic inquiry; to scan LE only use
  `bluetoothctl scan le`.
- `btmgmt` exits at once when stdin is closed (background jobs); keep stdin open.
- `apt-get -qq` hides removals: always run `apt-get install -s ...` and read every `Remv` line first.
- A fight started through the API ends by itself after the inactivity timeout, which
  is easy to miss when testing by hand.
