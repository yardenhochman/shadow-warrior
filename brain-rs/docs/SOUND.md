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

## Not done yet

- Combo cues (`combo3/5/10`): brain-rs has no combo notion yet (the simulator counts hits
  within 1 s).
- Round, KO and taunt cues: need the opponent/round logic from the simulator.
- Assets: rendered sfx (shadow-warrior-40f) and voice lines (shadow-warrior-cmp).
