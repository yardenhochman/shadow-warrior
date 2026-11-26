# Sound cues

Event-triggered sound effects and voice lines, played from files next to the music
(`src/services/sound_manager.rs`, bead shadow-warrior-hqm). No synthesis: every sound is a
file. Off unless `sound.enabled: true`.

## Config

```yaml
sound:
  enabled: true
  dir: "./sounds"          # cue files are looked up here
  volume: 1.0              # master gain (linear)
  music_duck_db: -9        # music level while a cue plays
  duck_ms: 300             # music stays ducked this long after the last cue ends
  max_concurrent: 4        # cues playing at once; extra triggers are dropped
  hook_from: 45            # punch power tiers: jab < 45 <= hook < 75 <= heavy
  heavy_from: 75
  cues:
    fight_started: { files: [fight1.wav, fight2.wav], gain: 1.0 }
    punch_heavy:   { files: [heavy1.wav, heavy2.wav], min_interval_ms: 150 }
```

Per cue: `files` (any format rodio decodes: wav, flac, mp3, ...), `gain` (linear, default 1),
`min_interval_ms` (drop triggers closer than this to the cue's last play, default 0).

## Cues

| Cue | Event |
| --- | --- |
| `session_started` | `SessionStarted` (arena back to IDLE) |
| `energy_full` | `EnergyFull` |
| `fight_started` | `FightStarted` |
| `fight_ended_time_elapsed`, `fight_ended_inactivity`, `fight_ended_manual` | `FightEnded` by reason; each falls back to `fight_ended` when not configured |
| `punch_jab`, `punch_hook`, `punch_heavy` | `PunchDetected`, tier by power (0..100) |

A cue with several files picks one at random and never the same file twice in a row.
Unconfigured cues are silent.

## Behaviour

- **Mixing**: cues use their own output stream, opened on the first cue, beside the music
  stream; the OS mixes them. Music is ducked (`MusicManager::set_duck`) while any cue plays.
- **Failures never block the game**: a missing or undecodable file, or no audio device, is
  logged once per file and the cue is skipped. Files can be added while the brain runs.
- **Dev machines**: this plays through the default output. Unit tests use a fake output and
  never open a device.

## Audio health (dashboard cards)

`/api/state` -> `hardware.health` carries two blocks, shown as the Microphone and Speakers cards.

**`audio_in`** (microphone, or the station stream when `station.stream_audio`):
`source` (`microphone` | `station`), `device` (name as cpal reports it), `pulse_source` (PipeWire/Pulse
source node and description behind an alias device such as `default`, via `pactl`), `sample_rate`,
`level_db` / `peak_db` (band level of the shout front-end, peak held 3 s), `reason` and a derived
`status`: `disabled` (`audio.input: false`) | `no_microphone` | `no_data` (no level for 3 s) | `silent`
(below -60 dB) | `signal`.

`no_microphone`: the brain never captures from a sink monitor or from an ALSA **null device** ("Discard
all samples (playback) or generate zero samples (capture)", also `null`); a null device is rejected
even with `audio.reject_monitor: false`. It reports `audio.connected: false` with `last_error`
"no microphone: only null/monitor devices present (<device>)" and retries `AudioManager::start`
every 5 s (doubling to 60 s), so a USB mic plugged in later is picked up without a restart. A capture
that is lost after it started (unplug while running) is still not restarted (shadow-warrior-8ev).

**`audio_out`**: `sink` / `sink_description` / `sink_bluetooth` / `volume_pct` / `muted` of the default
PipeWire/Pulse sink (polled with `pactl` every 5 s by `services/audio_output.rs`; rodio plays to this
default sink; absent without `pactl`), `route` (`bluetooth` | `builtin` | `unknown`), and the Bluetooth
speaker supervisor: `speaker_state` (`not_configured` | `connected` | `connecting` | `failed`),
`speaker_attempts`, `speaker_retry_in_sec`; link details and `last_error` stay in `health.speaker`. Music
playing is `health.music.powered`. The card says "Playing via <speaker>" or "Falling back to built-in
audio". There is no output test endpoint: nothing in the API plays a sound on demand.

## ShoutLevel: the live shout meter signal

`ShoutDetected` fires ~300-500 ms after onset (sustained, scored): right for counting shouts, far too
late for a live LED meter. `Event::ShoutLevel { level, db_above_floor }` is the continuous companion
(`services/shout_meter.rs`, run next to the detector on the same 16 ms front-end features for both the
local mic and the station stream; it does not touch `ShoutDetected`).

```
raw   = clamp((db_above_floor - lo) / (hi - lo), 0, 1)    lo/hi = audio.shout_meter_lo_db / _hi_db
        (lo and hi are raised by shout_playback_margin_db while our speakers play)
gate  = 0.25 + 0.75 * passed/4     passed = voiced, centroid, tilt, flatness checks of the detector
level = attack: exponential, tau = audio.shout_meter_attack_ms (0 = immediate); release: exponential, tau = audio.shout_meter_release_ms
```

- `db_above_floor` is the band-passed level over the adaptive noise floor (the detector's distance
  rejection applies: a far or steady sound hugs the floor and reads 0).
- The gate makes a loud non-shout (talking, music, a clap) read dim, not zero: all four checks failed
  = 25 %, all passed = 100 %. `audio.shout_meter_gate: false` = level only.
- Published at most every 2nd frame (~31 Hz) while the level is above zero, plus one final 0; nothing
  while silent. Frames dropped by the watch channel are credited, so the decay stays in real time.
- Consumers: the LED `ShoutMeter` layer (docs/LEDS.md) and a small live bar on the dashboard audio card.

| Key | Default | Meaning |
| --- | --- | --- |
| `audio.shout_meter_lo_db` | 35 | dB above the floor where the meter starts (reads 0 below) |
| `audio.shout_meter_hi_db` | 60 | dB above the floor where it is full |
| `audio.shout_meter_gate` | false | dim frames that do not look like a shout |
| `audio.shout_meter_release_ms` | 200 | release time constant |
| `audio.shout_meter_attack_ms` | 50 | attack time constant; filters key clicks and claps (a 30 ms click reaches ~45 %) |

Tuned on the station mic (tune1 recording, 35 shouts): shouts peak ~70 dB above the floor, loud talking ~28 dB, claps ~65 dB.
With lo 35 / hi 60 / attack 50 ms talking reads 0, claps ~0.6, shouts 1.0, and a shout crosses 50 % in ~90 ms. The spectral
gate did not separate the classes on this mic (all pass ~3 of 4 checks), so it is off. Retune these per mic gain.

Not tuned on recordings yet: use `shout_replay --features-csv` output to pick lo/hi for the I2S mics.

## Tuning on recordings

`shout_replay` (`src/bin/shout_replay.rs`, logic in `services/shout_replay.rs`) replays a WAV through
the same front-end and `ShoutDetector` the brain runs and scores the detections against a labels
file. Use it to tune the detector for the I2S microphones (ICS-43434 via a station; the detector was
first tuned on a USB mic). A debug build is fine; it runs far faster than real time and reports audio
time. (`eval-wav` is the older per-second report without labels.)

Recording workflow:

1. `uv run station-fw/tools/dual_record.py` records the station and the reference mic at once and writes
   `brain-rs/clips/dual/<label>_<time>/{station_1.wav, bomge.wav, cues.json}`. `cues.json` is
   `{"cues":[{"kind":"SHOUT|CLAP|SOFT|KNOCK|QUIET|MOVE","t":seconds,"prompt":"NEAR ..."}],"start_offset_sec":N}`.
2. The two files start at slightly different moments. Find the offset from the first 3 claps
   (cues of kind CLAP) and optionally write an aligned copy:
   `uv run station-fw/tools/align_dual.py brain-rs/clips/dual/<rec>/ [--write]`
   (prints `offset` = bomge time minus station time; `--write` creates `bomge_aligned.wav`).
3. Replay from `brain-rs/` (stereo is mixed `(l+r)/2` like `station_manager::on_chunk`):

```bash
cargo run --bin shout_replay -- clips/dual/<rec>/station_1.wav --labels clips/dual/<rec>/cues.json \
    --features-csv /tmp/features.csv
# other options: --config config.yaml  --set shout_margin_db=10 (repeatable)  --offset 0.2  --onset-db 10
# grid search, sorted by hit rate desc, false positives asc, median latency asc:
cargo run --bin shout_replay -- clips/dual/<rec>/station_1.wav --labels clips/dual/<rec>/cues.json \
    --sweep shout_margin_db=8,10,12,15 --sweep shout_min_features=2,3 --sweep vad_hangover_frames=6,10 --top 10
```

Tunables are the `audio:` keys of `config.yaml` (`shout_margin_db`, `vad_hangover_frames`,
`shout_min_features`, `shout_min_duration_sec`, `shout_score_threshold`, `shout_sensitivity`,
`shout_f0_min_hz`, ...), read from `--config` (default `config.yaml`; missing means `Config::default`)
and then `--set`. A cue's audio time is `t + start_offset_sec + --offset`.

Scoring: cues are rough (people react 0.2-0.8 s after the cue; a shout lasts about 1 s). A SHOUT cue is
a HIT when a `ShoutDetected` fires within `[t-0.3, t+2.5]` s. A detection outside every SHOUT window is a
false positive, counted under the kind of the cue window it landed in (CLAP, KNOCK, SOFT, QUIET, MOVE) or
`none`. The per-cue table shows the detection delay from the cue and from the **acoustic onset**: the
first 16 ms frame whose band-passed level is at least `--onset-db` (default 10) above the detector's own
noise floor, searched from `t-0.3` to the detection. `latency = detection - onset` is the responsiveness
number (it includes the frame itself and the 0.2 s `shout_min_duration_sec`). The summary adds the hit
rate overall and per NEAR/MID/FAR block (when the cue `prompt` contains those words).

`--features-csv` writes one row per frame: `time` (frame end), `band_db`, `noise_floor_db`,
`db_above_floor`, `vad_prob`, `gate_open`, `f0_hz`, `pitch_conf`, `centroid_hz`, `tilt_db_per_khz`,
`flatness`, `features_passed`, `shouty`, `score`, `detection`, for plotting against the cues.

## Not done yet

- Combo cues (`combo3/5/10`): brain-rs has no combo notion yet (the simulator counts hits
  within 1 s).
- Round, KO and taunt cues: need the opponent/round logic from the simulator.
- Assets: rendered sfx (shadow-warrior-40f) and voice lines (shadow-warrior-cmp).
