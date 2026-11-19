//! The layer interface and the inputs layers react to.
use super::px::Px;

/// A punch on the bag, normalised.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Punch {
    /// 0..100, as `PunchDetected.power` reports it (jab < 45, hook < 75, heavy above).
    pub power: f32,
    /// Monotonically increasing per `LiveInputs`; a scene remembers the last id it spawned for,
    /// so a punch is never drawn twice by the same strip.
    pub id: u64,
}

/// Everything live that layers may react to, refreshed by the caller before each frame.
#[derive(Clone, Debug, Default)]
pub struct Inputs {
    /// Persistent energy bar level, 0..1 (the arena's WARMING energy / threshold).
    pub energy: f32,
    /// Instantaneous shout level, 0..1 (layers smooth it themselves).
    pub shout: f32,
    /// Recent punch activity, 0..1: a decaying average of punch power (drives the fire base).
    pub rage: f32,
    /// Punches since the previous frame. The compositor owner spawns one-shot layers for these;
    /// layers may also read them directly.
    pub punches: Vec<Punch>,
}

/// Per-frame context.
pub struct Ctx<'a> {
    /// Seconds since the previous frame.
    pub dt: f32,
    /// Seconds since the compositor started.
    pub t: f32,
    pub inputs: &'a Inputs,
}

/// One drawable thing. `out` is pre-cleared; write only the pixels you cover.
pub trait Layer: Send {
    fn name(&self) -> &'static str;
    fn render(&mut self, ctx: &Ctx, out: &mut [Px]);
    /// One-shot layers return true once they have drawn their last frame.
    fn done(&self) -> bool {
        false
    }
}
