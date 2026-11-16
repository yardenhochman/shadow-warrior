//! Hardware-free integration tests: the manager graph `main()` builds, driven
//! through the event bus, with WLED controllers and Tasmota plugs replaced by
//! mock HTTP devices. BLE and the microphone are not started; their events
//! (`PresenceDetected`, `ShoutDetected`, `PunchDetected`) are published by hand,
//! exactly as the real services publish them.
//!
//! Covers the arena loop end to end: presence -> WARMING (energy from shouts)
//! -> FIGHT (punches) -> COOLDOWN -> next round -> SUSPENDED, and what each
//! state does to the LEDs and plugs.
use std::sync::Arc;
use std::time::Duration;

use crate::config::{Config, TasmotaPlug, WledEffect};
use crate::core::status_registry::StatusRegistry;
use crate::core::{ArenaState, Event, EventBus, SharedEventBus, StateMachine};
use crate::services::{
    ArenaManager, AudioManager, BleManager, DatabaseManager, TasmotaManager, WledManager,
};
use crate::test_util::{dead_addr, mock_http, Seen};

/// Marker colour per state, so the LED request log shows which effect was sent.
const IDLE_RGB: u8 = 1;
const WARMING_RGB: u8 = 2;
const FIGHT_RGB: u8 = 3;
const COOLDOWN_RGB: u8 = 4;

struct Station {
    bus: SharedEventBus,
    sm: Arc<StateMachine>,
    arena: Arc<ArenaManager>,
    registry: Arc<StatusRegistry>,
    // Keeps the API command channel open for the arena manager.
    _api_tx: tokio::sync::mpsc::UnboundedSender<crate::api::status::ArenaCommand>,
}

fn plug(name: &str, ip: &str, on_states: &[&str]) -> TasmotaPlug {
    TasmotaPlug {
        ip: ip.into(),
        name: name.into(),
        on_states: on_states.iter().map(|s| s.to_string()).collect(),
    }
}

/// Build the same graph as `main()` with short timers. `wled` is a controller
/// address, `uv` and `lights` are Tasmota plug addresses.
async fn station(wled: &str, uv: &str, lights: &str) -> Station {
    let mut config = Config::default();
    config.arena.warming_energy_threshold = 100.0;
    config.arena.shout_energy_multiplier = 100.0; // one full-intensity shout fills the bar
    config.arena.energy_decay_rate = 0.0;
    config.arena.fight_duration_sec = 60;
    config.arena.fight_inactivity_timeout_sec = 2;
    config.arena.cooldown_duration_sec = 1;
    let fx = |r: u8| WledEffect {
        effect: "solid".into(),
        colors: Some(vec![[r, 0, 0]]),
        ..Default::default()
    };
    config.wled.effects.idle = fx(IDLE_RGB);
    config.wled.effects.warming = fx(WARMING_RGB);
    config.wled.effects.fight = fx(FIGHT_RGB);
    config.wled.effects.cooldown = fx(COOLDOWN_RGB);
    config.tasmota.plugs = vec![
        plug("uv", uv, &["fight"]),
        plug("lights", lights, &["warming", "fight", "cooldown"]),
    ];

    let bus: SharedEventBus = Arc::new(EventBus::new(1000));
    let sm = Arc::new(StateMachine::new(bus.clone()));
    let registry = Arc::new(StatusRegistry::new());
    registry.spawn(&bus);

    let db = Arc::new(DatabaseManager::new("sqlite::memory:").await.unwrap());
    let wled_mgr = Arc::new(WledManager::new(config.wled.clone(), db, bus.clone()));
    wled_mgr.set_controllers_for_test(&[wled]).await;
    wled_mgr.clone().start_event_loop();
    let tasmota = Arc::new(TasmotaManager::new(config.tasmota.clone(), bus.clone()));
    tasmota.clone().start_event_loop();

    let audio = Arc::new(AudioManager::new(config.audio.clone(), bus.clone()));
    let ble = Arc::new(BleManager::new(config.ble.clone(), bus.clone()));
    let arena = Arc::new(ArenaManager::new(config, sm.clone(), bus.clone(), audio, ble));
    let (api_tx, api_rx) = tokio::sync::mpsc::unbounded_channel();
    arena.clone().start(api_rx).await.unwrap();
    // Let the spawned event loops subscribe before the first transition.
    tokio::task::yield_now().await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    Station { bus, sm, arena, registry, _api_tx: api_tx }
}

async fn wait_state(sm: &StateMachine, want: ArenaState, secs: u64) -> bool {
    for _ in 0..secs * 50 {
        if sm.current_state().await == want {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    false
}

/// Wait until some logged request contains `needle`.
async fn saw(seen: &Seen, needle: &str) -> bool {
    for _ in 0..100 {
        if seen.lock().await.iter().any(|r| r.contains(needle)) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    false
}

/// The `Power on/off` commands a plug received, in order.
async fn cmnds(seen: &Seen) -> Vec<String> {
    seen.lock()
        .await
        .iter()
        .filter_map(|r| {
            let q = r.lines().next()?.split_once("cmnd=")?.1;
            Some(q.split([' ', '&']).next()?.replace("%20", " "))
        })
        .collect()
}

fn marker(rgb: u8) -> String {
    format!("[[{rgb},0,0]]")
}

fn shout(intensity: f32) -> Event {
    Event::shout_detected(intensity, 0.5, -20.0)
}

#[tokio::test]
async fn full_round_drives_leds_plugs_and_stats() {
    let (wled, _, wled_seen) =
        mock_http(&[("*", 200, "{}"), ("GET /json/effects", 200, r#"["Solid"]"#)]).await;
    let (uv, _, uv_seen) = mock_http(&[("*", 200, "{}")]).await;
    let (lights, _, lights_seen) = mock_http(&[("*", 200, "{}")]).await;
    let st = station(&wled, &uv, &lights).await;
    assert_eq!(st.sm.current_state().await, ArenaState::Suspended);

    // A player walks in: SUSPENDED -> IDLE -> WARMING (presence fires once).
    st.bus.publish(Event::PresenceDetected { detected: true });
    assert!(wait_state(&st.sm, ArenaState::Warming, 3).await, "presence reaches WARMING");
    assert!(saw(&wled_seen, &marker(WARMING_RGB)).await, "LEDs show the warming effect");
    assert!(saw(&lights_seen, "Power%20on").await, "lights plug turned on for warming");
    assert!(saw(&uv_seen, "Power%20off").await, "UV plug is off outside the fight");

    // Punches and a weak shout in WARMING change nothing.
    st.bus.publish(Event::punch_detected(80.0, 0.0, 0.0, 8.0));
    st.bus.publish(shout(0.2));
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(st.sm.current_state().await, ArenaState::Warming);
    assert_eq!(st.arena.get_statistics().await.current_session_punches, 0, "punches only count in FIGHT");

    // A full-intensity shout fills the energy bar: WARMING -> FIGHT.
    st.bus.publish(shout(1.0));
    assert!(wait_state(&st.sm, ArenaState::Fight, 3).await, "energy full reaches FIGHT");
    assert!(saw(&wled_seen, &marker(FIGHT_RGB)).await, "LEDs show the fight effect");
    assert!(saw(&uv_seen, "Power%20on").await, "UV plug turned on for the fight");
    assert_eq!(st.arena.get_statistics().await.total_sessions, 1);

    // Three hits are counted and the max power is tracked.
    for power in [30.0, 90.0, 60.0] {
        st.bus.publish(Event::punch_detected(power, 0.0, 0.0, power / 10.0));
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    let stats = st.arena.get_statistics().await;
    assert_eq!(stats.current_session_punches, 3);
    assert_eq!(stats.current_session_max_power, 90.0);

    // No more activity: the inactivity timeout ends the fight.
    assert!(wait_state(&st.sm, ArenaState::Cooldown, 8).await, "inactivity timeout -> COOLDOWN");
    assert!(saw(&wled_seen, &marker(COOLDOWN_RGB)).await, "LEDs show the cooldown effect");
    // The player is still present, so COOLDOWN rolls into the next round.
    assert!(wait_state(&st.sm, ArenaState::Warming, 4).await, "present player gets the next round");

    // The player leaves: SUSPENDED and the LEDs are switched off.
    st.bus.publish(Event::PresenceDetected { detected: false });
    assert!(wait_state(&st.sm, ArenaState::Suspended, 3).await);
    assert!(saw(&wled_seen, r#""on":false"#).await, "LEDs switched off when suspended");

    // UV was on exactly during the fight; the lights stayed on for warming, fight, cooldown.
    let uv_cmds = cmnds(&uv_seen).await;
    let on = uv_cmds.iter().position(|c| c == "Power on").expect("UV on");
    assert!(uv_cmds[..on].iter().all(|c| c == "Power off"), "{uv_cmds:?}");
    assert!(uv_cmds[on + 1..].iter().any(|c| c == "Power off"), "UV off again after the fight: {uv_cmds:?}");

    // Health registry saw every device respond.
    let h = st.registry.snapshot();
    assert!(h.wled[&wled].connected, "WLED healthy: {:?}", h.wled);
    assert!(h.tasmota["uv"].connected && h.tasmota["lights"].connected, "plugs healthy: {:?}", h.tasmota);
}

#[tokio::test]
async fn stray_events_are_ignored_outside_their_state() {
    let (wled, _, _) = mock_http(&[("*", 200, "{}")]).await;
    let (uv, _, _) = mock_http(&[("*", 200, "{}")]).await;
    let (lights, _, _) = mock_http(&[("*", 200, "{}")]).await;
    let st = station(&wled, &uv, &lights).await;

    // SUSPENDED: shouts and punches do nothing (not even a state change).
    st.bus.publish(shout(1.0));
    st.bus.publish(Event::punch_detected(99.0, 0.0, 0.0, 9.9));
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(st.sm.current_state().await, ArenaState::Suspended);
    assert_eq!(st.arena.get_statistics().await.current_session_punches, 0);

    // IDLE: still no reaction to either (only presence starts a round).
    st.sm.transition(ArenaState::Idle).await.unwrap();
    st.bus.publish(shout(1.0));
    st.bus.publish(Event::punch_detected(99.0, 0.0, 0.0, 9.9));
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(st.sm.current_state().await, ArenaState::Idle);
    assert_eq!(st.arena.get_statistics().await.current_session_punches, 0);

    // A shout during the fight does not restart the energy bar or end the fight.
    st.sm.transition(ArenaState::Warming).await.unwrap();
    st.sm.transition(ArenaState::Fight).await.unwrap();
    st.bus.publish(shout(1.0));
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(st.sm.current_state().await, ArenaState::Fight);
}

#[tokio::test]
async fn dead_devices_never_block_the_game() {
    // WLED and both plugs refuse connections.
    let (wled, uv, lights) = (dead_addr().await, dead_addr().await, dead_addr().await);
    let st = station(&wled, &uv, &lights).await;

    st.bus.publish(Event::PresenceDetected { detected: true });
    assert!(wait_state(&st.sm, ArenaState::Warming, 5).await, "WARMING despite dead devices");
    st.bus.publish(shout(1.0));
    assert!(wait_state(&st.sm, ArenaState::Fight, 5).await, "FIGHT despite dead devices");
    st.bus.publish(Event::punch_detected(50.0, 0.0, 0.0, 5.0));
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(st.arena.get_statistics().await.current_session_punches, 1);
    assert!(wait_state(&st.sm, ArenaState::Cooldown, 10).await, "COOLDOWN despite dead devices");

    // The failures are visible in the health registry instead.
    for _ in 0..100 {
        let h = st.registry.snapshot();
        if h.wled.get(&wled).is_some_and(|d| d.error_count > 0)
            && h.tasmota.get("uv").is_some_and(|d| d.error_count > 0)
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("device errors not reported: {:?}", st.registry.snapshot());
}
