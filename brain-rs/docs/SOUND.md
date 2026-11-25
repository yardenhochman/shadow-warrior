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

## Not done yet

- Combo cues (`combo3/5/10`): brain-rs has no combo notion yet (the simulator counts hits
  within 1 s).
- Round, KO and taunt cues: need the opponent/round logic from the simulator.
- Assets: rendered sfx (shadow-warrior-40f) and voice lines (shadow-warrior-cmp).
