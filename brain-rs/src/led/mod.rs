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
//! Not wired into `wled_manager` yet (shadow-warrior-19b).
#![allow(dead_code, unused_imports)]

mod blend;
mod compositor;
mod layer;
pub mod layers;
mod px;

#[cfg(test)]
mod preview;

pub use blend::{blend, Blend};
pub use compositor::Compositor;
pub use layer::{Ctx, Inputs, Layer, Punch};
pub use px::{lerp3, Px, Smoother};

#[cfg(test)]
mod scenes {
    use super::layers::{EnergyBar, EnergyBarStyle, ShoutGlow, ShoutGlowStyle};
    use super::*;

    /// WARMING as the arena plays it: shouts raise the energy in steps (it drains very slowly,
    /// 0.5 of 100 points per second) and each shout is a short loud burst on the live level.
    pub fn warming_frames(seconds: f32, fps: f32) -> Vec<Vec<u8>> {
        // (time, energy gained 0..1)
        let shouts = [(1.0, 0.18), (3.0, 0.20), (4.6, 0.25), (7.0, 0.17), (9.5, 0.45)];
        let mut c = Compositor::new(180, 32);
        c.push(Box::new(EnergyBar::new(EnergyBarStyle::default())), Blend::Normal, 1.0);
        c.push(Box::new(ShoutGlow::new(ShoutGlowStyle::default())), Blend::Add, 1.0);
        let (dt, mut energy, mut frames) = (1.0 / fps, 0.0f32, Vec::new());
        for f in 0..(seconds * fps) as usize {
            let t = f as f32 * dt;
            let mut shout = 0.0f32;
            for (t0, gain) in shouts {
                if t >= t0 {
                    let age = t - t0;
                    // a 0.9 s burst: fast attack, then a flutter, then decay
                    shout = shout.max((1.0 - (-age * 40.0).exp()) * (-age / 0.35).exp() * (0.8 + 0.2 * (age * 25.0).sin()));
                    if age < dt {
                        energy = (energy + gain).min(1.0);
                    }
                }
            }
            energy = (energy - 0.005 * dt).max(0.0);
            frames.push(c.render(dt, &Inputs { energy, shout, ..Default::default() }).to_vec());
        }
        frames
    }

    /// `LED_PREVIEW_OUT=/some/dir cargo test led::scenes -- --ignored`, then
    /// `ffmpeg -i warming.ppm warming.png` and open the image.
    #[test]
    #[ignore]
    fn render_warming_filmstrip() {
        let dir = std::env::var("LED_PREVIEW_OUT").expect("set LED_PREVIEW_OUT to an output directory");
        let frames = warming_frames(13.0, 30.0);
        // one row per 2 frames (15 rows/s), 5 px per LED
        let rows: Vec<Vec<u8>> = frames.iter().step_by(2).cloned().collect();
        preview::write_ppm(&std::path::Path::new(&dir).join("warming.ppm"), &rows, 5, 3).unwrap();
    }

    #[test]
    fn warming_scene_runs_and_ends_on_a_full_bar() {
        let frames = warming_frames(13.0, 30.0);
        assert_eq!(frames.len(), 390);
        assert!(frames[0].iter().all(|&b| b == 0), "starts dark");
        let lit = |f: &[u8]| f.chunks(3).filter(|p| p[0] > 20).count();
        let mid = lit(&frames[(2.5 * 30.0) as usize]);
        let late = lit(&frames[(8.5 * 30.0) as usize]);
        assert!(mid > 20 && late > mid, "bar grows with the shouts: {mid} -> {late}");
        // the last shout fills the bar at 9.5 s; it flashes while the energy is still at 100%
        let flash = frames[(9.5 * 30.0) as usize..(10.5 * 30.0) as usize].iter().any(|f| f.iter().all(|&b| b == 255));
        assert!(flash, "full bar flashes white");
    }
}
