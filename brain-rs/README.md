# Shadow Warrior Brain (Rust)

Modern Rust rewrite of the Shadow Warrior training system brain with improved architecture and new game mechanics.

## Features

### Architecture
- **Async/await** throughout using Tokio runtime
- **Event-driven** pub/sub architecture
- **State machine** with lifecycle hooks
- **Modular services** for each hardware component
- **Type-safe configuration** with serde

### Hardware Integration
- **BLE** - Punching bag via btleplug (Nordic UART Service)
- **BLE status service** - optional read-only GATT server (`ble_status` config, BlueZ via `bluer`, Linux only) so a phone can read state/health; no extra build deps beyond `libdbus-1-dev`, needs `bluetoothd` at runtime
- **WLED** - LED controllers via HTTP JSON API + UDP realtime (WARLS protocol)
- **Tasmota** - Smart plugs via HTTP
- **Audio** - Microphone capture via cpal with shout detection
- **Music** - Playlist playback via rodio

### Game Logic

**States:** `SUSPENDED` ↔ `IDLE` → `WARMING` → `FIGHT` → `COOLDOWN` → `IDLE`

1. **SUSPENDED** - System idle, no presence detected
2. **IDLE** - Waiting for player, breathing LED effect
3. **WARMING** - Player building energy by shouting
   - Energy bar fills with shout intensity
   - Energy decays over time
   - Transitions to FIGHT when bar is full
4. **FIGHT** - Active fighting session
   - Music starts playing from playlist
   - Tracks punches and activity
   - Ends after configured duration OR inactivity timeout
5. **COOLDOWN** - Post-fight recovery period
   - Automatically transitions back to IDLE

### Web Dashboard
- Real-time state visualization
- Energy bar display (WARMING state)
- Fight timer and statistics
- Hardware connection status
- Manual controls
- SSE (Server-Sent Events) for live updates

## Configuration

Edit `config.yaml`:

```yaml
server:
  host: "0.0.0.0"
  port: 8000

arena:
  warming_energy_threshold: 100.0
  fight_duration_sec: 180
  fight_inactivity_timeout_sec: 30
  cooldown_duration_sec: 60
  shout_energy_multiplier: 10.0
  energy_decay_rate: 0.5

ble:
  auto_connect: true
  device_name_patterns: ["shadow", "warrior", "punch", "bag"]
  alpha: 0.8
  threshold: 2.0

wled:
  controllers:
    - ip: "192.168.1.100"
      num_leds: 180   # one column: 3 m x 60 LEDs/m (one controller per column)
    # A controller hosting several columns lists them: strips: [{column: 1, leds: 180}, ...]
    # (see docs/LEDS.md, "Layout")

tasmota:
  plugs:
    - ip: "192.168.1.200"
      name: "main_lights"

music:
  playlist_dir: "./music"
  shuffle: true
  volume: 0.8
```

LED modes per arena state (idle/cooldown built-in effects, warming/fight UDP realtime): see
[docs/LEDS.md](docs/LEDS.md).

Sensor station (radar presence, remote microphones, levels): see [docs/STATION.md](docs/STATION.md).

A WLED controller `ip` may carry an HTTP port (`"192.168.1.5:8090"`); realtime UDP still goes
to that host's port 21324. The browser simulator's bridge uses this to stand in for WLED
(see `simulator/web/README.md`, "Brain mode"). Any key can be overridden from the environment
with a `BRAIN_` prefix, e.g. `BRAIN_WLED='{controllers=[{ip="127.0.0.1:8090",num_leds=180}]}'`.
`audio.input: false` (`BRAIN_AUDIO='{input=false}'`) runs without microphone capture.

### BLE status service: threat model

The optional `ble_status` GATT server (off by default) is reachable by any BLE
device within tens of meters, through walls. Two flags control exposure:

- `require_encryption` (default `true`): BlueZ requires an encrypted (bonded)
  link to read the characteristics. Pair the owner's phone out of band
  (`bluetoothctl`, pairable on, then pair/trust from the phone) and keep the
  brain non-pairable otherwise. Just-Works pairing is encrypted but not
  authenticated (no MITM protection); authenticated pairing would need a BlueZ
  agent with IO capabilities. `false` is a debug opt-out (e.g. nRF Connect
  without pairing): anyone in range can then read, and a warning is logged.
  Note bluer has no security flag on notifications; verify on hardware that an
  unpaired client cannot subscribe.
- `expose_activity` (default `false`): with `false` only `on` or `suspended`
  (the exact arena state is hidden, since warming/fight/cooldown reveal that
  someone is in the arena), the schedule-active flag and per-device
  connected/stale are published. With `true` the exact state is published and
  the summary also carries presence, uptime, energy %, remaining fight
  time, punches, max power and total sessions, which reveal occupancy and
  activity patterns.

Devices are identified only by opaque ids (`ble`, `audio`, `music`, `wled0`,
`tasmota1`, ... by sorted order); IPs and user labels are never sent over BLE.

## Networking and offline operation

The arena must work with no internet and with the venue WiFi absent. The brain
only needs a local network to reach the WLED controllers, Tasmota plugs and
dashboard clients; BLE (punching bag, speakers) and the microphone do not use it.

**Provisioning (OS configuration, not part of the brain):**

- Run a dedicated 2.4 GHz access point for the arena (WLED/Tasmota ESPs are
  2.4 GHz only). Pin it to channel 1, 6 or 11 (no auto channel) to avoid
  channel hopping that drops the ESPs.
- Connect the brain over ethernet, and have the AP bridge/route to it (a router
  with DHCP reservations is simplest). Give WLED controllers and plugs fixed
  addresses or DHCP reservations, since `config.yaml` refers to them by IP.
- The server listens on `0.0.0.0:<server.port>`, so the dashboard is reachable
  from any client on the arena network.

**Behaviour when the network is missing or flaky:**

- Startup never waits on the network. An unreachable or unresolvable WLED
  controller is skipped with a warning (`wled_manager::start`); the BLE manager
  retries every `ble.retry_interval_sec`; Tasmota calls use short timeouts and
  one retry and never block the state machine.
- A 30 s `GET /json/info` poll keeps WLED health current while idle: three
  consecutive failures mark a controller disconnected (`WledDisconnected`),
  and the next success reconnects it. A controller skipped at boot gets its
  UDP realtime socket created once the poll sees it alive.
- The punching bag link is probed every `ble.retry_interval_sec`; a dropped
  bag is detected, reconnected and has fight mode restored.
- Device health, including `stale`, is exposed under `hardware` in
  `GET /api/state`.

**Installing a station:** see [docs/STATION_INSTALL.md](docs/STATION_INSTALL.md) for the full checklist
(OS, network, Bluetooth dongle, deploy as a systemd service, verification).

## Development

### System dependencies

Several crates link against system C libraries, which need their development headers plus `pkg-config` and a C compiler at build time:

| Library | Pulled in by | Fedora | Debian / Raspberry Pi OS |
|---------|--------------|--------|--------------------------|
| D-Bus | `btleplug` (BLE central) and `bluer` (BLE status GATT server, Linux only), both via `libdbus-sys` | `dbus-devel` | `libdbus-1-dev` |
| ALSA | `cpal` / `rodio` (audio, via `alsa-sys`) | `alsa-lib-devel` | `libasound2-dev` |
| OpenSSL | `reqwest` (native-tls, via `openssl-sys`) | `openssl-devel` | `libssl-dev` |
| pkg-config | build scripts of the above | `pkgconf-pkg-config` | `pkg-config` |
| C compiler | bundled SQLite (`sqlx` → `libsqlite3-sys`) | `gcc` | `build-essential` |

SQLite is compiled from source (the `bundled` feature), so no system SQLite package is needed.

Fedora:
```bash
sudo dnf install gcc pkgconf-pkg-config dbus-devel alsa-lib-devel openssl-devel
```

Debian / Raspberry Pi OS (building on the Pi itself):
```bash
sudo apt install build-essential pkg-config libdbus-1-dev libasound2-dev libssl-dev
```

At runtime the target machine needs the matching shared libraries (`libdbus-1`, `libasound2`, `libssl`) and a running BlueZ daemon for BLE.

### Build
```bash
cargo build --release
```

### Cross-compiling for Raspberry Pi

Targets:
- Raspberry Pi 3 / Orange Pi Zero 3W: `aarch64-unknown-linux-gnu`

The same three libraries are needed **for the target architecture** (e.g. `libdbus-1-dev:arm64`, `libasound2-dev:arm64`), along with a cross linker and `PKG_CONFIG_ALLOW_CROSS=1`. Rather than setting this up on the host, use the podman build image (`Dockerfile.build`), which installs them:

```bash
just image       # build the podman image once
just build-arm64   # aarch64
```

The container mounts the repository root (not just `brain-rs/`) because `station-proto` is a path dependency in `../station-fw`.

`Cross.toml` holds the equivalent package list for [`cross`](https://github.com/cross-rs/cross) (`cross build --target <triple>`).

When adding a dependency that links a new system library, add its `-dev` package to `Dockerfile.build`, `Cross.toml` and the table above.

### Run
```bash
cargo run
```

### Test
```bash
cargo test
```

### Recording raw audio for detector tuning
Do not use Voice Memos or similar apps: they apply noise suppression/AGC and lossy compression,
which makes the clips useless for tuning the shout detector. Use the recorder (needs `ffmpeg`):
```bash
just record yael              # -> clips/yael_1.wav, next run yael_2.wav, ...
just record yael -d 0         # pick the input device (macOS avfoundation index; ALSA name on Linux)
uv run deploy/record_clip.py --list   # list input devices
```
Raw 48 kHz mono 16-bit WAV. Enter stops; it prints duration, peak and clipped-sample count (back
the mic off if it clips). ffmpeg needs ~1.5 s to open the mic: wait for the "recording" line.
To record on the station itself through the brain's own pipeline, use `POST /api/audio/record/start`.

### Evaluating WAV files offline (comparing microphones)
`just eval` runs the brain's own front-end and shout detector (`src/services/wav_eval.rs`, the
same `FrontEnd` + earshot VAD + `ShoutDetector` code the live brain runs) over WAV files, with
the `audio:` settings from `config.yaml` (plus `BRAIN_*` env):
```bash
just eval clips/a.wav clips/b.wav                       # per-second table + every detection
just eval clips/a.wav --set shout_margin_db=12 --set vad_threshold=0.4   # try other tunables
just eval clips/a.wav --csv /tmp/frames.csv --json /tmp/report.json      # per-frame features
just eval clips/a.wav --playback                         # as if our speakers were playing
```
Input: 16/24/32-bit PCM or float WAV at any rate. Stereo is evaluated as `left`, `right` and `mix`
separately. Per file and channel it prints peak/clipping, then one row per second (max band level,
adaptive noise floor, max margin over the floor, mean VAD, VAD-gate %, frames above the margin,
centroid/tilt/flatness of the loudest quarter of frames, median voiced F0, % shouty frames,
detections), every `SHOUT` with its time, and a summary that counts, among frames loud enough,
which check failed (gate, voiced, centroid, tilt, flatness): that is what stopped a missed shout.
Time is the frame index, not the wall clock, so runs are deterministic.

Caveats: the noise floor and the VAD start fresh at the start of each file, so the first seconds
differ from a long-running brain (give each file a few seconds of room tone before the first
shout). A file that starts with digital silence makes the floor start at -100 dB and noise then
reads as shouting (bead shadow-warrior-aei).

To compare two microphones (e.g. station I2S ICS-43434 vs BOMGE U202 + cardioid):
1. Record both at the same time from the same shouts, at the same spot and distance: either one
   stereo file with one mic per channel (macOS: an Aggregate Device in Audio MIDI Setup, or
   `ffmpeg -f avfoundation -i ":1" -f avfoundation -i ":2" -filter_complex amerge=inputs=2 both.wav`),
   or one raw file per mic from the same session (`just record`, `POST /api/audio/record/start`).
   Start with 5 s of room tone, then shouts at the distances that matter (player spot, arena edge),
   plus talking, music at game volume and bag punches as negatives.
2. `just eval both.wav` (or `just eval stationmic.wav bomge.wav`).
3. Compare per mic: detections on the shouts (hit rate) and on the negatives (false alarms), the
   margin over the floor on the same shout (higher = more headroom, better distance rejection),
   the floor level (self-noise), clipping on the loudest shouts, and the blocker counts.

## API Endpoints

### Status API
- `GET /api/state` - Complete brain state snapshot
- `GET /api/events` - SSE stream for real-time updates
- `GET /api/stats` - Detailed statistics

### Control API
- `POST /api/arena/presence` - Trigger presence detection
  ```json
  {"detected": true}
  ```
- `POST /api/arena/suspend` - Force suspend arena

## Project Structure

```
brain-rs/
├── src/
│   ├── main.rs              # Entry point + axum server
│   ├── config.rs            # Configuration management
│   ├── models.rs            # Data structures
│   ├── core/
│   │   ├── events.rs        # Event bus
│   │   └── state_machine.rs # State machine
│   ├── services/
│   │   ├── arena_manager.rs # Orchestration
│   │   ├── ble_manager.rs   # BLE communication
│   │   ├── audio_manager.rs # Audio + energy bar
│   │   ├── wled_manager.rs  # LED effects
│   │   ├── tasmota_manager.rs # Smart plugs
│   │   └── music_manager.rs # Music playback
│   ├── api/
│   │   ├── status.rs        # Status endpoints
│   │   └── arena.rs         # Control endpoints
│   └── web/                 # Web dashboard
│       ├── index.html
│       ├── style.css
│       └── app.js
├── config.yaml              # Configuration
└── Cargo.toml
```

## License

MIT
