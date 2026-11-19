//! One punch: a shockwave that leaves the bag and runs up and down the column.
use crate::led::layer::{Ctx, Layer};
use crate::led::px::Px;

/// Look of one punch tier.
#[derive(Clone, Debug)]
pub struct TierStyle {
    pub color: [f32; 3],
    /// Brightness of the wavefront at the bag (added on top of the base).
    pub peak: f32,
    /// Front speed (LEDs per second), front half-width (LEDs), total life (seconds).
    pub speed: f32,
    pub sigma: f32,
    pub life: f32,
    /// Whole-strip flash: peak strength (0 = none) and decay time constant (seconds).
    pub flash: f32,
    pub flash_tau: f32,
}

#[derive(Clone, Debug)]
pub struct PunchStyle {
    /// Power (0..100) below which a punch is a jab / a hook; at or above `hook_max` it is heavy.
    pub jab_max: f32,
    pub hook_max: f32,
    pub jab: TierStyle,
    pub hook: TierStyle,
    pub heavy: TierStyle,
    /// Height of the bag along the column, 0 = bottom LED, 1 = top.
    pub bag_height: f32,
    /// Seconds of delay per unit of `bag_distance` (inner columns first).
    pub delay_per_distance: f32,
    /// Fraction of brightness lost at the outermost column.
    pub outer_falloff: f32,
    /// Radius of the bright core that lights at the bag at the moment of impact (LEDs).
    pub core_sigma: f32,
}

impl Default for PunchStyle {
    fn default() -> Self {
        PunchStyle {
            jab_max: 45.0,
            hook_max: 75.0,
            // amber, ~38 degrees: far from the 150-200 degree teal of shouts
            jab: TierStyle {
                color: [1.0, 0.55, 0.08],
                peak: 0.55,
                speed: 110.0,
                sigma: 6.0,
                life: 0.45,
                flash: 0.0,
                flash_tau: 0.1,
            },
            hook: TierStyle {
                color: [1.0, 0.72, 0.38],
                peak: 0.8,
                speed: 170.0,
                sigma: 10.0,
                life: 0.65,
                flash: 0.0,
                flash_tau: 0.1,
            },
            heavy: TierStyle {
                color: [1.0, 0.93, 0.80],
                peak: 1.0,
                speed: 260.0,
                sigma: 16.0,
                life: 0.95,
                flash: 0.45,
                flash_tau: 0.09,
            },
            bag_height: 0.45,
            delay_per_distance: 0.08,
            outer_falloff: 0.35,
            core_sigma: 6.0,
        }
    }
}

impl PunchStyle {
    pub fn tier(&self, power: f32) -> &TierStyle {
        if power < self.jab_max {
            &self.jab
        } else if power < self.hook_max {
            &self.hook
        } else {
            &self.heavy
        }
    }
}

/// Meant for `Blend::Add` (or Screen). Finished once the wave has left the strip or died.
pub struct PunchPulse {
    tier: TierStyle,
    centre: f32,
    core_sigma: f32,
    /// Seconds still to wait before the wave leaves the bag.
    delay: f32,
    age: f32,
    /// Overall scale from power within the tier and column distance.
    gain: f32,
}

impl PunchPulse {
    /// `power` 0..100; `bag_distance` from the column layout; `len` strip LEDs.
    pub fn new(style: &PunchStyle, power: f32, bag_distance: f32, len: usize) -> Self {
        let tier = style.tier(power).clone();
        let p = (power / 100.0).clamp(0.0, 1.0);
        PunchPulse {
            tier,
            centre: style.bag_height.clamp(0.0, 1.0) * len.saturating_sub(1) as f32,
            core_sigma: style.core_sigma,
            delay: bag_distance.max(0.0) * style.delay_per_distance,
            age: 0.0,
            // stronger within the tier is brighter; outer columns see less
            gain: (0.75 + 0.25 * p) * (1.0 - style.outer_falloff * bag_distance.clamp(0.0, 1.0)),
        }
    }

    fn finished(&self) -> bool {
        self.delay <= 0.0 && self.age >= self.tier.life
    }
}

impl Layer for PunchPulse {
    fn name(&self) -> &'static str {
        "punch_pulse"
    }

    fn render(&mut self, ctx: &Ctx, out: &mut [Px]) {
        if self.delay > 0.0 {
            self.delay -= ctx.dt;
            if self.delay > 0.0 {
                return;
            }
            // spent part of this frame waiting: start with the remainder
            self.age = -self.delay;
            self.delay = 0.0;
        } else {
            self.age += ctx.dt;
        }
        if self.finished() {
            return;
        }
        let t = &self.tier;
        let life = (1.0 - self.age / t.life).clamp(0.0, 1.0);
        let front = self.age * t.speed;
        // the front thins and fades as it travels; the core and flash fade much faster
        let amp = t.peak * self.gain * life * life;
        let core = (-self.age / 0.07).exp();
        let flash = t.flash * self.gain * (-self.age / t.flash_tau).exp();
        for (i, px) in out.iter_mut().enumerate() {
            let d = (i as f32 - self.centre).abs();
            let w = (d - front) / (t.sigma * (1.0 + 0.5 * self.age / t.life));
            let mut k = amp * (-w * w).exp();
            k += t.peak * self.gain * core * (-(d / self.core_sigma).powi(2)).exp();
            k += flash;
            if k > 0.002 {
                *px = Px::rgba(t.color[0], t.color[1], t.color[2], k.min(1.5));
            }
        }
    }

    fn done(&self) -> bool {
        self.finished()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::led::layer::Inputs;

    fn run(p: &mut PunchPulse, frames: usize) -> Vec<Vec<Px>> {
        let inputs = Inputs::default();
        (0..frames)
            .map(|k| {
                let mut out = vec![Px::CLEAR; 180];
                p.render(
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
        f.iter().flatten().map(|p| p.a).fold(0.0, f32::max)
    }

    #[test]
    fn wave_leaves_the_bag_in_both_directions_and_ends() {
        let s = PunchStyle::default();
        let mut p = PunchPulse::new(&s, 60.0, 0.0, 180);
        let f = run(&mut p, 40);
        let bag = (0.45 * 179.0) as usize;
        let farthest = |frame: &[Px], dir: i32| {
            (0..180)
                .filter(|&i| (i as i32 - bag as i32) * dir > 0 && frame[i].a > 0.1)
                .map(|i| (i as i32 - bag as i32).abs())
                .max()
                .unwrap_or(0)
        };
        assert!(
            farthest(&f[8], 1) > farthest(&f[2], 1) + 10,
            "front moves up"
        );
        assert!(farthest(&f[8], -1) > farthest(&f[2], -1) + 10, "and down");
        assert!(p.done());
        assert!(f[39].iter().all(|px| px.a == 0.0));
    }

    #[test]
    fn heavy_is_brighter_bigger_and_flashes_the_whole_strip_a_jab_does_not() {
        let s = PunchStyle::default();
        let jab = run(&mut PunchPulse::new(&s, 30.0, 0.0, 180), 20);
        let heavy = run(&mut PunchPulse::new(&s, 95.0, 0.0, 180), 20);
        assert!(peak(&heavy) > peak(&jab) + 0.3);
        let lit = |f: &[Vec<Px>]| {
            f.iter()
                .map(|fr| fr.iter().filter(|p| p.a > 0.1).count())
                .max()
                .unwrap()
        };
        assert!(lit(&heavy) > lit(&jab));
        assert!(
            heavy[1].iter().all(|p| p.a > 0.1),
            "heavy flashes every LED"
        );
        assert!(jab[1].iter().any(|p| p.a < 0.05), "jab does not");
    }

    #[test]
    fn outer_columns_start_later_and_dimmer() {
        let s = PunchStyle::default();
        let first_lit = |p: &mut PunchPulse| {
            run(p, 30)
                .iter()
                .position(|f| f.iter().any(|px| px.a > 0.05))
                .unwrap()
        };
        let inner = first_lit(&mut PunchPulse::new(&s, 60.0, 0.0, 180));
        let outer = first_lit(&mut PunchPulse::new(&s, 60.0, 1.0, 180));
        assert_eq!(inner, 0);
        assert!(outer >= 2, "outer starts {outer} frames later");
        let pk = |d| peak(&run(&mut PunchPulse::new(&s, 60.0, d, 180), 30));
        assert!(pk(1.0) < pk(0.0));
    }

    #[test]
    fn colours_keep_out_of_the_shout_hues() {
        // hue of each tier colour must lie outside 150..200 degrees
        let s = PunchStyle::default();
        for t in [&s.jab, &s.hook, &s.heavy] {
            let [r, g, b] = t.color;
            let (mx, mn) = (r.max(g).max(b), r.min(g).min(b));
            let hue = if mx == r {
                60.0 * ((g - b) / (mx - mn)).rem_euclid(6.0)
            } else if mx == g {
                60.0 * ((b - r) / (mx - mn) + 2.0)
            } else {
                60.0 * ((r - g) / (mx - mn) + 4.0)
            };
            assert!(!(150.0..=200.0).contains(&hue), "{hue}");
        }
    }
}
