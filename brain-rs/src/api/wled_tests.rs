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
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Mutex;

type Routes = Arc<Mutex<HashMap<String, (u16, String)>>>;
type Seen = Arc<Mutex<Vec<String>>>;

/// Mock WLED device: routes keyed by "METHOD /path". Unknown routes give 404.
/// Routes can be changed while running to simulate a device coming up.
async fn mock_wled(routes: &[(&str, u16, &str)]) -> (String, Routes, Seen) {
    let table: Routes = Arc::new(Mutex::new(
        routes.iter().map(|(k, c, b)| (k.to_string(), (*c, b.to_string()))).collect(),
    ));
    let seen: Seen = Arc::new(Mutex::new(Vec::new()));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let (t, s) = (table.clone(), seen.clone());
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else { return };
            let (t, s) = (t.clone(), s.clone());
            tokio::spawn(async move {
                let mut buf = vec![0u8; 16384];
                let mut total = 0;
                loop {
                    let n = sock.read(&mut buf[total..]).await.unwrap_or(0);
                    total += n;
                    let text = String::from_utf8_lossy(&buf[..total]).to_string();
                    if let Some(h) = text.find("\r\n\r\n") {
                        let len = text
                            .lines()
                            .find_map(|l| {
                                l.to_lowercase()
                                    .strip_prefix("content-length:")
                                    .and_then(|v| v.trim().parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        if total >= h + 4 + len || n == 0 {
                            break;
                        }
                    } else if n == 0 {
                        break;
                    }
                }
                let req = String::from_utf8_lossy(&buf[..total]).to_string();
                let mut parts = req.split_whitespace();
                let key = format!("{} {}", parts.next().unwrap_or(""), parts.next().unwrap_or(""));
                s.lock().await.push(req.clone());
                let (code, body) = t.lock().await.get(&key).cloned().unwrap_or((404, String::new()));
                let resp = format!(
                    "HTTP/1.1 {} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    code,
                    body.len(),
                    body
                );
                let _ = sock.write_all(resp.as_bytes()).await;
            });
        }
    });
    (addr, table, seen)
}

/// An address nothing listens on (connection refused).
async fn dead_addr() -> String {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    l.local_addr().unwrap().to_string()
}

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
        state_machine: sm,
        arena_tx,
        db: db.clone(),
        discovery: Arc::new(DiscoveryManager::new(db, ble, bus)),
        wled,
        schedule,
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
