use crate::{
    handlers::media::image_validation::validate_magic_bytes,
    infra::app_state::AppState,
};

use axum::{
    body::Body,
    extract::{self, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use ferrex_core::{
    database::traits::ImageRecord, error::MediaError,
    infra::image_service::CachePolicy,
};
use ferrex_model::{ImageSize, image::ImageQuery};

// Used by demo feature
#[allow(unused)]
use ferrex_model::image::ImageVariant;

use httpdate::{fmt_http_date, parse_http_date};
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use thiserror::Error;
use tracing::{error, warn};

/// Maximum image size to serve (50 MB). Images larger than this are rejected.
/// This prevents memory exhaustion from malicious or corrupted files.
const MAX_IMAGE_SIZE_BYTES: u64 = 50 * 1024 * 1024;

/// Size threshold for logging warnings about large images (5 MB).
const LARGE_IMAGE_WARN_THRESHOLD: u64 = 5 * 1024 * 1024;

// Simple counters for image responses on this process
static IMAGE_RESP_200: AtomicU64 = AtomicU64::new(0);
static IMAGE_RESP_304: AtomicU64 = AtomicU64::new(0);
static IMAGE_RESP_MISS: AtomicU64 = AtomicU64::new(0);

/// Errors that can occur during image lookup or loading.
#[derive(Debug, Error)]
enum ImageError {
    #[error("image too large: {0} bytes")]
    TooLarge(u64),
    #[error("cache read failed: {0}")]
    CacheRead(#[from] MediaError),
    #[error("cache entry missing or corrupt")]
    CorruptOrMissingCache,
    #[error("not modified")]
    NotModified(ImageNotModified),
    #[cfg(feature = "demo")]
    #[error("not available in demo mode")]
    DemoError,
}

/// Indicates a conditional request matched and we should return 304.
#[derive(Debug)]
struct ImageNotModified {
    etag: String,
    last_modified: String,
}

/// Fully-loaded cached image with headers already computed.
#[derive(Debug)]
struct CachedImage {
    bytes: Vec<u8>,
    content_type: &'static str,
    len: usize,
    etag: String,
    last_modified: String,
}

/// Serve cached images
pub async fn serve_image_handler(
    headers: HeaderMap,
    State(state): State<AppState>,
    extract::Json(query): extract::Json<ImageQuery>,
) -> impl IntoResponse {
    let t_start = std::time::Instant::now();

    let elapsed = || t_start.elapsed().as_millis();

    // Fast path: serve cached bytes if available, otherwise enqueue the image
    let uow = state.unit_of_work();
    let repo = uow.images.clone();

    let record = match repo.lookup_cached_image(query.iid, query.imz).await {
        Ok(v) => v,
        Err(e) => {
            error!("Failed to lookup cached image record: {}", e);
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    if let Some(record) = record {
        match get_cached_image(&state, &record, &headers).await {
            Ok(cached) => {
                IMAGE_RESP_200.fetch_add(1, Ordering::Relaxed);
                return build_image_response(
                    cached,
                    record.imz,
                    query.imz,
                    elapsed(),
                )
                .into_response();
            }
            Err(ImageError::NotModified(not_mod)) => {
                IMAGE_RESP_304.fetch_add(1, Ordering::Relaxed);
                return build_not_modified_response(
                    not_mod,
                    record.imz,
                    query.imz,
                    elapsed(),
                )
                .into_response();
            }
            Err(ImageError::CorruptOrMissingCache) => {
                state.image_service().enqueue_cache(
                    query.iid,
                    query.imz,
                    CachePolicy::Refresh,
                );
            }
            Err(ImageError::TooLarge(_)) => {
                return StatusCode::UNPROCESSABLE_ENTITY.into_response();
            }
            #[cfg(feature = "demo")]
            Err(ImageError::DemoError) => {
                return StatusCode::NOT_FOUND.into_response();
            }
            Err(e) => {
                error!("Failed to load cached image bytes: {}", e);
            }
        }
    } else {
        state.image_service().enqueue_cache(
            query.iid,
            query.imz,
            CachePolicy::Ensure,
        );
    }

    IMAGE_RESP_MISS.fetch_add(1, Ordering::Relaxed);
    cache_miss_response(query.imz, elapsed()).into_response()
}

/// Load cached image bytes atomically and honor conditional headers.
async fn get_cached_image(
    state: &AppState,
    record: &ImageRecord,
    headers: &HeaderMap,
) -> Result<CachedImage, ImageError> {
    #[cfg(feature = "demo")]
    if matches!(record.imz.image_variant(), ImageVariant::Thumbnail) {
        return Err(ImageError::DemoError);
    }
    let byte_len = record.byte_len.max(0) as u64;
    if byte_len > MAX_IMAGE_SIZE_BYTES {
        return Err(ImageError::TooLarge(byte_len));
    }

    if byte_len > LARGE_IMAGE_WARN_THRESHOLD {
        warn!(
            "Serving large image from cache: iid={}, imz={:?} ({} bytes)",
            record.iid, record.imz, byte_len
        );
    }

    let modified = system_time_from_utc(record.modified_at);
    let last_modified = fmt_http_date(modified);
    let etag = format!("\"{}\"", record.integrity);

    // Conditional headers
    if let Some(if_none_match) = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        && if_none_match.split(',').any(|t| t.trim() == etag)
    {
        return Err(ImageError::NotModified(ImageNotModified {
            etag: etag.clone(),
            last_modified: last_modified.clone(),
        }));
    }

    if let Some(if_modified_since) = headers
        .get(header::IF_MODIFIED_SINCE)
        .and_then(|v| v.to_str().ok())
        && let Ok(since_time) = parse_http_date(if_modified_since)
        && modified <= since_time
    {
        return Err(ImageError::NotModified(ImageNotModified {
            etag: etag.clone(),
            last_modified: last_modified.clone(),
        }));
    }

    let bytes = state
        .image_service()
        .read_cached_bytes(record)
        .await
        .map_err(|e| match &e {
            MediaError::NotFound(_) | MediaError::InvalidMedia(_) => {
                ImageError::CorruptOrMissingCache
            }
            _ => ImageError::CacheRead(e),
        })?;

    let byte_len = bytes.len();

    if byte_len != record.byte_len as usize {
        return Err(ImageError::CorruptOrMissingCache);
    }

    let content_type = match validate_magic_bytes(&bytes) {
        Ok(ct) => ct,
        Err(_) => return Err(ImageError::CorruptOrMissingCache),
    };

    Ok(CachedImage {
        len: byte_len,
        bytes,
        content_type,
        etag,
        last_modified,
    })
}

/// Build a 200 OK response for a cached image.
fn build_image_response(
    cached: CachedImage,
    served: ImageSize,
    requested: ImageSize,
    latency_ms: u128,
) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, cached.content_type)
        .header(header::CONTENT_LENGTH, cached.len.to_string())
        .header(header::ETAG, cached.etag)
        .header(header::LAST_MODIFIED, cached.last_modified)
        // .header(header::CACHE_CONTROL, "public, max-age=604800, immutable")
        .header(header::CACHE_CONTROL, "no-store")
        .header(header::CACHE_CONTROL, "no-cache")
        .header(header::PRAGMA, "no-cache")
        .header("X-Variant-Served", served.to_tmdb_param())
        .header("X-Variant-Requested", requested.to_tmdb_param())
        .header("X-Serve-Latency-Ms", latency_ms.to_string())
        .body(Body::from(cached.bytes))
        .unwrap()
}

/// Build a 304 Not Modified response using computed cache headers.
fn build_not_modified_response(
    not_mod: ImageNotModified,
    served: ImageSize,
    requested: ImageSize,
    latency_ms: u128,
) -> Response {
    Response::builder()
        .status(StatusCode::NOT_MODIFIED)
        .header(header::ETAG, not_mod.etag)
        .header(header::LAST_MODIFIED, not_mod.last_modified)
        // .header(header::CACHE_CONTROL, "public, max-age=604800, immutable")
        .header(header::CACHE_CONTROL, "no-store")
        .header(header::CACHE_CONTROL, "no-cache")
        .header(header::PRAGMA, "no-cache")
        .header("X-Variant-Served", served.to_tmdb_param())
        .header("X-Variant-Requested", requested.to_tmdb_param())
        .header("X-Serve-Latency-Ms", latency_ms.to_string())
        .body(Body::empty())
        .unwrap()
}

fn cache_miss_response(requested: ImageSize, latency_ms: u128) -> Response {
    Response::builder()
        .status(StatusCode::ACCEPTED)
        // Do not let clients cache miss responses.
        .header(header::CACHE_CONTROL, "no-store")
        .header("X-Variant-Requested", requested.to_tmdb_param())
        .header("X-Serve-Latency-Ms", latency_ms.to_string())
        .header("X-Cache", "miss")
        .body(Body::empty())
        .unwrap()
}

fn system_time_from_utc(dt: chrono::DateTime<chrono::Utc>) -> SystemTime {
    let secs = dt.timestamp().max(0) as u64;
    UNIX_EPOCH + Duration::from_secs(secs)
}
