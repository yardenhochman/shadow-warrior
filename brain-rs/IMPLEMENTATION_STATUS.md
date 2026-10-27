# Shadow Warrior Brain - Implementation Status

> **For humans.** This is a periodic snapshot (2026-10-26) of what the Rust brain does
> and what has and has not been verified on real hardware.
> **Agents must not use this file for task tracking or "what to do next".**
> Work items live in beads (`bd ready`, `bd list`, `bd show <id>`); it is the single
> source of truth and this file will lag behind it. Do not add TODO lists here.

## Overview

The brain is a Tokio daemon. Services talk to each other through a broadcast event bus
(`src/core/events.rs`) and a game state machine (`src/core/state_machine.rs`):

```
SUSPENDED <-> IDLE -> WARMING -> FIGHT -> COOLDOWN -> IDLE
```

- IDLE: LEDs breathe to attract players.
- WARMING: shouts fill the energy bar.
- FIGHT: bar full; music, fire effects, UV lights; punches score.
- SUSPENDED: game switched off (manually, or by the schedule).

All code below is unit-tested without hardware (`cargo test`, `cargo clippy --all-targets`
are both clean). **"Tested" means synthetic signals and mock HTTP servers, not real devices.**

## What exists

| Area | Module | State |
|---|---|---|
| Config | `config.rs`, `config.yaml` | figment (YAML + env), serde defaults so old configs keep loading |
| Event bus / state machine | `core/` | done |
| Hardware status registry | `core/status_registry.rs` | per-device connection, last-seen, last-error, stale flag, via bus events; shown in the Hardware tab |
| Arena game logic | `services/arena_manager.rs` | state flow, energy bar, fight timers |
| Audio front-end | `services/audio_frontend.rs` (+ `ring`, `clip`) | band-pass/anti-alias, noise floor, frame features, ring buffer, clip recorder (WAV + JSON) |
| Audio capture | `services/audio_manager.rs` | cpal mic -> front-end -> features channel |
| Shout detection | `services/shout_detector.rs` | VAD-gated, dB-above-floor, pitch/spectral checks, playback-aware margin, <300 ms in frame counts |
| Presence | `services/presence.rs` | fused evidence (mic + punches, pluggable `PresenceSource`), enter/leave hysteresis; re-fires on schedule resume; manual API goes through fusion |
| Schedule | `services/schedule_manager.rs` | daily windows (local time, optional days, overnight), persisted in SQLite |
| Music | `services/music_manager.rs` | rodio playlist, gapless queue-ahead advance |
| WLED | `services/wled_manager.rs` (+ `protocol`) | HTTP effects/palettes with device name resolution + cache, UDP realtime |
| Tasmota | `services/tasmota_manager.rs` | per-state plug mapping (`on_states`), concurrent, retry once |
| BLE punching bag | `services/ble_manager.rs` | Nordic UART via btleplug |
| Discovery / devices | `services/discovery_manager.rs`, `database_manager.rs` | SQLite devices + settings tables |
| Web dashboard | `src/web/` | state, energy bar, stats, hardware, controls, schedule editor, lights (WLED) tab, shout tunables, clip recording, device health |

## HTTP API

State/events: `GET /api/state`, `/api/events` (SSE), `/api/logs`, `/api/stats`.
Arena: `POST /api/arena/{presence,suspend,shout,punch,state,reset_stats}`,
`GET|POST /api/arena/config`.
Schedule: `GET|POST /api/schedule`.
WLED: `GET /api/wled/{effects,palettes}`, `POST /api/wled/effect`.
Audio: `POST /api/audio/clip` (on-demand clip recording).
Devices: `POST /api/discovery/scan`, `GET|POST /api/devices`, `POST /api/devices/:id`.

## Not verified on real hardware

Everything has only run against synthetic inputs. Expect tuning:

- **Shout and presence thresholds** (dB margins, decay, leave timeout, F0/centroid limits) are
  educated guesses. They need real clips from the clip recorder: party noise at ~100 m,
  distant crowd, shouts, bag hits, wind, our own playback.
- **Earshot VAD on real audio** (it is trained on normal speech and may score shouts low).
- **Gapless music**: audible gap at track boundaries, stutter on a Pi Zero, whether
  `Player::len()` behaves as assumed on the real backend.
- **Tasmota/WLED/BLE** behaviour on flaky Wi-Fi and BLE links.

## Known gaps and design notes

- Schedule edits via the API persist in SQLite and override `config.yaml`; an invalid
  `config.yaml` schedule fails startup.
- Semantics of the schedule (windows, forced SUSPENDED when off) were designed from issue
  titles and still need an owner review.
- Devices that drop now publish disconnect/error events (BLE stream end, audio stream loss, WLED after 3 consecutive failures), but there is no auto-reconnect yet (BLE `connected` is never reset, audio does not restart).
- Idle devices (BLE without punches, WLED without polling, audio without level events) show as stale in `hardware.health` after `server.health_stale_sec` (300 s) although connected; there is no keepalive.
- No model-based shout classifier yet (a hook exists in `shout_detector.rs`).
- `legacy/wpa_supplicant.conf` in git history contains real Wi-Fi passphrases (rotate them).

For the current, authoritative list of open work, run `bd list` in the repo root.

## Build and run

```bash
cargo run                      # dashboard on http://localhost:8000
cargo test && cargo clippy --all-targets
just --list                    # Pi cross-compile recipes (podman)
```

See `README.md` for hardware setup and system build dependencies.
