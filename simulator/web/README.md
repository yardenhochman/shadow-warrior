# Shadow Warrior: browser arena simulator

Play and tune the installation with no hardware: shout into your mic to wake the
shadow, fight it on screen, and watch the four LED columns react. Game logic is a
JavaScript port of `brain-rs`, so tuning here carries over to `brain-rs/config.yaml`.

## Run

Static files only (no build step, no CDN, no dependencies).

```sh
cd simulator/web
python3 -m http.server 8000
# open http://localhost:8000
```

The mic needs a secure context: `localhost` or https (GitHub Pages works as-is).
Without a mic, the SHOUT button feeds synthetic frames through the same detector.

## Controls

| Action | Mouse / touch | Keys |
| --- | --- | --- |
| Someone enters/leaves the arena | "Someone in arena" toggle | `P` |
| Shout | hold SHOUT (a tap = 0.45 s shout) | hold `S` |
| Punch | tap/click the arena; hold to charge (30 to 100 power over 0.6 s) | `J` 25, `K` 55, `L` 90, hold `Space` to charge |
| Real microphone | "Enable mic" | |

Hits within 1 s chain into a combo (+10% damage per hit, up to x2). Knocking the
shadow out makes it rise again with 25% more HP; set "knocking out the shadow ends the
fight" to end the fight on a KO instead.

Panels: arena state, timers and energy; the shout detector meter (band level, noise
floor, floor + margin, VAD, the four feature checks, score against threshold, sustained
time); tunables with **Export YAML**; and an event log. `time_scale` fast-forwards
the game clock so long fights and cooldowns go by quicker. The mic always runs in real time.

## How it maps to brain-rs

| Sim file | brain-rs source | Fidelity |
| --- | --- | --- |
| `js/state_machine.js` | `src/core/state_machine.rs` | Faithful: states, allowed transitions, hook order |
| `js/arena_manager.js` | `src/services/arena_manager.rs`, `EnergyBar` in `audio_manager.rs` | Faithful: presence/shout/punch/energy-full handling, 100 ms decay tick, 1 s fight monitor with `num_seconds()` truncation, cooldown sleep task. Startup goes to IDLE as in `main.rs` |
| `js/audio_frontend.js` | `src/services/audio_frontend.rs` | Faithful: 4th-order band-pass 250 Hz-4 kHz, linear resampler to 16 kHz, 256-sample frames, asymmetric noise floor, FFT centroid/tilt/flatness, autocorrelation pitch. **VAD is a proxy** (see below) |
| `js/shout_detector.js` | `src/services/shout_detector.rs` | Faithful: VAD gate with hangover, margin (+ playback margin), 4 feature checks, score, gap tolerance, min duration, intensity |
| `js/presence.js` | `src/services/presence.rs` | Faithful: `MicSource`, `PunchSource`, `PresenceFusion` (leaky accumulator, enter threshold, leave timeout, force) |
| `js/wled.js` | `src/services/wled_manager.rs`, `config.yaml wled.effects` | "brain-rs" mode: approximations of WLED Breathe/Ocean, Rainbow, Fire 2012/Lava, plus the ported `generate_fight_frame` red pulse. "Sim proposal" mode is new |
| `js/config.js` | `config.yaml`, `src/config.rs` defaults | Faithful defaults; export uses config.yaml key names and sections |
| `js/scene.js`, `js/sfx.js`, `js/feature_source.js`, `js/main.js` | - | Simulator only |

Music: as in `main.rs`, `FightStarted` starts music and sets `playback_active` (which
raises the shout and presence margins), and `SessionEnded` stops it. The sim plays a
synthesised drum loop in its place.

### Proposals (not in brain-rs)

- **Presence toggle**: a simulated presence sensor that adds 1 evidence-second per
  second (source `sim_sensor`, default weight). Mic and punches also count, as in
  brain-rs. After switching it off, the normal `leave_timeout_sec` applies. Force
  absent/present call `PresenceFusion::force` (the API override).
- **VAD proxy**: earshot is a neural VAD that can't run in the browser. The sim uses
  `vadProxy()` (pitch confidence x level above floor x low flatness). Expect a
  different VAD gate than on the Pi.
- **Opponent**: health, damage (`power x damage_per_power x combo`), KO and respawn.
  Punches are power 0-100 events, without the IMU path.
- **Sim proposal LED effects**: IDLE violet breathing with a rolling wave, WARMING
  energy-bar fill that heats from blue to white with a flash on each shout, FIGHT
  fire plus a shockwave per punch (full flash for >75 power) and a red warning in the
  last 10 s, COOLDOWN embers that cool to calm blue.

### brain-rs bugs found while porting (sim fixes are on by default)

Turn off the matching toggles under "brain-rs fidelity" to reproduce brain-rs as written.

1. **Fight ends within ~1 s (inactivity)**: `last_activity_at` is set only at construction
   and on punches (`arena_manager.rs:55`, `:224`). It is not reset on entering FIGHT
   (`on_enter` Fight, `arena_manager.rs:472-480`). If no punch landed in the last 30 s
   before the fight, the 1 s monitor (`:288-302`) ends the fight right away with
   `InactivityTimeout`. Fix toggle: "reset inactivity timer when FIGHT starts".
2. **WARMING is unreachable after the first presence loss**: `PresenceDetected` fires only
   when the fused state changes (`presence.rs:127-172`). `handle_presence` (`arena_manager.rs:180-188`)
   goes SUSPENDED→IDLE on one event and needs a second `true` event for IDLE→WARMING. That
   second event never arrives while the player stays. A player still present after
   COOLDOWN→IDLE is also stuck. Once a player leaves (→SUSPENDED), every later player
   reaches only IDLE. Fix toggle: "re-arm WARMING when presence persists".
3. **The cooldown "fire" effect is the wrong effect**: WLED has no effect named "fire" ("Fire 2012"
   is id 66), so `resolve_effect_id` falls back to the static table, where `"fire" => 16`
   is "Saw" (`wled_manager/protocol.rs:20`). The same table has `"rainbow" => 8`
   (Colorloop, not Rainbow 9), `"scan" => 5` and `"twinkle" => 15`, and duplicates in
   `add_controller` (`wled_manager.rs:181-184`). The static palette ids
   (`protocol.rs:25-36`) don't match WLED's list either (Ocean is 9, Lava 8,
   Rainbow 11). The sim renders the intended fire/lava effect.
4. **Unreachable pitch range**: `pitch_autocorr` searches only 70-500 Hz
   (`audio_frontend.rs:303-304`), so `shout_f0_max_hz: 700` above 500 has no
   effect. High shouts (500-700 Hz, common for screaming) may count as unvoiced or as
   their sub-harmonic.
5. **WARMING has no timeout or feedback**: the config uses HTTP `rainbow` for WARMING
   (`config.yaml wled.effects.warming`), so the "energy bar" from AGENTS.md is never
   shown. `generate_warming_frame` (`wled_manager.rs:458`) says "speeds up with energy"
   but ignores energy and runs only in realtime mode. WARMING also has no timeout: it
   lasts until presence is lost.
6. **Music may keep playing**: music stops only on `SessionEnded` (COOLDOWN entry,
   `main.rs:234`). A fight that ends via FIGHT→SUSPENDED/IDLE leaves it playing and
   `playback_active` raised.
7. Known and not replicated: punch detection counting gravity, and the WARLS UDP frame
   format (`generate_*_frame` omit the per-LED index; protocol 1 is limited to 255 LEDs).

## Tuning workflow

Move sliders (changed keys turn yellow; values persist in localStorage), play, then
**Export YAML** and merge the printed keys into `brain-rs/config.yaml`. Keys under
"simulator only" are never exported.

## Fight music

The FIGHT state plays the installation's playlist (`music/playlist.tsv`: Doom,
Naruto, Pantera, Metallica, RATM, Mortal Kombat…). Audio files are not committed.

- **Locally:** run `music/fetch.sh` once (needs `yt-dlp` and `ffmpeg`), then
  serve the **repo root** (`python3 -m http.server` in the repo) and open
  `http://localhost:8000/simulator/web/`. The sim finds `../../music/` by itself.
- **GitHub Pages / phone:** tap **Load music** and pick the MP3s (or any audio files).
  The browser keeps them (IndexedDB), so after one load the playlist is back on
  every visit, offline too. Loading a new set replaces the saved one.
- With no tracks it falls back to the synth drum loop. ⏭ skips a track; each
  fight starts the next track (a sim proposal; brain-rs restarts from track 1).
- The same folder works for brain-rs: see [music/README.md](../../music/README.md).

## Test

`test/e2e.mjs` drives the whole loop headless (fake mic, shout to FIGHT, music,
punches, COOLDOWN, re-arm, YAML export) at desktop and phone sizes. Usage is in
the file header; it exits 1 on any problem.
