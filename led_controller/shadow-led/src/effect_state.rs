use smart_leds::RGB8;
use smart_led_effects::{strip::{EffectIterator, Breathe, Strobe, Meteor}, Srgb};
use palette::named;
use crate::transitions::EffectTransition;
use crate::led_effects::EnergyBar;

/// Simple idle effect that returns black LEDs
struct IdleEffect {
    led_count: usize,
}

impl IdleEffect {
    fn new(led_count: usize) -> Self {
        Self { led_count }
    }
}

impl EffectIterator for IdleEffect {
    fn name(&self) -> &'static str {
        "idle"
    }

    fn next(&mut self) -> Option<Vec<Srgb<u8>>> {
        Some(vec![Srgb::new(0, 0, 0); self.led_count])
    }
}

/// Simple effect that returns a static frame
struct StaticFrameEffect {
    frame: Vec<Srgb<u8>>,
}

impl StaticFrameEffect {
    fn new(frame: Vec<Srgb<u8>>) -> Self {
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
pub enum TransitionState {
    None,
    Active {
        transition_effect: Box<dyn EffectIterator>,
    },
}

/// Core state machine for LED effects
pub struct EffectState {
    pub mode: EffectMode,
    pub power_level: u8,           // 0-100 for energy bar
    pub target_power: u8,          // Target power for smooth animation
    pub transition_state: TransitionState,
    pub last_frame: Vec<RGB8>,     // Last rendered frame for transition capture

    // Effect iterator for current mode
    pub current_effect: Option<Box<dyn EffectIterator>>,

    // Optimization: track if state has changed since last render
    state_changed: bool,
}

impl EffectState {
    pub fn new(led_count: usize) -> Self {
        Self {
            mode: EffectMode::Idle,
            power_level: 0,
            target_power: 0,
            transition_state: TransitionState::None,
            last_frame: vec![RGB8::default(); led_count],
            current_effect: None,
            state_changed: false,
        }
    }

    /// Initiate a crossfade transition to a new mode
    pub fn transition_to(&mut self, new_mode: EffectMode, _current_time: u64) {
        self.mode = new_mode;
        self.state_changed = true;

        // Create new effect iterator for the mode
        let new_effect = self.create_effect_iterator(new_mode);

        // Create static frame effect from last frame
        let from_frame_srgb = crate::led_effects::vec_rgb8_to_vec_srgbu8(self.last_frame.clone());
        let from_effect = Box::new(StaticFrameEffect::new(from_frame_srgb));

        // Create crossfade transition (500ms at 30 FPS = 15 frames)
        let transition_frames = 15;
        let transition_effect = Box::new(EffectTransition::new(
            from_effect,
            new_effect,
            transition_frames,
        ));

        self.transition_state = TransitionState::Active {
            transition_effect,
        };

        // Set current_effect to the new effect (will be used after transition completes)
        self.current_effect = Some(self.create_effect_iterator(new_mode));
    }

    /// Set power level for energy bar (triggers smooth animation, not mode transition)
    pub fn set_power(&mut self, power: u8) {
        self.target_power = power.min(100);
        self.state_changed = true;
        // Note: Animation happens in update_frame()
    }

    /// Set mode instantly (no transition) - used for Idle
    pub fn set_mode_instant(&mut self, mode: EffectMode, _current_time: u64) {
        self.mode = mode;
        self.transition_state = TransitionState::None;
        self.state_changed = true;

        // Create new effect iterator for the mode
        self.current_effect = Some(self.create_effect_iterator(mode));
    }

    /// Create an effect iterator for the given mode
    fn create_effect_iterator(&self, mode: EffectMode) -> Box<dyn EffectIterator> {
        let led_count = self.last_frame.len();
        match mode {
            EffectMode::Idle => {
                // For idle, create a static black effect
                Box::new(IdleEffect::new(led_count))
            }
            EffectMode::Breathing => {
                // Use explicit u8 Srgb so component type satisfies numeric trait bounds
                Box::new(Breathe::new(led_count, Some(named::RED.into()), None))
            }
            EffectMode::EnergyBar => {
                Box::new(
                    EnergyBar::new(
                        led_count as u8,
                        named::WHITE.into(),
                        named::RED.into(),
                        self.power_level as f32 / 100.0,
                        self.power_level as f32 / 100.0,
                        15.0,
                    ))
            }
            EffectMode::Electricity => {
                // Use Strobe for electricity effect
                use std::time::Duration;
                Box::new(Strobe::new(led_count, Some(named::STEELBLUE.into()), Duration::from_millis(100), None))
            }
            EffectMode::EnergyPulse => {
                // Use Meteor for energy pulse
                Box::new(Meteor::new(led_count, Some(named::YELLOW.into()), None, None))
            }
        }
    }

    /// Render current state (transition if active, else current effect)
    pub fn render(&mut self) -> Option<Vec<Srgb<u8>>> {
        let result = match &mut self.transition_state {
            TransitionState::Active { transition_effect } => {
                // Use transition effect
                let result = transition_effect.next();
                if result.is_none() {
                    // Transition complete, switch to normal mode
                    self.transition_state = TransitionState::None;
                }
                result
            }
            TransitionState::None => {
                // Use current effect
                self.current_effect.as_mut().map(|effect| effect.next()).flatten()
            }
        };
        
        // Update last_frame for transition capture
        if let Some(ref frame) = result {
            self.last_frame = crate::led_effects::vec_srgbu8_to_vec_rgb8(frame.clone());
        }
        
        result
    }

}

#[cfg(test)]
mod tests {

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
        let mut state = EffectState::new(5);
        state.set_mode_instant(EffectMode::Idle, 0);
        let frame = state.render().unwrap();
        assert_eq!(frame.len(), 5);
        assert!(frame.iter().all(|&pixel| pixel == Srgb::new(0, 0, 0)));
    }

    #[test]
    fn test_render_optimization_no_change() {
        let mut state = EffectState::new(10);
        // Set to Idle mode (should not change)
        state.set_mode_instant(EffectMode::Idle, 0);
        
        // First render
        let frame1 = state.render().unwrap();
        
        // Second render without any state changes - should return cached frame
        let frame2 = state.render().unwrap();
        
        // Frames should be identical since state didn't change
        assert_eq!(frame1, frame2);
        assert_eq!(frame1.len(), 10);
        // All LEDs should be off in Idle mode
        assert!(frame1.iter().all(|&pixel| pixel == Srgb::new(0, 0, 0)));
    }

    #[test]
    fn test_render_optimization_with_change() {
        let mut state = EffectState::new(10);
        
        // First render in Idle
        let frame1 = state.render().unwrap();
        
        // Change power (should trigger re-render)
        state.set_power(50);
        state.transition_to(EffectMode::EnergyBar, 0);
        
        // Second render after state change
        let frame2 = state.render().unwrap();
        
        // Frames should be different
        assert_ne!(frame1, frame2);
    }
}