# Sensor station link

`services/station_manager.rs` consumes the frames of the ESP32 sensor station (`station-fw/`,
wire format in `station-fw/station-proto`) and turns them into brain events. The station only
reports; the brain decides (no thresholds in firmware).

## Config (`station`, off by default)

```yaml
station:
  enabled: true
  udp_bind: "0.0.0.0:4210"        # null = no UDP; one frame per datagram
  station_cmd_addr: null          # "ip:4211"; unset = source address of the last datagram, port 4211
  serial_port: "/dev/ttyUSB0"     # null = no serial; reopened every 2 s after unplug
  serial_baud: 115200             # 921600 for the raw-audio firmware build
  stream_audio: false             # see below
  link_timeout_sec: 3.0           # no frame for this long -> StationDisconnected
  stats_interval_sec: 5.0
presence:
  radar_units: 0.5                # evidence per StationRadar event (about 2 per second)
  source_weights: { radar: 1.0 }
```

Environment override for a test brain (never open the dev machine's microphone):
`BRAIN_AUDIO='{input=false}' BRAIN_BLE='{auto_connect=false}' BRAIN_SERVER='{port=8099}'
BRAIN_STATION='{enabled=true,serial_port="/dev/cu.usbserial-0001"}'`.

## BLE provisioning (`station.provision`, off by default)

A station without working Wi-Fi advertises a GATT service (`station-fw/docs/BLE.md`).
`services/station_provisioner.rs` scans for it, reads the one-time challenge, writes the sealed
Wi-Fi settings and the brain's address, and disconnects. The station saves, reboots into Wi-Fi mode
(Wi-Fi and BLE do not fit in RAM together) and drops the link without a status notification. Success is
the station's UDP frames reaching `station_manager`: a `StationConnected` whose id equals the `id=<hex>`
from the `info` characteristic, within `up_timeout_sec`; otherwise `failed("no frames after provisioning:
wrong Wi-Fi settings?")` (the station returns to provisioning after 5 minutes, and is retried with backoff).
The pre-write status is only used to skip stations already Up or Connecting. Stations already up are left alone.

```yaml
station:
  provision:
    enabled: true
    wifi_ssid: "arena-wifi"      # the only required key
    wifi_pass: "..."             # never logged or serialized
    provision_key: ""            # default: env SYSTEM_SECRET; never logged
    brain_addr: ""               # empty = LAN IP toward the default route + udp_bind port
    adapter: null                # default: ble.adapter
    scan_interval_sec: 30
    scan_timeout_sec: 8
    up_timeout_sec: 60           # wait for the station's UDP frames after the write
    retry_backoff_sec: 60        # per station, doubles per failure
    retry_backoff_max_sec: 900
```

Without a passphrase (neither `provision_key` nor `SYSTEM_SECRET`) provisioning stays off and the
brain logs why. Progress is published as `StationProvision {station, state, detail}`
(found | provisioning | up(`station-<id>`) | failed(reason)) and shown per station in the status API
(`health.station_provision`). The bag and the provisioner serialise their scan/connect phases
with one lock (`ble_manager::ADAPTER_LOCK`); on a Pi 3 with a single adapter provisioning scans can
delay a bag reconnect by up to `scan_timeout_sec`; use `adapter:` for a second dongle if that matters.

### Field setup
1. Put `SYSTEM_SECRET=<random string>` in the gitignored `system.env` at the repo root (export it to the brain, e.g. systemd `EnvironmentFile=`; the firmware build
   reads the same file).
2. `just flash` each station (the firmware takes the secret at build time).
3. Set `station.provision.enabled: true` and `wifi_ssid`/`wifi_pass`, power the stations on.
   The brain provisions every station it sees; nothing else to configure.

A known-answer vector (`known_answer_vector` test in the provisioner) pins the sealed bytes for
key `sw-test`, challenge `0123456789ab`, so firmware can be cross-checked.

## What the brain does with it

| Station message | Brain |
|---|---|
| Hello | `StationConnected` (id, firmware), health `station.name` |
| Radar | `StationRadar{present, distance_cm, energies}` (changes at once, else every 500 ms). `present` is the radar's bool for target states 1..=3 only (4..=6 are background calibration). The `radar` presence source adds `radar_units` evidence per event with `present`; no energy thresholds (static energy falls with distance). |
| Audio (levels) | `StationLevel` (2 Hz), health `rms_dbfs_l/r` |
| Direction | `StationDirection{angle_deg, confidence}` (5 Hz), health `direction_deg`. Informational: no game decision uses it. |
| AudioChunk | with `stream_audio`: mixed to mono, 16 kHz, into the normal front-end and shout detector (`AudioManager::attach_external` / `push_external`). Dropped chunks (jump in `frame`) are replaced by silence, up to 5 s, so time stays real-time. |

`stream_audio: true` replaces the local microphone (`AudioManager::start` is skipped, the
station is the audio device). The brain sends `StartAudio` once per second, over UDP to the
station or over the serial port; the station stops by itself 5 s after the last one.
Raw audio needs the 921600 baud firmware on serial; over UDP it needs nothing special.

Link health is in `/api/state` under `hardware.health.station`: connected, last seen, radar,
levels, direction, `frames`, `frames_lost` (sequence gaps), `bad_frames` (CRC/COBS/format),
`audio_gap_frames` (silence filled). The same numbers go out as `StationLinkStats` events.

## Design notes

- `StationState` is a pure state machine (frame and time in, events and samples out); the
  transports are thin readers around it. Tests build frames with `station_proto::encode_frame`.
- Serial and UDP share one `StreamSplitter`, so serial log text is skipped and damaged frames
  are counted as `bad_frames`. A station restart (sequence or audio frame counter going backwards)
  is not counted as loss.
- Only the mono mix (L+R)/2 is fed to the detector; L/R stay separate only in the level events.
