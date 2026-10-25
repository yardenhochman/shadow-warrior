//! End-to-end tests: API handlers + WledManager against a local mock WLED device.
use super::*;
use crate::api::status::SharedState;
use crate::config::Config;
use crate::core::{EventBus, StateMachine};
use crate::services::ble_manager::BleManager;
use crate::services::database_manager::DatabaseManager;
use crate::services::discovery_manager::DiscoveryManager;
use crate::services::schedule_manager::{LocalClock, ScheduleManager};
use crate::services::wled_manager::WledManager;
use std::sync::Arc;
use crate::test_util::{dead_addr, mock_http as mock_wled};

async fn state_with(ips: &[&str]) -> AppState {
    let config = Config::default();
    let db = Arc::new(DatabaseManager::new("sqlite::memory:").await.unwrap());
    let bus = Arc::new(EventBus::new(100));
    let sm = Arc::new(StateMachine::new(bus.clone()));
    let ble = Arc::new(BleManager::new(config.ble.clone(), bus.clone()));
    let wled = Arc::new(WledManager::new(config.wled.clone(), db.clone(), bus.clone()));
    wled.set_controllers_for_test(ips).await;
    let schedule = Arc::new(ScheduleManager::new(
        config.schedule.clone(),
        Arc::new(LocalClock),
        sm.clone(),
        bus.clone(),
    ));
    let (arena_tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    Arc::new(SharedState {
        event_bus: bus.clone(),
        health: Arc::new(crate::core::status_registry::StatusRegistry::new()),
        state_machine: sm,
        arena_tx,
        db: db.clone(),
        discovery: Arc::new(DiscoveryManager::new(db, ble, bus.clone())),
        wled,
        schedule,
        audio: Arc::new(crate::services::AudioManager::new(config.audio.clone(), bus.clone())),
    })
}

const EFFECTS: &str = r#"["Solid","Blink","Breathe","Wipe","Fire 2012"]"#;
const PALETTES: &str = r#"["Default","Random","Lava"]"#;

fn names(r: &ApiResponse<NameList>) -> Vec<String> {
    r.data.as_ref().expect("data").names.clone()
}

fn effect(json: &str) -> WledEffect {
    serde_json::from_str(json).unwrap()
}

#[tokio::test]
async fn effects_and_palettes_served_from_device() {
    let (addr, _, _) =
        mock_wled(&[("GET /json/effects", 200, EFFECTS), ("GET /json/palettes", 200, PALETTES)]).await;
    let st = state_with(&[&addr]).await;
    let Json(e) = get_effects(State(st.clone())).await;
    assert!(e.success);
    assert_eq!(names(&e), ["Solid", "Blink", "Breathe", "Wipe", "Fire 2012"]);
    let Json(p) = get_palettes(State(st)).await;
    assert_eq!(names(&p), ["Default", "Random", "Lava"]);
}

#[tokio::test]
async fn unreachable_device_reports_error_with_empty_cache() {
    let st = state_with(&[&dead_addr().await]).await;
    let Json(e) = get_effects(State(st.clone())).await;
    assert!(!e.success);
    assert!(e.error.unwrap().contains("no WLED controller reachable"));
    let Json(p) = get_palettes(State(st)).await;
    assert!(!p.success);
}

#[tokio::test]
async fn no_controllers_reports_error() {
    let st = state_with(&[]).await;
    let Json(e) = get_effects(State(st)).await;
    assert!(!e.success);
}

#[tokio::test]
async fn warm_caches_tolerates_dead_device_and_retries_later() {
    let (addr, routes, _) = mock_wled(&[]).await; // everything 404s at first
    let st = state_with(&[&dead_addr().await, &addr]).await;
    assert_eq!(st.wled.warm_caches().await, (false, false));

    routes.lock().await.insert("GET /json/effects".into(), (200, EFFECTS.into()));
    // partial: effects come up, palettes still failing
    assert_eq!(st.wled.warm_caches().await, (true, false));

    routes.lock().await.insert("GET /json/palettes".into(), (200, PALETTES.into()));
    assert_eq!(st.wled.warm_caches().await, (true, true));
}

#[tokio::test]
async fn cache_serves_after_device_goes_away() {
    let (addr, routes, _) = mock_wled(&[("GET /json/effects", 200, EFFECTS)]).await;
    let st = state_with(&[&addr]).await;
    st.wled.warm_caches().await;
    routes.lock().await.clear();
    let Json(e) = get_effects(State(st)).await;
    assert!(e.success, "cached list expected");
    assert_eq!(names(&e).len(), 5);
}

#[tokio::test]
async fn falls_through_dead_controller_to_live_one() {
    let (addr, _, _) = mock_wled(&[("GET /json/effects", 200, EFFECTS)]).await;
    let st = state_with(&[&dead_addr().await, &addr]).await;
    let Json(e) = get_effects(State(st)).await;
    assert!(e.success);
}

#[tokio::test]
async fn set_effect_resolves_names_via_device_lists_and_posts_state() {
    let (addr, _, seen) = mock_wled(&[
        ("GET /json/effects", 200, EFFECTS),
        ("GET /json/palettes", 200, PALETTES),
        ("POST /json/state", 200, "{}"),
    ])
    .await;
    let st = state_with(&[&addr]).await;
    let eff = effect(r#"{"effect":"fire 2012","palette":"Lava","speed":150,"colors":[[255,0,0]]}"#);
    let Json(r) = set_effect(State(st), Json(eff)).await;
    assert!(r.success, "{:?}", r.error);
    assert_eq!(r.data.unwrap().controllers_updated, 1);
    let seen = seen.lock().await;
    let post = seen.iter().find(|q| q.starts_with("POST /json/state")).unwrap();
    let body: serde_json::Value = serde_json::from_str(post.split("\r\n\r\n").nth(1).unwrap()).unwrap();
    assert_eq!(body["seg"][0]["fx"], 4); // index in device list
    assert_eq!(body["seg"][0]["pal"], 2);
    assert_eq!(body["seg"][0]["sx"], 150);
    assert_eq!(body["seg"][0]["col"], serde_json::json!([[255, 0, 0]]));
}

#[tokio::test]
async fn set_effect_uses_static_fallback_when_lists_unavailable() {
    let (addr, _, seen) = mock_wled(&[("POST /json/state", 200, "{}")]).await;
    let st = state_with(&[&addr]).await;
    let Json(r) = set_effect(State(st), Json(effect(r#"{"effect":"fire","palette":"lava"}"#))).await;
    assert!(r.success);
    let seen = seen.lock().await;
    let post = seen.iter().find(|q| q.starts_with("POST /json/state")).unwrap();
    assert!(post.contains("\"fx\":16") && post.contains("\"pal\":22"));
}

#[tokio::test]
async fn warmed_cache_avoids_refetch_on_apply() {
    let (addr, _, seen) = mock_wled(&[
        ("GET /json/effects", 200, EFFECTS),
        ("POST /json/state", 200, "{}"),
    ])
    .await;
    let st = state_with(&[&addr]).await;
    st.wled.warm_caches().await;
    let gets = |v: &Vec<String>| v.iter().filter(|q| q.starts_with("GET /json/effects")).count();
    let before = gets(&*seen.lock().await);
    let Json(r) = set_effect(State(st), Json(effect(r#"{"effect":"wipe"}"#))).await;
    assert!(r.success);
    assert_eq!(gets(&*seen.lock().await), before);
}

#[tokio::test]
async fn set_effect_partial_failure_counts_only_successes() {
    let (good, _, _) = mock_wled(&[("POST /json/state", 200, "{}")]).await;
    let (bad, _, _) = mock_wled(&[("POST /json/state", 500, "{}")]).await;
    let st = state_with(&[&dead_addr().await, &bad, &good]).await;
    let Json(r) = set_effect(State(st), Json(effect(r#"{"effect":"solid"}"#))).await;
    assert!(r.success);
    assert_eq!(r.data.unwrap().controllers_updated, 1);
}

#[tokio::test]
async fn set_effect_all_controllers_down_reports_zero_updated() {
    let st = state_with(&[&dead_addr().await]).await;
    let Json(r) = set_effect(State(st), Json(effect(r#"{"effect":"solid"}"#))).await;
    assert_eq!(r.data.unwrap().controllers_updated, 0);
}

#[tokio::test]
async fn set_effect_rejects_empty_and_realtime() {
    let st = state_with(&[]).await;
    let Json(r) = set_effect(State(st.clone()), Json(effect(r#"{"effect":"  "}"#))).await;
    assert!(!r.success);
    let Json(r) = set_effect(State(st), Json(effect(r#"{"effect":"Realtime"}"#))).await;
    assert!(!r.success);
}
