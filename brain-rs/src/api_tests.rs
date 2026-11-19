//! HTTP API integration tests: the real router (`build_router`) over the same
//! manager graph `main()` builds, driven with `tower::ServiceExt::oneshot`.
//! No hardware: WLED has no controllers, BLE and the microphone are not started.
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

use crate::api::SharedState;
use crate::config::Config;
use crate::core::status_registry::StatusRegistry;
use crate::core::{EventBus, SharedEventBus, StateMachine};
use crate::services::schedule_manager::LocalClock;
use crate::services::{
    ArenaManager, AudioManager, BleManager, DatabaseManager, DiscoveryManager, ScheduleManager,
    WledManager,
};

struct Api {
    app: Router,
    state: Arc<SharedState>,
}

/// Build an `AppState` backed by an in-memory DB with the arena manager running
/// (no presence fusion, no hardware) and the router on top of it.
async fn api() -> Api {
    let mut config = Config::default();
    config.arena.warming_energy_threshold = 100.0;
    config.arena.energy_decay_rate = 0.0;

    let bus: SharedEventBus = Arc::new(EventBus::new(1000));
    let sm = Arc::new(StateMachine::new(bus.clone()));
    let health = Arc::new(StatusRegistry::new());
    health.spawn(&bus);
    let db = Arc::new(DatabaseManager::new("sqlite::memory:").await.unwrap());
    let wled = Arc::new(WledManager::new(
        config.wled.clone(),
        db.clone(),
        bus.clone(),
    ));
    let audio = Arc::new(AudioManager::new(config.audio.clone(), bus.clone()));
    let ble = Arc::new(BleManager::new(config.ble.clone(), bus.clone()));
    let schedule = Arc::new(
        ScheduleManager::new(
            config.schedule.clone(),
            Arc::new(LocalClock),
            sm.clone(),
            bus.clone(),
        )
        .with_persistence(db.clone())
        .await,
    );
    let discovery = Arc::new(DiscoveryManager::new(db.clone(), ble.clone(), bus.clone()));
    let arena = Arc::new(ArenaManager::new(
        config,
        sm.clone(),
        bus.clone(),
        audio.clone(),
        ble,
    ));
    let (arena_tx, arena_rx) = tokio::sync::mpsc::unbounded_channel();
    arena.clone().start(arena_rx).await.unwrap();
    sm.transition(crate::core::ArenaState::Idle).await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    let state = Arc::new(SharedState {
        event_bus: bus,
        state_machine: sm,
        arena_tx,
        db,
        discovery,
        wled,
        schedule,
        health,
        audio,
        presence_manual: None,
    });
    Api {
        app: crate::build_router(state.clone()),
        state,
    }
}

impl Api {
    async fn send(&self, method: Method, uri: &str, body: Option<Value>) -> (StatusCode, String) {
        let mut req = Request::builder().method(method).uri(uri);
        let body = match body {
            Some(v) => {
                req = req.header("content-type", "application/json");
                Body::from(v.to_string())
            }
            None => Body::empty(),
        };
        let resp = self
            .app
            .clone()
            .oneshot(req.body(body).unwrap())
            .await
            .unwrap();
        let status = resp.status();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        (status, String::from_utf8_lossy(&bytes).into_owned())
    }

    async fn json(&self, method: Method, uri: &str, body: Option<Value>) -> Value {
        let (status, text) = self.send(method, uri, body).await;
        assert_eq!(status, StatusCode::OK, "{uri}: {text}");
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("{uri}: bad json {e}: {text}"))
    }

    async fn get(&self, uri: &str) -> Value {
        self.json(Method::GET, uri, None).await
    }

    async fn post(&self, uri: &str, body: Value) -> Value {
        self.json(Method::POST, uri, Some(body)).await
    }

    async fn arena_state(&self) -> String {
        let v = self.get("/api/state").await;
        v["data"]["arena"]["current_state"]
            .as_str()
            .unwrap()
            .to_string()
    }

    async fn wait_state(&self, want: &str) {
        for _ in 0..50 {
            if self.arena_state().await.eq_ignore_ascii_case(want) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("state never became {want}, is {}", self.arena_state().await);
    }
}

#[tokio::test]
async fn get_state_reports_idle_arena() {
    let api = api().await;
    let v = api.get("/api/state").await;
    assert_eq!(v["success"], true);
    assert!(v["data"]["arena"]["current_state"]
        .as_str()
        .unwrap()
        .eq_ignore_ascii_case("idle"));
    assert!(v["data"]["hardware"].is_object());
    assert!(v["data"]["statistics"].is_object());
}

#[tokio::test]
async fn presence_moves_idle_to_warming() {
    let api = api().await;
    let v = api
        .post("/api/arena/presence", json!({"detected": true}))
        .await;
    assert_eq!(v["success"], true);
    api.wait_state("warming").await;
}

#[tokio::test]
async fn suspend_endpoint_suspends_a_warming_arena() {
    let api = api().await;
    api.post("/api/arena/presence", json!({"detected": true}))
        .await;
    api.wait_state("warming").await;
    // Behaviour note: /suspend only publishes presence=false, which moves WARMING to SUSPENDED.
    let v = api.post("/api/arena/suspend", json!({})).await;
    assert_eq!(v["success"], true);
    api.wait_state("suspended").await;
}

#[tokio::test]
async fn shout_and_punch_return_ok_and_publish_events() {
    let api = api().await;
    let mut rx = api.state.event_bus.subscribe();
    assert_eq!(
        api.post("/api/arena/shout", json!({"intensity": 0.8}))
            .await["success"],
        true
    );
    assert_eq!(
        api.post("/api/arena/punch", json!({"power": 500.0})).await["success"],
        true
    );
    let mut saw = (false, false);
    while let Ok(Ok(ev)) = tokio::time::timeout(Duration::from_millis(200), rx.recv()).await {
        match ev {
            crate::core::Event::ShoutDetected { .. } => saw.0 = true,
            crate::core::Event::PunchDetected { .. } => saw.1 = true,
            _ => {}
        }
    }
    assert_eq!(saw, (true, true));
}

#[tokio::test]
async fn shout_fills_energy_and_starts_fight() {
    let api = api().await;
    api.post("/api/arena/presence", json!({"detected": true}))
        .await;
    api.wait_state("warming").await;
    let mut t = api.get("/api/arena/config").await["data"].clone();
    t["shout_energy_multiplier"] = json!(1000.0);
    api.post("/api/arena/config", t).await;
    api.post("/api/arena/shout", json!({"intensity": 1.0}))
        .await;
    api.wait_state("fight").await;
}

#[tokio::test]
async fn set_state_valid_and_invalid() {
    let api = api().await;
    let v = api
        .post("/api/arena/state", json!({"state": "Warming"}))
        .await;
    assert_eq!(v["success"], true, "{v}");
    assert!(api.arena_state().await.eq_ignore_ascii_case("warming"));

    // Warming -> Cooldown is not an allowed transition.
    let v = api
        .post("/api/arena/state", json!({"state": "Cooldown"}))
        .await;
    assert_eq!(v["success"], false);
    assert!(
        v["error"]
            .as_str()
            .unwrap()
            .to_lowercase()
            .contains("invalid"),
        "{v}"
    );
    assert!(api.arena_state().await.eq_ignore_ascii_case("warming"));
}

#[tokio::test]
async fn set_state_unknown_state_is_rejected_by_extractor() {
    let api = api().await;
    let (status, _) = api
        .send(
            Method::POST,
            "/api/arena/state",
            Some(json!({"state": "Bogus"})),
        )
        .await;
    assert!(status.is_client_error(), "{status}");
    let (status, _) = api
        .send(Method::POST, "/api/arena/state", Some(json!({})))
        .await;
    assert!(status.is_client_error(), "{status}");
}

#[tokio::test]
async fn arena_config_round_trip() {
    let api = api().await;
    let v = api.get("/api/arena/config").await;
    assert_eq!(v["success"], true);
    let mut t = v["data"].clone();
    t["fight_duration_sec"] = json!(123);
    t["cooldown_duration_sec"] = json!(7);
    let v = api.post("/api/arena/config", t).await;
    assert_eq!(v["success"], true);
    // The update goes through the arena command channel; poll for it.
    for _ in 0..50 {
        let d = api.get("/api/arena/config").await["data"].clone();
        if d["fight_duration_sec"] == 123 {
            assert_eq!(d["cooldown_duration_sec"], 7);
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("config update never applied");
}

#[tokio::test]
async fn arena_config_rejects_malformed_body() {
    let api = api().await;
    let (status, _) = api
        .send(
            Method::POST,
            "/api/arena/config",
            Some(json!({"fight_duration_sec": "x"})),
        )
        .await;
    assert!(status.is_client_error(), "{status}");
}

#[tokio::test]
async fn wled_effects_without_controllers_is_an_error_response() {
    let api = api().await;
    // Behaviour note: errors are HTTP 200 with success=false.
    let v = api.get("/api/wled/effects").await;
    assert_eq!(v["success"], false);
    assert!(v["error"].as_str().is_some_and(|e| !e.is_empty()), "{v}");
    assert!(v.get("data").is_none());
}

#[tokio::test]
async fn wled_set_effect_requires_effect_name() {
    let api = api().await;
    let v = api.post("/api/wled/effect", json!({"effect": " "})).await;
    assert_eq!(v["success"], false);
    assert!(v["error"].as_str().unwrap().contains("required"));
}

#[tokio::test]
async fn schedule_get_returns_config_and_active_flag() {
    let api = api().await;
    let v = api.get("/api/schedule").await;
    assert_eq!(v["success"], true);
    assert!(v["data"]["config"].is_object());
    assert!(v["data"]["active"].is_boolean());
}

#[tokio::test]
async fn index_route_serves_html_and_bad_routes_fail() {
    let api = api().await;
    let (status, body) = api.send(Method::GET, "/", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.to_lowercase().contains("<html"), "{body}");
    let (status, _) = api.send(Method::GET, "/api/nope", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = api.send(Method::GET, "/api/arena/presence", None).await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
}
