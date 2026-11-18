//! Layered LED renderer for the arena strips (WLED realtime scenes).
//!
//! A scene is a stack of [`Layer`]s composited at a fixed frame rate into one RGB frame:
//!
//! * **persistent layers** (`Compositor::push`) live as long as the scene: a base texture,
//!   the energy bar, a live-shout glow...
//! * **one-shot layers** (`Compositor::spawn`) are created by an event (a punch) and removed
//!   when they report `done()`. Their number is capped, so memory and CPU stay bounded.
//!
//! Layers are pure functions of `(dt, Inputs)` and draw into one reused buffer, so rendering
//! does no per-frame allocation and tests are exact. Colours are display-space `f32` (what the
//! eye should see); WLED applies its own gamma. Idle and cooldown stay WLED built-in effects;
//! this module is for the states that react live (warming, fight).
//!
//! Warming is wired into `wled_manager`; fight scenes are still to come (shadow-warrior-19b).
// One-shot layers, extra blend modes and punch inputs are for the fight scene; re-exports
// are the module's public surface. Drop this once fight uses them.
#![allow(dead_code, unused_imports)]

mod blend;
mod compositor;
mod input;
mod layer;
pub mod layers;
mod px;
pub mod scenes;

#[cfg(test)]
mod preview;

pub use blend::{blend, Blend};
pub use compositor::Compositor;
pub use input::LiveInputs;
pub use layer::{Ctx, Inputs, Layer, Punch};
pub use px::{lerp3, Px, Smoother};
