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

    // Set false by the schedule service while the game is scheduled off
    schedule_gate: Arc<std::sync::atomic::AtomicBool>,

    // Last fused presence. PresenceDetected fires only on change, so a player
    // who stays through COOLDOWN (or entered while SUSPENDED) needs this to warm up.
    present: Arc<std::sync::atomic::AtomicBool>,
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
            schedule_gate: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            present: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Share the schedule gate; presence is ignored while it reads false.
    pub fn with_schedule_gate(mut self, gate: Arc<std::sync::atomic::AtomicBool>) -> Self {
        self.schedule_gate = gate;
        self
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
            last_activity_at: self.last_activity_at.clone(),
            total_sessions: self.total_sessions.clone(),
            current_session_punches: self.current_session_punches.clone(),
            current_session_max_power: self.current_session_max_power.clone(),
            present: self.present.clone(),
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
        if detected && !self.schedule_gate.load(std::sync::atomic::Ordering::SeqCst) {
            info!("Presence ignored: game is scheduled off");
            return;
        }
        self.present.store(detected, std::sync::atomic::Ordering::SeqCst);
        let current_state = self.state_machine.current_state().await;

        if detected {
            // Player entered arena. Presence fires once, so go on to WARMING
            // right away instead of waiting in IDLE for a second event.
            if current_state == ArenaState::Suspended {
                info!("Presence detected, transitioning to IDLE then WARMING");
                if self.state_machine.transition(ArenaState::Idle).await.is_ok() {
                    let _ = self.state_machine.transition(ArenaState::Warming).await;
                }
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
            // A shout is activity: restart the WARMING timeout clock.
            *self.last_activity_at.write().await = Utc::now();
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
                if current_state == ArenaState::Warming {
                    // WARMING gives up after `warming_timeout_sec` without a shout (timer starts on
                    // entry and is reset by each shout). The player may still be standing there:
                    // PresenceDetected fires only on change and only COOLDOWN->IDLE re-arms
                    // WARMING, so the arena stays in IDLE until presence flips (leave and return)
                    // instead of bouncing IDLE -> WARMING -> IDLE forever.
                    let timeout = self.config.read().await.warming_timeout_sec;
                    let last = *self.last_activity_at.read().await;
                    if timeout > 0 && (Utc::now() - last).num_seconds() >= timeout as i64 {
                        info!("Warming timeout ({}s without a shout), back to IDLE", timeout);
                        let _ = self.state_machine.transition(ArenaState::Idle).await;
                    }
                    continue;
                }
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
            warming_timeout_sec: config.warming_timeout_sec,
            fight_duration_sec: config.fight_duration_sec,
            fight_inactivity_timeout_sec: config.fight_inactivity_timeout_sec,
            cooldown_duration_sec: config.cooldown_duration_sec,
            shout_sensitivity: audio_config.shout_sensitivity,
            vad_threshold: audio_config.vad_threshold,
            shout_margin_db: audio_config.shout_margin_db,
            shout_playback_margin_db: audio_config.shout_playback_margin_db,
            shout_min_duration_sec: audio_config.shout_min_duration_sec,
            shout_min_features: audio_config.shout_min_features,
            shout_gap_frames: audio_config.shout_gap_frames,
            shout_score_threshold: audio_config.shout_score_threshold,
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
        config.warming_timeout_sec = tunables.warming_timeout_sec;
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
        self.audio_manager.update_shout_tunables(&tunables).await;

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
    last_activity_at: Arc<RwLock<DateTime<Utc>>>,
    total_sessions: Arc<RwLock<u64>>,
    current_session_punches: Arc<RwLock<u32>>,
    current_session_max_power: Arc<RwLock<f32>>,
    present: Arc<std::sync::atomic::AtomicBool>,
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
                // WARMING timeout clock starts on entry (shared with the fight inactivity clock).
                *self.last_activity_at.write().await = Utc::now();
            }
            ArenaState::Fight => {
                let now = Utc::now();
                *self.fight_started_at.write().await = Some(now);
                // Inactivity clock starts with the fight, not at the last punch of
                // a previous fight (or process start), or the fight ends in 1s.
                *self.last_activity_at.write().await = now;
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
                let present = self.present.clone();
                tokio::spawn(async move {
                    let cooldown_duration = config_clone.read().await.cooldown_duration_sec;
                    sleep(std::time::Duration::from_secs(cooldown_duration)).await;
                    let current = state_machine.current_state().await;
                    if current == ArenaState::Cooldown {
                        info!("Cooldown complete, transitioning to IDLE");
                        let _ = state_machine.transition(ArenaState::Idle).await;
                        // A player still standing there gets the next round.
                        if present.load(std::sync::atomic::Ordering::SeqCst)
                            && state_machine.current_state().await == ArenaState::Idle
                        {
                            info!("Player still present, transitioning to WARMING");
                            let _ = state_machine.transition(ArenaState::Warming).await;
                        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::EventBus;
    use std::time::Duration;

    async fn wait_state(sm: &StateMachine, want: ArenaState) -> bool {
        for _ in 0..100 {
            if sm.current_state().await == want {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        false
    }

    /// Issue 9: presence fires only on change, so one "present" must reach
    /// WARMING from SUSPENDED, and a player who stays gets another round.
    #[tokio::test]
    async fn present_player_reaches_warming_and_gets_next_round() {
        let mut config = Config::default();
        config.arena.cooldown_duration_sec = 0;
        let bus: SharedEventBus = Arc::new(EventBus::new(100));
        let sm = Arc::new(StateMachine::new(bus.clone()));
        let audio = Arc::new(AudioManager::new(config.audio.clone(), bus.clone()));
        let ble = Arc::new(BleManager::new(config.ble.clone(), bus.clone()));
        let arena = Arc::new(ArenaManager::new(config, sm.clone(), bus.clone(), audio, ble));
        let (_tx, rx) = tokio::sync::mpsc::unbounded_channel();
        arena.clone().start(rx).await.unwrap();
        assert_eq!(sm.current_state().await, ArenaState::Suspended);

        bus.publish(Event::PresenceDetected { detected: true });
        assert!(wait_state(&sm, ArenaState::Warming).await, "one presence event reaches WARMING");

        sm.transition(ArenaState::Fight).await.unwrap();
        sm.transition(ArenaState::Cooldown).await.unwrap();
        assert!(wait_state(&sm, ArenaState::Warming).await, "still present after COOLDOWN -> WARMING");

        bus.publish(Event::PresenceDetected { detected: false });
        assert!(wait_state(&sm, ArenaState::Suspended).await);
    }

    /// WARMING times out to IDLE without shouts, and a present player is not bounced
    /// straight back into WARMING.
    #[tokio::test]
    async fn warming_times_out_to_idle_and_stays_there() {
        let mut config = Config::default();
        config.arena.warming_timeout_sec = 1;
        let bus: SharedEventBus = Arc::new(EventBus::new(100));
        let sm = Arc::new(StateMachine::new(bus.clone()));
        let audio = Arc::new(AudioManager::new(config.audio.clone(), bus.clone()));
        let ble = Arc::new(BleManager::new(config.ble.clone(), bus.clone()));
        let arena = Arc::new(ArenaManager::new(config, sm.clone(), bus.clone(), audio, ble));
        let (_tx, rx) = tokio::sync::mpsc::unbounded_channel();
        arena.clone().start(rx).await.unwrap();

        bus.publish(Event::PresenceDetected { detected: true });
        assert!(wait_state(&sm, ArenaState::Warming).await);
        assert!(wait_state_for(&sm, ArenaState::Idle, 4000).await, "times out to IDLE");
        tokio::time::sleep(Duration::from_millis(2500)).await;
        assert_eq!(sm.current_state().await, ArenaState::Idle, "present player does not loop back");

        // Presence flipping re-arms it.
        bus.publish(Event::PresenceDetected { detected: false });
        assert!(wait_state(&sm, ArenaState::Suspended).await);
        bus.publish(Event::PresenceDetected { detected: true });
        assert!(wait_state(&sm, ArenaState::Warming).await);
    }

    /// 0 disables the timeout.
    #[tokio::test]
    async fn warming_timeout_zero_is_off() {
        let mut config = Config::default();
        config.arena.warming_timeout_sec = 0;
        let bus: SharedEventBus = Arc::new(EventBus::new(100));
        let sm = Arc::new(StateMachine::new(bus.clone()));
        let audio = Arc::new(AudioManager::new(config.audio.clone(), bus.clone()));
        let ble = Arc::new(BleManager::new(config.ble.clone(), bus.clone()));
        let arena = Arc::new(ArenaManager::new(config, sm.clone(), bus.clone(), audio, ble));
        let (_tx, rx) = tokio::sync::mpsc::unbounded_channel();
        arena.clone().start(rx).await.unwrap();
        bus.publish(Event::PresenceDetected { detected: true });
        assert!(wait_state(&sm, ArenaState::Warming).await);
        tokio::time::sleep(Duration::from_millis(2500)).await;
        assert_eq!(sm.current_state().await, ArenaState::Warming);
    }

    async fn wait_state_for(sm: &StateMachine, want: ArenaState, ms: u64) -> bool {
        for _ in 0..ms / 20 {
            if sm.current_state().await == want {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        false
    }
}
