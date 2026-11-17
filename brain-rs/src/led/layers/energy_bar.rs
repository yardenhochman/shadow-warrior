//! The persistent WARMING energy bar.
use crate::led::layer::{Ctx, Layer};
use crate::led::px::{lerp3, Px, Smoother};

#[derive(Clone, Debug)]
pub struct EnergyBarStyle {
    /// Colour at the start of the strip.
    pub low: [f32; 3],
    /// Colour at the far end of the strip (the bar is a gradient along the strip, not the fill).
    pub high: [f32; 3],
    /// Colour at the bar's leading edge.
    pub head: [f32; 3],
    /// How many LEDs the leading-edge glow trails behind the edge.
    pub head_len: f32,
    /// Seconds to rise / to fall (the arena's energy already drains slowly; this only smooths).
    pub attack: f32,
    pub release: f32,
    /// White flashes per second while the bar is full.
    pub full_flash_hz: f32,
}

impl Default for EnergyBarStyle {
    fn default() -> Self {
        EnergyBarStyle {
            low: [1.0, 0.08, 0.0],
            high: [1.0, 0.75, 0.10],
            head: [1.0, 0.95, 0.70],
            head_len: 6.0,
            attack: 0.10,
            release: 0.60,
            full_flash_hz: 3.0,
        }
    }
}

/// A bar that fills from the first LED in proportion to `Inputs::energy`. The edge is
/// anti-aliased (fractional coverage becomes alpha), so a slow rise moves smoothly even on a
/// short strip.
pub struct EnergyBar {
    style: EnergyBarStyle,
    level: Smoother,
    full_for: f32,
}

impl EnergyBar {
    pub fn new(style: EnergyBarStyle) -> Self {
        let level = Smoother::new(style.attack, style.release);
        EnergyBar { style, level, full_for: 0.0 }
    }

    /// Current smoothed fill, 0..1.
    pub fn level(&self) -> f32 {
        self.level.value
    }
}

impl Layer for EnergyBar {
    fn name(&self) -> &'static str {
        "energy_bar"
    }

    fn render(&mut self, ctx: &Ctx, out: &mut [Px]) {
        let n = out.len();
        if n == 0 {
            return;
        }
        let lvl = self.level.step(ctx.inputs.energy.clamp(0.0, 1.0), ctx.dt);
        let edge = lvl * n as f32;
        let last = (n - 1).max(1) as f32;
        for (i, px) in out.iter_mut().enumerate() {
            let cover = (edge - i as f32).clamp(0.0, 1.0);
            if cover <= 0.0 {
                continue;
            }
            let mut c = lerp3(self.style.low, self.style.high, i as f32 / last);
            let behind = edge - (i as f32 + 0.5);
            if behind < self.style.head_len {
                let g = (1.0 - behind.max(0.0) / self.style.head_len).powi(2);
                c = lerp3(c, self.style.head, g * 0.9);
            }
            *px = Px::rgba(c[0], c[1], c[2], cover);
        }
        if ctx.inputs.energy >= 0.999 {
            self.full_for += ctx.dt;
            if ((self.full_for * self.style.full_flash_hz) as u32).is_multiple_of(2) {
                out.iter_mut().for_each(|p| *p = Px::rgba(1.0, 1.0, 1.0, 1.0));
            }
        } else {
            self.full_for = 0.0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::led::layer::Inputs;

    fn run(bar: &mut EnergyBar, energy: f32, frames: usize, n: usize) -> Vec<Px> {
        let inputs = Inputs { energy, ..Default::default() };
        let mut out = vec![Px::CLEAR; n];
        for _ in 0..frames {
            out.iter_mut().for_each(|p| *p = Px::CLEAR);
            bar.render(&Ctx { dt: 1.0 / 30.0, t: 0.0, inputs: &inputs }, &mut out);
        }
        out
    }

    fn lit(out: &[Px]) -> usize {
        out.iter().filter(|p| p.a > 0.0).count()
    }

    #[test]
    fn empty_energy_draws_nothing() {
        let mut bar = EnergyBar::new(EnergyBarStyle::default());
        assert_eq!(lit(&run(&mut bar, 0.0, 10, 180)), 0);
    }

    #[test]
    fn half_energy_fills_half_the_strip_from_the_start() {
        let mut bar = EnergyBar::new(EnergyBarStyle::default());
        let out = run(&mut bar, 0.5, 90, 180); // 3 s: fully settled
        assert!((bar.level() - 0.5).abs() < 0.01);
        assert!((lit(&out) as i32 - 90).abs() <= 1, "lit {}", lit(&out));
        assert!(out[0].a == 1.0 && out[179].a == 0.0);
    }

    #[test]
    fn the_edge_is_anti_aliased() {
        let mut bar = EnergyBar::new(EnergyBarStyle { attack: 0.0, ..Default::default() });
        let out = run(&mut bar, 0.5 + 0.25 / 180.0, 1, 180); // edge at 90.25
        assert_eq!(out[89].a, 1.0);
        assert!((out[90].a - 0.25).abs() < 1e-4, "{}", out[90].a);
        assert_eq!(out[91].a, 0.0);
    }

    #[test]
    fn rises_quickly_and_falls_slowly() {
        let mut bar = EnergyBar::new(EnergyBarStyle::default());
        run(&mut bar, 0.8, 15, 180); // 0.5 s up
        assert!(bar.level() > 0.76, "{}", bar.level());
        let peak = bar.level();
        run(&mut bar, 0.0, 15, 180); // 0.5 s down
        assert!(bar.level() > peak * 0.4 && bar.level() < peak, "{}", bar.level());
    }

    #[test]
    fn head_is_brighter_than_the_body_and_a_full_bar_flashes() {
        let mut bar = EnergyBar::new(EnergyBarStyle { attack: 0.0, ..Default::default() });
        let out = run(&mut bar, 0.5, 1, 180);
        assert!(out[89].g > out[40].g + 0.2, "head {:?} body {:?}", out[89], out[40]);
        let mut full = EnergyBar::new(EnergyBarStyle { attack: 0.0, ..Default::default() });
        let on = run(&mut full, 1.0, 1, 180);
        assert_eq!(on[100], Px::rgba(1.0, 1.0, 1.0, 1.0), "first flash phase is white");
    }
}
