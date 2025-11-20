use crate::AppState;
use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use ferrex_core::database::traits::ImageLookupParams;
use httpdate::{fmt_http_date, parse_http_date};
use serde::Deserialize;
use std::path::{Path as FsPath, PathBuf};
use tracing::{debug, error, info, warn};

#[derive(Debug, Deserialize)]
pub struct ImageQuery {
    /// Legacy TMDB size (e.g., w185, w500, original)
    size: Option<String>,
    w: Option<u32>,
    max_width: Option<u32>,
    /// Preferred output format (future use): avif|webp|jpeg
    fmt: Option<String>,
    /// Quality hint 0-100 (future use)
    quality: Option<u8>,
}

/// Serve cached images as streamed bytes with proper HTTP caching
/// Path format: /images/{type}/{id}/{category}/{index}
/// Example: /images/movie/12345/poster/0
pub async fn serve_image_handler(
    State(state): State<AppState>,
    Path((media_type, media_id, category, index)): Path<(String, String, String, u32)>,
    Query(query): Query<ImageQuery>,
    headers: HeaderMap,
) -> impl IntoResponse {
    info!(
        "Image request: type={}, id={}, category={}, index={}, size={:?}, w={:?}, fmt={:?}",
        media_type,
        media_id,
        category,
        index,
        query.size,
        query.w.or(query.max_width),
        query.fmt
    );

    // Validate media type
    if !["movie", "series", "season", "episode", "person"].contains(&media_type.as_str()) {
        warn!("Invalid media type: {}", media_type);
        return Err(StatusCode::BAD_REQUEST);
    }

    // Validate category
    if !["poster", "backdrop", "logo", "still", "profile"].contains(&category.as_str()) {
        warn!("Invalid image category: {}", category);
        return Err(StatusCode::BAD_REQUEST);
    }

    // Determine desired variant name (TMDB size) from w/maxWidth or legacy size
    let requested_w = query.w.or(query.max_width);
    let desired_variant = if let Some(w) = requested_w {
        Some(map_width_to_tmdb_variant(&category, w).to_string())
    } else {
        query.size.clone().or_else(|| Some("w500".to_string()))
    };

    // Create lookup parameters (using desired variant string)
    let params = ImageLookupParams {
        media_type: media_type.clone(),
        media_id: media_id.clone(),
        image_type: category.clone(),
        index,
        variant: desired_variant.clone(),
    };

    // Try to ensure the desired variant asynchronously
    let ready_path = match state.image_service.ensure_variant_async(&params).await {
        Ok(p) => p,
        Err(e) => {
            error!("ensure_variant_async failed: {}", e);
            None
        }
    };

    // If ready, stream it; else fall back to best available
    let (image_path, served_variant) = if let Some(path) = ready_path {
        (
            path,
            params.variant.clone().unwrap_or_else(|| "w500".to_string()),
        )
    } else {
        match state.image_service.pick_best_available(&params).await {
            Ok(Some((fallback_path, fallback_variant))) => (fallback_path, fallback_variant),
            Ok(None) => {
                warn!(
                    "No fallback available: {}/{}/{}/{}",
                    media_type, media_id, category, index
                );
                return Err(StatusCode::NOT_FOUND);
            }
            Err(e) => {
                error!("Failed to select fallback: {}", e);
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            }
        }
    };

    // Normalize path to respect configured CACHE_DIR if DB stored relative paths
    let resolved_path = normalize_image_path(&image_path, state.config.cache_dir.as_path());

    // Determine content type based on file extension or metadata
    let content_type = match resolved_path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_lowercase())
        .as_deref()
    {
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        Some("avif") => "image/avif",
        _ => "image/jpeg",
    };

    // Get file metadata for caching headers
    let meta = match tokio::fs::metadata(&resolved_path).await {
        Ok(m) => m,
        Err(e) => {
            error!("Failed to stat image file {:?}: {}", resolved_path, e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    let file_size = meta.len();
    let modified = match meta.modified() {
        Ok(t) => t,
        Err(e) => {
            warn!("No modified time for {:?}: {}", image_path, e);
            std::time::SystemTime::UNIX_EPOCH
        }
    };
    let last_modified = fmt_http_date(modified);

    // Weak ETag based on size and mtime
    let secs = modified
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_else(|_| std::time::Duration::from_secs(0))
        .as_secs();
    let etag_value = format!("W/\"{}-{}\"", file_size, secs);

    // Conditional requests: If-None-Match / If-Modified-Since
    if let Some(if_none_match) = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
    {
        if if_none_match.split(',').any(|t| t.trim() == etag_value) {
            return Ok::<_, StatusCode>(
                Response::builder()
                    .status(StatusCode::NOT_MODIFIED)
                    .header(header::ETAG, etag_value)
                    .header(header::LAST_MODIFIED, last_modified)
                    .header(header::CACHE_CONTROL, "public, max-age=86400")
                    .header(
                        "X-Variant-Requested",
                        desired_variant.unwrap_or_else(|| "w500".to_string()),
                    )
                    .header("X-Variant-Served", served_variant)
                    .body(Body::empty())
                    .unwrap(),
            );
        }
    }

    if let Some(if_modified_since) = headers
        .get(header::IF_MODIFIED_SINCE)
        .and_then(|v| v.to_str().ok())
    {
        if let Ok(since_time) = parse_http_date(if_modified_since) {
            if modified <= since_time {
                return Ok::<_, StatusCode>(
                    Response::builder()
                        .status(StatusCode::NOT_MODIFIED)
                        .header(header::ETAG, etag_value)
                        .header(header::LAST_MODIFIED, last_modified)
                        .header(header::CACHE_CONTROL, "public, max-age=86400")
                        .header(
                            "X-Variant-Requested",
                            desired_variant.unwrap_or_else(|| "w500".to_string()),
                        )
                        .header("X-Variant-Served", served_variant)
                        .body(Body::empty())
                        .unwrap(),
                );
            }
        }
    }

    // Read entire file and send as single buffer (faster for small/medium images)
    let data = match tokio::fs::read(&resolved_path).await {
        Ok(d) => d,
        Err(e) => {
            error!("Failed to read image file {:?}: {}", resolved_path, e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    let mut resp = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CONTENT_LENGTH, data.len().to_string())
        .header(header::ETAG, etag_value)
        .header(header::LAST_MODIFIED, last_modified)
        .header(header::CACHE_CONTROL, "public, max-age=86400")
        .header("X-Variant-Served", served_variant);

    if let Some(req) = desired_variant {
        resp = resp.header("X-Variant-Requested", req);
    }

    Ok::<_, StatusCode>(resp.body(Body::from(data)).unwrap())
}

/// Normalize a stored image path to the current configured cache root.
/// This handles older DB entries that stored relative paths like "./cache/images/...".
fn normalize_image_path(original: &FsPath, cache_root: &FsPath) -> PathBuf {
    if original.is_absolute() {
        return original.to_path_buf();
    }

    let s = original.to_string_lossy();
    if let Some(rest) = s.strip_prefix("./cache/") {
        return cache_root.join(rest);
    }
    if let Some(rest) = s.strip_prefix("cache/") {
        return cache_root.join(rest);
    }

    // Assume any other relative path is relative to the cache root
    cache_root.join(s.as_ref())
}

/// Map desired width to a TMDB variant string for a given category
fn map_width_to_tmdb_variant(category: &str, w: u32) -> &'static str {
    // Choose nearest bucket >= requested; fall back to largest known or original
    match category {
        // Posters: 92, 154, 185, 342, 500, 780, original
        "poster" => match w {
            0..=92 => "w92",
            93..=154 => "w154",
            155..=185 => "w185",
            186..=342 => "w342",
            343..=500 => "w500",
            501..=780 => "w780",
            _ => "original",
        },
        // Backdrops: 300, 780, 1280, original
        "backdrop" => match w {
            0..=300 => "w300",
            301..=780 => "w780",
            781..=1280 => "w1280",
            _ => "original",
        },
        // Stills: 92, 185, 300, 500, original
        "still" => match w {
            0..=92 => "w92",
            93..=185 => "w185",
            186..=300 => "w300",
            301..=500 => "w500",
            _ => "original",
        },
        // Logos: prefer original to avoid artifacts
        "logo" => "original",
        // Profiles: 45, 185, h632 (height), map width thresholds to 45 or 185 else original
        "profile" => match w {
            0..=45 => "w45",
            46..=185 => "w185",
            _ => "original",
        },
        _ => "w500",
    }
}
