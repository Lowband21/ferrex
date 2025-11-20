use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
};
use ferrex_core::MetadataExtractor;
use serde_json::json;
use std::path::PathBuf;

use crate::{
    transcoding::{job::JobPriority, profiles::TranscodingProfile},
    AppState,
};
use uuid;

/// Test endpoint to extract metadata from a single file
pub async fn test_metadata_extraction(
    State(_state): State<AppState>,
    Path(file_path): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let path = PathBuf::from(file_path);

    if !path.exists() {
        return Ok(Json(json!({
            "error": "File not found",
            "path": path.to_string_lossy()
        })));
    }

    let mut extractor = MetadataExtractor::new();

    match extractor.extract_metadata(&path) {
        Ok(metadata) => {
            // Log the extracted metadata
            tracing::info!("Extracted metadata: {:?}", metadata);

            Ok(Json(json!({
                "success": true,
                "metadata": metadata,
                "hdr_info": {
                    "color_primaries": metadata.color_primaries,
                    "color_transfer": metadata.color_transfer,
                    "color_space": metadata.color_space,
                    "bit_depth": metadata.bit_depth,
                    "is_hdr": metadata.color_transfer.as_ref()
                        .map(|t| t == "smpte2084" || t == "arib-std-b67")
                        .unwrap_or(false)
                }
            })))
        }
        Err(e) => {
            tracing::error!("Failed to extract metadata: {}", e);
            Ok(Json(json!({
                "error": format!("Failed to extract metadata: {}", e)
            })))
        }
    }
}

/// Test endpoint to start a transcoding job
pub async fn test_transcoding(
    State(state): State<AppState>,
    Path(file_path): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let path = PathBuf::from(&file_path);

    if !path.exists() {
        return Ok(Json(json!({
            "error": "File not found",
            "path": path.to_string_lossy()
        })));
    }

    // First, we need to add this file to the database to get a media_id
    // For testing, we'll create a temporary media entry
    let media_id = uuid::Uuid::new_v4().to_string();

    // Extract metadata first
    let mut extractor = MetadataExtractor::new();
    let metadata = match extractor.extract_metadata(&path) {
        Ok(m) => Some(m),
        Err(e) => {
            tracing::warn!("Failed to extract metadata: {}", e);
            None
        }
    };

    // Create a media file entry
    let media_file = ferrex_core::MediaFile {
        id: uuid::Uuid::parse_str(&media_id).unwrap(),
        path: path.clone(),
        filename: path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("test_media.mp4")
            .to_string(),
        size: std::fs::metadata(&path).ok().map(|m| m.len()).unwrap_or(0),
        created_at: chrono::Utc::now(),
        media_file_metadata: metadata,
        library_id: uuid::Uuid::nil(),
    };

    // Store in database
    if let Err(e) = state.db.backend().store_media(media_file).await {
        return Ok(Json(json!({
            "error": format!("Failed to add media to database: {}", e)
        })));
    }

    // Create a basic transcoding profile
    let profile = TranscodingProfile {
        name: "test_1080p".to_string(),
        video_codec: "libx264".to_string(),
        audio_codec: "aac".to_string(),
        video_bitrate: "5000k".to_string(),
        audio_bitrate: "192k".to_string(),
        resolution: Some("1920x1080".to_string()),
        preset: "medium".to_string(),
        apply_tone_mapping: false,
    };

    // Now start transcoding with the media_id
    match state
        .transcoding_service
        .start_transcoding(&media_id, profile, None, Some(JobPriority::Normal))
        .await
    {
        Ok(job_id) => Ok(Json(json!({
            "success": true,
            "job_id": job_id,
            "media_id": media_id,
            "message": "Transcoding job started"
        }))),
        Err(e) => {
            tracing::error!("Failed to start transcoding: {}", e);
            // Clean up the test media entry
            let _ = state.db.backend().delete_media(&media_id).await;
            Ok(Json(json!({
                "error": format!("Failed to start transcoding: {}", e)
            })))
        }
    }
}

/// Test endpoint to check transcoding job status
pub async fn test_transcode_status(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    match state.transcoding_service.get_job_status(&job_id).await {
        Some(job) => {
            Ok(Json(json!({
                "success": true,
                "job": {
                    "id": job.id,
                    "media_path": job.media_id, // This is actually the file path
                    "status": job.status,
                    "profile": job.profile.name,
                    "output_dir": job.output_dir.to_string_lossy(),
                    "playlist_path": job.playlist_path.to_string_lossy(),
                    "segments": job.segments,
                    "retry_count": job.retry_count,
                    "error": job.error,
                    "process_pid": job.process_pid,
                }
            })))
        }
        None => Ok(Json(json!({
            "error": "Job not found",
            "job_id": job_id
        }))),
    }
}

/// Test endpoint to start HLS adaptive streaming
pub async fn test_hls_streaming(
    State(state): State<AppState>,
    Path(file_path): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let path = PathBuf::from(&file_path);

    if !path.exists() {
        return Ok(Json(json!({
            "error": "File not found",
            "path": path.to_string_lossy()
        })));
    }

    // Create a temporary media entry
    let media_id = uuid::Uuid::new_v4().to_string();

    // Extract metadata
    let mut extractor = MetadataExtractor::new();
    let metadata = match extractor.extract_metadata(&path) {
        Ok(m) => Some(m),
        Err(e) => {
            tracing::warn!("Failed to extract metadata: {}", e);
            None
        }
    };

    // Create media file entry
    let media_file = ferrex_core::MediaFile {
        id: uuid::Uuid::parse_str(&media_id).unwrap(),
        path: path.clone(),
        filename: path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("test_media.mp4")
            .to_string(),
        size: std::fs::metadata(&path).ok().map(|m| m.len()).unwrap_or(0),
        created_at: chrono::Utc::now(),
        media_file_metadata: metadata,
        library_id: uuid::Uuid::nil(),
    };

    // Store in database
    let id = match state.db.backend().store_media(media_file).await {
        Err(e) => {
            return Ok(Json(json!({
            "error": format!("Failed to add media to database: {}", e)
            })))
        }
        Ok(id) => id,
    };

    // Start adaptive transcoding
    match state
        .transcoding_service
        .start_adaptive_transcoding(id, Some(JobPriority::High))
        .await
    {
        Ok(job_id) => {
            let master_url = format!("http://localhost:3000/transcode/{}/master.m3u8", id);
            Ok(Json(json!({
                "success": true,
                "job_id": job_id,
                "media_id": id,
                "master_playlist_url": master_url,
                "variant_urls": {
                    "360p": format!("http://localhost:3000/transcode/{}/variant/adaptive_360p/playlist.m3u8", media_id),
                    "480p": format!("http://localhost:3000/transcode/{}/variant/adaptive_480p/playlist.m3u8", media_id),
                    "720p": format!("http://localhost:3000/transcode/{}/variant/adaptive_720p/playlist.m3u8", media_id),
                    "1080p": format!("http://localhost:3000/transcode/{}/variant/adaptive_1080p/playlist.m3u8", media_id),
                },
                "message": "HLS adaptive streaming started. Test the master_playlist_url in VLC.",
                "vlc_command": format!("vlc {}", master_url)
            })))
        }
        Err(e) => {
            tracing::error!("Failed to start adaptive transcoding: {}", e);
            let _ = state.db.backend().delete_media(&media_id).await;
            Ok(Json(json!({
                "error": format!("Failed to start adaptive transcoding: {}", e)
            })))
        }
    }
}
