# LED design

Two ways to drive a WLED strip, one per arena state.

| State | Mode | Who animates | Look |
|-------|------|--------------|------|
| Suspended | off | - | strip off |
| Idle | **built-in WLED effect** (HTTP) | the controller | ember red fading to black, ~11 s cycle |
| Warming | **UDP realtime** | the brain, 30 fps | energy bar (persistent) + shout glow (live) |
| Fight | **UDP realtime** | the brain, 30 fps | punch pulses, shout effects, music-driven effects, optional energy bar |
| Cooldown | **built-in WLED effect** (HTTP) | the controller | "Fire 2012", Lava palette |

**Rule:** only idle and cooldown use built-in effects. Warming and fight are always realtime.

## Why

- **Idle and cooldown** are slow ambient looks that do not react to anything. One HTTP POST on
  the state change and the controller animates by itself, so the brain sends nothing and the
  look survives a brain restart or Wi-Fi hiccup. Effects, palettes and presets can be changed
  without reflashing WLED.
- **Warming and fight** react to live data (shout level, energy, punches, music). They need
  several things drawn on top of each other and changed every frame. WLED segments cannot blend,
  fade a glow around a moving edge, or overlay one-shot punch pulses, and changing segment sizes
  by HTTP many times a second is too much for an ESP32-C3 (one rebooted under 4 Hz GET polling).
  UDP realtime costs the controller almost nothing.

## Idle: the breathing look

WLED effect `Fade`, speed 40, brightness 180, colours `[[255,25,0],[0,0,0]]` (config
`wled.effects.idle`). It is a triangle wave between the two colours, cycle
`65.5 s / (1 + speed/8)` = 10.9 s. Chosen on the real strip on 2026-11-17 ("E3").

Preview without hardware:

```
uv run deploy/render_idle.py out.mp4 --sx 40 --bri 180     # simulates Fade; not realtime
```

The effect ids and palette ids differ between WLED builds. The brain resolves names against the
controller (`GET /json/eff`, `/json/pal`) and falls back to a table pinned to WLED 0.15.4
(`wled_manager/protocol.rs`). Prefer exact device names in the config.

## Layout: columns, strips, controllers

The arena has 4-5 columns around the punching bag, which hangs in the **centre**. Each column
is one 3 m strip, 60 LEDs/m = 180 LEDs. Normally one WLED controller drives one column.
A controller may host 2-4 strips; its LED range is then the strips concatenated in order
(`num_leds` = 180 x strips) and ONE UDP DRGB/DNRGB frame covers all of them.

```yaml
wled:
  columns: 4                      # optional; default = highest column + 1
  controllers:
    - {ip: "10.0.0.55", num_leds: 180}          # one strip, column = list index (0)
    - ip: "10.0.0.56"                           # two strips, columns 1 and 2
      strips:                                   # consecutive in LED order; num_leds optional (sum)
        - {column: 1, leds: 180}
        - {column: 2, leds: 180, reverse: true} # wired top-down (LED 0 at the top)
```

Validation at load: the strips' LEDs must sum to `num_leds` if given, no column twice, no
column >= `columns`. The brain renders **one compositor per strip** and concatenates the
strips (flipping `reverse` ones) into the controller's frame. Scenes get the strip's
`led::ColumnLayout` (`column`, `columns_total`, `bag_distance`, `leds`, `reverse`).
`bag_distance` is 0 at the bag and 1 at the outermost column (5 columns: 1, .5, 0, .5, 1;
4 columns: 1, 1/3, 1/3, 1), so punch shockwaves can travel inner columns first. Warming is the
same on every column for now.

The browser simulator's live bridge can show this layout (one UDP listener per controller, strips
cut into columns): see `simulator/web/README.md`, "Layout: several controllers and strips".

LED-count adoption (`/json/info` `leds.count`) resizes a single-strip controller; for a
multi-strip controller a different count only logs a warning and the configured layout stays.

## Warming and fight: the `led` module (`src/led/`)

A small compositor renders into one RGB buffer per controller and the realtime loop sends it
as UDP DRGB (port 21324). No dependency on a rendering crate; ideas come from
`smart-led-effects-rs`, but long-lived effect iterators do not suit stacking several dynamic
overlays.

- `Layer` trait: `render(ctx, out)` draws into a buffer; `done()` lets one-shots end.
- `Compositor`: stacks layers with blend modes (`Normal`, `Add`, `Screen`, `Max`), a master
  brightness, and a capped list of one-shot layers (punch flashes). The buffers are reused, so
  memory and CPU stay bounded however many punches land.
- `Inputs` (filled every frame from the event bus): `energy` (0..1), `shout` (live level),
  `punches` (power each).
- Time is real `dt`, not frame count.

Scenes:

- **Warming:** `EnergyBar` (persistent: rises fast, falls slowly, anti-aliased edge, bright head,
  white flash while full) + `ShoutGlow` (added around the bar head, wider and brighter with a
  louder shout, nothing in silence).
- **Fight** (`scenes::fight(len, layout)` -> `FightScene`, one per strip; all numbers in
  `FightStyle` = `EmberStyle` + `PunchStyle` + `ShoutRippleStyle` + `EnergyBarStyle`):
  - `Ember` (persistent): dark-red/orange flicker scrolling up from the bottom of the column,
    peak brightness 0.30 at rest, 0.48 at full `rage` (decaying average of punch power), reaching
    38% / 62% of the strip. Kept dim on purpose so punches pop.
  - `PunchPulse` (one-shot per punch, `Blend::Add`, max 8 alive per strip, oldest dropped): a
    shockwave from the bag height (`bag_height` 0.45 of the column) up and down, plus a short
    bright core at the bag. Starts `bag_distance x 80 ms` late (inner columns first) and is up to
    35% dimmer on the outermost column. Tiers by power (0..100): jab < 45 small amber; hook < 75
    larger orange-white; heavy >= 75 large white-hot with a ~90 ms whole-strip flash.
  - `ShoutRipple` (persistent, `Blend::Add`): teal (hue ~175) glow around the bag height with
    ripples running outwards, wider and brighter with the live shout level. Teal is reserved for
    shouts; every punch colour stays out of 150-200 degrees (tested).
  - Optional thin `EnergyBar` under the fire: `FightStyle.energy_bar` (default off).
  - Later: a music layer (needs a level/beat tap on playback).
  - Punches reach every strip once: `LiveInputs::on_punch` gives each `Punch` a monotonically
    increasing `id`; each `FightScene` spawns only ids above its own cursor, so the shared
    `Inputs` can feed all strips. A strip whose LED count (or bag distance) changes gets a new
    scene (in-flight pulses are dropped).

Preview a scene as a filmstrip image (time down, LEDs across):

```
LED_PREVIEW_OUT=<dir> cargo test led::scenes -- --ignored
# -> warming.ppm, fight_columns.ppm (5 columns side by side), fight_strip.ppm (centre column)
```

All colours and timings live in style structs (`EnergyBarStyle`, `ShoutGlowStyle`, `EmberStyle`,
`PunchStyle`/`TierStyle`, `ShoutRippleStyle`, `FightStyle`).

## Status

- Built and tested: compositor, `EnergyBar`, `ShoutGlow`, `LiveInputs` (event smoothing).
- **Warming is wired**: `wled_manager`'s realtime loop renders `led::scenes::warming(len)` per
  strip (rebuilt when the device reports a different LED count) at `udp_fps`, using real
  `dt`. Inputs come from the event bus: `EnergyChanged` -> `energy / max_energy` clamped to 0..1;
  `ShoutDetected` -> `shout = max(shout, intensity)`, decaying with tau 0.35 s per frame.
  Frames are sent every tick even when nothing changes (WLED realtime timeout is 2 s).
  Verified only by tests and the filmstrip preview; not yet seen on a strip.
- **Fight is wired**: `Scene::Fight` in `wled_manager` renders one `FightScene` per strip,
  fed by the same event drain as warming plus `PunchDetected { power }` (0..100). Leaving
  FIGHT: the next state's effect is an HTTP effect, which stops the realtime task (cooldown's
  built-in Fire takes over). Verified by unit tests, a UDP integration test
  (`fight_punch_pulse_reaches_the_inner_strip_first`) and the filmstrip; not yet seen on a
  strip or in the simulator. Not done: music layer.
- Controller power limit (`maxpwr` 1000 mA) is unchanged until the strip's PSU rating is known.
