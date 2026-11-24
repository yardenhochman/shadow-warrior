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
