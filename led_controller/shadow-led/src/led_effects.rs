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
    level_pixels: u8,
    start_color: Srgb,
    end_color: Srgb,
    pixels: u8,
    current_value: u8,
    frames_per_pixels: f32, // speed of the animation, in frames. e.g. for 30 FPS, 15 means fillup the entire bar in 15 0.5 seconds.
    // If level is 0.5 and pixels is 15, then it will take 7.5 frames to fill up to level_pixels
    delta: u8,
}

impl EnergyBar {
    pub fn new(
        led_count: u8,
        start_color: Srgb,
        end_color: Srgb,
        start_level: f32,
        level: f32,
        frames_per_pixel: f32,
    ) -> Self {
        let target_pixels = ((level.clamp(0.0, 1.0) * led_count as f32).round()) as u8;
        let current_pixels = (start_level.clamp(0.0, 1.0) * led_count as f32).round() as u8;
        let delta = target_pixels - current_pixels;
        EnergyBar {
            level,
            level_pixels: target_pixels,
            start_color,
            end_color,
            pixels: led_count,
            current_value: current_pixels,
            frames_per_pixels: frames_per_pixel,
            delta: delta,
        }
    }
}

impl EffectIterator for EnergyBar {
    fn name(&self) -> &'static str {
        "EnergyBar"
    }

    fn next(&mut self) -> Option<std::vec::Vec<Srgb<u8>>> {
        if self.current_value >= self.level_pixels {
            return None;
        }

        let mut pixels_vec = Vec::with_capacity(self.pixels as usize);
        let next = self.current_value + self.delta;
        for i in 0..next {
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

            pixels_vec.push(Srgb::new(r8, g8, b8).into());
        }

        let pixel_color = pixels_vec;
        self.current_value = next;
        Some(pixel_color)
    }
}
