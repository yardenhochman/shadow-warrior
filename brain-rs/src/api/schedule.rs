use axum::{extract::State, Json};

use crate::api::status::AppState;
use crate::config::ScheduleConfig;
use crate::models::ApiResponse;
use crate::services::schedule_manager::ScheduleStatus;

/// GET /api/schedule - Current schedule config and whether the game is active
pub async fn get_schedule(State(state): State<AppState>) -> Json<ApiResponse<ScheduleStatus>> {
    Json(ApiResponse::success(state.schedule.status().await))
}

/// POST /api/schedule - Replace the schedule (in memory) and apply it now
pub async fn update_schedule(
    State(state): State<AppState>,
    Json(payload): Json<ScheduleConfig>,
) -> Json<ApiResponse<ScheduleStatus>> {
    match state.schedule.update_config(payload).await {
        Ok(()) => Json(ApiResponse::success(state.schedule.status().await)),
        Err(e) => Json(ApiResponse::error(e)),
    }
}
