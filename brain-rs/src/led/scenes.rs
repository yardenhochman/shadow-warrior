//! Scene constructors: which layers, in which order, for each arena state.
use super::layers::{
    Ember, EmberStyle, EnergyBar, EnergyBarStyle, PunchPulse, PunchStyle, ShoutGlow,
    ShoutGlowStyle, ShoutMeter, ShoutMeterStyle, ShoutRipple, ShoutRippleStyle,
};
use super::{Blend, ColumnLayout, Compositor, Inputs};

/// Most one-shot layers alive at once in warming (it spawns none today).
const MAX_ONESHOTS: usize = 32;
/// Most punch pulses alive at once per strip in a fight; the oldest is dropped beyond this.
pub const FIGHT_MAX_PULSES: usize = 8;

/// WARMING: the persistent energy bar with the live shout glow added around its head.
pub fn warming(len: usize) -> Compositor {
    warming_with(len, ShoutMeterStyle::default())
}

/// WARMING with an explicit shout meter look: bar, then the live shout meter (screened over the
/// bar, from the base up to `shout_level`), then the glow. Intensity 0 leaves the meter out.
pub fn warming_with(len: usize, meter: ShoutMeterStyle) -> Compositor {
    let mut c = Compositor::new(len, MAX_ONESHOTS);
    c.push(
        Box::new(EnergyBar::new(EnergyBarStyle::default())),
        Blend::Normal,
        1.0,
    );
    if meter.intensity > 0.0 {
        c.push(Box::new(ShoutMeter::new(meter)), Blend::Screen, 1.0);
    }
    c.push(
        Box::new(ShoutGlow::new(ShoutGlowStyle::default())),
        Blend::Add,
        1.0,
    );
    c
}

/// Everything tunable about the fight look.
#[derive(Clone, Debug)]
pub struct FightStyle {
    pub ember: EmberStyle,
    pub punch: PunchStyle,
    pub shout: ShoutRippleStyle,
    /// Optional thin energy bar under the fire (off by default).
    pub energy_bar: bool,
    pub energy_bar_style: EnergyBarStyle,
    /// Opacity of the energy bar when enabled.
    pub energy_bar_opacity: f32,
    pub max_pulses: usize,
    /// Live shout meter at the base of the column, under the ripple and the punch pulses.
    /// `None` (the default here) = no meter; the arena config enables it at a reduced intensity.
    pub shout_meter: Option<ShoutMeterStyle>,
}

impl Default for FightStyle {
    fn default() -> Self {
        let mut shout = ShoutRippleStyle::default();
        let punch = PunchStyle::default();
        shout.bag_height = punch.bag_height;
        FightStyle {
            ember: EmberStyle::default(),
            punch,
            shout,
            energy_bar: false,
            energy_bar_style: EnergyBarStyle::default(),
            energy_bar_opacity: 0.5,
            max_pulses: FIGHT_MAX_PULSES,
            shout_meter: None,
        }
    }
}

/// One column's FIGHT scene: ember base + optional energy bar + shout ripple (persistent) and
/// a punch pulse spawned per punch. Remembers the last punch id it spawned for, so the shared
/// `Inputs` can be handed to every strip's scene (and to the same scene twice) without
/// drawing a punch more than once.
pub struct FightScene {
    comp: Compositor,
    layout: ColumnLayout,
    style: FightStyle,
    last_punch: u64,
}

impl FightScene {
    pub fn new(layout: ColumnLayout, style: FightStyle) -> Self {
        let mut comp = Compositor::new(layout.leds, style.max_pulses);
        comp.push(
            Box::new(Ember::new(style.ember.clone())),
            Blend::Normal,
            1.0,
        );
        if style.energy_bar {
            comp.push(
                Box::new(EnergyBar::new(style.energy_bar_style.clone())),
                Blend::Normal,
                style.energy_bar_opacity,
            );
        }
        if let Some(m) = style.shout_meter.clone().filter(|m| m.intensity > 0.0) {
            comp.push(Box::new(ShoutMeter::new(m)), Blend::Screen, 1.0);
        }
        comp.push(
            Box::new(ShoutRipple::new(style.shout.clone())),
            Blend::Add,
            1.0,
        );
        FightScene {
            comp,
            layout,
            style,
            last_punch: 0,
        }
    }

    pub fn layout(&self) -> &ColumnLayout {
        &self.layout
    }

    pub fn active_pulses(&self) -> usize {
        self.comp.active_oneshots()
    }

    /// Spawn pulses for punches not seen yet, then render; `len * 3` RGB bytes, LED 0 = bottom.
    pub fn render(&mut self, dt: f32, inputs: &Inputs) -> &[u8] {
        let seen = self.last_punch;
        for p in inputs.punches.iter().filter(|p| p.id > seen) {
            self.last_punch = self.last_punch.max(p.id);
            let pulse = PunchPulse::new(
                &self.style.punch,
                p.power,
                self.layout.bag_distance,
                self.layout.leds,
            );
            self.comp.spawn(Box::new(pulse), Blend::Add, 1.0);
        }
        self.comp.render(dt, inputs)
    }
}

/// FIGHT with the default look for one column.
pub fn fight(len: usize, layout: ColumnLayout) -> FightScene {
    fight_with(len, layout, FightStyle::default())
}

/// FIGHT with an explicit style (e.g. the shout meter on).
pub fn fight_with(len: usize, layout: ColumnLayout, style: FightStyle) -> FightScene {
    FightScene::new(
        ColumnLayout {
            leds: len,
            ..layout
        },
        style,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::led::{preview, Punch};

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

    // ---- shout meter ----

    fn meter_inputs(energy: f32, shout_level: f32) -> Inputs {
        Inputs { energy, shout_level, ..Default::default() }
    }

    /// Render `frames` frames of warming and return the last one as RGB triples.
    fn warming_frame(len: usize, energy: f32, level: f32) -> Vec<[u8; 3]> {
        let mut c = warming(len);
        let mut out = Vec::new();
        for _ in 0..4 {
            out = c.render(DT, &meter_inputs(energy, level)).to_vec();
        }
        out.chunks(3).map(|p| [p[0], p[1], p[2]]).collect()
    }

    /// Meter pixels are the ones with real blue: the bar is red to amber (blue ~0).
    fn meter_lit(f: &[[u8; 3]]) -> usize {
        f.iter().filter(|p| p[2] > 128).count()
    }

    #[test]
    fn meter_lights_the_expected_number_of_leds_per_strip_over_the_bar() {
        for len in [180, 60] {
            for (level, want) in [(0.0, 0), (0.5, len / 2), (1.0, len)] {
                let f = warming_frame(len, 0.4, level);
                let n = meter_lit(&f);
                assert!(
                    (n as i32 - want as i32).abs() <= 3,
                    "len {len} level {level}: {n} lit, want ~{want}"
                );
                if want > 0 {
                    assert!(f[0][2] > 128, "grows from the base");
                    assert!(level == 1.0 || f[len - 1][2] < 20, "top stays dark");
                }
            }
        }
    }

    #[test]
    fn meter_colour_differs_from_the_bar_and_adds_no_more_than_its_own_colour() {
        let on = warming_frame(180, 0.1, 0.6); // bar 0..18, meter 0..108
        let wide = |p: [u8; 3]| p.map(i32::from);
        let (bar_only, meter_only) = (wide(warming_frame(180, 0.1, 0.0)[10]), wide(on[80]));
        assert!(on[10][2] > 128, "where they overlap the screened meter reads as light, not as bar");
        assert!(bar_only[0] > bar_only[2] + 100, "bar is red-ish {bar_only:?}");
        assert!(meter_only[2] > meter_only[0] + 100 && meter_only[1] > meter_only[0] + 50, "{meter_only:?}");
        // alone, deep inside the body, it is exactly its configured colour
        let alone = warming_frame(180, 0.0, 1.0);
        assert_eq!(alone[20], [0, 200, 255]);
        // over a full white-ish bar nothing exceeds the byte range or turns darker
        let over = warming_frame(180, 0.9, 1.0);
        let under = warming_frame(180, 0.9, 0.0);
        for (o, u) in over.iter().zip(&under) {
            assert!(o.iter().zip(u).all(|(a, b)| a >= b), "screen never darkens: {o:?} {u:?}");
        }
    }

    #[test]
    fn a_zero_intensity_meter_is_left_out_of_warming() {
        let mut c = warming_with(60, ShoutMeterStyle::new([0, 200, 255], 0.0));
        let f = c.render(DT, &meter_inputs(0.0, 1.0)).to_vec();
        assert!(f.iter().all(|&b| b == 0));
    }

    #[test]
    fn meter_on_a_multi_strip_controller_follows_each_strips_length_and_direction() {
        use crate::led::layout::append_strip;
        let strips = [ColumnLayout::new(0, 4, 60, false), ColumnLayout::new(1, 4, 180, true)];
        let mut comps: Vec<Compositor> = strips.iter().map(|s| warming(s.leds)).collect();
        let mut frame = Vec::new();
        for _ in 0..4 {
            frame.clear();
            for (c, s) in comps.iter_mut().zip(&strips) {
                append_strip(&mut frame, c.render(DT, &meter_inputs(0.0, 0.5)), s.reverse);
            }
        }
        let px: Vec<[u8; 3]> = frame.chunks(3).map(|p| [p[0], p[1], p[2]]).collect();
        let (a, b) = px.split_at(60);
        assert!((meter_lit(a) as i32 - 30).abs() <= 3 && a[0][2] > 128 && a[59][2] < 20, "forward strip: base = first LED");
        assert!((meter_lit(b) as i32 - 90).abs() <= 3 && b[179][2] > 128 && b[0][2] < 20, "reversed strip: base = last LED");
    }

    #[test]
    fn fight_meter_is_scaled_down_and_off_without_a_style() {
        let style = FightStyle {
            shout_meter: Some(ShoutMeterStyle::new([0, 200, 255], 0.5)),
            ..FightStyle::default()
        };
        let mut with = fight_with(180, ColumnLayout::single(180), style);
        let mut without = fight(180, ColumnLayout::single(180));
        let inp = meter_inputs(0.0, 1.0);
        let (mut w, mut wo) = (Vec::new(), Vec::new());
        for _ in 0..4 {
            w = with.render(DT, &inp).to_vec();
            wo = without.render(DT, &inp).to_vec();
        }
        assert_eq!(w[20 * 3 + 2], 128, "half-strength cyan (blue 255 * 0.5) over the dark base");
        assert!(wo.chunks(3).all(|p| p[2] < 60), "no meter without the style");
        assert!(w.chunks(3).map(|p| p[2]).max().unwrap() <= 128 + 40, "stays subtle");
    }

    // ---- fight ----

    const DT: f32 = 1.0 / 30.0;

    fn punch(power: f32, id: u64) -> Inputs {
        Inputs {
            punches: vec![Punch { power, id }],
            ..Default::default()
        }
    }

    fn brightest(f: &[u8]) -> u8 {
        *f.iter().max().unwrap()
    }

    fn first_bright_frame(layout: ColumnLayout, power: f32) -> usize {
        let mut sc = fight(180, layout);
        for k in 0..40 {
            let inp = if k == 0 {
                punch(power, 1)
            } else {
                Inputs::default()
            };
            if brightest(sc.render(DT, &inp)) > 150 {
                return k;
            }
        }
        panic!("never bright");
    }

    #[test]
    fn with_no_punch_only_the_dim_ember_shows() {
        let mut sc = fight(180, ColumnLayout::new(0, 5, 180, false));
        let mut peak = 0;
        for _ in 0..600 {
            peak = peak.max(brightest(sc.render(
                DT,
                &Inputs {
                    rage: 1.0,
                    ..Default::default()
                },
            )));
        }
        assert!(peak > 20, "ember is visible: {peak}");
        assert!(
            peak <= (0.5 * 255.0) as u8,
            "base stays under the cap: {peak}"
        );
        assert_eq!(sc.active_pulses(), 0);
    }

    #[test]
    fn a_punch_spawns_one_pulse_once_even_if_the_inputs_repeat() {
        let mut sc = fight(180, ColumnLayout::single(180));
        let inp = punch(60.0, 7);
        sc.render(DT, &inp);
        assert_eq!(sc.active_pulses(), 1);
        sc.render(DT, &inp);
        sc.render(DT, &inp);
        assert_eq!(sc.active_pulses(), 1, "same id is not spawned again");
        sc.render(DT, &punch(60.0, 8));
        assert_eq!(sc.active_pulses(), 2);
    }

    #[test]
    fn every_strip_gets_each_punch_exactly_once() {
        let layouts: Vec<_> = (0..5)
            .map(|c| ColumnLayout::new(c, 5, 180, false))
            .collect();
        let mut scenes: Vec<_> = layouts.iter().map(|l| fight(180, *l)).collect();
        for (k, power) in [30.0, 80.0, 55.0].into_iter().enumerate() {
            let inp = punch(power, k as u64 + 1);
            for s in scenes.iter_mut() {
                s.render(DT, &inp);
            }
            for s in scenes.iter_mut() {
                s.render(DT, &Inputs::default());
            }
        }
        assert!(scenes.iter().all(|s| s.active_pulses() == 3));
    }

    #[test]
    fn inner_column_lights_before_the_outer_one() {
        let inner = first_bright_frame(ColumnLayout::new(2, 5, 180, false), 70.0);
        let mid = first_bright_frame(ColumnLayout::new(1, 5, 180, false), 70.0);
        let outer = first_bright_frame(ColumnLayout::new(0, 5, 180, false), 70.0);
        assert!(inner < mid && mid < outer, "{inner} {mid} {outer}");
    }

    #[test]
    fn heavy_is_brighter_than_a_jab_and_pulses_end() {
        let layout = ColumnLayout::single(180);
        let energy = |power: f32| {
            let mut sc = fight(180, layout);
            let mut sum = 0u64;
            for k in 0..60 {
                let inp = if k == 0 {
                    punch(power, 1)
                } else {
                    Inputs::default()
                };
                sum += sc.render(DT, &inp).iter().map(|&b| b as u64).sum::<u64>();
            }
            assert_eq!(sc.active_pulses(), 0, "pulse ended within 2 s");
            sum
        };
        let (jab, heavy) = (energy(30.0), energy(95.0));
        assert!(heavy > jab * 2, "{jab} vs {heavy}");
        let mut sc = fight(180, layout);
        let jab_peak = brightest(sc.render(DT, &punch(30.0, 1)));
        let mut sc = fight(180, layout);
        assert!(brightest(sc.render(DT, &punch(95.0, 1))) > jab_peak);
    }

    #[test]
    fn pulse_cap_is_respected() {
        let mut sc = fight(180, ColumnLayout::single(180));
        for id in 1..=30 {
            sc.render(DT, &punch(60.0, id));
        }
        assert_eq!(sc.active_pulses(), FIGHT_MAX_PULSES);
    }

    #[test]
    fn shout_overlay_is_teal_and_punches_are_not() {
        let mut sc = fight(180, ColumnLayout::single(180));
        let mut f = Vec::new();
        for _ in 0..10 {
            f = sc
                .render(
                    DT,
                    &Inputs {
                        shout: 1.0,
                        ..Default::default()
                    },
                )
                .to_vec();
        }
        let bag = (0.45 * 179.0) as usize;
        let (r, g, b) = (
            f[bag * 3] as i32,
            f[bag * 3 + 1] as i32,
            f[bag * 3 + 2] as i32,
        );
        assert!(g > 60 && b > 50 && b > r + 30, "teal at the bag: {r} {g} {b}");
    }

    #[test]
    fn energy_bar_is_optional() {
        let inp = Inputs {
            energy: 1.0,
            ..Default::default()
        };
        let run = |bar: bool| {
            let style = FightStyle {
                energy_bar: bar,
                ..Default::default()
            };
            let mut sc = FightScene::new(ColumnLayout::single(180), style);
            let mut f = Vec::new();
            for _ in 0..40 {
                f = sc.render(DT, &inp).to_vec();
            }
            f[170 * 3..170 * 3 + 3]
                .iter()
                .map(|&b| b as u32)
                .sum::<u32>()
        };
        assert!(
            run(true) > run(false) + 40,
            "top of the strip lit only with the bar"
        );
    }

    fn fight_frames(layouts: &[ColumnLayout], seconds: f32) -> Vec<Vec<u8>> {
        // (time, power) and a shout at 5.2 s
        let punches = [
            (0.5, 30.0),
            (1.5, 60.0),
            (2.5, 95.0),
            (3.1, 40.0),
            (3.4, 50.0),
            (3.7, 85.0),
        ];
        let mut scenes: Vec<_> = layouts.iter().map(|l| fight(l.leds, *l)).collect();
        let mut frames = Vec::new();
        let (mut rage, mut id) = (0.0f32, 0u64);
        for f in 0..(seconds * 30.0) as usize {
            let t = f as f32 * DT;
            let mut inp = Inputs::default();
            for (t0, power) in punches {
                if t >= t0 && t - DT < t0 {
                    id += 1;
                    inp.punches.push(Punch { power, id });
                    rage = (rage + power / 100.0 * 0.35).min(1.0);
                }
            }
            rage *= (-DT / 4.0f32).exp();
            inp.rage = rage;
            if (5.2..6.4).contains(&t) {
                let age = t - 5.2;
                inp.shout = (1.0 - (-age * 40.0).exp()) * (-age / 0.5).exp();
            }
            // one row: all columns side by side with a dark gap
            let mut row = Vec::new();
            for s in scenes.iter_mut() {
                row.extend_from_slice(s.render(DT, &inp));
                row.extend_from_slice(&[40, 40, 40]);
            }
            frames.push(row);
        }
        frames
    }

    /// `LED_PREVIEW_OUT=/some/dir cargo test led::scenes::tests::render_fight -- --ignored`.
    /// Writes fight_columns.ppm (5 columns side by side, time down) and fight_strip.ppm
    /// (the centre column only, time across: rotate so LED 0 is at the bottom).
    #[test]
    #[ignore]
    fn render_fight_filmstrip() {
        let dir =
            std::env::var("LED_PREVIEW_OUT").expect("set LED_PREVIEW_OUT to an output directory");
        let layouts: Vec<_> = (0..5)
            .map(|c| ColumnLayout::new(c, 5, 180, false))
            .collect();
        let frames = fight_frames(&layouts, 7.0);
        let rows: Vec<Vec<u8>> = frames.iter().step_by(1).cloned().collect();
        preview::write_ppm(
            &std::path::Path::new(&dir).join("fight_columns.ppm"),
            &rows,
            1,
            2,
        )
        .unwrap();
        let rows: Vec<Vec<u8>> = frames
            .iter()
            .step_by(2)
            .map(|f| f[2 * 181 * 3..2 * 181 * 3 + 180 * 3].to_vec())
            .collect();
        preview::write_ppm(
            &std::path::Path::new(&dir).join("fight_strip.ppm"),
            &rows,
            5,
            3,
        )
        .unwrap();
    }

    #[test]
    fn fight_filmstrip_frames_are_sane() {
        let layouts: Vec<_> = (0..5)
            .map(|c| ColumnLayout::new(c, 5, 180, false))
            .collect();
        let frames = fight_frames(&layouts, 7.0);
        assert_eq!(frames.len(), 210);
        assert!(frames.iter().all(|f| f.len() == 5 * 181 * 3));
    }
}
