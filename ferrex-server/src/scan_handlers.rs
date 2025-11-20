use crate::{scan_manager, AppState};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{Json, Sse},
};
use ferrex_core::ScanRequest;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{error, info, warn};
use uuid::Uuid;

// Library scan handler
pub async fn scan_library_handler(
    State(state): State<AppState>,
    Path(library_id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, StatusCode> {
    info!("Scan request for library: {}", library_id);

    // Get the library details
    info!("Fetching library with ID: {}", library_id);
    let library = match state.db.backend().get_library(&library_id).await {
        Ok(Some(lib)) => {
            info!(
                "Found library: {} (ID: {}, Type: {:?})",
                lib.name, lib.id, lib.library_type
            );
            lib
        }
        Ok(None) => {
            warn!("Library not found: {}", library_id);
            return Ok(Json(json!({
                "status": "error",
                "error": "Library not found"
            })));
        }
        Err(e) => {
            warn!("Failed to get library: {}", e);
            return Ok(Json(json!({
                "status": "error",
                "error": e.to_string()
            })));
        }
    };

    // Check if library is enabled
    if !library.enabled {
        return Ok(Json(json!({
            "status": "error",
            "error": "Library is disabled"
        })));
    }

    // Check for force rescan parameter
    let force_rescan = params
        .get("force")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false);

    // Check if we should use streaming scanner (default to true for libraries)
    let use_streaming = params
        .get("streaming")
        .map(|v| v != "false" && v != "0")
        .unwrap_or(true);

    let library_name = library.name.clone();

    // Start the scan
    let scan_result = if use_streaming {
        // Use new streaming scanner for better performance
        state
            .scan_manager
            .start_library_scan(Arc::new(library), force_rescan)
            .await
    } else {
        // Convert to library scan
        let temp_library = Arc::new(ferrex_core::Library {
            id: Uuid::new_v4(),
            name: format!(
                "Temporary scan for {}",
                library
                    .paths
                    .first()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default()
            ),
            library_type: library.library_type.clone(),
            paths: library.paths.clone(),
            scan_interval_minutes: 0,
            enabled: true,
            last_scan: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            media: None,
            auto_scan: false,
            watch_for_changes: false,
            analyze_on_scan: false,
            max_retry_attempts: 3,
        });
        state
            .scan_manager
            .start_library_scan(temp_library, force_rescan)
            .await
    };

    match scan_result {
        Ok(scan_id) => {
            // Update library last scan time
            let _ = state
                .db
                .backend()
                .update_library_last_scan(&library_id)
                .await;

            info!("Library scan started with ID: {}", scan_id);
            Ok(Json(json!({
                "status": "success",
                "scan_id": scan_id,
                "message": format!("Scan started for library: {}", library_name)
            })))
        }
        Err(e) => {
            warn!("Failed to start library scan: {}", e);
            Ok(Json(json!({
                "status": "error",
                "error": e.to_string()
            })))
        }
    }
}

// New scan management handlers
pub async fn start_scan_handler(
    State(state): State<AppState>,
    Json(request): Json<ScanRequest>,
) -> Result<Json<Value>, StatusCode> {
    // Convert ScanRequest to library scan
    let library_id = request.library_id.unwrap_or_else(Uuid::new_v4);
    let paths = if let Some(paths) = request.paths {
        paths.into_iter().map(std::path::PathBuf::from).collect()
    } else if let Some(path) = request.path {
        vec![std::path::PathBuf::from(path)]
    } else {
        vec![]
    };

    let temp_library = Arc::new(ferrex_core::Library {
        id: library_id,
        name: format!("Scan {}", library_id),
        library_type: request
            .library_type
            .unwrap_or(ferrex_core::LibraryType::Movies),
        paths,
        scan_interval_minutes: 0,
        enabled: true,
        last_scan: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        media: None,
        auto_scan: false,
        watch_for_changes: false,
        analyze_on_scan: false,
        max_retry_attempts: 3,
    });

    match state
        .scan_manager
        .start_library_scan(temp_library, request.force_rescan)
        .await
    {
        Ok(scan_id) => Ok(Json(json!({
            "status": "success",
            "scan_id": scan_id,
            "message": "Scan started successfully"
        }))),
        Err(e) => {
            warn!("Failed to start scan: {}", e);
            Ok(Json(json!({
                "status": "error",
                "error": e.to_string()
            })))
        }
    }
}

pub async fn scan_progress_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
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
    Path(id): Path<String>,
) -> Result<
    Sse<impl futures_util::Stream<Item = Result<axum::response::sse::Event, anyhow::Error>>>,
    StatusCode,
> {
    info!("SSE connection requested for scan {}", id);
    let receiver = state.scan_manager.subscribe_to_progress(id.clone()).await;
    Ok(scan_manager::scan_progress_sse(id, receiver))
}

pub async fn media_events_sse_handler(
    State(state): State<AppState>,
) -> Result<
    Sse<impl futures_util::Stream<Item = Result<axum::response::sse::Event, anyhow::Error>>>,
    StatusCode,
> {
    info!("SSE connection requested for media events");
    let receiver = state.scan_manager.subscribe_to_media_events().await;
    Ok(scan_manager::media_events_sse(receiver))
}

/// Scan all enabled libraries
pub async fn scan_all_libraries_handler(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, StatusCode> {
    info!("Scan request for all libraries");

    // Get force rescan parameter
    let force_rescan = params
        .get("force")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false);

    // Get all libraries
    let libraries = match state.db.backend().list_libraries().await {
        Ok(libs) => libs,
        Err(e) => {
            error!("Failed to get libraries: {}", e);
            return Ok(Json(json!({
                "status": "error",
                "error": e.to_string()
            })));
        }
    };

    let enabled_libraries: Vec<_> = libraries.into_iter().filter(|lib| lib.enabled).collect();

    if enabled_libraries.is_empty() {
        return Ok(Json(json!({
            "status": "error",
            "error": "No enabled libraries found"
        })));
    }

    info!(
        "Found {} enabled libraries to scan",
        enabled_libraries.len()
    );

    let mut scan_ids = Vec::new();
    let mut errors = Vec::new();

    // Start scan for each enabled library
    for library in enabled_libraries {
        info!(
            "Starting scan for library: {} ({})",
            library.name, library.id
        );

        match state
            .scan_manager
            .start_library_scan(Arc::new(library.clone()), force_rescan)
            .await
        {
            Ok(scan_id) => {
                info!("Started scan {} for library {}", scan_id, library.name);
                scan_ids.push(json!({
                    "library_id": library.id,
                    "library_name": library.name,
                    "scan_id": scan_id
                }));

                // Update last scan time
                let _ = state
                    .db
                    .backend()
                    .update_library_last_scan(&library.id.to_string())
                    .await;
            }
            Err(e) => {
                error!("Failed to start scan for library {}: {}", library.name, e);
                errors.push(json!({
                    "library_id": library.id,
                    "library_name": library.name,
                    "error": e.to_string()
                }));
            }
        }
    }

    if scan_ids.is_empty() && !errors.is_empty() {
        return Ok(Json(json!({
            "status": "error",
            "message": "Failed to start any scans",
            "errors": errors
        })));
    }

    Ok(Json(json!({
        "status": "success",
        "message": format!("Started {} scan(s)", scan_ids.len()),
        "scans": scan_ids,
        "errors": if errors.is_empty() { None } else { Some(errors) }
    })))
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
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    match state.scan_manager.cancel_scan(&id).await {
        Ok(_) => Ok(Json(json!({
            "status": "success",
            "message": "Scan cancelled"
        }))),
        Err(e) => Ok(Json(json!({
            "status": "error",
            "error": e.to_string()
        }))),
    }
}
