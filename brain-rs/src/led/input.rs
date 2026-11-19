//! Turns the arena's discrete events into the continuous [`Inputs`] a scene renders from.
use super::layer::{Inputs, Punch};

/// How long a single shout event keeps the live level up (exponential time constant, seconds).
const SHOUT_DECAY_TAU: f32 = 0.35;
/// How fast punch activity ("rage") fades, seconds, and how much a full-power punch adds.
const RAGE_DECAY_TAU: f32 = 4.0;
const RAGE_PER_FULL_PUNCH: f32 = 0.35;
/// Punches queued in one frame beyond this are dropped (a stalled loop must not flood).
const MAX_PENDING_PUNCHES: usize = 16;

/// Event-fed state behind [`Inputs`]. Not shared: the render loop owns one, drains the event
/// bus into it with `on_*` and calls [`LiveInputs::frame`] once per frame.
#[derive(Clone, Debug, Default)]
pub struct LiveInputs {
    energy: f32,
    shout: f32,
    rage: f32,
    next_punch_id: u64,
    pending: Vec<Punch>,
}

impl LiveInputs {
    /// `EnergyChanged { energy, max_energy }` (arena points, the dashboard shows 0..100).
    /// Normalised to 0..1 against the event's own max, so it follows tuned thresholds.
    pub fn on_energy(&mut self, energy: f32, max_energy: f32) {
        self.energy = if max_energy > 0.0 && energy.is_finite() {
            (energy / max_energy).clamp(0.0, 1.0)
        } else {
            0.0
        };
    }

    /// `ShoutDetected { intensity }` (0.25..1 from the detector). A shout is an event, not a
    /// level, so it sets a level that [`Self::frame`] then decays; a louder one wins, a quieter
    /// one does not cut a louder one short.
    pub fn on_shout(&mut self, intensity: f32) {
        if intensity.is_finite() {
            self.shout = self.shout.max(intensity.clamp(0.0, 1.0));
        }
    }

    /// `PunchDetected { power }` (0..100). Delivered once, in the next [`Self::frame`].
    pub fn on_punch(&mut self, power: f32) {
        if !power.is_finite() {
            return;
        }
        let power = power.clamp(0.0, 100.0);
        self.next_punch_id += 1;
        if self.pending.len() < MAX_PENDING_PUNCHES {
            self.pending.push(Punch { power, id: self.next_punch_id });
        }
        self.rage = (self.rage + power / 100.0 * RAGE_PER_FULL_PUNCH).min(1.0);
    }

    /// Inputs for the next frame, `dt` seconds after the previous one.
    pub fn frame(&mut self, dt: f32) -> Inputs {
        let out = Inputs {
            energy: self.energy,
            shout: self.shout,
            rage: self.rage,
            punches: std::mem::take(&mut self.pending),
        };
        self.rage *= (-dt.max(0.0) / RAGE_DECAY_TAU).exp();
        if self.rage < 0.003 {
            self.rage = 0.0;
        }
        self.shout *= (-dt.max(0.0) / SHOUT_DECAY_TAU).exp();
        if self.shout < 0.003 {
            self.shout = 0.0;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn energy_is_normalised_by_the_events_max_and_clamped() {
        let mut i = LiveInputs::default();
        i.on_energy(50.0, 100.0);
        assert_eq!(i.frame(0.033).energy, 0.5);
        i.on_energy(30.0, 60.0);
        assert_eq!(i.frame(0.033).energy, 0.5, "follows a tuned threshold");
        i.on_energy(150.0, 100.0);
        assert_eq!(i.frame(0.033).energy, 1.0);
        i.on_energy(-5.0, 100.0);
        assert_eq!(i.frame(0.033).energy, 0.0);
        i.on_energy(10.0, 0.0);
        assert_eq!(
            i.frame(0.033).energy,
            0.0,
            "zero max does not divide by zero"
        );
        i.on_energy(f32::NAN, 100.0);
        assert_eq!(i.frame(0.033).energy, 0.0);
    }

    #[test]
    fn one_shout_gives_a_short_decaying_glow() {
        let mut i = LiveInputs::default();
        i.on_shout(0.8);
        let first = i.frame(1.0 / 30.0).shout;
        assert_eq!(first, 0.8, "attack is immediate");
        let mut prev = first;
        let mut t = 0.0;
        while t < 2.0 {
            let s = i.frame(1.0 / 30.0).shout;
            assert!(s <= prev);
            prev = s;
            t += 1.0 / 30.0;
        }
        assert_eq!(prev, 0.0, "gone after 2 s");
    }

    #[test]
    fn quieter_shout_does_not_cut_a_louder_one_and_dt_is_real_time() {
        let mut i = LiveInputs::default();
        i.on_shout(0.9);
        i.on_shout(0.3);
        assert_eq!(i.frame(0.35).shout, 0.9);
        // after one time constant the level is 1/e of the original
        let s = i.frame(0.0).shout;
        assert!((s - 0.9 / std::f32::consts::E).abs() < 1e-4, "{s}");
        // a long frame decays more than a short one
        let (mut a, mut b) = (LiveInputs::default(), LiveInputs::default());
        a.on_shout(1.0);
        b.on_shout(1.0);
        a.frame(0.5);
        b.frame(0.01);
        assert!(a.frame(0.0).shout < b.frame(0.0).shout);
    }

    #[test]
    fn punches_are_delivered_once_with_increasing_ids_and_raise_rage() {
        let mut i = LiveInputs::default();
        i.on_punch(30.0);
        i.on_punch(90.0);
        let f = i.frame(1.0 / 30.0);
        assert_eq!(f.punches.len(), 2);
        assert!(f.punches[1].id > f.punches[0].id);
        assert_eq!(f.punches[1].power, 90.0);
        assert!(f.rage > 0.3);
        assert!(i.frame(1.0 / 30.0).punches.is_empty(), "not repeated");
        i.on_punch(120.0);
        assert_eq!(i.frame(0.0).punches[0].power, 100.0, "clamped");
        for _ in 0..1200 {
            i.frame(1.0 / 30.0);
        }
        assert_eq!(i.frame(0.0).rage, 0.0, "rage decays away");
        i.on_punch(f32::NAN);
        assert!(i.frame(0.0).punches.is_empty());
    }
}
