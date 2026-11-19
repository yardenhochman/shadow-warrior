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

## Live hardware (real strip and bag)

The bridge (`simulator/live/bridge.py`, run by `live.py`) serves this page and connects it to
real hardware. It has two modes:

| Mode | Who runs the game | Strip | Bag (BLE) |
| --- | --- | --- | --- |
| **sim** (`live.py start`) | this page's JS port | page frames -> WLED | the bridge |
| **brain** (`live.py brain`) | the real `brain-rs` | brain frames -> WLED, shown in the page | the brain |

Needs [uv](https://docs.astral.sh/uv/) (`curl -LsSf https://astral.sh/uv/install.sh | sh`);
`uv` fetches `aiohttp` and `bleak` itself. Brain mode also needs Rust (`cargo`) the first time,
to build `brain-rs`. Linux (BlueZ) or macOS. `live.py stop | status | log`; PIDs and logs are in
`~/.local/state/shadow-warrior-live/`.

### Sim mode

```sh
simulator/live/live.py start --wled <WLED IP>   # from the repo root; open http://localhost:8090/
```

Stop the brain first: BLE allows one central, and two writers fight over the strip.

- **Strip**: each rendered frame (the same colours as the LED columns panel) goes over a
  WebSocket at ~30 fps. The four columns are laid end to end on the strip (column 1 first,
  each base first; the sim draws 180 LEDs per column by default, like a real column: one 3 m strip at
  60 LEDs/m. A resampler area-averages when the strip length differs; the LED count comes from WLED
  at startup). The real installation is one WLED controller per column (4-5 columns), or controllers
  hosting several strips: see "Layout" below and brain-rs/docs/LEDS.md. Tick **Serpentine** when every other column runs top-down.
  The bridge applies `--brightness 0.7` and `--gamma 2.2` (screen colours to LED duty) and
  sends WLED UDP realtime DNRGB to port 21324 with a 2 s timeout. When the page closes,
  mirroring is switched off or the bridge stops, WLED goes back to its own effect.
  The newest tab drives the strip; other tabs show "another tab is driving it" and a
  **Use this tab** button. Browsers pause hidden tabs, so keep the simulator tab visible.
- **Bag**: the bridge connects to `ShadowWarriorPunchingBag` over BLE (bleak), writes
  `bags.alpha`/`bags.threshold` from `brain-rs/config.yaml` (0.8, 2.0 m/s^2), and keeps fight
  mode (IMU on) while a page is connected. As `ble_manager.rs` does, it folds each burst of
  acceleration notifications into one punch (power = min(100, 10 x m/s^2), 350 ms refractory)
  and the page feeds it to the same `punch()` a click on the arena uses. A punch counts only in FIGHT, as in brain-rs. Without
  Bluetooth or with `--no-ble` the strip still works.
- **Endpoints**: `GET /live/status`; `POST /live/punch?power=60` (same message as a bag punch);
  `GET`/`POST /live/threshold?value=2` reads/sets the bag threshold (RAM only, reset on bag reboot).

### Brain mode

```sh
simulator/live/live.py brain --wled <WLED IP>   # builds/starts brain-rs + bridge; open http://localhost:8090/
```

The bridge stands in for WLED. `live.py brain` starts `brain-rs/target/release/shadow-warrior-brain`
(built with `cargo build --release` if missing) in `brain-rs/` with its own `config.yaml`, except
that `wled.controllers` point at the bridge (`BRAIN_WLED` env override; nothing in the file changes).

- **Strip**: the brain's UDP realtime (FIGHT) arrives at the bridge on 21324, goes on
  **unchanged** to the real WLED and is split back into the four columns on the page (the inverse
  of the sim layout, Serpentine included). The brain's HTTP effects (IDLE, WARMING, COOLDOWN) are
  proxied to WLED, which renders them; the page then shows WLED's own live view. So the LED
  columns show what the strip shows. `--wled none`: no strip, the bridge answers `/json` itself.
- **Game**: state comes from the brain (`/api/state`, polled; `/api/events`, SSE), proxied under
  `/brain/` on the same origin. Presence toggle, SHOUT (one `intensity 1` shout every 350 ms while
  held), Jab/Hook/Heavy, Force absent/present and the **Set state** buttons call
  `/api/arena/{presence,shout,punch,state}`. The JS engine, its shout detector and tunables are
  hidden; the brain uses its own mic, music and config.
- **Bag**: the brain connects it (the bridge holds no BLE in this mode).
- No microphone: `BRAIN_AUDIO='{input=false}' live.py brain ...`. Other `BRAIN_*` overrides pass
  through, e.g. `BRAIN_BLE='{auto_connect=false}'`, `BRAIN_PORT=8001` (brain API port).
- Endpoints: `GET /live/frame?seq=N` (a frame sent to the pages, for tests), `/json/*` (stand-in WLED).

#### Layout: several controllers and strips

The real install is 4-5 columns, each one 3 m strip of 180 LEDs, normally one WLED controller per
column; some controllers host 2-4 strips (their pixel array is the strips concatenated, one UDP
frame covers all). Without `--layout` the bridge keeps the old behaviour (one controller split
into 4 equal columns, Serpentine checkbox). With `--layout` it emulates WLED's LED **output
mapping** (strip order, length, reverse), not WLED segments (realtime UDP ignores segments).

`--layout FILE|JSON` takes the brain-rs `wled:` shape (`brain-rs/docs/LEDS.md`): a YAML/JSON file
(a brain `config.yaml` works, its `wled:` section is used) or inline JSON:

```yaml
columns: 4                          # optional; default = highest column + 1
controllers:
  - {ip: "127.0.0.1", num_leds: 180}   # one strip = column 0 (column = list index)
  - {ip: "127.0.0.2", num_leds: 180}   # column 1
  - ip: "127.0.0.3"                    # two strips on one controller: columns 2 and 3
    strips:                            # consecutive in LED order, num_leds optional (sum)
      - {column: 2, leds: 180}
      - {column: 3, leds: 180, reverse: true}   # LED 0 at the top
```

```sh
simulator/live/live.py brain --wled none --layout simulator/live/layouts/four-columns.yaml
```

`live.py brain --layout` starts the bridge with the layout **and** the brain with matching
`wled.controllers` (`BRAIN_WLED` is generated from the same file). To run against a brain that is
already up, use `live.py start --brain http://127.0.0.1:8000 --wled none --layout FILE`.
`layouts/four-columns.yaml` is one simulated controller hosting four strips: no loopback aliases
needed, so it is the easy macOS setup.

- One UDP listener per controller on `<ip host>:21324` (the brain sends to each controller's ip,
  port 21324, even if the ip has `:httpport`). DRGB, DNRGB (start offsets), DRGBW and WARLS are
  decoded into the controller's global pixel array, cut into its strips, `reverse` flipped, and
  written into the strip's column. The page gets the canonical frame (columns in order, each
  bottom-first, serpentine off); leave **Serpentine** unticked. A column with no data yet, or
  whose controller's realtime timeout has passed (at the next frame from any controller), is dark.
- The page shows exactly four columns: columns >= 4 are not shown, missing ones are dark; a
  layout with more than four columns makes the bridge log a warning at start and list it in
  `layout_warnings` of `/live/status`; a
  strip shorter than the longest is padded dark at the top. All strips are 180 LEDs normally.
- macOS only has `127.0.0.1`; add aliases (`sudo ifconfig lo0 alias 127.0.0.2 up`, same for
  .3 and .4; they vanish at reboot) and use them as controller ips, with the brain's
  `BRAIN_WLED='{controllers=[{ip="127.0.0.1"},...]}'` pointing at the same ips. Linux routes all of
  `127.0.0.0/8` already. Extras ignored by brain-rs, for local testing: `bind: "host:port"` (UDP
  listen address instead of `<ip host>:21324`; for senders you control, the brain always uses
  21324) and `forward: "<host>"`.
- Real strips: only a controller with `forward: "<real WLED host>"` gets its packets forwarded
  (unchanged, to that host's 21324). `--wled` cannot be combined with `--layout`. WLED's own live
  view is not mirrored (idle/cooldown effects are not shown; the page keeps the last frame).
- HTTP: the bridge also listens on each controller's ip host (same port as the bridge) and
  answers `/json/info` with that controller's `num_leds` (the first controller with that host
  when several share one), other hosts get the total. State/effects are the shared stand-in.
  `/live/status` lists controllers (packets, streaming, strips).
- Test: `uv run simulator/live/test_layout.py` (hardware-free: mapping unit tests plus UDP ->
  `/live/frame` for four single-strip controllers and one two-strip controller with `reverse`).

#### At home (Pi or laptop)

Same machine for brain and bridge (Pi or laptop): run the brain-mode command above there with
your WLED's IP, then open `http://<that machine>:8090/` from a laptop or phone (add `--host 0.0.0.0`).

Brain already running elsewhere (e.g. the Pi's systemd service):

1. Bridge, on any machine in the LAN: `simulator/live/live.py start --host 0.0.0.0 --wled <WLED IP> --brain http://<pi>:8000`.
2. Brain: point its WLED at the bridge, without editing `config.yaml`, with the environment
   variable `BRAIN_WLED='{controllers=[{ip="<bridge host>:8090",num_leds=180}]}'`
   (systemd: `systemctl edit shadow-brain`, `Environment=...`), or set
   `wled.controllers: [{ip: "<bridge host>:8090", num_leds: 180}]`. Restart the brain.
   It sends realtime to `<bridge host>:21324`, so UDP 21324 must reach the bridge.
3. Open `http://<bridge host>:8090/`.

Back to the real WLED: remove `BRAIN_WLED` (or restore `wled.controllers` to the WLED IP) and
restart the brain; stop the bridge with `live.py stop`.

### From GitHub Pages: Connect to device

An https page can't call a plain-http device on your LAN (browsers block mixed content and
private-network requests), so the Pages build doesn't try. **Connect to device** (top bar) opens
`http://<address>:<port>/`, the simulator served by the bridge on that machine, in its sim or
brain mode. Defaults: `shadow-warrior.local`, port 8090; the last address is remembered, and
`?device=<host[:port]>` pre-fills it (deep link).

## Controls

| Action | Mouse / touch | Keys |
| --- | --- | --- |
| Someone enters/leaves the arena | "Someone in arena" toggle | `P` |
| Shout | hold SHOUT (a tap = 0.45 s shout) | hold `S` |
| Punch | tap/click the arena; hold to charge (30 to 100 power over 0.6 s) | `J` 25, `K` 55, `L` 90, hold `Space` to charge |
| Real microphone | "Enable mic" | |

Hits within 1 s chain into a combo (+10% damage per hit, up to x2). Knocking the
shadow out plays the victory finale, then a tougher shadow rises for the next round (see
"Victory and rounds" below); set "knocking out the shadow ends the fight" to end the fight
after the finale instead.

Panels: the LED columns with the effect preset, a **Now playing** readout and the
**Effects** section (below); arena state, timers and energy; the shout detector meter (band level, noise
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
| `js/wled.js` | `src/services/wled_manager.rs`, `config.yaml wled.effects` | "brain-rs" mode: approximations of the OLD brain-rs looks (WLED Breathe/Ocean, Rainbow, Fire 2012/Lava, the red fight pulse). brain-rs has since moved: idle is Fade/ember, warming and fight are rendered by `src/led/` (see `brain-rs/docs/LEDS.md`); in brain mode the page shows the real frames. "Sim proposal" mode is new |
| `js/config.js` | `config.yaml`, `src/config.rs` defaults | Faithful defaults; export uses config.yaml key names and sections |
| `js/scene.js`, `js/sfx.js`, `js/hitfx.js`, `js/presets.js`, `js/feature_source.js`, `js/main.js` | - | Simulator only |

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
- **Sim proposal LED effects**: IDLE violet breathing with a rolling wave; WARMING an
  energy bar in four stages (0-25 % calm deep-blue bar with a breathing glow above it,
  25-50 % violet with sparks rising past the fill line, 50-75 % magenta-red with a
  crackling fill line and a band chasing round the columns, 75-100 % red to orange to a
  near-white top with flicker, strobe hints and lightning arcs between columns), each
  shout a kick up the bar sized by its intensity; FIGHT fire that grows with punching
  (candle when idle) and a red warning in the last 10 s; COOLDOWN embers that cool to
  calm blue. Every proposal frame is capped at a mean LED level of 0.72 (`FRAME_CAP`),
  so the real strip never gets a white wall.
- **Punch signatures** (`js/hitfx.js`): each punch gets a named effect for its tier,
  never the same one twice in a row, shared by the LEDs, the arena and the sound.
  Jabs (< 45): a small spark, flick or twin dot in a rotating colour on one column.
  Hooks (45-74): a band sweeping across the columns in the hook's direction, a coloured
  ring, or a two-beat pop. Heavies (75+): THUNDER (blue-violet bolts), GOLD RUSH (spark
  fountain), SHATTER (crimson flash breaking into falling shards), STROBE (colour pops),
  SPLIT (green fronts from the inner columns outward), SUPERNOVA (blackout beat, then a
  rainbow-fringed explosion), each with a hit-stop, rings, tint and its own sound layer.
  Deep in a combo the big ones come more often and leave their colour on the column tops.
  Tunables `hit_fx_variety` (0 = the old white flash) and `hit_fx_intensity`.
- **Shouts in FIGHT**: visual only (brain-rs ignores them): a teal sound-wave ring on
  the columns and on screen. Teal is kept out of the punch colours.
- **Effect presets** (`js/presets.js`, the dropdown above the LED columns): a preset gives every
  stage its look. Stages: idle/attract, WARMING bar (4 stages), fight start, FIGHT ambient
  (grows with rage), jab, hook and heavy hits, the FIGHT shout ripple and the victory. Presets:
  - *brain-rs effects*: what brain-rs used to ask WLED for (Breathe/Ocean, Rainbow, the red realtime
    pulse; superseded, see LEDS.md); no fight-start, hit, shout or victory effects, as on the device.
  - *Sim proposal effects*: the original proposal (violet idle, 4-stage bar, fire, the six heavy
    signatures) plus its victory.
  - *Inferno*: lava-lamp idle, a heat gauge that gets embers, flames over the line and a white-hot
    shimmer, lava that rises with rage; hits ERUPTION, MELTDOWN, FIRESTORM.
  - *Ice / ghost*: ghost wisps and frost twinkle, a jagged frost line that crawls up with crystal
    spikes, ice that shatters at the fight start; hits AVALANCHE, GLACIER, SPECTRE.
  - *Storm*: drizzle with distant sheet lightning, a bar of churning cloud with rain falling into it
    and bolts at the line, a downpour with flashes that grows with rage; hits LIGHTNING,
    THUNDERCLAP, HAIL.
  - *Neon dojo*: thin neon outlines on black, a tick-marked meter with chasers, more and faster
    chasers with rage; hits KATANA, OVERDRIVE, NEON BURST.
  - *Retro arcade*: chunky blocks (about 15 strip LEDs each) with gaps, a power-up meter whose colour
    steps green to red per stage with a level-up blink, equalizer bars; hits K.O. FLASH, PIXEL
    BLAST, FIREBALL.

  Under **Effects: mix stages, preview, fine-tune** each stage can take its preset from another one
  (e.g. WARMING from Inferno, hits from Storm), has **preview** buttons (each WARMING stage, every
  jab/hook/heavy kind, opening, ambient, shout ripple, victory) that fire the effect on the LEDs at
  once whatever the arena is doing, and a **Fine-tune** block: Speed, Size, Intensity and Colour
  shift per stage, with **More…** for details. Choices persist in localStorage
  (`sw-sim-effects-v1`, only overrides and changed values). **Now playing** under the columns names
  every effect on the LEDs (stage, preset, effect, power/hue/escalation, time left) and the frame
  level; **Last 8 effects** lists what fired. Every frame stays under the 0.72 cap, and punch
  colours never use teal/aqua (150-200 deg), also after a colour shift: that hue is for shouts.
- **Screen overlays** (Effects section, default *Minimal*): the LED columns are the main show, so
  the arena draws no rings, bolts, colour washes or blackouts over the bag and the columns. A shout
  in FIGHT shows as a teal pulse on the sound strip along the bottom edge (which also shows the mic
  level); heavy names sit on the floor under the bag, banners on the floor below the columns.
  *Full* brings back the rings and washes of the punch signatures.
- **Victory and rounds**: a knock-out plays a 6.5 s finale on one clock (`VICTORY` in `hitfx.js`).
  LEDs: the shadow's crimson floods the columns and shatters into falling shards, the winner's
  colours erupt from the floor and build to a climax at 2.4 s, then a calm golden afterglow fades
  out. Arena: the shadow crumbles into embers, SHADOW DEFEATED above the columns and the round's
  stats (time, punches, max combo, hardest hit) on the floor (one line above the title on phones).
  Sound: a synthesised shatter, a climbing arpeggio into a big major chord with timpani, cymbal
  and cheer, then chimes (peak about -4 dBFS). The fight clock pauses during the finale. Then a
  new shadow rises for round 2, 3, ... with `round_hp_growth` more health each round (default 0.3:
  900, 1170, 1521 HP) and, from round 2, heals `round_regen_per_sec` x (round - 1) HP/s whenever no
  punch landed for 1 s (default 4). The HP bar shows the round.
- **Fight start**: the full bar opens the arena: the columns suck in, then explode into
  the fire 0.35 s later, with a screen ring and a synthesised riser, sub boom, gong, war
  drums and crowd swell on the same beat.

### brain-rs bugs found while porting (sim fixes are on by default)

Turn off the matching toggles under "brain-rs fidelity" to reproduce brain-rs as written.

1. *(fixed in brain-rs: FIGHT entry resets the activity clock)* **Fight ends within ~1 s (inactivity)**: `last_activity_at` is set only at construction
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
3. *(fixed in brain-rs: ids read from the controller and pinned by tests to WLED 0.15.4; cooldown is "Fire 2012"/"Lava")* **The cooldown "fire" effect is the wrong effect**: WLED has no effect named "fire" ("Fire 2012"
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
5. *(energy bar fixed: WARMING is now UDP realtime, `led::scenes::warming`, driven by energy and shout events; the missing timeout remains)* **WARMING has no timeout or feedback**: the config uses HTTP `rainbow` for WARMING
   (`config.yaml wled.effects.warming`), so the "energy bar" from AGENTS.md is never
   shown. `generate_warming_frame` (`wled_manager.rs:458`) says "speeds up with energy"
   but ignores energy and runs only in realtime mode. WARMING also has no timeout: it
   lasts until presence is lost.
6. **Music may keep playing**: music stops only on `SessionEnded` (COOLDOWN entry,
   `main.rs:234`). A fight that ends via FIGHT→SUSPENDED/IDLE leaves it playing and
   `playback_active` raised.
7. Known and not replicated: punch detection counting gravity, and the WARLS UDP frame
   format (`generate_*_frame` omit the per-LED index; protocol 1 is limited to 255 LEDs).
8. *(fixed: a key an effect leaves out no longer inherits the default's; `Config::default` idle is now Fade/ember too)* **IDLE runs on the Ocean palette** (found in brain mode): `config.yaml` sets `wled.effects.idle`
   to Fade with ember-red colours and no `palette`, but the YAML is merged over
   `Config::default()`, whose idle effect has `palette: "ocean"`, so the key survives and the brain
   posts `pal: 9`: the strip fades in blue/teal. Same for any effect that leaves out a key the
   default sets. Workaround: give every effect an explicit `palette` (e.g. `"Default"`).

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
the file header; it exits 1 on any problem. `test/live.mjs` checks live mode against a
running bridge: frame rate, sent frames equal to the rendered LEDs, the resampler, a bridge
punch counted in FIGHT and, with `WLED_HOST` set, WLED in realtime mode. `test/brain.mjs`
drives a running brain through the page in brain mode (presence and SHOUT to FIGHT, LED columns
equal to the brain's UDP frames, a page punch counted by the brain, state buttons).
`test/connect.mjs` checks the static build offline and the Connect to device navigation.
`e2e.mjs` also covers preset switching, a per-stage override (kept across a reload), preview
buttons, the now-playing readout and KO -> victory -> round 2. `test/effects.mjs` fires every
preset x stage, measures the frame mean level (fails over 0.72), checks that no punch hue is teal
and that the victory sound peaks below 0 dBFS, and writes contact sheets (LEDs per preset and
stage, the victory on the LEDs and in the arena, overlays minimal vs full, the effects panel at
desktop and phone width) and `levels.md` to `SHOTS`.
