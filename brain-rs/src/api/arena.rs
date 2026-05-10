use axum::{extract::State, Json};

use crate::api::status::{AppState, ArenaCommand};
use crate::core::Event;
use crate::models::{
    ApiResponse, GameTunables, PresenceRequest, PunchRequest, ShoutRequest, StateTransitionRequest,
};

/// POST /api/arena/presence - Trigger presence detection
pub async fn presence(
    State(state): State<AppState>,
    Json(payload): Json<PresenceRequest>,
) -> Json<ApiResponse<()>> {
    state.event_bus.publish(Event::PresenceDetected {
        detected: payload.detected,
    });

    Json(ApiResponse::<()>::ok())
}

/// POST /api/arena/suspend - Force suspend the arena
pub async fn suspend(State(state): State<AppState>) -> Json<ApiResponse<()>> {
    state.event_bus.publish(Event::PresenceDetected {
        detected: false,
    });

    Json(ApiResponse::<()>::ok())
}

/// POST /api/arena/shout - Simulate a shout
pub async fn shout(
    State(state): State<AppState>,
    Json(payload): Json<ShoutRequest>,
) -> Json<ApiResponse<()>> {
    state.event_bus.publish(Event::ShoutDetected {
        intensity: payload.intensity,
        duration_sec: 1.0,
        level_db: 80.0,
    });

    Json(ApiResponse::<()>::ok())
}

/// POST /api/arena/punch - Simulate a punch
pub async fn punch(
    State(state): State<AppState>,
    Json(payload): Json<PunchRequest>,
) -> Json<ApiResponse<()>> {
    state
        .event_bus
        .publish(Event::punch_detected(payload.power, 0.0, 0.0, 0.0));

    Json(ApiResponse::<()>::ok())
}

/// POST /api/arena/state - Force a state transition
pub async fn set_state(
    State(state): State<AppState>,
    Json(payload): Json<StateTransitionRequest>,
) -> Json<ApiResponse<()>> {
    match state.state_machine.transition(payload.state).await {
        Ok(_) => Json(ApiResponse::<()>::ok()),
        Err(e) => Json(ApiResponse::<()>::error(e.to_string())),
    }
}

/// POST /api/arena/reset_stats - Reset statistics
pub async fn reset_stats(State(state): State<AppState>) -> Json<ApiResponse<()>> {
    let _ = state.arena_tx.send(ArenaCommand::ResetStatistics);
    Json(ApiResponse::<()>::ok())
}

/// GET /api/arena/config - Get game tunables
pub async fn get_config(State(state): State<AppState>) -> Json<ApiResponse<GameTunables>> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let _ = state.arena_tx.send(ArenaCommand::GetTunables(tx));

    match rx.await {
        Ok(tunables) => Json(ApiResponse::success(tunables)),
        Err(_) => Json(ApiResponse::error("Failed to get config")),
    }
}

/// POST /api/arena/config - Update game tunables
pub async fn update_config(
    State(state): State<AppState>,
    Json(payload): Json<GameTunables>,
) -> Json<ApiResponse<()>> {
    let _ = state.arena_tx.send(ArenaCommand::UpdateTunables(payload));
    Json(ApiResponse::<()>::ok())
}
