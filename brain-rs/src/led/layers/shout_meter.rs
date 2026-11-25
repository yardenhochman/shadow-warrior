//! The live SHOUT METER: a cyan segment growing from the base of the column to
//! `level * column_length`, over the (red/amber) energy bar. The energy bar says how much
//! the player has shouted so far, slowly; this says how loud they are shouting right now.
use crate::led::layer::{Ctx, Layer};
use crate::led::px::{lerp3, Px, Smoother};

#[derive(Clone, Debug)]
pub struct ShoutMeterStyle {
    /// Meter colour (display space). Must read differently from the energy bar (red to amber).
    pub color: [f32; 3],
    /// Colour of the leading edge (the colour lightened towards white).
    pub head: [f32; 3],
    /// Overall strength 0..1 (the fight scene uses a fraction): scales the alpha, so the
    /// layer never adds more than `intensity * color` per channel.
    pub intensity: f32,
    /// LEDs the top edge fades out over, above the exact edge.
    pub soft_len: f32,
    /// LEDs of lighter head below the edge.
    pub head_len: f32,
    /// Seconds to rise / fall. The input is already fast (30 Hz events), this only removes
    /// the stair-stepping between them: rise at once, fall over a few frames.
    pub attack: f32,
    pub release: f32,
}

impl ShoutMeterStyle {
    /// From `led.shout_meter_color` (RGB 0..255) and an intensity.
    pub fn new(rgb: [u8; 3], intensity: f32) -> Self {
        let color = [rgb[0] as f32 / 255.0, rgb[1] as f32 / 255.0, rgb[2] as f32 / 255.0];
        ShoutMeterStyle {
            color,
            head: lerp3(color, [1.0, 1.0, 1.0], 0.5),
            intensity: intensity.clamp(0.0, 1.0),
            soft_len: 2.5,
            head_len: 3.0,
            attack: 0.0,
            release: 0.04,
        }
    }
}

impl Default for ShoutMeterStyle {
    fn default() -> Self {
        Self::new([0, 200, 255], 1.0)
    }
}

/// Composite with `Blend::Screen` (or Add) after the energy bar. Draws nothing in silence.
pub struct ShoutMeter {
    style: ShoutMeterStyle,
    level: Smoother,
}

impl ShoutMeter {
    pub fn new(style: ShoutMeterStyle) -> Self {
        let level = Smoother::new(style.attack, style.release);
        ShoutMeter { style, level }
    }

    pub fn level(&self) -> f32 {
        self.level.value
    }
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl Layer for ShoutMeter {
    fn name(&self) -> &'static str {
        "shout_meter"
    }

    fn render(&mut self, ctx: &Ctx, out: &mut [Px]) {
        let n = out.len();
        let lvl = self.level.step(ctx.inputs.shout_level.clamp(0.0, 1.0), ctx.dt);
        if n == 0 || lvl < 0.002 || self.style.intensity <= 0.0 {
            return;
        }
        let edge = lvl * n as f32;
        let soft = self.style.soft_len.max(0.1);
        for (i, px) in out.iter_mut().enumerate() {
            // LED i covers [i, i+1): fully on once the edge is past it, fading out over `soft`
            // LEDs above the edge.
            let x = edge - i as f32;
            let a = smoothstep(-soft, 1.0, x);
            if a <= 0.0 {
                continue;
            }
            let near_head = (1.0 - (edge - i as f32 - 0.5).max(0.0) / self.style.head_len).clamp(0.0, 1.0);
            let c = lerp3(self.style.color, self.style.head, near_head * near_head);
            *px = Px::rgba(c[0], c[1], c[2], a * self.style.intensity);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::led::layer::Inputs;
    use crate::led::layers::EnergyBarStyle;

    fn run(m: &mut ShoutMeter, level: f32, frames: usize, n: usize) -> Vec<Px> {
        let inputs = Inputs { shout_level: level, ..Default::default() };
        let mut out = vec![Px::CLEAR; n];
        for _ in 0..frames {
            out.iter_mut().for_each(|p| *p = Px::CLEAR);
            m.render(&Ctx { dt: 1.0 / 30.0, t: 0.0, inputs: &inputs }, &mut out);
        }
        out
    }

    fn lit(out: &[Px]) -> usize {
        out.iter().filter(|p| p.a > 0.0).count()
    }

    #[test]
    fn zero_level_draws_nothing() {
        let mut m = ShoutMeter::new(ShoutMeterStyle::default());
        assert_eq!(lit(&run(&mut m, 0.0, 10, 180)), 0);
    }

    #[test]
    fn half_level_lights_half_the_strip_from_the_base_with_a_soft_top() {
        let mut m = ShoutMeter::new(ShoutMeterStyle::default());
        let out = run(&mut m, 0.5, 3, 180);
        assert!(out[0].a > 0.99 && out[89].a > 0.99, "solid body");
        assert!(out[179].a == 0.0 && out[100].a == 0.0);
        // fully-on LEDs = 90 (the edge), then a short fade, not a cliff
        let full = out.iter().filter(|p| p.a > 0.99).count();
        assert!((full as i32 - 90).abs() <= 1, "{full}");
        assert!(out[90].a > 0.0 && out[90].a < 1.0, "soft edge {}", out[90].a);
        assert!(lit(&out) <= 90 + 4, "fade is only a few LEDs: {}", lit(&out));
    }

    #[test]
    fn full_level_lights_the_whole_strip_and_other_lengths_scale() {
        let mut m = ShoutMeter::new(ShoutMeterStyle::default());
        assert_eq!(lit(&run(&mut m, 1.0, 3, 180)), 180);
        let mut short = ShoutMeter::new(ShoutMeterStyle::default());
        let out = run(&mut short, 0.5, 3, 60);
        let full = out.iter().filter(|p| p.a > 0.99).count();
        assert!((full as i32 - 30).abs() <= 1, "{full}");
    }

    #[test]
    fn rises_at_once_and_falls_in_a_few_frames() {
        let mut m = ShoutMeter::new(ShoutMeterStyle::default());
        run(&mut m, 0.8, 1, 180);
        assert!((m.level() - 0.8).abs() < 1e-5, "attack is immediate");
        run(&mut m, 0.0, 1, 180);
        assert!(m.level() > 0.0 && m.level() < 0.8);
        run(&mut m, 0.0, 15, 180);
        assert!(m.level() < 0.01);
    }

    #[test]
    fn colour_is_clearly_not_the_energy_bars() {
        let s = ShoutMeterStyle::default();
        let bar = EnergyBarStyle::default();
        let dist = |a: [f32; 3], b: [f32; 3]| {
            ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
        };
        for c in [bar.low, bar.high, bar.head] {
            assert!(dist(s.color, c) > 0.9, "{c:?} vs {:?}", s.color);
            assert!(s.color[2] > s.color[0], "meter is blue-ish, the bar red-ish");
        }
    }

    #[test]
    fn intensity_caps_the_light_it_adds() {
        let mut m = ShoutMeter::new(ShoutMeterStyle::new([0, 200, 255], 0.5));
        let out = run(&mut m, 1.0, 3, 50);
        let max_a = out.iter().map(|p| p.a).fold(0.0, f32::max);
        assert!((max_a - 0.5).abs() < 1e-5, "{max_a}");
        let mut off = ShoutMeter::new(ShoutMeterStyle::new([0, 200, 255], 0.0));
        assert_eq!(lit(&run(&mut off, 1.0, 3, 50)), 0);
    }
}
