//! How a layer's pixel combines with what is already on the strip.
use super::px::Px;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blend {
    /// Alpha-over: the layer covers what is below in proportion to its alpha.
    Normal,
    /// Add light (glows, sparks). Clamped at the end of the frame.
    Add,
    /// `1 - (1-dst)(1-src)`: brightens without blowing out.
    Screen,
    /// Per-channel maximum.
    Max,
}

/// Blend `src` (scaled by `opacity` and its own alpha) onto `dst`.
pub fn blend(dst: [f32; 3], src: Px, mode: Blend, opacity: f32) -> [f32; 3] {
    let a = (src.a * opacity).clamp(0.0, 1.0);
    let s = [src.r, src.g, src.b];
    let mut out = dst;
    for c in 0..3 {
        out[c] = match mode {
            Blend::Normal => dst[c] * (1.0 - a) + s[c] * a,
            Blend::Add => dst[c] + s[c] * a,
            Blend::Screen => 1.0 - (1.0 - dst[c]) * (1.0 - (s[c] * a).clamp(0.0, 1.0)),
            Blend::Max => dst[c].max(s[c] * a),
        };
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Px = Px::rgba(1.0, 0.0, 0.0, 1.0);

    #[test]
    fn normal_is_alpha_over() {
        assert_eq!(blend([0.0, 0.0, 1.0], RED, Blend::Normal, 1.0), [1.0, 0.0, 0.0]);
        assert_eq!(blend([0.0, 0.0, 1.0], RED, Blend::Normal, 0.5), [0.5, 0.0, 0.5]);
        assert_eq!(blend([0.2, 0.3, 0.4], Px::CLEAR, Blend::Normal, 1.0), [0.2, 0.3, 0.4]);
    }

    #[test]
    fn add_accumulates_and_screen_does_not_clip() {
        assert_eq!(blend([0.6, 0.0, 0.0], RED, Blend::Add, 1.0)[0], 1.6);
        let s = blend([0.6, 0.0, 0.0], RED, Blend::Screen, 1.0);
        assert!((s[0] - 1.0).abs() < 1e-6);
        let half = blend([0.5, 0.0, 0.0], Px::rgba(0.5, 0.0, 0.0, 1.0), Blend::Screen, 1.0);
        assert!((half[0] - 0.75).abs() < 1e-6, "{half:?}");
    }

    #[test]
    fn max_keeps_the_brighter_channel() {
        assert_eq!(blend([0.3, 0.9, 0.0], Px::rgba(0.6, 0.2, 0.0, 1.0), Blend::Max, 1.0), [0.6, 0.9, 0.0]);
    }

    #[test]
    fn opacity_scales_every_mode() {
        assert_eq!(blend([0.0; 3], RED, Blend::Add, 0.25)[0], 0.25);
        assert_eq!(blend([0.0; 3], RED, Blend::Max, 0.0)[0], 0.0);
    }
}
