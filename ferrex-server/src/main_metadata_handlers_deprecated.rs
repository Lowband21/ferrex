// This file contains the deprecated metadata handlers that were moved from main.rs
// These will be removed once the client is updated to use the new reference-based API

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::{error, info, warn};
use ferrex_core::{MediaFilters, MetadataExtractor};
use crate::AppState;

#[derive(Deserialize)]
pub struct MetadataRequest {
    pub path: String,
}

#[derive(Deserialize)]
pub struct BatchMetadataRequest {
    pub media_ids: Vec<String>,
    pub priority: Option<String>, // "posters_only" or "full"
}

#[derive(Deserialize)]
pub struct QueueMissingMetadataRequest {
    pub media_ids: Vec<String>,
}

pub async fn metadata_handler(Json(request): Json<MetadataRequest>) -> Result<Json<Value>, StatusCode> {
    info!("Metadata extraction request for: {}", request.path);

    let mut extractor = MetadataExtractor::new();

    match extractor.extract_metadata(&request.path) {
        Ok(metadata) => {
            info!("Metadata extraction successful for: {}", request.path);
            Ok(Json(json!({
                "status": "success",
                "metadata": metadata
            })))
        }
        Err(e) => {
            warn!("Metadata extraction failed for {}: {}", request.path, e);
            Ok(Json(json!({
                "status": "error",
                "error": e.to_string()
            })))
        }
    }
}

pub async fn fetch_metadata_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    info!("Metadata fetch request for media ID: {}", id);

    // This is deprecated functionality
    Err(StatusCode::NOT_IMPLEMENTED)
}

pub async fn fetch_show_metadata_handler(
    State(state): State<AppState>,
    Path(show_name): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    info!("TV show metadata fetch request for show: {}", show_name);
    
    // This is deprecated functionality
    Err(StatusCode::NOT_IMPLEMENTED)
}

pub async fn fetch_metadata_batch_handler(
    State(state): State<AppState>,
    Json(request): Json<BatchMetadataRequest>,
) -> Result<Json<Value>, StatusCode> {
    info!(
        "Batch metadata request for {} items",
        request.media_ids.len()
    );

    // This is deprecated functionality
    Err(StatusCode::NOT_IMPLEMENTED)
}

pub async fn queue_missing_metadata_handler(
    State(state): State<AppState>,
    Json(request): Json<QueueMissingMetadataRequest>,
) -> Result<Json<Value>, StatusCode> {
    info!(
        "Queuing metadata fetch for {} items",
        request.media_ids.len()
    );

    // This is deprecated functionality
    Err(StatusCode::NOT_IMPLEMENTED)
}