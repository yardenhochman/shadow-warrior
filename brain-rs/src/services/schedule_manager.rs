//! Schedule service: enables/disables the game by daily time windows.
//!
//! While the game is scheduled off the arena is forced to SUSPENDED and
//! presence detection is ignored (via a shared gate). The clock is injectable
//! so everything is unit-testable without real time.

use chrono::{Datelike, NaiveDateTime, NaiveTime, Weekday};
use serde::Serialize;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::info;

use crate::config::{ScheduleConfig, ScheduleWindow};
use crate::core::{ArenaState, Event, SharedEventBus, StateMachine};
use crate::services::database_manager::DatabaseManager;

const SETTING_KEY: &str = "schedule";

/// Source of local wall-clock time.
pub trait Clock: Send + Sync {
    fn now(&self) -> NaiveDateTime;
}

pub struct LocalClock;

impl Clock for LocalClock {
    fn now(&self) -> NaiveDateTime {
        chrono::Local::now().naive_local()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ScheduleStatus {
    pub config: ScheduleConfig,
    pub active: bool,
}

struct ParsedWindow {
    days: Vec<Weekday>,
    start: NaiveTime,
    end: NaiveTime,
}

fn parse_time(s: &str) -> Result<NaiveTime, String> {
    NaiveTime::parse_from_str(s, "%H:%M").map_err(|_| format!("invalid time '{}', expected HH:MM", s))
}

fn parse_window(w: &ScheduleWindow) -> Result<ParsedWindow, String> {
    let days = w
        .days
        .iter()
        .map(|d| Weekday::from_str(d).map_err(|_| format!("invalid day '{}'", d)))
        .collect::<Result<Vec<_>, _>>()?;
    let start = parse_time(&w.start)?;
    let end = parse_time(&w.end)?;
    if start == end {
        return Err(format!("window {}-{} is empty", w.start, w.end));
    }
    Ok(ParsedWindow { days, start, end })
}

fn parse_config(cfg: &ScheduleConfig) -> Result<Vec<ParsedWindow>, String> {
    cfg.windows.iter().map(parse_window).collect()
}

impl ParsedWindow {
    fn runs_on(&self, day: Weekday) -> bool {
        self.days.is_empty() || self.days.contains(&day)
    }

    /// A window belongs to the day it starts on; overnight windows spill over
    /// into the next calendar day.
    fn contains(&self, now: NaiveDateTime) -> bool {
        let t = now.time();
        let day = now.date().weekday();
        if self.start < self.end {
            self.runs_on(day) && t >= self.start && t < self.end
        } else {
            (self.runs_on(day) && t >= self.start) || (self.runs_on(day.pred()) && t < self.end)
        }
    }
}

/// Whether the game is active at `now` under `cfg`.
/// Disabled schedule or no windows => always active.
pub fn is_active(cfg: &ScheduleConfig, now: NaiveDateTime) -> Result<bool, String> {
    if !cfg.enabled || cfg.windows.is_empty() {
        return Ok(true);
    }
    Ok(parse_config(cfg)?.iter().any(|w| w.contains(now)))
}

pub struct ScheduleManager {
    config: Mutex<ScheduleConfig>,
    clock: Arc<dyn Clock>,
    state_machine: Arc<StateMachine>,
    event_bus: SharedEventBus,
    gate: Arc<AtomicBool>,
    /// Last applied activity (None until first tick).
    last_active: Mutex<Option<bool>>,
    db: Option<Arc<DatabaseManager>>,
}

impl ScheduleManager {
    pub fn new(
        config: ScheduleConfig,
        clock: Arc<dyn Clock>,
        state_machine: Arc<StateMachine>,
        event_bus: SharedEventBus,
    ) -> Self {
        Self {
            config: Mutex::new(config),
            clock,
            state_machine,
            event_bus,
            gate: Arc::new(AtomicBool::new(true)),
            last_active: Mutex::new(None),
            db: None,
        }
    }

    /// Persist API edits in `db` and restore the saved schedule, which
    /// overrides the config.yaml one. An unreadable saved schedule is ignored.
    pub async fn with_persistence(mut self, db: Arc<DatabaseManager>) -> Self {
        match db.get_setting(SETTING_KEY).await {
            Ok(Some(json)) => match serde_json::from_str::<ScheduleConfig>(&json) {
                Ok(cfg) if parse_config(&cfg).is_ok() => {
                    info!("Schedule: restored saved config");
                    self.config = Mutex::new(cfg);
                }
                _ => tracing::warn!("Ignoring invalid saved schedule"),
            },
            Ok(None) => {}
            Err(e) => tracing::warn!("Could not load saved schedule: {}", e),
        }
        self.db = Some(db);
        self
    }

    /// Check a config without applying it; used to fail fast on startup.
    pub fn validate(cfg: &ScheduleConfig) -> Result<(), String> {
        parse_config(cfg).map(|_| ())
    }

    /// Gate shared with the arena manager: false while scheduled off.
    pub fn gate(&self) -> Arc<AtomicBool> {
        self.gate.clone()
    }

    pub async fn status(&self) -> ScheduleStatus {
        let config = self.config.lock().await.clone();
        let active = is_active(&config, self.clock.now()).unwrap_or(true);
        ScheduleStatus { config, active }
    }

    /// Validate and replace the schedule, then apply it immediately.
    pub async fn update_config(&self, cfg: ScheduleConfig) -> Result<(), String> {
        parse_config(&cfg)?;
        if let Some(db) = &self.db {
            let json = serde_json::to_string(&cfg).map_err(|e| e.to_string())?;
            db.set_setting(SETTING_KEY, &json)
                .await
                .map_err(|e| format!("failed to persist schedule: {e}"))?;
        }
        *self.config.lock().await = cfg;
        self.tick().await;
        Ok(())
    }

    /// Evaluate the schedule once and apply any change.
    pub async fn tick(&self) {
        let cfg = self.config.lock().await.clone();
        let active = match is_active(&cfg, self.clock.now()) {
            Ok(a) => a,
            Err(e) => {
                tracing::warn!("Invalid schedule, treating game as active: {}", e);
                true
            }
        };

        let mut last = self.last_active.lock().await;
        let previous = *last;
        *last = Some(active);
        drop(last);
        if previous == Some(active) {
            return;
        }

        self.gate.store(active, Ordering::SeqCst);
        self.event_bus.publish(Event::ScheduleChanged { active });
        let current = self.state_machine.current_state().await;
        if active {
            info!("Schedule: game enabled");
            if previous.is_some() && current == ArenaState::Suspended {
                let _ = self.state_machine.transition(ArenaState::Idle).await;
            }
        } else {
            info!("Schedule: game disabled, suspending arena");
            let _ = self.state_machine.transition(ArenaState::Suspended).await;
        }
    }

    /// Spawn the periodic evaluation loop.
    pub fn start(self: Arc<Self>) {
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(15));
            loop {
                ticker.tick().await;
                self.tick().await;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::EventBus;
    use chrono::NaiveDate;
    use std::sync::Mutex as StdMutex;

    struct FakeClock(StdMutex<NaiveDateTime>);
    impl FakeClock {
        fn at(y: i32, m: u32, d: u32, h: u32, min: u32) -> Arc<Self> {
            Arc::new(Self(StdMutex::new(dt(y, m, d, h, min))))
        }
        fn set(&self, t: NaiveDateTime) {
            *self.0.lock().unwrap() = t;
        }
    }
    impl Clock for FakeClock {
        fn now(&self) -> NaiveDateTime {
            *self.0.lock().unwrap()
        }
    }

    fn dt(y: i32, m: u32, d: u32, h: u32, min: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d).unwrap().and_hms_opt(h, min, 0).unwrap()
    }

    fn win(days: &[&str], start: &str, end: &str) -> ScheduleWindow {
        ScheduleWindow {
            days: days.iter().map(|s| s.to_string()).collect(),
            start: start.into(),
            end: end.into(),
        }
    }

    fn cfg(windows: Vec<ScheduleWindow>) -> ScheduleConfig {
        ScheduleConfig { enabled: true, windows }
    }

    // 2025-01-06 is a Monday.
    #[test]
    fn simple_window() {
        let c = cfg(vec![win(&[], "09:00", "17:00")]);
        assert!(!is_active(&c, dt(2025, 1, 6, 8, 59)).unwrap());
        assert!(is_active(&c, dt(2025, 1, 6, 9, 0)).unwrap());
        assert!(is_active(&c, dt(2025, 1, 6, 16, 59)).unwrap());
        assert!(!is_active(&c, dt(2025, 1, 6, 17, 0)).unwrap());
    }

    #[test]
    fn day_filter() {
        let c = cfg(vec![win(&["mon"], "09:00", "17:00")]);
        assert!(is_active(&c, dt(2025, 1, 6, 10, 0)).unwrap());
        assert!(!is_active(&c, dt(2025, 1, 7, 10, 0)).unwrap());
    }

    #[test]
    fn overnight_window_spills_to_next_day() {
        let c = cfg(vec![win(&["fri"], "22:00", "02:00")]);
        assert!(is_active(&c, dt(2025, 1, 10, 23, 0)).unwrap()); // Fri night
        assert!(is_active(&c, dt(2025, 1, 11, 1, 59)).unwrap()); // Sat early
        assert!(!is_active(&c, dt(2025, 1, 11, 2, 0)).unwrap());
        assert!(!is_active(&c, dt(2025, 1, 10, 1, 0)).unwrap()); // Fri early: Thu not allowed
        assert!(!is_active(&c, dt(2025, 1, 11, 23, 0)).unwrap()); // Sat night
    }

    #[test]
    fn disabled_or_empty_is_always_active() {
        let mut c = cfg(vec![win(&[], "09:00", "10:00")]);
        c.enabled = false;
        assert!(is_active(&c, dt(2025, 1, 6, 3, 0)).unwrap());
        assert!(is_active(&cfg(vec![]), dt(2025, 1, 6, 3, 0)).unwrap());
    }

    #[test]
    fn invalid_config_rejected() {
        assert!(is_active(&cfg(vec![win(&[], "25:00", "10:00")]), dt(2025, 1, 6, 3, 0)).is_err());
        assert!(is_active(&cfg(vec![win(&["xyz"], "09:00", "10:00")]), dt(2025, 1, 6, 3, 0)).is_err());
        assert!(is_active(&cfg(vec![win(&[], "09:00", "09:00")]), dt(2025, 1, 6, 3, 0)).is_err());
    }

    async fn setup(c: ScheduleConfig, clock: Arc<FakeClock>) -> (Arc<ScheduleManager>, Arc<StateMachine>) {
        let bus = Arc::new(EventBus::new(16));
        let sm = Arc::new(StateMachine::new(bus.clone()));
        sm.transition(ArenaState::Idle).await.unwrap();
        (Arc::new(ScheduleManager::new(c, clock, sm.clone(), bus)), sm)
    }

    #[tokio::test]
    async fn tick_suspends_and_resumes() {
        let clock = FakeClock::at(2025, 1, 6, 8, 0);
        let (m, sm) = setup(cfg(vec![win(&[], "09:00", "17:00")]), clock.clone()).await;
        let gate = m.gate();

        m.tick().await; // off at startup
        assert_eq!(sm.current_state().await, ArenaState::Suspended);
        assert!(!gate.load(Ordering::SeqCst));

        clock.set(dt(2025, 1, 6, 9, 0));
        m.tick().await;
        assert_eq!(sm.current_state().await, ArenaState::Idle);
        assert!(gate.load(Ordering::SeqCst));

        clock.set(dt(2025, 1, 6, 17, 0));
        m.tick().await;
        assert_eq!(sm.current_state().await, ArenaState::Suspended);
    }

    #[tokio::test]
    async fn startup_inside_window_keeps_state() {
        let clock = FakeClock::at(2025, 1, 6, 10, 0);
        let (m, sm) = setup(cfg(vec![win(&[], "09:00", "17:00")]), clock).await;
        m.tick().await;
        assert_eq!(sm.current_state().await, ArenaState::Idle);
    }

    #[tokio::test]
    async fn publishes_event_only_on_change() {
        let clock = FakeClock::at(2025, 1, 6, 8, 0);
        let bus = Arc::new(EventBus::new(16));
        let sm = Arc::new(StateMachine::new(bus.clone()));
        let mut rx = bus.subscribe();
        let m = ScheduleManager::new(cfg(vec![win(&[], "09:00", "17:00")]), clock, sm, bus);
        m.tick().await;
        m.tick().await;
        let mut changes = 0;
        while let Ok(e) = rx.try_recv() {
            if matches!(e, Event::ScheduleChanged { active: false }) {
                changes += 1;
            }
        }
        assert_eq!(changes, 1);
    }

    #[tokio::test]
    async fn update_config_validates_and_applies() {
        let clock = FakeClock::at(2025, 1, 6, 8, 0);
        let (m, sm) = setup(ScheduleConfig::default(), clock).await;
        m.tick().await;
        assert_eq!(sm.current_state().await, ArenaState::Idle);

        assert!(m.update_config(cfg(vec![win(&[], "bad", "10:00")])).await.is_err());
        assert_eq!(sm.current_state().await, ArenaState::Idle);

        m.update_config(cfg(vec![win(&[], "09:00", "17:00")])).await.unwrap();
        assert_eq!(sm.current_state().await, ArenaState::Suspended);
        assert!(!m.status().await.active);
    }

    #[tokio::test]
    async fn persisted_schedule_survives_restart() {
        let db = Arc::new(DatabaseManager::new("sqlite::memory:").await.unwrap());
        let clock = FakeClock::at(2025, 1, 6, 8, 0);
        let (m, _sm) = setup(ScheduleConfig::default(), clock.clone()).await;
        let m = Arc::try_unwrap(m).ok().unwrap().with_persistence(db.clone()).await;
        m.update_config(cfg(vec![win(&[], "09:00", "17:00")])).await.unwrap();

        // "Restart": fresh manager with the default config picks up the saved one.
        let (m2, _sm2) = setup(ScheduleConfig::default(), clock).await;
        let m2 = Arc::try_unwrap(m2).ok().unwrap().with_persistence(db).await;
        assert_eq!(m2.status().await.config.windows.len(), 1);
    }

    #[tokio::test]
    async fn invalid_saved_schedule_is_ignored() {
        let db = Arc::new(DatabaseManager::new("sqlite::memory:").await.unwrap());
        db.set_setting(SETTING_KEY, r#"{"enabled":true,"windows":[{"days":[],"start":"bad","end":"10:00"}]}"#)
            .await
            .unwrap();
        let clock = FakeClock::at(2025, 1, 6, 8, 0);
        let (m, _sm) = setup(ScheduleConfig::default(), clock).await;
        let m = Arc::try_unwrap(m).ok().unwrap().with_persistence(db).await;
        assert!(m.status().await.config.windows.is_empty());
    }

    #[test]
    fn validate_rejects_bad_config() {
        assert!(ScheduleManager::validate(&cfg(vec![win(&[], "25:00", "10:00")])).is_err());
        assert!(ScheduleManager::validate(&cfg(vec![win(&[], "09:00", "17:00")])).is_ok());
    }
}
