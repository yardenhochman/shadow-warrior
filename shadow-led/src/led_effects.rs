use log;
use palette::Srgb;
use smart_led_effects::strip::EffectIterator;
use smart_leds::RGB8;
pub const FRAME_RATE: u32 = 30; // 30 FPS
pub const FRAME_DURATION_MS: u32 = 1000 / FRAME_RATE;

pub fn srgbu8_to_rgb8(input: &Srgb<u8>) -> RGB8 {
    RGB8::new(input.red, input.green, input.blue)
}

pub fn rgb8_to_srgbu8(input: RGB8) -> Srgb<u8> {
    Srgb::new(input.r, input.g, input.b)
}

pub fn vec_srgbu8_to_vec_rgb8(input: &Vec<Srgb<u8>>) -> Vec<RGB8> {
    input.into_iter().map(srgbu8_to_rgb8).collect()
}

pub fn vec_rgb8_to_vec_srgbu8(input: Vec<RGB8>) -> Vec<Srgb<u8>> {
    input.into_iter().map(rgb8_to_srgbu8).collect()
}


/// Simple effect that returns a static frame
pub struct StaticFrameEffect{
    frame: Vec<Srgb<u8>>,
}

impl StaticFrameEffect {
    pub fn new(frame: Vec<Srgb<u8>>) -> Self {
        Self { frame }
    }
}

impl EffectIterator for StaticFrameEffect {
    fn name(&self) -> &'static str {
        "static_frame"
    }

    fn next(&mut self) -> Option<Vec<Srgb<u8>>> {
        Some(self.frame.clone())
    }
}

pub struct EmptyEffect;

impl EffectIterator for EmptyEffect {
    fn name(&self) -> &'static str {
        "empty"
    }

    fn next(&mut self) -> Option<Vec<Srgb<u8>>> {
        None
    }
}

pub struct PerpetuateEffect{
    perpetuated: Box<dyn EffectIterator>,
    last_frame: Vec<Srgb<u8>>,
}

impl PerpetuateEffect {
    pub fn new(perpetuated: Box<dyn EffectIterator>) -> Self {
        Self { perpetuated, last_frame: vec![] }
    }
}

impl EffectIterator for PerpetuateEffect {
    fn name(&self) -> &'static str {
        "perpetuate"
    }

    fn next(&mut self) -> Option<Vec<Srgb<u8>>> {
        match self.perpetuated.next() {
            Some(frame) => {
                self.last_frame = frame;
                Some(self.last_frame.clone())
            },
            None => Some(self.last_frame.clone()),
        }
    }
}
 
pub struct EnergyBar {
    pub level: f32,
    level_pixels: usize,
    start_color: Srgb,
    end_color: Srgb,
    pixels: usize,
    current_value: usize,
    pixels_per_frame: f32, // effect speed in pixels per frame, for the entire bar
}

impl EnergyBar {
    /// Create a new EnergyBar effect
    /// - `pixels`: total number of pixels in the strip
    /// - `start_color`: color at the start of the bar
    /// - `end_color`: color at the end of the bar
    /// - `start_level`: initial level (0.0 to 1.0)
    /// - `level`: target level (0.0 to 1.0)
    /// - `speed_frames`: number of frames to reach from 0 to full
    pub fn new(
        pixels: usize,
        start_color: Srgb,
        end_color: Srgb,
        start_level: f32,
        level: f32,
        speed_frames: f32,
    ) -> Self {
        let target_pixels = ((level.clamp(0.0, 1.0) * pixels as f32).round()) as usize;
        let current_pixels = (start_level.clamp(0.0, 1.0) * pixels as f32).round() as usize;
        assert!(speed_frames > 0.0, "speed_frames must be greater than 0");
        let pixels_per_frame = pixels as f32 / speed_frames;
        EnergyBar {
            level,
            level_pixels: target_pixels,
            start_color,
            end_color,
            pixels,
            current_value: current_pixels,
            pixels_per_frame,
        }
    }
}

impl EffectIterator for EnergyBar {
    fn name(&self) -> &'static str {
        "EnergyBar"
    }

    fn next(&mut self) -> Option<std::vec::Vec<Srgb<u8>>> {
        if self.current_value == self.level_pixels {
            return None;
        }

        let mut pixels_vec = Vec::with_capacity(self.pixels);
        let next = if self.current_value > self.level_pixels {
           (self.current_value - self.pixels_per_frame.round() as usize).max(self.level_pixels)
        } else {
            (self.current_value + self.pixels_per_frame.round() as usize).min(self.level_pixels)
        };
        for i in 0..self.pixels {
            let color = if i > next {
                palette::named::BLACK
            } else {
                let t = i as f32 / self.pixels as f32;
                let sr = self.start_color.red;
                let sg = self.start_color.green;
                let sb = self.start_color.blue;
                let er = self.end_color.red;
                let eg = self.end_color.green;
                let eb = self.end_color.blue;

                let r = (sr * (1.0 - t) + er * t).clamp(0.0, 1.0);
                let g = (sg * (1.0 - t) + eg * t).clamp(0.0, 1.0);
                let b = (sb * (1.0 - t) + eb * t).clamp(0.0, 1.0);

                let r8 = (r * 255.0).round() as u8;
                let g8 = (g * 255.0).round() as u8;
                let b8 = (b * 255.0).round() as u8;
                Srgb::new(r8, g8, b8)
            };
            pixels_vec.push(color.into());
        }

        let pixel_color = pixels_vec;
        self.current_value = next;
        Some(pixel_color)
    }
}

/// Effect wrapper that adjusts the brightness of pixels from an inner effect
pub struct BrightnessFilterEffect {
    inner: Box<dyn EffectIterator>,
    brightness_factor: f32,
}

impl BrightnessFilterEffect {
    /// Create a new brightness filter effect
    /// - `inner`: the effect to wrap and adjust
    /// - `brightness_factor`: factor to multiply brightness by (0.0 to 1.0 for dimming, >1.0 for brightening)
    pub fn new(inner: Box<dyn EffectIterator>, brightness_factor: f32) -> Self {
        Self {
            inner,
            brightness_factor: brightness_factor.max(0.0),
        }
    }
}

impl EffectIterator for BrightnessFilterEffect {
    fn name(&self) -> &'static str {
        "brightness_filter"
    }

    fn next(&mut self) -> Option<Vec<Srgb<u8>>> {
        self.inner.next().map(|frame| {
            frame
                .iter()
                .map(|pixel| {
                    let r = ((pixel.red as f32) * self.brightness_factor).min(255.0) as u8;
                    let g = ((pixel.green as f32) * self.brightness_factor).min(255.0) as u8;
                    let b = ((pixel.blue as f32) * self.brightness_factor).min(255.0) as u8;
                    Srgb::new(r, g, b)
                })
                .collect()
        })
    }
}

/// Effect wrapper that applies gamma correction to pixels from an inner effect
pub struct GammaCorrectionEffect {
    inner: Box<dyn EffectIterator>,
    gamma: f32,
}

impl GammaCorrectionEffect {
    /// Create a new gamma correction effect
    /// - `inner`: the effect to wrap and adjust
    /// - `gamma`: gamma value (typical range 1.5 to 2.5 for display correction)
    ///   - gamma > 1.0: brightens the image (compensates for dark display)
    ///   - gamma < 1.0: darkens the image
    ///   - gamma = 1.0: no effect
    pub fn new(inner: Box<dyn EffectIterator>, gamma: f32) -> Self {
        Self {
            inner,
            gamma: gamma.max(0.1), // clamp to avoid division issues
        }
    }

    /// Apply gamma correction to a single color value (0-255)
    fn apply_gamma(value: u8, gamma: f32) -> u8 {
        let normalized = value as f32 / 255.0; // normalize to 0.0-1.0
        let corrected = normalized.powf(1.0 / gamma); // apply gamma correction
        (corrected * 255.0).min(255.0) as u8 // scale back to 0-255
    }
}

impl EffectIterator for GammaCorrectionEffect {
    fn name(&self) -> &'static str {
        "gamma_correction"
    }

    fn next(&mut self) -> Option<Vec<Srgb<u8>>> {
        self.inner.next().map(|frame| {
            frame
                .iter()
                .map(|pixel| {
                    let r = Self::apply_gamma(pixel.red, self.gamma);
                    let g = Self::apply_gamma(pixel.green, self.gamma);
                    let b = Self::apply_gamma(pixel.blue, self.gamma);
                    Srgb::new(r, g, b)
                })
                .collect()
        })
    }
}
