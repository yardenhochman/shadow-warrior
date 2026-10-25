use axum::{extract::State, Json};
use serde::Serialize;

use crate::api::status::AppState;
use crate::config::WledEffect;
use crate::models::ApiResponse;

#[derive(Serialize)]
pub struct NameList {
    /// Index in the list is the WLED id.
    pub names: Vec<String>,
}

#[derive(Serialize)]
pub struct ApplyResult {
    pub controllers_updated: usize,
}

/// GET /api/wled/effects - effect names reported by the WLED device
pub async fn get_effects(State(state): State<AppState>) -> Json<ApiResponse<NameList>> {
    match state.wled.list_effects().await {
        Ok(names) => Json(ApiResponse::success(NameList { names })),
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

/// GET /api/wled/palettes - palette names reported by the WLED device
pub async fn get_palettes(State(state): State<AppState>) -> Json<ApiResponse<NameList>> {
    match state.wled.list_palettes().await {
        Ok(names) => Json(ApiResponse::success(NameList { names })),
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

#[cfg(test)]
#[path = "wled_tests.rs"]
mod tests;

/// POST /api/wled/effect - apply an effect to all controllers immediately.
/// Body is a WledEffect, e.g. {"effect":"Fire 2012","palette":"Lava","speed":150,"colors":[[255,0,0]]}
pub async fn set_effect(
    State(state): State<AppState>,
    Json(effect): Json<WledEffect>,
) -> Json<ApiResponse<ApplyResult>> {
    if effect.effect.trim().is_empty() {
        return Json(ApiResponse::error("'effect' is required"));
    }
    match state.wled.apply_effect(&effect).await {
        Ok(n) => Json(ApiResponse::success(ApplyResult { controllers_updated: n })),
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}
