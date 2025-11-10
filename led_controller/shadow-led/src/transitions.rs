use smart_led_effects::strip::EffectIterator;
use crate::led_effects::PerpetuateEffect;
use palette::Srgb;



pub struct CrossfadeTransitionEffect {
    from: Box<dyn EffectIterator>,
    to: Box<dyn EffectIterator>,
    duration_frames: u8,
    position: u8,
}

impl CrossfadeTransitionEffect {
    pub fn new(from: Box<dyn EffectIterator>, to: Box<dyn EffectIterator>, duration_frames: u8) -> Self {
        Self { from: Box::new(PerpetuateEffect::new(from)), to, duration_frames, position: 0 }
    }
}

impl EffectIterator for CrossfadeTransitionEffect {
    fn name(&self) -> &'static str {
        "crossfade"
    }

    fn next(&mut self) -> Option<Vec<Srgb<u8>>> {
        if self.position >= self.duration_frames {
            // Transition complete
            return self.to.next();
        }
        
        let from_frame = self.from.next()?;
        let to_frame = self.to.next()?;

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
