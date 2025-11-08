use smart_leds_trait::RGB8;

/// Type alias for transition functions that blend between two frames
pub type TransitionFn = fn(&[RGB8], &[RGB8], f32) -> Vec<RGB8>;

/// Linear crossfade transition between two frames
pub fn crossfade(from_frame: &[RGB8], to_frame: &[RGB8], progress: f32) -> Vec<RGB8> {
    assert_eq!(from_frame.len(), to_frame.len(), "Frame lengths must match");

    from_frame.iter().zip(to_frame.iter()).map(|(from, to)| {
        RGB8 {
            r: lerp(from.r, to.r, progress),
            g: lerp(from.g, to.g, progress),
            b: lerp(from.b, to.b, progress),
        }
    }).collect()
}

/// Linear interpolation between two u8 values
fn lerp(a: u8, b: u8, t: f32) -> u8 {
    ((a as f32) * (1.0 - t) + (b as f32) * t) as u8
}

/// Calculate transition progress (0.0 to 1.0) based on elapsed time
pub fn calculate_progress(elapsed_ms: u32, duration_ms: u32) -> f32 {
    if duration_ms == 0 {
        1.0
    } else {
        (elapsed_ms as f32 / duration_ms as f32).min(1.0)
    }
}

/// Check if transition is complete
pub fn is_transition_complete(elapsed_ms: u32, duration_ms: u32) -> bool {
    elapsed_ms >= duration_ms
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lerp() {
        assert_eq!(lerp(0, 255, 0.0), 0);
        assert_eq!(lerp(0, 255, 1.0), 255);
        assert_eq!(lerp(0, 255, 0.5), 127);
    }

    #[test]
    fn test_crossfade() {
        let from = vec![RGB8::new(255, 0, 0), RGB8::new(0, 255, 0)];
        let to = vec![RGB8::new(0, 255, 0), RGB8::new(0, 0, 255)];

        let result = crossfade(&from, &to, 0.5);

        assert_eq!(result.len(), 2);
        assert_eq!(result[0], RGB8::new(127, 127, 0)); // Halfway between red and green
        assert_eq!(result[1], RGB8::new(0, 127, 127)); // Halfway between green and blue
    }

    #[test]
    fn test_calculate_progress() {
        assert_eq!(calculate_progress(0, 1000), 0.0);
        assert_eq!(calculate_progress(500, 1000), 0.5);
        assert_eq!(calculate_progress(1000, 1000), 1.0);
        assert_eq!(calculate_progress(1500, 1000), 1.0); // Clamped
    }

    #[test]
    fn test_is_transition_complete() {
        assert!(!is_transition_complete(500, 1000));
        assert!(is_transition_complete(1000, 1000));
        assert!(is_transition_complete(1500, 1000));
    }
}