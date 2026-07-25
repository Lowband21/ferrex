use std::{fmt, str::FromStr};

use axum::{
    Extension, Json,
    extract::{Path as AxumPath, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::Response,
};
use ferrex_core::{
    api::types::ApiResponse,
    domain::users::{auth::domain::value_objects::SessionScope, user::User},
};
use ferrex_model::{
    StartTranscodeRequest, TranscodeJobStatusResponse, TranscodeQualityProfile,
};
use serde::Deserialize;
use tokio_util::io::ReaderStream;
use tracing::{error, warn};
use uuid::Uuid;

use crate::{
    handlers::stream::stream_handlers::{
        PlaybackHttpError, ensure_playback_source_available,
        load_playback_source,
    },
    infra::{
        app_state::AppState,
        transcode::{TranscodeStatusLookupError, rendition_root},
    },
};

#[derive(Deserialize)]
pub struct TranscodeAuthQuery {
    #[serde(default)]
    access_token: Option<String>,
}

impl fmt::Debug for TranscodeAuthQuery {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TranscodeAuthQuery")
            .field(
                "access_token",
                &self.access_token.as_ref().map(|_| "<redacted>"),
            )
            .finish()
    }
}

pub async fn start_transcode_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    AxumPath(media_id): AxumPath<Uuid>,
    Json(request): Json<StartTranscodeRequest>,
) -> Result<Json<ApiResponse<TranscodeJobStatusResponse>>, PlaybackHttpError> {
    let source = load_playback_source(&state, media_id).await?;
    ensure_playback_source_available(&state, &source)?;
    let status = state
        .transcode_manager()
        .start(user.id, source, request.profile)
        .await;
    Ok(Json(ApiResponse::success(status)))
}

pub async fn transcode_status_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    AxumPath(job_id): AxumPath<Uuid>,
) -> Result<Json<ApiResponse<TranscodeJobStatusResponse>>, StatusCode> {
    match state.transcode_manager().status(user.id, job_id).await {
        Ok(status) => Ok(Json(ApiResponse::success(status))),
        Err(TranscodeStatusLookupError::NotFound) => Err(StatusCode::NOT_FOUND),
        Err(TranscodeStatusLookupError::Forbidden) => {
            Err(StatusCode::FORBIDDEN)
        }
    }
}

pub async fn transcode_asset_handler(
    State(state): State<AppState>,
    AxumPath((media_id, profile, asset)): AxumPath<(Uuid, String, String)>,
    headers: HeaderMap,
    Query(query): Query<TranscodeAuthQuery>,
) -> Result<Response, PlaybackHttpError> {
    validate_playback_token(&state, &headers, query.access_token.as_deref())
        .await?;

    let source = load_playback_source(&state, media_id).await?;
    ensure_playback_source_available(&state, &source)?;

    let profile = TranscodeQualityProfile::from_str(&profile)
        .map_err(|_| PlaybackHttpError::media_not_found())?;
    if !safe_asset_name(&asset) {
        return Err(PlaybackHttpError::media_not_found());
    }

    let root =
        rendition_root(state.config().transcode_cache_dir(), media_id, profile);
    let path = root.join(&asset);
    let metadata = tokio::fs::symlink_metadata(&path)
        .await
        .map_err(|_| PlaybackHttpError::file_missing())?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(PlaybackHttpError::file_missing());
    }
    let file = tokio::fs::File::open(&path).await.map_err(|err| {
        warn!(?err, %media_id, profile = %profile, "could not open transcode asset");
        PlaybackHttpError::file_missing()
    })?;
    let content_type = if asset == "index.m3u8" {
        "application/vnd.apple.mpegurl"
    } else {
        "video/mp2t"
    };

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CONTENT_LENGTH, metadata.len().to_string())
        .header(header::CACHE_CONTROL, "private, no-store")
        .body(axum::body::Body::from_stream(ReaderStream::new(file)))
        .map_err(|err| {
            error!(?err, %media_id, "could not build transcode response");
            PlaybackHttpError::internal()
        })
}

async fn validate_playback_token(
    state: &AppState,
    headers: &HeaderMap,
    query_token: Option<&str>,
) -> Result<(), PlaybackHttpError> {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .or(query_token)
        .ok_or_else(PlaybackHttpError::missing_token)?;

    let validated = state
        .auth_service()
        .validate_session_token(token)
        .await
        .map_err(|_| PlaybackHttpError::invalid_token())?;
    match validated.scope {
        SessionScope::Full | SessionScope::Playback => Ok(()),
    }
}

fn safe_asset_name(asset: &str) -> bool {
    if asset == "index.m3u8" {
        return true;
    }
    let Some(sequence) = asset
        .strip_prefix("segment-")
        .and_then(|value| value.strip_suffix(".ts"))
    else {
        return false;
    };
    sequence.len() == 5 && sequence.bytes().all(|byte| byte.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transcode_asset_names_are_closed_and_non_traversable() {
        for accepted in ["index.m3u8", "segment-00000.ts", "segment-99999.ts"] {
            assert!(safe_asset_name(accepted), "{accepted}");
        }
        for rejected in [
            "master.m3u8",
            "segment-1.ts",
            "segment-00000.m4s",
            "../index.m3u8",
            "%2e%2e",
            "segment-0000a.ts",
        ] {
            assert!(!safe_asset_name(rejected), "{rejected}");
        }
    }
}
