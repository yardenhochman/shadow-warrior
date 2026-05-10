use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

use super::events::{Event, SharedEventBus};

/// Arena states for the Shadow Warrior system
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ArenaState {
    /// System suspended, no activity
    Suspended,
    /// Waiting for player
    Idle,
    /// Player warming up, building energy
    Warming,
    /// Active fight session
    Fight,
    /// Cool down period after fight
    Cooldown,
}

impl fmt::Display for ArenaState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ArenaState::Suspended => write!(f, "SUSPENDED"),
            ArenaState::Idle => write!(f, "IDLE"),
            ArenaState::Warming => write!(f, "WARMING"),
            ArenaState::Fight => write!(f, "FIGHT"),
            ArenaState::Cooldown => write!(f, "COOLDOWN"),
        }
    }
}

impl FromStr for ArenaState {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "SUSPENDED" => Ok(ArenaState::Suspended),
            "IDLE" => Ok(ArenaState::Idle),
            "WARMING" => Ok(ArenaState::Warming),
            "FIGHT" => Ok(ArenaState::Fight),
            "COOLDOWN" => Ok(ArenaState::Cooldown),
            _ => Err(format!("Unknown state: {}", s)),
        }
    }
}

impl ArenaState {
    /// Check if a transition from this state to another is valid
    pub fn can_transition_to(&self, to: ArenaState) -> bool {
        use ArenaState::*;
        matches!(
            (self, to),
            // Suspended can transition to/from Idle
            (Suspended, Idle) | (Idle, Suspended) |
            // Normal flow
            (Idle, Warming) |
            (Warming, Fight) |
            (Fight, Cooldown) |
            (Cooldown, Idle) |
            // Emergency transitions
            (Warming, Idle) | // Can abort warming
            (Fight, Idle) |   // Can end fight early
            (_, Suspended)    // Can always suspend
        )
    }
}

/// State machine for managing arena states with lifecycle hooks
pub struct StateMachine {
    current_state: Arc<RwLock<ArenaState>>,
    state_entered_at: Arc<RwLock<DateTime<Utc>>>,
    event_bus: SharedEventBus,
    hooks: Arc<RwLock<Vec<Box<dyn StateHook>>>>,
}

impl StateMachine {
    /// Create a new state machine starting in Suspended state
    pub fn new(event_bus: SharedEventBus) -> Self {
        Self {
            current_state: Arc::new(RwLock::new(ArenaState::Suspended)),
            state_entered_at: Arc::new(RwLock::new(Utc::now())),
            event_bus,
            hooks: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Get the current state
    pub async fn current_state(&self) -> ArenaState {
        *self.current_state.read().await
    }

    /// Get the time when the current state was entered
    pub async fn state_entered_at(&self) -> DateTime<Utc> {
        *self.state_entered_at.read().await
    }

    /// Get the duration spent in the current state
    #[allow(dead_code)]
    pub async fn time_in_state(&self) -> Duration {
        let entered_at = self.state_entered_at().await;
        Utc::now() - entered_at
    }

    /// Register a state hook
    pub async fn register_hook(&self, hook: Box<dyn StateHook>) {
        self.hooks.write().await.push(hook);
    }

    /// Transition to a new state
    pub async fn transition(&self, to: ArenaState) -> Result<(), StateTransitionError> {
        let from = self.current_state().await;

        if from == to {
            return Ok(()); // Already in target state
        }

        if !from.can_transition_to(to) {
            return Err(StateTransitionError::InvalidTransition { from, to });
        }

        info!("State transition: {} -> {}", from, to);

        // Call on_exit hooks
        let hooks = self.hooks.read().await;
        for hook in hooks.iter() {
            if let Err(e) = hook.on_exit(from).await {
                warn!("Hook on_exit error during {} -> {}: {}", from, to, e);
            }
        }

        // Update state
        {
            let mut current = self.current_state.write().await;
            let mut entered_at = self.state_entered_at.write().await;
            *current = to;
            *entered_at = Utc::now();
        }

        // Publish state change event
        self.event_bus.publish(Event::state_changed(
            from.to_string(),
            to.to_string(),
        ));

        // Call on_enter hooks
        for hook in hooks.iter() {
            if let Err(e) = hook.on_enter(to).await {
                warn!("Hook on_enter error during {} -> {}: {}", from, to, e);
            }
        }

        // Call on_transition hooks
        for hook in hooks.iter() {
            if let Err(e) = hook.on_transition(from, to).await {
                warn!("Hook on_transition error during {} -> {}: {}", from, to, e);
            }
        }

        Ok(())
    }

    /// Force transition to a state without validation (for emergency stops)
    #[allow(dead_code)]
    pub async fn force_transition(&self, to: ArenaState) {
        let from = self.current_state().await;
        warn!("Force transition: {} -> {}", from, to);

        // Update state directly
        {
            let mut current = self.current_state.write().await;
            let mut entered_at = self.state_entered_at.write().await;
            *current = to;
            *entered_at = Utc::now();
        }

        // Publish event
        self.event_bus.publish(Event::state_changed(
            from.to_string(),
            to.to_string(),
        ));

        // Call hooks (best effort, ignore errors)
        let hooks = self.hooks.read().await;
        for hook in hooks.iter() {
            let _ = hook.on_exit(from).await;
            let _ = hook.on_enter(to).await;
            let _ = hook.on_transition(from, to).await;
        }
    }
}

/// Hook interface for state machine lifecycle events
#[async_trait]
pub trait StateHook: Send + Sync {
    /// Called when exiting a state
    async fn on_exit(&self, _state: ArenaState) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    /// Called when entering a state
    async fn on_enter(&self, _state: ArenaState) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    /// Called after a state transition completes
    async fn on_transition(
        &self,
        _from: ArenaState,
        _to: ArenaState,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
}

/// State transition errors
#[derive(Debug, thiserror::Error)]
pub enum StateTransitionError {
    #[error("Invalid transition from {from} to {to}")]
    InvalidTransition { from: ArenaState, to: ArenaState },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::EventBus;

    #[tokio::test]
    async fn test_state_machine_transitions() {
        let event_bus = Arc::new(EventBus::new(10));
        let sm = StateMachine::new(event_bus);

        // Start in Suspended
        assert_eq!(sm.current_state().await, ArenaState::Suspended);

        // Suspended -> Idle
        sm.transition(ArenaState::Idle).await.unwrap();
        assert_eq!(sm.current_state().await, ArenaState::Idle);

        // Idle -> Warming
        sm.transition(ArenaState::Warming).await.unwrap();
        assert_eq!(sm.current_state().await, ArenaState::Warming);

        // Warming -> Fight
        sm.transition(ArenaState::Fight).await.unwrap();
        assert_eq!(sm.current_state().await, ArenaState::Fight);

        // Fight -> Cooldown
        sm.transition(ArenaState::Cooldown).await.unwrap();
        assert_eq!(sm.current_state().await, ArenaState::Cooldown);

        // Cooldown -> Idle
        sm.transition(ArenaState::Idle).await.unwrap();
        assert_eq!(sm.current_state().await, ArenaState::Idle);
    }

    #[tokio::test]
    async fn test_invalid_transitions() {
        let event_bus = Arc::new(EventBus::new(10));
        let sm = StateMachine::new(event_bus);

        sm.transition(ArenaState::Idle).await.unwrap();

        // Invalid: Idle -> Fight (must go through Warming)
        let result = sm.transition(ArenaState::Fight).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_can_transition() {
        assert!(ArenaState::Idle.can_transition_to(ArenaState::Warming));
        assert!(ArenaState::Warming.can_transition_to(ArenaState::Fight));
        assert!(!ArenaState::Idle.can_transition_to(ArenaState::Fight));
        assert!(ArenaState::Fight.can_transition_to(ArenaState::Suspended)); // Emergency
    }
}
