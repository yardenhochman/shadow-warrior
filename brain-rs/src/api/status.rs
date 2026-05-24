use crate::models::GameTunables;
use axum::{
    extract::State,
    response::sse::{Event, KeepAlive, Sse},
    response::IntoResponse,
    Json,
};
use std::convert::Infallible;
use std::sync::Arc;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

use tokio::sync::{oneshot, mpsc::UnboundedSender};

use crate::core::SharedEventBus;
use crate::models::{
    ApiResponse, ArenaStateInfo, BrainState, ConnectionStatus, HardwareStatus, MusicStatus,
    Statistics,
};

pub type AppState = Arc<SharedState>;

pub enum ArenaCommand {
    GetState(oneshot::Sender<ArenaStateInfo>),
    GetStatistics(oneshot::Sender<Statistics>),
    ResetStatistics,
    GetTunables(oneshot::Sender<GameTunables>),
    UpdateTunables(GameTunables),
}

pub struct SharedState {
    pub event_bus: SharedEventBus,
    pub state_machine: Arc<crate::core::StateMachine>,
    pub arena_tx: UnboundedSender<ArenaCommand>,
    pub db: Arc<crate::services::database_manager::DatabaseManager>,
    pub discovery: Arc<crate::services::discovery_manager::DiscoveryManager>,
    pub wled: Arc<crate::services::wled_manager::WledManager>,
}

/// GET /api/state - Get complete brain state
pub async fn get_state(State(state): State<AppState>) -> Json<ApiResponse<BrainState>> {
    // Request state via channel
    let (state_tx, state_rx) = oneshot::channel();
    let (stats_tx, stats_rx) = oneshot::channel();

    let _ = state.arena_tx.send(ArenaCommand::GetState(state_tx));
    let _ = state.arena_tx.send(ArenaCommand::GetStatistics(stats_tx));

    let arena_state = state_rx.await.unwrap_or_else(|_| ArenaStateInfo {
        current_state: crate::core::ArenaState::Suspended,
        state_entered_at: chrono::Utc::now(),
        time_in_state_sec: 0.0,
        energy: None,
        max_energy: None,
        energy_percentage: None,
        fight_elapsed_sec: None,
        fight_remaining_sec: None,
        time_since_last_activity_sec: None,
    });

    let statistics = stats_rx.await.unwrap_or_else(|_| Statistics {
        total_sessions: 0,
        current_session_punches: 0,
        current_session_max_power: 0.0,
        latest_acceleration: None,
        latest_audio_level_db: None,
        uptime_sec: 0.0,
    });

    // Build hardware status
    let hardware = HardwareStatus {
        ble: ConnectionStatus::default(), // TODO: Get from BLE manager
        audio: ConnectionStatus {
            connected: true, // TODO: Get from audio manager
            device_name: Some("Default".to_string()),
            last_error: None,
            connected_at: None,
        },
        wled: state.wled.get_status().await,
        tasmota: vec![], // TODO: Get from Tasmota manager
        music: MusicStatus::default(), // TODO: Get from music manager
    };

    let brain_state = BrainState {
        arena: arena_state,
        hardware,
        statistics,
    };

    Json(ApiResponse::success(brain_state))
}

/// GET /api/events - SSE stream for real-time events
pub async fn get_events(State(state): State<AppState>) -> impl IntoResponse {
    let event_rx = state.event_bus.subscribe();
    let stream = BroadcastStream::new(event_rx).filter_map(|result| match result {
        Ok(event) => {
            // Convert our Event to SSE Event
            let json_data = serde_json::to_string(&event).ok()?;
            Some(Ok::<_, Infallible>(
                Event::default()
                    .event(event.event_type())
                    .data(json_data),
            ))
        }
        Err(_) => None,
    });

    Sse::new(stream).keep_alive(KeepAlive::default())
}

/// GET /api/stats - Get detailed statistics
pub async fn get_statistics(State(state): State<AppState>) -> Json<ApiResponse<Statistics>> {
    let (tx, rx) = oneshot::channel();
    let _ = state.arena_tx.send(ArenaCommand::GetStatistics(tx));

    let statistics = rx.await.unwrap_or_else(|_| Statistics {
        total_sessions: 0,
        current_session_punches: 0,
        current_session_max_power: 0.0,
        latest_acceleration: None,
        latest_audio_level_db: None,
        uptime_sec: 0.0,
    });

    Json(ApiResponse::success(statistics))
}

use std::sync::OnceLock;
use tokio::sync::broadcast;

static LOG_BROADCASTER: OnceLock<broadcast::Sender<String>> = OnceLock::new();

pub fn get_log_broadcaster() -> &'static broadcast::Sender<String> {
    LOG_BROADCASTER.get_or_init(|| {
        let (tx, _) = broadcast::channel(1000);
        tx
    })
}

/// GET /api/logs - SSE stream for real-time system logs
pub async fn get_logs(State(_state): State<AppState>) -> impl IntoResponse {
    let log_rx = get_log_broadcaster().subscribe();
    let stream = BroadcastStream::new(log_rx).filter_map(|result| match result {
        Ok(log) => {
            Some(Ok::<_, Infallible>(
                Event::default().data(log),
            ))
        }
        Err(_) => None,
    });

    Sse::new(stream).keep_alive(KeepAlive::default())
}

