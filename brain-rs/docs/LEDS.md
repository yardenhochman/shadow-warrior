# LED design

Two ways to drive a WLED strip, one per arena state.

| State | Mode | Who animates | Look |
|-------|------|--------------|------|
| Suspended | off | - | strip off |
| Idle | **built-in WLED effect** (HTTP) | the controller | ember fading to black with a short rest, ~5 s cycle (Tri Fade, see below) |
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

WLED effect **`Tri Fade`** (id 56 on 0.15.4), speed 96, brightness 44, colours
`[[255,25,0],[0,0,0],[0,0,0]]` (config `wled.effects.idle`). It fades colour 1 -> 2 -> 3 -> 1, a third
of the cycle each: ember -> black, a dark rest, black -> ember. Cycle
`65.536 s / ((speed >> 3) + 1)` (0.15.4 `mode_tricolor_fade`): 5.04 s at speed 96 to 103.
Chosen on the real strip on 2026-11-19, after comparing Fade (a triangle) and Sine (a smoothed
triangle). No built-in effect is a pure sine in time; revisit later (a brain-rendered idle
could be, and could layer two effects, but then the brain must be up).

**Brightness 44 is deliberate.** The controller's power limit (ABL, `maxpwr` 1000 mA) scales
the whole frame down when the estimate is higher. Full ember on 180 LEDs at brightness 180 would
be about 2.8 A, so the top of the wave was clipped flat for ~75% of the cycle (measured through
`/json/info` `leds.pwr`). At 44 the wave stays under the limit (120-690 mA). A bigger idle needs
a bigger PSU and a higher `maxpwr` (the PSU rating is unknown). Warming and fight frames are
clipped the same way.

Earlier versions of this doc said Fade was an 11 s cycle (wrong: pre-0.15 formula); on 0.15.4
Fade is `65.536 s / ((speed >> 3) + 10)`, 4.4 s at speed 40, 5.04 s at speed 24.

Preview without hardware:

```
uv run deploy/render_idle.py out.mp4 --sx 24 --bri 180     # simulates Fade (not Tri Fade); not realtime
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

Controllers found at runtime by discovery (mDNS) are added with `auto_layout`: one 180-LED strip
(until the probe says otherwise) on the lowest
column no strip drives yet (never a column already assigned). A discovered address that is already
configured is ignored; when `columns` is set and all of them are taken it is logged and not added.
Without `columns` the arena grows by one column. Configure controllers explicitly in
`config.yaml` for a real install; discovery is a convenience.

The browser simulator's live bridge can show this layout (one UDP listener per controller, strips
cut into columns): see `simulator/web/README.md`, "Layout: several controllers and strips".

### Layout probe (`GET /json/cfg`)

Once per connect (the first successful health poll, and again after the controller was
declared disconnected and came back) the brain reads `GET /json/cfg` -> `hw.led.ins[]`, one
entry per LED output. Never polled: an ESP32-C3 rebooted under 4 Hz GET polling. A refused or
timed-out probe is retried on the next health poll (30 s); an answer that cannot be used
(404, not JSON, no `ins`, outputs that do not follow each other from LED 0) is logged and the
`/json/info` `leds.count` path below applies until the next reconnect.

Real WLED 0.15.4 (ESP32-C3), `hw.led` with everything but the layout trimmed (fixture:
`src/services/wled_manager/fixtures/cfg_0_15_4_one_strip.json`):

```json
{"total":180, "maxpwr":1000, "ins":[{"start":0,"len":180,"pin":[10],"order":0,"rev":false,
  "skip":0,"type":22,"ref":false,"rgbwm":0,"freq":0,"maxpwr":1000,"ledma":55}]}
```

What the probe does with it:

- `auto_layout: true` controllers (all found by discovery or the database; opt-in in config
  with just `{ip: ..., auto_layout: true}`, `num_leds` optional): **one strip per output**,
  lengths from `len`. The first keeps the controller's column, further outputs take the lowest
  free columns (limited by `columns`; an output with no free column is logged and not drawn).
  `reverse` per position stays what the config says (false for new ones).
- Controllers with an explicit layout: the **config wins**. If the probe disagrees (different
  number of outputs or lengths) a warning names both and nothing changes; a multi-strip config is
  never silently overridden. One exception: one configured strip on one output takes the
  output's length (like the `/json/info` count does).
- Column is not probeable. It comes from `strips[].column` / the list order (pinned at load, see
  below) / the next free column for discovered ones. A naming convention (WLED name `col2`) is
  not implemented.
- the system status API (`wled[].outputs`: `start`, `len`, `rev`, `order`) and the log show what was
  probed. Outputs carrying `rev: true` are logged as "(WLED rev)".

Columns of controllers without `strips` are resolved once at load (the list index becomes an
explicit one-strip layout), so `remove_controller` cannot shift the later ones.

LED-count adoption (`/json/info` `leds.count`, every 30 s poll) resizes a single-strip
controller; for a multi-strip controller a different count only logs a warning and the
configured layout stays.

### OPEN: does WLED's per-output `rev` apply to realtime UDP frames?

Unverified, deliberately not guessed in code. WLED's per-output "Reverse" (LED preferences)
flips the output in the bus layer, so it may or may not touch DRGB/DNRGB realtime data. The
brain's own `reverse` (strip config) is the only flip it applies. Until this is checked on a
real controller **leave WLED's Reverse off** on every output and use `reverse: true` in
`config.yaml`; if the probe reports `rev: true` the brain only reports it. To settle it: set
Reverse on one output, send a DRGB frame with only LED 0 lit and look which end lights (bd
issue filed for this).

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
  Verified by tests, the filmstrip and the browser simulator (brain mode); not yet seen on a strip.
- **Fight is wired**: `Scene::Fight` in `wled_manager` renders one `FightScene` per strip,
  fed by the same event drain as warming plus `PunchDetected { power }` (0..100). Leaving
  FIGHT: the next state's effect is an HTTP effect, which stops the realtime task (cooldown's
  built-in Fire takes over). Verified by unit tests, a UDP integration test
  (`fight_punch_pulse_reaches_the_inner_strip_first`), the filmstrip, and by eye in the browser
  simulator (brain mode, 4 columns x 180 LEDs, `simulator/live/live.py brain --layout ...`);
  not yet seen on a strip. Not done: music layer.
- Controller power limit (`maxpwr` 1000 mA) is unchanged until the strip's PSU rating is known.
