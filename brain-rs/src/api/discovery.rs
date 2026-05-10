use axum::{
    extract::{Path, State},
    response::IntoResponse,
    Json,
};
use std::sync::Arc;
use serde::Deserialize;

use crate::api::SharedState;

#[derive(Debug, Deserialize)]
pub struct AddDeviceRequest {
    pub name: String,
    pub device_type: String,
    pub host: String,
    pub port: i32,
    pub metadata: Option<String>,
}

pub async fn scan_hardware(State(state): State<Arc<SharedState>>) -> impl IntoResponse {
    match state.discovery.start_scan().await {
        Ok(_) => Json(serde_json::json!({ "status": "scan_started" })).into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ).into_response(),
    }
}

pub async fn get_devices(State(state): State<Arc<SharedState>>) -> impl IntoResponse {
    match state.db.get_devices().await {
        Ok(devices) => Json(serde_json::json!(devices)).into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ).into_response(),
    }
}

pub async fn add_device(
    State(state): State<Arc<SharedState>>,
    Json(payload): Json<AddDeviceRequest>,
) -> impl IntoResponse {
    match state.db.add_device(&payload.name, &payload.device_type, &payload.host, payload.port, payload.metadata.as_deref()).await {
        Ok(id) => Json(serde_json::json!({ "status": "success", "id": id })).into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ).into_response(),
    }
}

pub async fn delete_device(
    State(state): State<Arc<SharedState>>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    // Look up device before deleting so we can update runtime state
    if let Ok(devices) = state.db.get_devices().await {
        if let Some(dev) = devices.iter().find(|d| d.id == id) {
            if dev.device_type == "wled" {
                let _ = state.wled.remove_controller(&dev.host).await;
            }
        }
    }

    match state.db.delete_device(id).await {
        Ok(_) => Json(serde_json::json!({ "status": "success" })).into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ).into_response(),
    }
}
