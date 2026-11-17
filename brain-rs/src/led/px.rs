//! Pixel type and small animation helpers.

/// One pixel: display-space colour in 0..1 plus straight (non-premultiplied) alpha.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Px {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Px {
    /// Fully transparent: draws nothing.
    pub const CLEAR: Px = Px { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };

    pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Px {
        Px { r, g, b, a }
    }

    /// Opaque colour.
    pub fn opaque(c: [f32; 3]) -> Px {
        Px { r: c[0], g: c[1], b: c[2], a: 1.0 }
    }

    pub fn with_alpha(self, a: f32) -> Px {
        Px { a, ..self }
    }

    pub fn rgb(self) -> [f32; 3] {
        [self.r, self.g, self.b]
    }
}

/// Linear interpolation of two colours.
pub fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0);
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

/// Exponential smoother with separate rising (`attack`) and falling (`release`) time
/// constants in seconds. A zero constant means "jump".
#[derive(Clone, Copy, Debug)]
pub struct Smoother {
    pub value: f32,
    pub attack: f32,
    pub release: f32,
}

impl Smoother {
    pub fn new(attack: f32, release: f32) -> Self {
        Smoother { value: 0.0, attack, release }
    }

    pub fn step(&mut self, target: f32, dt: f32) -> f32 {
        let tau = if target > self.value { self.attack } else { self.release };
        let k = if tau <= 0.0 { 1.0 } else { 1.0 - (-dt / tau).exp() };
        self.value += (target - self.value) * k;
        self.value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoother_rises_fast_and_falls_slow() {
        let mut s = Smoother::new(0.1, 1.0);
        for _ in 0..15 {
            s.step(1.0, 1.0 / 30.0); // 0.5 s
        }
        assert!(s.value > 0.95, "attack: {}", s.value);
        let peak = s.value;
        for _ in 0..15 {
            s.step(0.0, 1.0 / 30.0); // 0.5 s
        }
        assert!(s.value > 0.5 * peak && s.value < peak, "release is slower: {}", s.value);
    }

    #[test]
    fn smoother_zero_constant_jumps() {
        let mut s = Smoother::new(0.0, 0.0);
        assert_eq!(s.step(0.7, 0.033), 0.7);
        assert_eq!(s.step(0.0, 0.033), 0.0);
    }

    #[test]
    fn lerp_clamps() {
        assert_eq!(lerp3([0.0; 3], [1.0; 3], 2.0), [1.0; 3]);
        assert_eq!(lerp3([0.0; 3], [1.0; 3], -1.0), [0.0; 3]);
    }
}
