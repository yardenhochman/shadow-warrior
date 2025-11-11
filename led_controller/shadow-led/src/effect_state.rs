use std::fmt::Display;

use crate::command_handler::LedCommand;
use crate::led_effects::EmptyEffect;
use crate::led_effects::EnergyBar;
use palette::named;
use smart_led_effects::{
    strip::{Breathe, EffectIterator, Meteor, Strobe},
    Srgb,
};

/// Available LED effect modes
pub enum EffectMode {
    Idle(EmptyEffect),
    Breathing(Breathe),
    EnergyBar(EnergyBar),
    Electricity(Meteor),
    EnergyPulse(Strobe),
}

impl Display for EffectMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EffectMode::Idle(_) => write!(f, "Idle"),
            EffectMode::Breathing(_) => write!(f, "Breathing"),
            EffectMode::EnergyBar(_) => write!(f, "EnergyBar"),
            EffectMode::Electricity(_) => write!(f, "Electricity"),
            EffectMode::EnergyPulse(_) => write!(f, "EnergyPulse"),
        }
    }
}

/// Core state machine for LED effects
pub struct EffectState {
    pub mode: EffectMode,
    pub last_frame: Vec<Srgb<u8>>, // Last rendered frame for transition capture
}

impl EffectState {
    pub fn new(led_count: usize) -> Self {
        Self {
            mode: EffectMode::Idle(EmptyEffect {}),
            last_frame: vec![palette::named::BLACK; led_count],
        }
    }

    pub fn next_frame(&mut self) -> Option<Vec<Srgb<u8>>> {
        let effect: &mut dyn EffectIterator = match &mut self.mode {
            EffectMode::Idle(effect) => effect,
            EffectMode::Breathing(effect) => effect,
            EffectMode::EnergyBar(effect) => effect,
            EffectMode::Electricity(effect) => effect,
            EffectMode::EnergyPulse(effect) => effect,
        };
        effect.next()
    }

    /// Initiate a crossfade transition to a new mode
    pub fn transition_to(&mut self, command: LedCommand) {
        self.mode = match command {
            LedCommand::EnergyBar(power) => {
                let current_power = if let EffectMode::EnergyBar(ref energy_bar) = self.mode {
                    energy_bar.level
                } else {
                    0.0
                };
                EffectMode::EnergyBar(EnergyBar::new(
                    self.last_frame.len(),
                    named::WHITE.into(),
                    named::RED.into(),
                    current_power,
                    power as f32 / 100.0,
                    15.0,
                ))
            }
            LedCommand::Breathing => EffectMode::Breathing(Breathe::new(
                self.last_frame.len(),
                Some(named::RED.into()),
                None,
            )),
            LedCommand::Idle => EffectMode::Idle(EmptyEffect {}),
            LedCommand::Electricity => EffectMode::Electricity(Meteor::new(
                self.last_frame.len(),
                Some(named::YELLOW.into()),
                None,
                None,
            )),
            LedCommand::EnergyPulse => EffectMode::EnergyPulse(Strobe::new(
                self.last_frame.len(),
                Some(named::STEELBLUE.into()),
                std::time::Duration::from_millis(100),
                None,
            )),
        };
        // Create new effect iterator for the mode        // Set the transition effect as the current effect iterator
    }
}
