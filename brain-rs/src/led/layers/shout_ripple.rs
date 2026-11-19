//! Fight-mode shout feedback: a cool teal glow around the bag with ripples running out of it.
//! Teal is reserved for shouts; punches stay in the amber/white range.
use crate::led::layer::{Ctx, Layer};
use crate::led::px::{Px, Smoother};

#[derive(Clone, Debug)]
pub struct ShoutRippleStyle {
    /// Teal, hue ~175 degrees.
    pub color: [f32; 3],
    /// Brightness at full level.
    pub gain: f32,
    /// Bag height along the column (0 bottom .. 1 top); the glow is centred there.
    pub bag_height: f32,
    /// Half-width in LEDs at silence / full level.
    pub min_sigma: f32,
    pub max_sigma: f32,
    /// Ripple spatial period (LEDs) and speed (LEDs per second, outwards).
    pub ripple_period: f32,
    pub ripple_speed: f32,
    pub attack: f32,
    pub release: f32,
}

impl Default for ShoutRippleStyle {
    fn default() -> Self {
        ShoutRippleStyle {
            color: [0.0, 0.85, 0.75],
            gain: 0.7,
            bag_height: 0.45,
            min_sigma: 10.0,
            max_sigma: 75.0,
            ripple_period: 22.0,
            ripple_speed: 90.0,
            attack: 0.03,
            release: 0.3,
        }
    }
}

pub struct ShoutRipple {
    style: ShoutRippleStyle,
    level: Smoother,
    phase: f32,
}

impl ShoutRipple {
    pub fn new(style: ShoutRippleStyle) -> Self {
        let level = Smoother::new(style.attack, style.release);
        ShoutRipple {
            style,
            level,
            phase: 0.0,
        }
    }
}

impl Layer for ShoutRipple {
    fn name(&self) -> &'static str {
        "shout_ripple"
    }

    fn render(&mut self, ctx: &Ctx, out: &mut [Px]) {
        let n = out.len();
        let s = &self.style;
        let lvl = self.level.step(ctx.inputs.shout.clamp(0.0, 1.0), ctx.dt);
        if n == 0 || lvl < 0.004 {
            return;
        }
        self.phase += ctx.dt * s.ripple_speed;
        let centre = s.bag_height.clamp(0.0, 1.0) * (n - 1) as f32;
        let sigma = s.min_sigma + (s.max_sigma - s.min_sigma) * lvl;
        for (i, px) in out.iter_mut().enumerate() {
            let d = (i as f32 - centre).abs();
            let env = (-(d / sigma).powi(2)).exp();
            let ripple =
                0.5 + 0.5 * ((d - self.phase) * std::f32::consts::TAU / s.ripple_period).cos();
            let k = s.gain * lvl * env * (0.35 + 0.65 * ripple);
            *px = Px::rgba(s.color[0], s.color[1], s.color[2], k);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::led::layer::Inputs;

    fn frame(shout: f32, frames: usize) -> Vec<Px> {
        let mut r = ShoutRipple::new(ShoutRippleStyle::default());
        let inputs = Inputs {
            shout,
            ..Default::default()
        };
        let mut out = vec![Px::CLEAR; 180];
        for k in 0..frames {
            out.iter_mut().for_each(|p| *p = Px::CLEAR);
            r.render(
                &Ctx {
                    dt: 1.0 / 30.0,
                    t: k as f32 / 30.0,
                    inputs: &inputs,
                },
                &mut out,
            );
        }
        out
    }

    #[test]
    fn silent_draws_nothing_and_loud_is_wider_and_brighter() {
        assert!(frame(0.0, 10).iter().all(|p| p.a == 0.0));
        let (q, l) = (frame(0.3, 10), frame(1.0, 10));
        let lit = |f: &[Px]| f.iter().filter(|p| p.a > 0.05).count();
        assert!(lit(&l) > lit(&q));
        assert!(
            l.iter().map(|p| p.a).fold(0.0, f32::max) > q.iter().map(|p| p.a).fold(0.0, f32::max)
        );
    }

    #[test]
    fn is_teal() {
        let c = ShoutRippleStyle::default().color;
        assert!(c[0] < 0.1 && c[1] > 0.6 && c[2] > 0.6);
    }
}
