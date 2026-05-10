use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{interval, sleep};
use tracing::{info, warn};

use crate::config::{ArenaConfig, Config};
use crate::core::{
    ArenaState, Event, FightEndReason, SharedEventBus, StateMachine, StateHook,
};
use crate::models::{ArenaStateInfo, Statistics};
use crate::services::{AudioManager, BleManager, EnergyBar};

#[derive(Clone)]
pub struct ArenaManager {
    config: Arc<RwLock<ArenaConfig>>,
    state_machine: Arc<StateMachine>,
    event_bus: SharedEventBus,
    audio_manager: Arc<AudioManager>,
    ble_manager: Arc<BleManager>,

    // State-specific data
    energy_bar: Arc<RwLock<Option<EnergyBar>>>,
    fight_started_at: Arc<RwLock<Option<DateTime<Utc>>>>,
    last_activity_at: Arc<RwLock<DateTime<Utc>>>,

    // Statistics
    total_sessions: Arc<RwLock<u64>>,
    current_session_punches: Arc<RwLock<u32>>,
    current_session_max_power: Arc<RwLock<f32>>,
    started_at: DateTime<Utc>,
}

impl ArenaManager {
    pub fn new(
        config: Config,
        state_machine: Arc<StateMachine>,
        event_bus: SharedEventBus,
        audio_manager: Arc<AudioManager>,
        ble_manager: Arc<BleManager>,
    ) -> Self {
        Self {
            config: Arc::new(RwLock::new(config.arena)),
            state_machine,
            event_bus,
            audio_manager,
            ble_manager,
            energy_bar: Arc::new(RwLock::new(None)),
            fight_started_at: Arc::new(RwLock::new(None)),
            last_activity_at: Arc::new(RwLock::new(Utc::now())),
            total_sessions: Arc::new(RwLock::new(0)),
            current_session_punches: Arc::new(RwLock::new(0)),
            current_session_max_power: Arc::new(RwLock::new(0.0)),
            started_at: Utc::now(),
        }
    }

    /// Start the arena manager
    pub async fn start(
        self: Arc<Self>,
        mut command_rx: tokio::sync::mpsc::UnboundedReceiver<crate::api::status::ArenaCommand>,
    ) -> Result<()> {
        info!("Starting arena manager");

        // Handle API commands via channel
        let manager_for_commands = self.clone();
        tokio::spawn(async move {
            while let Some(cmd) = command_rx.recv().await {
                use crate::api::status::ArenaCommand;
                match cmd {
                    ArenaCommand::GetState(tx) => {
                        let state = manager_for_commands.get_state_info().await;
                        let _ = tx.send(state);
                    }
                    ArenaCommand::GetStatistics(tx) => {
                        let stats = manager_for_commands.get_statistics().await;
                        let _ = tx.send(stats);
                    }
                    ArenaCommand::ResetStatistics => {
                        manager_for_commands.reset_statistics().await;
                    }
                    ArenaCommand::GetTunables(tx) => {
                        let tunables = manager_for_commands.get_tunables().await;
                        let _ = tx.send(tunables);
                    }
                    ArenaCommand::UpdateTunables(tunables) => {
                        manager_for_commands.update_tunables(tunables).await;
                    }
                }
            }
        });

        // Register state hooks for bookkeeping and event publishing only.
        // Hardware effects are now handled by each manager's own event loop.
        let hook = ArenaStateHook {
            config: self.config.clone(),
            event_bus: self.event_bus.clone(),
            state_machine: self.state_machine.clone(),
            energy_bar: self.energy_bar.clone(),
            fight_started_at: self.fight_started_at.clone(),
            total_sessions: self.total_sessions.clone(),
            current_session_punches: self.current_session_punches.clone(),
            current_session_max_power: self.current_session_max_power.clone(),
        };
        self.state_machine
            .register_hook(Box::new(hook))
            .await;

        // Music playback is now handled in main.rs via event subscription
        // (moved out to avoid holding non-Send MusicManager reference)

        // Subscribe to events
        let mut event_rx = self.event_bus.subscribe();
        let manager = self.clone();

        tokio::spawn(async move {
            loop {
                match event_rx.recv().await {
                    Ok(event) => {
                        manager.handle_event(event).await;
                    }
                    Err(e) => {
                        warn!("Event receive error: {}", e);
                        break;
                    }
                }
            }
        });

        // Start background tasks
        self.clone().start_energy_decay_task();
        self.clone().start_fight_monitor_task();

        Ok(())
    }

    /// Handle incoming events
    async fn handle_event(&self, event: Event) {
        match event {
            Event::PresenceDetected { detected } => {
                self.handle_presence(detected).await;
            }
            Event::ShoutDetected {
                intensity,
                duration_sec: _,
                level_db: _,
            } => {
                self.handle_shout(intensity).await;
            }
            Event::PunchDetected { power, .. } => {
                self.handle_punch(power).await;
            }
            Event::EnergyFull => {
                self.handle_energy_full().await;
            }
            _ => {}
        }
    }

    /// Handle presence detection
    async fn handle_presence(&self, detected: bool) {
        let current_state = self.state_machine.current_state().await;

        if detected {
            // Player entered arena
            if current_state == ArenaState::Suspended {
                info!("Presence detected, transitioning to IDLE");
                let _ = self.state_machine.transition(ArenaState::Idle).await;
            } else if current_state == ArenaState::Idle {
                info!("Presence detected in IDLE, transitioning to WARMING");
                let _ = self.state_machine.transition(ArenaState::Warming).await;
            }
        } else {
            // Player left arena
            if current_state != ArenaState::Suspended {
                info!("Presence lost, transitioning to SUSPENDED");
                let _ = self.state_machine.transition(ArenaState::Suspended).await;
            }
        }
    }

    /// Handle shout detection
    async fn handle_shout(&self, intensity: f32) {
        let current_state = self.state_machine.current_state().await;

        if current_state == ArenaState::Warming {
            // Add energy to energy bar
            if let Some(energy_bar) = self.energy_bar.read().await.as_ref() {
                energy_bar.add_energy(intensity).await;
            }
        }
    }

    /// Handle punch detection
    async fn handle_punch(&self, power: f32) {
        let current_state = self.state_machine.current_state().await;

        if current_state == ArenaState::Fight {
            // Update statistics
            *self.current_session_punches.write().await += 1;

            let mut max_power = self.current_session_max_power.write().await;
            if power > *max_power {
                *max_power = power;
            }

            // Update last activity time
            *self.last_activity_at.write().await = Utc::now();

            info!("Punch detected: power={:.1}, total punches={}", power, *self.current_session_punches.read().await);
        }
    }

    /// Handle energy bar full
    async fn handle_energy_full(&self) {
        let current_state = self.state_machine.current_state().await;

        if current_state == ArenaState::Warming {
            info!("Energy bar full, transitioning to FIGHT");
            let _ = self.state_machine.transition(ArenaState::Fight).await;
        }
    }

    /// Start energy decay task (for WARMING state)
    fn start_energy_decay_task(self: Arc<Self>) {
        tokio::spawn(async move {
            let mut ticker = interval(std::time::Duration::from_millis(100)); // 10Hz update

            loop {
                ticker.tick().await;

                let current_state = self.state_machine.current_state().await;
                if current_state == ArenaState::Warming {
                    if let Some(energy_bar) = self.energy_bar.read().await.as_ref() {
                        energy_bar.decay(0.1).await; // 0.1 seconds
                    }
                }
            }
        });
    }

    /// Start fight monitor task (check duration and inactivity)
    fn start_fight_monitor_task(self: Arc<Self>) {
        tokio::spawn(async move {
            let mut ticker = interval(std::time::Duration::from_secs(1)); // Check every second

            loop {
                ticker.tick().await;

                let current_state = self.state_machine.current_state().await;
                if current_state == ArenaState::Fight {
                    let fight_started = self.fight_started_at.read().await;

                    if let Some(started_at) = *fight_started {
                        let elapsed = Utc::now() - started_at;

                        let config = self.config.read().await;
                        // Check if fight duration exceeded
                        if elapsed.num_seconds() >= config.fight_duration_sec as i64 {
                            info!("Fight duration elapsed, ending fight");
                            drop(config); // Release lock
                            drop(fight_started); // Release lock
                            self.event_bus.publish(Event::FightEnded {
                                reason: FightEndReason::TimeElapsed,
                                timestamp: Utc::now(),
                            });
                            let _ = self.state_machine.transition(ArenaState::Cooldown).await;
                            continue;
                        }

                        // Check for inactivity
                        let last_activity = *self.last_activity_at.read().await;
                        let inactive_duration = Utc::now() - last_activity;

                        if inactive_duration.num_seconds()
                            >= config.fight_inactivity_timeout_sec as i64
                        {
                            info!("Fight inactivity timeout, ending fight");
                            drop(config); // Release lock
                            drop(fight_started); // Release lock
                            self.event_bus.publish(Event::FightEnded {
                                reason: FightEndReason::InactivityTimeout,
                                timestamp: Utc::now(),
                            });
                            let _ = self.state_machine.transition(ArenaState::Cooldown).await;
                        }
                    }
                }
            }
        });
    }

    /// Get current arena state info
    pub async fn get_state_info(&self) -> ArenaStateInfo {
        let current_state = self.state_machine.current_state().await;
        let state_entered_at = self.state_machine.state_entered_at().await;
        let time_in_state = Utc::now() - state_entered_at;

        let config = self.config.read().await;
        let mut info = ArenaStateInfo {
            current_state,
            state_entered_at,
            time_in_state_sec: time_in_state.num_milliseconds() as f64 / 1000.0,
            energy: None,
            max_energy: None,
            energy_percentage: None,
            fight_elapsed_sec: None,
            fight_remaining_sec: None,
            time_since_last_activity_sec: None,
        };

        // Add state-specific data
        if current_state == ArenaState::Warming {
            if let Some(energy_bar) = self.energy_bar.read().await.as_ref() {
                let current = energy_bar.current().await;
                let max = config.warming_energy_threshold;
                info.energy = Some(current);
                info.max_energy = Some(max);
                info.energy_percentage = Some(current / max);
            }
        } else if current_state == ArenaState::Fight {
            if let Some(started_at) = *self.fight_started_at.read().await {
                let elapsed = (Utc::now() - started_at).num_milliseconds() as f64 / 1000.0;
                let remaining = (config.fight_duration_sec as f64 - elapsed).max(0.0);
                info.fight_elapsed_sec = Some(elapsed);
                info.fight_remaining_sec = Some(remaining);

                let last_activity = *self.last_activity_at.read().await;
                let inactive = (Utc::now() - last_activity).num_milliseconds() as f64 / 1000.0;
                info.time_since_last_activity_sec = Some(inactive);
            }
        }

        info
    }

    /// Get statistics
    pub async fn get_statistics(&self) -> Statistics {
        Statistics {
            total_sessions: *self.total_sessions.read().await,
            current_session_punches: *self.current_session_punches.read().await,
            current_session_max_power: *self.current_session_max_power.read().await,
            latest_acceleration: None, // TODO: Get from BLE manager via events
            latest_audio_level_db: None, // TODO: Get from AudioManager via events
            uptime_sec: (Utc::now() - self.started_at).num_milliseconds() as f64 / 1000.0,
        }
    }

    /// Reset session statistics
    pub async fn reset_statistics(&self) {
        info!("Resetting arena statistics");
        *self.total_sessions.write().await = 0;
        *self.current_session_punches.write().await = 0;
        *self.current_session_max_power.write().await = 0.0;
    }

    /// Get current game tunables
    pub async fn get_tunables(&self) -> crate::models::GameTunables {
        let config = self.config.read().await;
        let audio_config_lock = self.audio_manager.config();
        let audio_config = audio_config_lock.read().await;
        let ble_config_lock = self.ble_manager.config();
        let ble_config = ble_config_lock.read().await;
        
        crate::models::GameTunables {
            warming_energy_threshold: config.warming_energy_threshold,
            shout_energy_multiplier: config.shout_energy_multiplier,
            energy_decay_rate: config.energy_decay_rate,
            fight_duration_sec: config.fight_duration_sec,
            fight_inactivity_timeout_sec: config.fight_inactivity_timeout_sec,
            cooldown_duration_sec: config.cooldown_duration_sec,
            shout_threshold_db: audio_config.shout_threshold_db,
            shout_sensitivity: audio_config.shout_sensitivity,
            vad_threshold: audio_config.vad_threshold,
            punch_threshold: ble_config.threshold,
            punch_alpha: ble_config.alpha,
        }
    }

    /// Update game tunables
    pub async fn update_tunables(&self, tunables: crate::models::GameTunables) {
        info!("Updating game tunables");
        let mut config = self.config.write().await;
        
        config.warming_energy_threshold = tunables.warming_energy_threshold;
        config.shout_energy_multiplier = tunables.shout_energy_multiplier;
        config.energy_decay_rate = tunables.energy_decay_rate;
        config.fight_duration_sec = tunables.fight_duration_sec;
        config.fight_inactivity_timeout_sec = tunables.fight_inactivity_timeout_sec;
        config.cooldown_duration_sec = tunables.cooldown_duration_sec;

        // Update active energy bar if it exists
        if let Some(energy_bar) = self.energy_bar.read().await.as_ref() {
            energy_bar.update_parameters(
                tunables.warming_energy_threshold,
                tunables.shout_energy_multiplier,
                tunables.energy_decay_rate
            ).await;
        }

        // Update hardware managers
        self.audio_manager.update_parameters(
            tunables.shout_threshold_db,
            tunables.shout_sensitivity,
            tunables.vad_threshold
        ).await;

        let _ = self.ble_manager.update_parameters(
            tunables.punch_alpha,
            tunables.punch_threshold
        ).await;
    }
}

// State hook: bookkeeping and event publishing only.
// Hardware managers (WLED, Tasmota) react to StateChanged via their own event loops.
#[derive(Clone)]
struct ArenaStateHook {
    config: Arc<RwLock<ArenaConfig>>,
    event_bus: SharedEventBus,
    state_machine: Arc<StateMachine>,
    energy_bar: Arc<RwLock<Option<EnergyBar>>>,
    fight_started_at: Arc<RwLock<Option<DateTime<Utc>>>>,
    total_sessions: Arc<RwLock<u64>>,
    current_session_punches: Arc<RwLock<u32>>,
    current_session_max_power: Arc<RwLock<f32>>,
}

// Implement StateHook for the lightweight wrapper
#[async_trait]
impl StateHook for ArenaStateHook {
    async fn on_enter(&self, state: ArenaState) -> Result<(), Box<dyn std::error::Error>> {
        info!("Entering state: {}", state);

        // Hardware effects (WLED, Tasmota) are applied by each manager's
        // own event loop reacting to the StateChanged event published by
        // StateMachine after this hook returns.
        match state {
            ArenaState::Suspended => {}
            ArenaState::Idle => {
                self.event_bus.publish(Event::SessionStarted {
                    timestamp: Utc::now(),
                });
            }
            ArenaState::Warming => {
                let config = self.config.read().await;
                let energy_bar = EnergyBar::new(
                    config.warming_energy_threshold,
                    config.shout_energy_multiplier,
                    config.energy_decay_rate,
                    self.event_bus.clone(),
                );
                *self.energy_bar.write().await = Some(energy_bar);
            }
            ArenaState::Fight => {
                *self.fight_started_at.write().await = Some(Utc::now());
                *self.current_session_punches.write().await = 0;
                *self.current_session_max_power.write().await = 0.0;
                *self.total_sessions.write().await += 1;
                self.event_bus.publish(Event::FightStarted {
                    timestamp: Utc::now(),
                });
            }
            ArenaState::Cooldown => {
                self.event_bus.publish(Event::SessionEnded {
                    timestamp: Utc::now(),
                });
                let state_machine = self.state_machine.clone();
                let config_clone = self.config.clone();
                tokio::spawn(async move {
                    let cooldown_duration = config_clone.read().await.cooldown_duration_sec;
                    sleep(std::time::Duration::from_secs(cooldown_duration)).await;
                    let current = state_machine.current_state().await;
                    if current == ArenaState::Cooldown {
                        info!("Cooldown complete, transitioning to IDLE");
                        let _ = state_machine.transition(ArenaState::Idle).await;
                    }
                });
            }
        }

        Ok(())
    }

    async fn on_exit(&self, state: ArenaState) -> Result<(), Box<dyn std::error::Error>> {
        info!("Exiting state: {}", state);

        match state {
            ArenaState::Warming => {
                // Clear energy bar
                *self.energy_bar.write().await = None;
            }
            ArenaState::Fight => {
                // Clear fight start time
                *self.fight_started_at.write().await = None;
            }
            _ => {}
        }

        Ok(())
    }
}
