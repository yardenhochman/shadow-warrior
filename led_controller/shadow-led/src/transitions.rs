use smart_led_effects::strip::EffectIterator;
use palette::Srgb;


pub struct EffectTransition {
    from: Box<dyn EffectIterator>,
    to: Box<dyn EffectIterator>,
    duration_frames: u8,
    position: u8,
}

impl EffectTransition {
    pub fn new(from: Box<dyn EffectIterator>, to: Box<dyn EffectIterator>, duration_frames: u8) -> Self {
        Self { from, to, duration_frames, position: 0 }
    }
}

impl EffectIterator for EffectTransition {
    fn name(&self) -> &'static str {
        "effect_transition"
    }

    fn next(&mut self) -> Option<Vec<Srgb<u8>>> {
        if self.position >= self.duration_frames {
            // Transition complete
            return None;
        }
        
        let from_frame = self.from.next()?;
        let to_frame = self.to.next()?;
        if to_frame.is_empty() && from_frame.is_empty() {
            return None;
        }
        let t = self.position as f32 / self.duration_frames as f32;
        self.position += 1;
        
        Some(from_frame.iter().zip(to_frame.iter()).map(|(from_pixel, to_pixel)| {
            let r = (from_pixel.red as f32 * (1.0 - t) + to_pixel.red as f32 * t) as u8;
            let g = (from_pixel.green as f32 * (1.0 - t) + to_pixel.green as f32 * t) as u8;
            let b = (from_pixel.blue as f32 * (1.0 - t) + to_pixel.blue as f32 * t) as u8;
            Srgb::new(r, g, b)
        })
        .collect())
    }
}
