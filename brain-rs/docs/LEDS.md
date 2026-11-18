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
- **Fight (planned):** ember/fire base, punch pulse one-shots scaled by power, shout overlay,
  optional energy bar, later a music layer (needs a level/beat tap on playback).

Preview a scene as a filmstrip image (time down, LEDs across):

```
LED_PREVIEW_OUT=<dir> cargo test led::scenes -- --ignored
```

All colours and timings live in style structs (`EnergyBarStyle`, `ShoutGlowStyle`).

## Status

- Built and tested: compositor, `EnergyBar`, `ShoutGlow`, `LiveInputs` (event smoothing).
- **Warming is wired**: `wled_manager`'s realtime loop renders `led::scenes::warming(len)` per
  controller (rebuilt when the device reports a different LED count) at `udp_fps`, using real
  `dt`. Inputs come from the event bus: `EnergyChanged` -> `energy / max_energy` clamped to 0..1;
  `ShoutDetected` -> `shout = max(shout, intensity)`, decaying with tau 0.35 s per frame.
  Frames are sent every tick even when nothing changes (WLED realtime timeout is 2 s).
  Verified only by tests and the filmstrip preview; not yet seen on a strip.
- Not yet wired: fight still draws the old pulsing red (`Scene::Fight` in `wled_manager.rs` is
  the seam for the new scene). Tracked in beads (`shadow-warrior-19b`).
- Controller power limit (`maxpwr` 1000 mA) is unchanged until the strip's PSU rating is known.
