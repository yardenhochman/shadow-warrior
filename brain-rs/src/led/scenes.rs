//! Scene constructors: which layers, in which order, for each arena state.
use super::layers::{EnergyBar, EnergyBarStyle, ShoutGlow, ShoutGlowStyle};
use super::{Blend, Compositor};

/// Most punch flashes alive at once (fight scenes; unused by warming).
const MAX_ONESHOTS: usize = 32;

/// WARMING: the persistent energy bar with the live shout glow added around its head.
pub fn warming(len: usize) -> Compositor {
    let mut c = Compositor::new(len, MAX_ONESHOTS);
    c.push(
        Box::new(EnergyBar::new(EnergyBarStyle::default())),
        Blend::Normal,
        1.0,
    );
    c.push(
        Box::new(ShoutGlow::new(ShoutGlowStyle::default())),
        Blend::Add,
        1.0,
    );
    c
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::led::{preview, Inputs};

    /// WARMING as the arena plays it: shouts raise the energy in steps (it drains very slowly,
    /// 0.5 of 100 points per second) and each shout is a short loud burst on the live level.
    fn warming_frames(seconds: f32, fps: f32) -> Vec<Vec<u8>> {
        // (time, energy gained 0..1)
        let shouts = [
            (1.0, 0.18),
            (3.0, 0.20),
            (4.6, 0.25),
            (7.0, 0.17),
            (9.5, 0.45),
        ];
        let mut c = warming(180);
        let (dt, mut energy, mut frames) = (1.0 / fps, 0.0f32, Vec::new());
        for f in 0..(seconds * fps) as usize {
            let t = f as f32 * dt;
            let mut shout = 0.0f32;
            for (t0, gain) in shouts {
                if t >= t0 {
                    let age = t - t0;
                    // a 0.9 s burst: fast attack, then a flutter, then decay
                    shout = shout.max(
                        (1.0 - (-age * 40.0).exp())
                            * (-age / 0.35).exp()
                            * (0.8 + 0.2 * (age * 25.0).sin()),
                    );
                    if age < dt {
                        energy = (energy + gain).min(1.0);
                    }
                }
            }
            energy = (energy - 0.005 * dt).max(0.0);
            frames.push(
                c.render(
                    dt,
                    &Inputs {
                        energy,
                        shout,
                        ..Default::default()
                    },
                )
                .to_vec(),
            );
        }
        frames
    }

    /// `LED_PREVIEW_OUT=/some/dir cargo test led::scenes -- --ignored`, then
    /// `ffmpeg -i warming.ppm warming.png` and open the image.
    #[test]
    #[ignore]
    fn render_warming_filmstrip() {
        let dir =
            std::env::var("LED_PREVIEW_OUT").expect("set LED_PREVIEW_OUT to an output directory");
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
        assert!(
            mid > 20 && late > mid,
            "bar grows with the shouts: {mid} -> {late}"
        );
        // the last shout fills the bar at 9.5 s; it flashes while the energy is still at 100%
        let flash = frames[(9.5 * 30.0) as usize..(10.5 * 30.0) as usize]
            .iter()
            .any(|f| f.iter().all(|&b| b == 255));
        assert!(flash, "full bar flashes white");
    }
}
