use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};

use crate::api::status::AppState;
use crate::models::ApiResponse;
use crate::services::session_recorder::RecordStatus;
use crate::services::audio_tuning::TuningState;
use crate::services::AudioManager;

#[derive(Debug, Default, Deserialize)]
pub struct ClipRequest {
    /// Free-form label embedded in the file name (default "manual").
    pub reason: Option<String>,
    /// Clip length in seconds (default: `audio.clip_seconds`).
    pub seconds: Option<f32>,
    /// Also write the raw (pre-filter) WAV (default: `audio.clip_raw`).
    pub raw: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct ClipResponse {
    pub path: String,
    pub raw_path: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct RecordStartRequest {
    pub label: Option<String>,
}

/// POST /api/audio/record/start - begin a long recording session
pub async fn record_start(
    State(state): State<AppState>,
    body: Option<Json<RecordStartRequest>>,
) -> Json<ApiResponse<RecordStatus>> {
    let label = body.and_then(|Json(r)| r.label).unwrap_or_default();
    Json(match state.audio.start_recording(&label).await {
        Ok(s) => ApiResponse::success(s),
        Err(e) => ApiResponse::error(format!("{e:#}")),
    })
}

/// POST /api/audio/record/stop - end the session and flush files
pub async fn record_stop(State(state): State<AppState>) -> Json<ApiResponse<RecordStatus>> {
    Json(ApiResponse::success(state.audio.stop_recording().await))
}

/// GET /api/audio/record/status
pub async fn record_status(State(state): State<AppState>) -> Json<ApiResponse<RecordStatus>> {
    Json(ApiResponse::success(state.audio.recording_status()))
}

/// POST /api/audio/clip - Save the last few seconds of mic audio + features to the clip dir
pub async fn record_clip(
    State(state): State<AppState>,
    body: Option<Json<ClipRequest>>,
) -> Json<ApiResponse<ClipResponse>> {
    Json(record(&state.audio, body.map(|Json(r)| r).unwrap_or_default()).await)
}

async fn record(audio: &AudioManager, req: ClipRequest) -> ApiResponse<ClipResponse> {
    let reason = req.reason.as_deref().unwrap_or("manual");
    let result = match req.seconds {
        Some(s) if !(s.is_finite() && s > 0.0) => {
            return ApiResponse::error("seconds must be positive");
        }
        Some(s) => {
            let dir = audio.config().read().await.clip_dir.clone();
            let raw = match req.raw {
                Some(r) => r,
                None => audio.config().read().await.clip_raw,
            };
            audio.record_clip(&dir, reason, s, raw)
        }
        None => audio.record_clip_default(reason, req.raw).await,
    };
    match result {
        Ok(o) => ApiResponse::success(ClipResponse {
            path: o.wav.display().to_string(),
            raw_path: o.raw.map(|p| p.display().to_string()),
        }),
        Err(e) => ApiResponse::error(e.to_string()),
    }
}

/// GET /api/audio/tuning - live values, defaults and descriptors of the tunable audio keys
pub async fn tuning_get(State(state): State<AppState>) -> Json<ApiResponse<TuningState>> {
    Json(ApiResponse::success(state.audio.tuning_state().await))
}

/// POST /api/audio/tuning - partial update `{key: value, ...}`; validated and clamped, returns the new state
pub async fn tuning_set(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Map<String, serde_json::Value>>,
) -> Json<ApiResponse<TuningState>> {
    if let Err(e) = state.audio.tuning_apply(&body).await {
        return Json(ApiResponse::error(e));
    }
    Json(ApiResponse::success(state.audio.tuning_state().await))
}

/// POST /api/audio/tuning/reset - back to the config defaults
pub async fn tuning_reset(State(state): State<AppState>) -> Json<ApiResponse<TuningState>> {
    state.audio.tuning_reset().await;
    Json(ApiResponse::success(state.audio.tuning_state().await))
}

/// POST /api/audio/tuning/save - persist the live values into the `audio` config section in the database
pub async fn tuning_save(State(state): State<AppState>) -> Json<ApiResponse<TuningState>> {
    match state.audio.tuning_save().await {
        Ok(()) => Json(ApiResponse::success(state.audio.tuning_state().await)),
        Err(e) => Json(ApiResponse::error(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::core::EventBus;
    use std::sync::Arc;

    fn audio() -> AudioManager {
        AudioManager::new(Config::default().audio, Arc::new(EventBus::new(10)))
    }

    #[tokio::test]
    async fn tuning_applies_live_and_resets() {
        let am = audio();
        let mut m = serde_json::Map::new();
        m.insert("shout_meter_lo_db".into(), serde_json::json!(12.0));
        am.tuning_apply(&m).await.unwrap();
        assert_eq!(am.config().read().await.shout_meter_lo_db, 12.0);
        assert_eq!(am.tuning_state().await.modified, vec!["shout_meter_lo_db"]);
        am.tuning_reset().await;
        assert!(am.tuning_state().await.modified.is_empty());
    }

    #[tokio::test]
    async fn tuning_save_persists_in_the_database_across_restarts() {
        use crate::services::database_manager::DatabaseManager;
        let p = std::env::temp_dir().join(format!("tuning-save-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&p);
        let url = format!("sqlite:{}", p.display());
        let db = DatabaseManager::new(&url).await.unwrap();
        let am = AudioManager::new(Config::default().audio, Arc::new(EventBus::new(10)))
            .with_config_store(db.config());
        let mut m = serde_json::Map::new();
        m.insert("shout_margin_db".into(), serde_json::json!(21.0));
        am.tuning_apply(&m).await.unwrap();
        assert_eq!(am.tuning_state().await.modified, vec!["shout_margin_db"]);
        am.tuning_save().await.unwrap();
        // saved: no longer "unsaved changes", and reset now returns to the saved value
        assert!(am.tuning_state().await.modified.is_empty());
        drop(am);
        drop(db);
        // new store on the same database file
        let db2 = DatabaseManager::new(&url).await.unwrap();
        let cfg = db2.config().load_config().await.unwrap();
        assert_eq!(cfg.audio.shout_margin_db, 21.0);
        let h = db2.config().history(5).await.unwrap();
        assert_eq!((h[0].key.as_str(), h[0].source.as_str()), ("audio", "tuning"));
        std::fs::remove_file(&p).ok();
    }

    #[tokio::test]
    async fn tuning_save_without_store_is_an_error() {
        assert!(audio().tuning_save().await.is_err());
    }

    #[tokio::test]
    async fn rejects_bad_seconds() {
        let r = record(&audio(), ClipRequest { reason: None, seconds: Some(-1.0), raw: None }).await;
        assert!(!r.success);
    }

    #[tokio::test]
    async fn errors_when_audio_not_started() {
        let r = record(&audio(), ClipRequest::default()).await;
        assert!(!r.success);
        assert!(r.error.unwrap().contains("not started"));
    }
}
