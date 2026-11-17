//! Live shout feedback: a glow at the bar's head that follows the instantaneous level.
use crate::led::layer::{Ctx, Layer};
use crate::led::px::{Px, Smoother};

#[derive(Clone, Debug)]
pub struct ShoutGlowStyle {
    /// Colour of the glow around the bar head.
    pub glow: [f32; 3],
    /// Colour of the faint wash over the whole strip.
    pub wash: [f32; 3],
    /// Wash strength at full level.
    pub wash_gain: f32,
    /// Glow half-width in LEDs at silence and at full level.
    pub min_sigma: f32,
    pub max_sigma: f32,
    /// Smoothing of the raw level, seconds (fast up so it feels live, a little slower down).
    pub attack: f32,
    pub release: f32,
}

impl Default for ShoutGlowStyle {
    fn default() -> Self {
        ShoutGlowStyle {
            glow: [1.0, 0.70, 0.30],
            wash: [0.9, 0.15, 0.0],
            wash_gain: 0.18,
            min_sigma: 3.0,
            max_sigma: 26.0,
            attack: 0.03,
            release: 0.25,
        }
    }
}

/// Meant to be composited with `Blend::Add` over the energy bar: it adds light around the
/// bar's head and a faint wash, and draws nothing in silence.
pub struct ShoutGlow {
    style: ShoutGlowStyle,
    level: Smoother,
}

impl ShoutGlow {
    pub fn new(style: ShoutGlowStyle) -> Self {
        let level = Smoother::new(style.attack, style.release);
        ShoutGlow { style, level }
    }
}

impl Layer for ShoutGlow {
    fn name(&self) -> &'static str {
        "shout_glow"
    }

    fn render(&mut self, ctx: &Ctx, out: &mut [Px]) {
        let n = out.len();
        let lvl = self.level.step(ctx.inputs.shout.clamp(0.0, 1.0), ctx.dt);
        if n == 0 || lvl < 0.002 {
            return;
        }
        let head = (ctx.inputs.energy.clamp(0.0, 1.0) * n as f32).max(2.0);
        let sigma = self.style.min_sigma + (self.style.max_sigma - self.style.min_sigma) * lvl;
        for (i, px) in out.iter_mut().enumerate() {
            let d = (i as f32 + 0.5 - head) / sigma;
            let g = (-d * d).exp() * lvl;
            let w = lvl * self.style.wash_gain;
            let c = [
                (self.style.glow[0] * g + self.style.wash[0] * w).min(1.0),
                (self.style.glow[1] * g + self.style.wash[1] * w).min(1.0),
                (self.style.glow[2] * g + self.style.wash[2] * w).min(1.0),
            ];
            *px = Px::opaque(c);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::led::blend::Blend;
    use crate::led::compositor::Compositor;
    use crate::led::layer::Inputs;
    use crate::led::layers::{EnergyBar, EnergyBarStyle};

    fn glow_frame(shout: f32, energy: f32, frames: usize) -> Vec<Px> {
        let mut g = ShoutGlow::new(ShoutGlowStyle::default());
        let inputs = Inputs { energy, shout, ..Default::default() };
        let mut out = vec![Px::CLEAR; 180];
        for _ in 0..frames {
            out.iter_mut().for_each(|p| *p = Px::CLEAR);
            g.render(&Ctx { dt: 1.0 / 30.0, t: 0.0, inputs: &inputs }, &mut out);
        }
        out
    }

    #[test]
    fn silence_draws_nothing() {
        assert!(glow_frame(0.0, 0.5, 10).iter().all(|p| p.a == 0.0));
    }

    #[test]
    fn loud_shout_glows_around_the_bar_head_and_wider_than_a_quiet_one() {
        let loud = glow_frame(1.0, 0.5, 10); // head at LED 90
        // red saturates over a plateau at full level; blue is never clamped, so its peak is the centre
        let peak = loud.iter().enumerate().max_by(|a, b| a.1.b.total_cmp(&b.1.b)).unwrap().0;
        assert!((peak as i32 - 90).abs() <= 1, "peak at {peak}");
        let quiet = glow_frame(0.3, 0.5, 10);
        let width = |f: &[Px]| f.iter().filter(|p| p.r > 0.25).count();
        assert!(width(&loud) > width(&quiet), "{} vs {}", width(&loud), width(&quiet));
        assert!(loud[0].r > 0.0, "faint wash reaches the whole strip");
    }

    #[test]
    fn glow_dies_down_after_the_shout() {
        let mut g = ShoutGlow::new(ShoutGlowStyle::default());
        let mut out = vec![Px::CLEAR; 180];
        let shout = Inputs { energy: 0.5, shout: 1.0, ..Default::default() };
        let quiet = Inputs { energy: 0.5, shout: 0.0, ..Default::default() };
        for _ in 0..10 {
            g.render(&Ctx { dt: 1.0 / 30.0, t: 0.0, inputs: &shout }, &mut out);
        }
        let mut peak_after = |frames: usize| {
            for _ in 0..frames {
                out.iter_mut().for_each(|p| *p = Px::CLEAR);
                g.render(&Ctx { dt: 1.0 / 30.0, t: 0.0, inputs: &quiet }, &mut out);
            }
            out.iter().map(|p| p.r).fold(0.0, f32::max)
        };
        let after_half_second = peak_after(15);
        let after_two_seconds = peak_after(45);
        assert!(after_half_second > after_two_seconds, "{after_half_second} {after_two_seconds}");
        assert!(after_two_seconds < 0.05, "{after_two_seconds}");
    }

    #[test]
    fn added_over_the_bar_it_brightens_without_hiding_it() {
        let mut c = Compositor::new(180, 8);
        c.push(Box::new(EnergyBar::new(EnergyBarStyle { attack: 0.0, ..Default::default() })), Blend::Normal, 1.0);
        let bar_only = c.render(1.0 / 30.0, &Inputs { energy: 0.5, ..Default::default() }).to_vec();

        let mut c2 = Compositor::new(180, 8);
        c2.push(Box::new(EnergyBar::new(EnergyBarStyle { attack: 0.0, ..Default::default() })), Blend::Normal, 1.0);
        c2.push(Box::new(ShoutGlow::new(ShoutGlowStyle::default())), Blend::Add, 1.0);
        let mut both = Vec::new();
        for _ in 0..5 {
            both = c2.render(1.0 / 30.0, &Inputs { energy: 0.5, shout: 1.0, ..Default::default() }).to_vec();
        }
        let sum = |f: &[u8]| f.iter().map(|&b| b as u32).sum::<u32>();
        assert!(sum(&both) > sum(&bar_only), "glow adds light");
        // the empty part of the strip stays dark far from the head
        assert!(both[170 * 3] < 80, "far end stays dim: {}", both[170 * 3]);
    }
}
