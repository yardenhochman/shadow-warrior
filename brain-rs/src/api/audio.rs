use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};

use crate::api::status::AppState;
use crate::models::ApiResponse;
use crate::services::AudioManager;

#[derive(Debug, Default, Deserialize)]
pub struct ClipRequest {
    /// Free-form label embedded in the file name (default "manual").
    pub reason: Option<String>,
    /// Clip length in seconds (default: `audio.clip_seconds`).
    pub seconds: Option<f32>,
}

#[derive(Debug, Serialize)]
pub struct ClipResponse {
    pub path: String,
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
            audio.record_clip(&dir, reason, s)
        }
        None => audio.record_clip_default(reason).await,
    };
    match result {
        Ok(p) => ApiResponse::success(ClipResponse { path: p.display().to_string() }),
        Err(e) => ApiResponse::error(e.to_string()),
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
    async fn rejects_bad_seconds() {
        let r = record(&audio(), ClipRequest { reason: None, seconds: Some(-1.0) }).await;
        assert!(!r.success);
    }

    #[tokio::test]
    async fn errors_when_audio_not_started() {
        let r = record(&audio(), ClipRequest::default()).await;
        assert!(!r.success);
        assert!(r.error.unwrap().contains("not started"));
    }
}
