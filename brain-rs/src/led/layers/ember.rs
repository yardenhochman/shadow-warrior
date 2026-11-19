//! The FIGHT base: a low, slow, dark-red/orange flicker rising from the bottom of the column.
use crate::led::layer::{Ctx, Layer};
use crate::led::px::{lerp3, Px, Smoother};

#[derive(Clone, Debug)]
pub struct EmberStyle {
    /// Colour of the coolest glowing coals and of the hottest flicker (brightest channel ~1;
    /// the level scales it).
    pub dark: [f32; 3],
    pub hot: [f32; 3],
    /// Peak brightness (of the brightest channel) at rest and at full rage. Keep both well
    /// below 1 so punch pulses, added on top, pop.
    pub rest_level: f32,
    pub rage_level: f32,
    /// How much of the column the fire reaches at rest and at full rage (fraction of the
    /// strip; the glow falls off towards the top).
    pub rest_height: f32,
    pub rage_height: f32,
    /// Flicker speed in noise cells per second at rest and at full rage.
    pub rest_speed: f32,
    pub rage_speed: f32,
    /// Noise cell size in LEDs (bigger = broader, slower-looking tongues).
    pub cell: f32,
    /// How fast the rage input is followed (seconds up / down).
    pub attack: f32,
    pub release: f32,
}

impl Default for EmberStyle {
    fn default() -> Self {
        EmberStyle {
            dark: [1.0, 0.05, 0.0],
            hot: [1.0, 0.38, 0.03],
            rest_level: 0.30,
            rage_level: 0.48,
            rest_height: 0.38,
            rage_height: 0.62,
            rest_speed: 0.9,
            rage_speed: 2.2,
            cell: 9.0,
            attack: 0.25,
            release: 1.2,
        }
    }
}

/// Integer hash to 0..1.
fn hash(x: i32, y: i32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x9E37_79B1) ^ (y as u32).wrapping_mul(0x85EB_CA6B);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^= h >> 15;
    (h & 0xFFFF) as f32 / 65535.0
}

/// Smooth value noise in 2D, 0..1.
fn noise(x: f32, y: f32) -> f32 {
    let (xi, yi) = (x.floor(), y.floor());
    let (fx, fy) = (x - xi, y - yi);
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let (xi, yi) = (xi as i32, yi as i32);
    let a = hash(xi, yi) + (hash(xi + 1, yi) - hash(xi, yi)) * sx;
    let b = hash(xi, yi + 1) + (hash(xi + 1, yi + 1) - hash(xi, yi + 1)) * sx;
    a + (b - a) * sy
}

pub struct Ember {
    style: EmberStyle,
    rage: Smoother,
    phase: f32,
}

impl Ember {
    pub fn new(style: EmberStyle) -> Self {
        let rage = Smoother::new(style.attack, style.release);
        Ember {
            style,
            rage,
            phase: 0.0,
        }
    }
}

impl Layer for Ember {
    fn name(&self) -> &'static str {
        "ember"
    }

    fn render(&mut self, ctx: &Ctx, out: &mut [Px]) {
        let n = out.len();
        if n == 0 {
            return;
        }
        let s = &self.style;
        let rage = self.rage.step(ctx.inputs.rage.clamp(0.0, 1.0), ctx.dt);
        let mix = |a: f32, b: f32| a + (b - a) * rage;
        self.phase += ctx.dt * mix(s.rest_speed, s.rage_speed);
        let level = mix(s.rest_level, s.rage_level);
        let reach = (mix(s.rest_height, s.rage_height) * n as f32).max(1.0);
        for (i, px) in out.iter_mut().enumerate() {
            let h = i as f32 / reach;
            if h >= 1.0 {
                continue;
            }
            // Fire rises: the noise field scrolls upwards over time.
            let y = i as f32 / s.cell - self.phase;
            let flick = 0.65 * noise(y, 0.0) + 0.35 * noise(y * 2.3 + 7.0, 3.0);
            let heat = (1.0 - h).powf(1.6) * (0.35 + 0.65 * flick);
            let c = lerp3(s.dark, s.hot, flick * (1.0 - h));
            let k = (heat * level).min(level);
            *px = Px::rgba(c[0] * k, c[1] * k, c[2] * k, 1.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::led::layer::Inputs;

    fn frames(rage: f32, n: usize) -> Vec<Vec<Px>> {
        let mut e = Ember::new(EmberStyle::default());
        let inputs = Inputs {
            rage,
            ..Default::default()
        };
        (0..n)
            .map(|k| {
                let mut out = vec![Px::CLEAR; 180];
                e.render(
                    &Ctx {
                        dt: 1.0 / 30.0,
                        t: k as f32 / 30.0,
                        inputs: &inputs,
                    },
                    &mut out,
                );
                out
            })
            .collect()
    }

    fn peak(f: &[Vec<Px>]) -> f32 {
        f.iter()
            .flatten()
            .map(|p| p.r.max(p.g).max(p.b))
            .fold(0.0, f32::max)
    }

    #[test]
    fn stays_dim_even_at_full_rage_and_rage_brightens_it() {
        let (calm, wild) = (frames(0.0, 120), frames(1.0, 120));
        assert!(peak(&calm) <= EmberStyle::default().rest_level + 1e-4);
        assert!(peak(&wild) <= EmberStyle::default().rage_level + 1e-4);
        assert!(peak(&wild) < 0.5);
        assert!(peak(&wild) > peak(&calm) + 0.05);
    }

    #[test]
    fn rises_from_the_bottom_and_flickers() {
        let f = frames(0.0, 60);
        assert!(
            f[30][0].a > 0.0 && f[30][0].r > f[30][120].r,
            "bottom brighter than the top"
        );
        assert_eq!(f[30][179], Px::CLEAR, "fire does not reach the top at rest");
        assert!(
            f[30]
                .iter()
                .take(40)
                .zip(&f[31])
                .any(|(a, b)| (a.r - b.r).abs() > 1e-4),
            "moves"
        );
        assert!(f[30][10].r > 0.0, "reddish");
        assert!(
            f[30][10].r >= f[30][10].g * 2.0,
            "dark red/orange, never green"
        );
    }
}
