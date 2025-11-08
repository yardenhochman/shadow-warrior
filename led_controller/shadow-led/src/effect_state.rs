use smart_leds_trait::RGB8;
use crate::transitions::{TransitionFn, crossfade, calculate_progress, is_transition_complete};

/// Available LED effect modes
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EffectMode {
    Idle,
    Breathing,
    EnergyBar,
    Electricity,
    EnergyPulse,
}

/// State for active transitions
#[derive(Debug, Clone)]
pub enum TransitionState {
    None,
    Active {
        from_frame: Vec<RGB8>,
        start_time: u64,
        duration_ms: u32,
        transition_fn: TransitionFn,
    },
}

/// Core state machine for LED effects
pub struct EffectState {
    pub mode: EffectMode,
    pub power_level: u8,           // 0-100 for energy bar
    pub target_power: u8,          // Target power for smooth animation
    pub transition_state: TransitionState,
    pub last_frame: Vec<RGB8>,     // Last rendered frame for transition capture

    // Animation state
    pub frame_counter: u64,
    pub breathing_phase: f32,
    pub energy_bar_fill: f32,      // Current animated fill level (0.0-1.0)
    pub electricity_start_time: Option<u64>,
    pub energy_pulse_position: Option<usize>,
}

impl EffectState {
    pub fn new(led_count: usize) -> Self {
        Self {
            mode: EffectMode::Idle,
            power_level: 0,
            target_power: 0,
            transition_state: TransitionState::None,
            last_frame: vec![RGB8::default(); led_count],
            frame_counter: 0,
            breathing_phase: 0.0,
            energy_bar_fill: 0.0,
            electricity_start_time: None,
            energy_pulse_position: None,
        }
    }

    /// Initiate a crossfade transition to a new mode
    pub fn transition_to(&mut self, new_mode: EffectMode, current_time: u64) {
        // Capture current frame for transition
        let from_frame = self.last_frame.clone();

        self.transition_state = TransitionState::Active {
            from_frame,
            start_time: current_time,
            duration_ms: 500, // Configurable transition duration
            transition_fn: crossfade,
        };

        self.mode = new_mode;

        // Reset mode-specific state
        match new_mode {
            EffectMode::Idle => {
                // No special state
            }
            EffectMode::Breathing => {
                self.breathing_phase = 0.0;
            }
            EffectMode::EnergyBar => {
                // Keep current power level, but sync animated fill
                self.energy_bar_fill = self.power_level as f32 / 100.0;
            }
            EffectMode::Electricity => {
                self.electricity_start_time = Some(current_time);
            }
            EffectMode::EnergyPulse => {
                self.energy_pulse_position = Some(0);
            }
        }
    }

    /// Set power level for energy bar (triggers smooth animation, not mode transition)
    pub fn set_power(&mut self, power: u8) {
        self.target_power = power.min(100);
        // Note: Animation happens in update_frame()
    }

    /// Set mode instantly (no transition) - used for Idle
    pub fn set_mode_instant(&mut self, mode: EffectMode, _current_time: u64) {
        self.mode = mode;
        self.transition_state = TransitionState::None;

        // Reset mode-specific state
        match mode {
            EffectMode::Idle => {
                // Clear any running effects
                self.electricity_start_time = None;
                self.energy_pulse_position = None;
            }
            _ => {} // Other modes should use transition_to
        }
    }

    /// Update animation state for current frame
    pub fn update_frame(&mut self, current_time: u64, led_count: usize) {
        self.frame_counter += 1;

        // Update transition state
        if let TransitionState::Active { start_time, duration_ms, .. } = self.transition_state {
            if is_transition_complete((current_time - start_time) as u32, duration_ms) {
                self.transition_state = TransitionState::None;
            }
        }

        // Update mode-specific animation state
        match self.mode {
            EffectMode::Breathing => {
                // 5-second breathing cycle
                let cycle_frames = 5 * 60; // 60 FPS
                self.breathing_phase = (self.frame_counter % cycle_frames) as f32 / cycle_frames as f32;
            }
            EffectMode::EnergyBar => {
                // Smooth power level animation
                let target_fill = self.target_power as f32 / 100.0;
                let diff = target_fill - self.energy_bar_fill;
                if diff.abs() > 0.001 {
                    // Animate towards target over ~300ms (18 frames at 60 FPS)
                    let animation_speed = 0.055; // Adjust for smooth feel
                    self.energy_bar_fill += diff * animation_speed;
                    // Clamp to prevent overshoot
                    if (self.energy_bar_fill - target_fill).abs() < 0.01 {
                        self.energy_bar_fill = target_fill;
                        self.power_level = self.target_power;
                    }
                }
            }
            EffectMode::Electricity => {
                // Check for 20-second timeout
                if let Some(start_time) = self.electricity_start_time {
                    if current_time - start_time >= 20_000 {
                        // Auto-transition to Idle
                        self.set_mode_instant(EffectMode::Idle, current_time);
                    }
                }
            }
            EffectMode::EnergyPulse => {
                // Move pulse along strip
                if let Some(pos) = self.energy_pulse_position {
                    if pos < led_count {
                        self.energy_pulse_position = Some(pos + 1);
                    } else {
                        // Pulse complete, auto-transition to Idle
                        self.set_mode_instant(EffectMode::Idle, current_time);
                    }
                }
            }
            EffectMode::Idle => {
                // No animation state to update
            }
        }
    }

    /// Render current state (transition if active, else current effect)
    pub fn render(&mut self, led_count: usize, current_time: u64) -> Vec<RGB8> {
        // Get the target frame for current mode
        let target_frame = self.render_mode_frame(led_count);

        // Apply transition if active
        let final_frame = match &self.transition_state {
            TransitionState::None => target_frame,
            TransitionState::Active { from_frame, start_time, duration_ms, transition_fn } => {
                let elapsed = (current_time - start_time) as u32;
                let progress = calculate_progress(elapsed, *duration_ms);
                transition_fn(from_frame, &target_frame, progress)
            }
        };

        // Store for next transition
        self.last_frame = final_frame.clone();
        final_frame
    }

    /// Render a single frame for the current mode (no transition)
    fn render_mode_frame(&self, led_count: usize) -> Vec<RGB8> {
        match self.mode {
            EffectMode::Idle => self.render_idle(led_count),
            EffectMode::Breathing => self.render_breathing(led_count),
            EffectMode::EnergyBar => self.render_energy_bar(led_count),
            EffectMode::Electricity => self.render_electricity(led_count),
            EffectMode::EnergyPulse => self.render_energy_pulse(led_count),
        }
    }

    fn render_idle(&self, led_count: usize) -> Vec<RGB8> {
        vec![RGB8::default(); led_count]
    }

    fn render_breathing(&self, led_count: usize) -> Vec<RGB8> {
        let envelope = ((2.0 * std::f32::consts::PI * self.breathing_phase).sin() + 1.0) / 2.0;
        let brightness = (160.0 * envelope) as u8; // Max brightness 160

        vec![RGB8 { r: brightness, g: 0, b: 0 }; led_count]
    }

    fn render_energy_bar(&self, led_count: usize) -> Vec<RGB8> {
        let mut pixels = vec![RGB8::default(); led_count];
        let num_leds = (led_count as f32 * self.energy_bar_fill) as usize;

        // Create gradient from white (start) to red (end)
        for i in 0..num_leds.min(led_count) {
            let ratio = if num_leds > 1 {
                i as f32 / (num_leds - 1) as f32
            } else {
                0.0
            };
            pixels[i] = RGB8 {
                r: 255,
                g: (255.0 * (1.0 - ratio)) as u8,
                b: (255.0 * (1.0 - ratio)) as u8,
            };
        }

        pixels
    }

    fn render_electricity(&self, led_count: usize) -> Vec<RGB8> {
        let mut pixels = vec![RGB8::default(); led_count];

        // Simple lightning effect: random sparks and bolts
        // This is a placeholder - full implementation would be more complex
        let time_seed = (self.frame_counter / 10) as u32; // Change every 10 frames

        // Add some random sparks
        for i in 0..15 { // 15 sparks
            let pos = ((time_seed.wrapping_mul(7).wrapping_add(i * 13)) % led_count as u32) as usize;
            let brightness = ((time_seed.wrapping_add(i)) % 200 + 55) as u8; // 55-255
            pixels[pos] = RGB8 {
                r: brightness,
                g: brightness / 2,
                b: brightness,
            };
        }

        // Add a traveling bolt
        let bolt_pos = (self.frame_counter / 3 % led_count as u64) as usize; // Move every 3 frames
        if bolt_pos < led_count {
            pixels[bolt_pos] = RGB8::new(255, 255, 255);
            // Add some spread
            if bolt_pos > 0 {
                pixels[bolt_pos - 1] = RGB8::new(200, 200, 255);
            }
            if bolt_pos < led_count - 1 {
                pixels[bolt_pos + 1] = RGB8::new(200, 200, 255);
            }
        }

        pixels
    }

    fn render_energy_pulse(&self, led_count: usize) -> Vec<RGB8> {
        let mut pixels = vec![RGB8::default(); led_count];

        if let Some(pos) = self.energy_pulse_position {
            if pos < led_count {
                pixels[pos] = RGB8::new(255, 0, 0);
                // Add some trail
                if pos > 0 {
                    pixels[pos - 1] = RGB8::new(128, 0, 0);
                }
                if pos > 1 {
                    pixels[pos - 2] = RGB8::new(64, 0, 0);
                }
            }
        }

        pixels
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_effect_state() {
        let state = EffectState::new(10);
        assert_eq!(state.mode, EffectMode::Idle);
        assert_eq!(state.power_level, 0);
        assert_eq!(state.last_frame.len(), 10);
    }

    #[test]
    fn test_set_power() {
        let mut state = EffectState::new(10);
        state.set_power(75);
        assert_eq!(state.target_power, 75);
    }

    #[test]
    fn test_render_idle() {
        let state = EffectState::new(5);
        let frame = state.render_idle(5);
        assert_eq!(frame.len(), 5);
        assert!(frame.iter().all(|&pixel| pixel == RGB8::default()));
    }

    #[test]
    fn test_render_energy_bar() {
        let mut state = EffectState::new(10);
        state.energy_bar_fill = 0.5; // 50%
        let frame = state.render_energy_bar(10);
        assert_eq!(frame.len(), 10);
        // First 5 LEDs should be lit
        assert!(frame[0] != RGB8::default());
        assert!(frame[4] != RGB8::default());
        assert_eq!(frame[5], RGB8::default());
    }
}