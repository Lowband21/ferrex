use crate::{media::scan::scan_manager::{media_events_sse, scan_progress_sse}, AppState};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Json, Sse},
};
use ferrex_core::{
    database::traits::{FolderProcessingStatus, MediaDatabaseTrait},
    LibraryID, MediaType, ScanRequest, ScanResponse, ScanStatus,
};
use ferrex_core::{EpisodeID, MovieID, SeasonID, SeriesID};
use futures::future::join_all;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{error, info, warn};
use uuid::Uuid;


/// POST /libraries/scan/pending
/// Start scans for all libraries that have pending/changed folders
pub async fn scan_all_libraries_handler(
    State(state): State<AppState>,
    Query(force_refresh): Query<bool>,
) -> Result<Json<ScanResponse>, StatusCode> {
    let libraries = state
        .db
        .backend()
        .list_libraries()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;


        let ids: Vec<LibraryID> = libraries.clone().into_iter().filter(|l| l.enabled).map(|library| library.id).collect();

        let mut folder_count: usize = 0;
        for id in &ids {
            folder_count += pending_folder_count_for_library(&state, *id).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        }


    let scan_result = state
            .scan_manager
            .start_library_scan(Arc::new(libraries), force_refresh)
            .await;

    let scan_id = match scan_result {
        Ok(scan_id) => {
            for id in &ids {
                let _ = state.db.backend().update_library_last_scan(&id).await;
            }

            scan_id
        },
        Err(e) => {
            error!("Failed to start library scan: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    };

    Ok(Json(ScanResponse::new(ScanStatus::Scanning, Some(scan_id), "Scan started".to_string())))
}


// Library scan handler
#[axum::debug_handler]
pub async fn scan_library_handler(
    State(state): State<AppState>,
    Json(req): Json<ScanRequest>,
) -> Result<Json<ScanResponse>, StatusCode> {
    let id = req.library_id;
    info!("Scan request for library: {}", id);

    let library = state
        .db
        .backend()
        .get_library(&id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    // Check if library is enabled
    if !library.enabled {
        return Ok(Json(ScanResponse::new_failed("Library is disabled".to_string())));
    }

    let library_name = library.name.clone();

    let scan_result = state
            .scan_manager
            .start_library_scan(Arc::new(vec![library]), req.force_refresh)
            .await;

    match scan_result {
        Ok(scan_id) => {
            // Update library last scan time
            let _ = state
                .db
                .backend()
                .update_library_last_scan(&id)
                .await;

            info!("Library scan started with ID: {}", scan_id);
            Ok(Json(ScanResponse::new_scan_started(scan_id, format!("Scan started for library: {}", library_name))))
        }
        Err(e) => {
            error!("Failed to start library scan: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// Count folders needing scan for a single library
async fn pending_folder_count_for_library(state: &AppState, lib_id: LibraryID) -> Result<usize, anyhow::Error> {
    let folders = state.database.backend().get_folder_inventory(lib_id).await?;
    let now = chrono::Utc::now();
    let count = folders
        .into_iter()
        .filter(|f| match f.processing_status {
            FolderProcessingStatus::Pending | FolderProcessingStatus::Queued => true,
            FolderProcessingStatus::Failed => f
                .next_retry_at
                .map(|t| t <= now)
                .unwrap_or(true),
            _ => false,
        })
        .count();
    Ok(count)
}


/// GET /libraries/{id}/scan/pending-count
pub async fn pending_count_for_library_handler(
    State(state): State<AppState>,
    Path(library_id): Path<Uuid>,
) -> Result<Json<Value>, StatusCode> {
    let lib_id = LibraryID(library_id);
    match pending_folder_count_for_library(&state, lib_id).await {
        Ok(count) => Ok(Json(json!({ "status": "success", "count": count }))),
        Err(e) => {
            warn!("Failed to count pending folders: {}", e);
            Ok(Json(json!({ "status": "error", "error": e.to_string() })))
        }
    }
}

pub async fn scan_progress_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, StatusCode> {
    match state.scan_manager.get_scan_progress(&id).await {
        Some(progress) => Ok(Json(json!({
            "status": "success",
            "progress": progress
        }))),
        None => Ok(Json(json!({
            "status": "error",
            "error": "Scan not found"
        }))),
    }
}

pub async fn scan_progress_sse_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<
    Sse<impl futures_util::Stream<Item = Result<axum::response::sse::Event, anyhow::Error>>>,
    StatusCode,
> {
    info!("SSE connection requested for scan {}", id);
    let receiver = state.scan_manager.subscribe_to_progress(id).await;
    Ok(scan_progress_sse(id, receiver))
}

pub async fn media_events_sse_handler(
    State(state): State<AppState>,
) -> Result<
    Sse<impl futures_util::Stream<Item = Result<axum::response::sse::Event, anyhow::Error>>>,
    StatusCode,
> {
    info!("SSE connection requested for media events");
    let receiver = state.scan_manager.subscribe_to_media_events().await;
    Ok(media_events_sse(receiver))
}

pub async fn active_scans_handler(
    State(state): State<AppState>,
) -> Result<Json<Value>, StatusCode> {
    let active_scans = state.scan_manager.get_active_scans().await;
    Ok(Json(json!({
        "status": "success",
        "scans": active_scans,
        "count": active_scans.len()
    })))
}

pub async fn scan_history_handler(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, StatusCode> {
    let limit = params
        .get("limit")
        .and_then(|l| l.parse::<usize>().ok())
        .unwrap_or(10);

    let history = state.scan_manager.get_scan_history(limit).await;
    Ok(Json(json!({
        "status": "success",
        "history": history,
        "count": history.len()
    })))
}

pub async fn cancel_scan_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> ScanResponse {
    match state.scan_manager.cancel_scan(&id).await {
        Ok(_) => ScanResponse::new_canceled(id),
        Err(e) => ScanResponse::new(ScanStatus::Scanning, Some(id), e.to_string())
    }
}
