use super::image_validation::validate_magic_bytes;
use crate::infra::app_state::AppState;
use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use ferrex_core::database::traits::ImageLookupParams;
use ferrex_core::domain::media::image::MediaImageKind;
use ferrex_core::infrastructure::media::image_service::TmdbImageSize;
use httpdate::{fmt_http_date, parse_http_date};
use serde::Deserialize;
use std::io::ErrorKind;
use std::path::{Component, Path as FsPath, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// Maximum image size to serve (20 MB). Images larger than this are rejected.
/// This prevents memory exhaustion from malicious or corrupted files.
const MAX_IMAGE_SIZE_BYTES: u64 = 20 * 1024 * 1024;

/// Size threshold for logging warnings about large images (5 MB).
const LARGE_IMAGE_WARN_THRESHOLD: u64 = 5 * 1024 * 1024;

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

// Simple counters for image responses on this process
static IMAGE_RESP_200: AtomicU64 = AtomicU64::new(0);
static IMAGE_RESP_304: AtomicU64 = AtomicU64::new(0);
static IMAGE_RESP_404: AtomicU64 = AtomicU64::new(0);

/// Serve cached images as streamed bytes with proper HTTP caching
/// Path format: /images/{type}/{id}/{category}/{index}
/// Example: /images/movie/12345/poster/0
pub async fn serve_image_handler(
    State(state): State<AppState>,
    Path((media_type, media_id, category, index)): Path<(
        String,
        String,
        String,
        u32,
    )>,
    Query(query): Query<ImageQuery>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let t_start = std::time::Instant::now();
    debug!(
        "Image request: type={}, id={}, category={}, index={}, size={:?}, w={:?}, fmt={:?}, quality={:?}",
        media_type,
        media_id,
        category,
        index,
        query.size,
        query.w.or(query.max_width),
        query.fmt,
        query.quality
    );

    // Validate media type
    if !["movie", "series", "season", "episode", "person"]
        .contains(&media_type.as_str())
    {
        warn!("Invalid media type: {}", media_type);
        return Err(StatusCode::BAD_REQUEST);
    }

    let category_kind = MediaImageKind::parse(&category);
    if category_kind.is_other() {
        warn!("Invalid image category: {}", category);
        return Err(StatusCode::BAD_REQUEST);
    }

    let plan = determine_variant_plan(&category_kind, &query);
    debug!("Variant plan: {:?}", plan);
    let requested_header_value = plan
        .requested_header
        .clone()
        .or_else(|| plan.lookup_variant.clone())
        .unwrap_or_else(|| "auto".to_string());

    let params = ImageLookupParams {
        media_type: media_type.clone(),
        media_id: media_id.clone(),
        image_type: category_kind.clone(),
        index,
        variant: plan.lookup_variant.clone(),
    };

    let mut ready_path: Option<PathBuf> = None;
    let mut found_image_id: Option<Uuid> = None;
    let mut served_variant: Option<String> = None;

    // Single ensure attempt using the plan's lookup variant.
    let mut attempt_params = params.clone();
    attempt_params.variant = plan.lookup_variant.clone();
    match state
        .image_service()
        .ensure_variant_async(&attempt_params)
        .await
    {
        Ok(report) => {
            if found_image_id.is_none() {
                found_image_id = report.image_id;
            }
            if let Some(path) = report.ready_path {
                ready_path = Some(path);
                served_variant = attempt_params.variant.clone();
            }
        }
        Err(e) => {
            error!(
                "ensure_variant_async failed for variant {:?}: {}",
                attempt_params.variant, e
            );
        }
    }

    let (mut image_path, served_variant) = if let Some(path) = ready_path {
        (
            path,
            served_variant.unwrap_or_else(|| {
                plan.lookup_variant
                    .clone()
                    .unwrap_or_else(|| "w500".to_string())
            }),
        )
    } else {
        let pick_result = if let Some(image_id) = found_image_id {
            state
                .image_service()
                .pick_best_available_for_image(
                    image_id,
                    plan.target_width,
                    params.variant.as_deref(),
                )
                .await
        } else {
            state
                .image_service()
                .pick_best_available(&params, plan.target_width)
                .await
        };

        match pick_result {
            Ok(Some((fallback_path, fallback_variant))) => {
                (fallback_path, fallback_variant)
            }
            Ok(None) => {
                // Harden backdrop behavior: synchronously fetch and serve original on first request.
                if matches!(category_kind, MediaImageKind::Backdrop) {
                    let desired = plan
                        .lookup_variant
                        .clone()
                        .unwrap_or_else(|| "original".to_string());
                    match redownload_variant(&state, &params, &desired).await {
                        Ok(new_path) => (new_path, desired),
                        Err(_) => {
                            warn!(
                                "No fallback available and on-demand fetch failed: {}/{}/{}/{}",
                                media_type,
                                media_id,
                                category_kind.as_str(),
                                index
                            );
                            IMAGE_RESP_404.fetch_add(1, Ordering::Relaxed);
                            return Err(StatusCode::NOT_FOUND);
                        }
                    }
                } else {
                    warn!(
                        "No fallback available: {}/{}/{}/{}",
                        media_type,
                        media_id,
                        category_kind.as_str(),
                        index
                    );
                    IMAGE_RESP_404.fetch_add(1, Ordering::Relaxed);
                    return Err(StatusCode::NOT_FOUND);
                }
            }
            Err(e) => {
                error!("Failed to select fallback: {}", e);
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            }
        }
    };

    let mut fetch_attempted = false;
    loop {
        let normalized =
            normalize_image_path(&image_path, state.config().cache_root());

        // ATOMIC READ: Read entire file into memory before serving.
        // This guarantees Content-Length matches actual bytes sent, avoiding race
        // conditions where a concurrent file replacement could cause corruption.
        match File::open(&normalized).await {
            Ok(mut file) => {
                let meta = match file.metadata().await {
                    Ok(m) => m,
                    Err(e) => {
                        error!(
                            "Failed to read metadata for open image file {:?}: {}",
                            normalized, e
                        );
                        return Err(StatusCode::INTERNAL_SERVER_ERROR);
                    }
                };

                let file_size = meta.len();

                // Check file size limits before reading into memory
                if file_size > MAX_IMAGE_SIZE_BYTES {
                    warn!(
                        "Image file too large: {:?} ({} bytes, max {})",
                        normalized, file_size, MAX_IMAGE_SIZE_BYTES
                    );
                    return Err(StatusCode::UNPROCESSABLE_ENTITY);
                }

                if file_size > LARGE_IMAGE_WARN_THRESHOLD {
                    warn!(
                        "Serving large image: {:?} ({} bytes)",
                        normalized, file_size
                    );
                }

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
                    && if_none_match.split(',').any(|t| t.trim() == etag_value)
                {
                    let elapsed = t_start.elapsed().as_millis().to_string();
                    let resp = Response::builder()
                        .status(StatusCode::NOT_MODIFIED)
                        .header(header::ETAG, &etag_value)
                        .header(header::LAST_MODIFIED, &last_modified)
                        .header(
                            header::CACHE_CONTROL,
                            "public, max-age=604800, immutable",
                        )
                        .header(
                            "X-Variant-Requested",
                            requested_header_value.as_str(),
                        )
                        .header("X-Variant-Served", served_variant.as_str())
                        .header("X-Serve-Latency-Ms", elapsed)
                        .body(Body::empty())
                        .unwrap();
                    IMAGE_RESP_304.fetch_add(1, Ordering::Relaxed);
                    return Ok::<_, StatusCode>(resp);
                }

                if let Some(if_modified_since) = headers
                    .get(header::IF_MODIFIED_SINCE)
                    .and_then(|v| v.to_str().ok())
                    && let Ok(since_time) = parse_http_date(if_modified_since)
                    && modified <= since_time
                {
                    let elapsed = t_start.elapsed().as_millis().to_string();
                    let resp = Response::builder()
                        .status(StatusCode::NOT_MODIFIED)
                        .header(header::ETAG, &etag_value)
                        .header(header::LAST_MODIFIED, last_modified)
                        .header(
                            header::CACHE_CONTROL,
                            "public, max-age=604800, immutable",
                        )
                        .header(
                            "X-Variant-Requested",
                            requested_header_value.as_str(),
                        )
                        .header("X-Variant-Served", served_variant.as_str())
                        .header("X-Serve-Latency-Ms", elapsed)
                        .body(Body::empty())
                        .unwrap();
                    IMAGE_RESP_304.fetch_add(1, Ordering::Relaxed);
                    return Ok::<_, StatusCode>(resp);
                }

                // ATOMIC READ: Read entire file into memory
                // This guarantees Content-Length matches actual bytes sent
                let mut bytes = Vec::with_capacity(file_size as usize);
                if let Err(e) = file.read_to_end(&mut bytes).await {
                    error!("Failed to read image file {:?}: {}", normalized, e);
                    return Err(StatusCode::INTERNAL_SERVER_ERROR);
                }

                // Validate image format via magic bytes
                let content_type = match validate_magic_bytes(&bytes) {
                    Ok(ct) => ct,
                    Err(reason) => {
                        warn!(
                            "Invalid image file {:?}: {:?}",
                            normalized, reason
                        );
                        return Err(StatusCode::UNPROCESSABLE_ENTITY);
                    }
                };

                // Use actual read size for Content-Length (not file metadata)
                let actual_size = bytes.len();

                // Build response with in-memory body
                let body = Body::from(bytes);
                let elapsed = t_start.elapsed().as_millis().to_string();
                let resp = Response::builder()
                    .status(StatusCode::OK)
                    .header(header::CONTENT_TYPE, content_type)
                    .header(header::CONTENT_LENGTH, actual_size.to_string())
                    .header(header::ETAG, etag_value)
                    .header(header::LAST_MODIFIED, last_modified)
                    .header(
                        header::CACHE_CONTROL,
                        "public, max-age=604800, immutable",
                    )
                    .header("X-Variant-Served", served_variant.as_str())
                    .header(
                        "X-Variant-Requested",
                        requested_header_value.as_str(),
                    )
                    .header("X-Serve-Latency-Ms", elapsed)
                    .body(body)
                    .unwrap();
                IMAGE_RESP_200.fetch_add(1, Ordering::Relaxed);
                return Ok::<_, StatusCode>(resp);
            }
            Err(e) if e.kind() == ErrorKind::NotFound && !fetch_attempted => {
                // First, retry open briefly to ride out rename races from the scanner/fetcher.
                let mut reopened: Option<File> = None;
                let mut delay = std::time::Duration::from_millis(3);
                for _ in 0..6 {
                    if let Ok(f) = File::open(&normalized).await {
                        reopened = Some(f);
                        break;
                    }
                    tokio::time::sleep(delay).await;
                    // exponential backoff up to ~100ms total
                    delay = delay.saturating_mul(2);
                }
                if let Some(mut file) = reopened {
                    // Found the file after retry; use atomic read like the Ok branch
                    let meta = match file.metadata().await {
                        Ok(m) => m,
                        Err(e) => {
                            error!(
                                "Failed to read metadata after reopen for {:?}: {}",
                                normalized, e
                            );
                            return Err(StatusCode::INTERNAL_SERVER_ERROR);
                        }
                    };
                    let file_size = meta.len();

                    // Check file size limits
                    if file_size > MAX_IMAGE_SIZE_BYTES {
                        warn!(
                            "Image file too large after retry: {:?} ({} bytes)",
                            normalized, file_size
                        );
                        return Err(StatusCode::UNPROCESSABLE_ENTITY);
                    }

                    let modified = match meta.modified() {
                        Ok(t) => t,
                        Err(e) => {
                            warn!(
                                "No modified time for {:?}: {}",
                                image_path, e
                            );
                            std::time::SystemTime::UNIX_EPOCH
                        }
                    };
                    let last_modified = fmt_http_date(modified);
                    // Build ETag
                    let secs = modified
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_else(|_| std::time::Duration::from_secs(0))
                        .as_secs();
                    let etag_value = format!("W/\"{}-{}\"", file_size, secs);

                    // ATOMIC READ: Read entire file into memory
                    let mut bytes = Vec::with_capacity(file_size as usize);
                    if let Err(e) = file.read_to_end(&mut bytes).await {
                        error!(
                            "Failed to read image file after retry {:?}: {}",
                            normalized, e
                        );
                        return Err(StatusCode::INTERNAL_SERVER_ERROR);
                    }

                    // Validate image format via magic bytes
                    let content_type = match validate_magic_bytes(&bytes) {
                        Ok(ct) => ct,
                        Err(reason) => {
                            warn!(
                                "Invalid image file after retry {:?}: {:?}",
                                normalized, reason
                            );
                            return Err(StatusCode::UNPROCESSABLE_ENTITY);
                        }
                    };

                    let actual_size = bytes.len();
                    let body = Body::from(bytes);
                    let elapsed = t_start.elapsed().as_millis().to_string();
                    let resp = Response::builder()
                        .status(StatusCode::OK)
                        .header(header::CONTENT_TYPE, content_type)
                        .header(header::CONTENT_LENGTH, actual_size.to_string())
                        .header(header::ETAG, etag_value)
                        .header(header::LAST_MODIFIED, last_modified)
                        .header(
                            header::CACHE_CONTROL,
                            "public, max-age=604800, immutable",
                        )
                        .header("X-Variant-Served", served_variant.as_str())
                        .header(
                            "X-Variant-Requested",
                            requested_header_value.as_str(),
                        )
                        .header("X-Serve-Latency-Ms", elapsed)
                        .body(body)
                        .unwrap();
                    IMAGE_RESP_200.fetch_add(1, Ordering::Relaxed);
                    return Ok::<_, StatusCode>(resp);
                }

                // If still not found, fall back to one synchronous fetch attempt.
                fetch_attempted = true;
                match redownload_variant(&state, &params, &served_variant).await
                {
                    Ok(new_path) => {
                        image_path = new_path;
                        continue;
                    }
                    Err(status) => return Err(status),
                }
            }
            Err(e) => {
                error!(
                    "Failed to open image file for streaming {:?}: {}",
                    normalized, e
                );
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncReadExt;

    // Verify that atomic read guarantees content-length matches bytes read
    #[tokio::test]
    async fn atomic_read_guarantees_content_length_matches_body() {
        let tmp = tempfile::tempdir().expect("tmpdir");
        let path = tmp.path().join("atomic_test.jpg");

        // Create valid JPEG with known size
        let mut content = vec![0xFF, 0xD8, 0xFF, 0xE0]; // JPEG header
        content.extend(vec![b'X'; 1000]);
        let expected_size = content.len();
        std::fs::write(&path, &content).expect("write");

        // Read atomically
        let mut file = File::open(&path).await.expect("open");
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).await.expect("read");

        // Validate content type
        let content_type = validate_magic_bytes(&bytes).expect("valid jpeg");
        assert_eq!(content_type, "image/jpeg");

        // Content-Length would equal bytes.len(), not metadata
        assert_eq!(bytes.len(), expected_size);
    }
}

async fn redownload_variant(
    state: &AppState,
    params: &ImageLookupParams,
    served_variant: &str,
) -> Result<PathBuf, StatusCode> {
    let mut lookup = params.clone();
    lookup.variant = Some(served_variant.to_string());

    match state.image_service().get_or_download_variant(&lookup).await {
        Ok(Some(path)) => {
            info!(
                "Fetched missing image variant on-demand: {}/{}/{}/{} variant {}",
                params.media_type,
                params.media_id,
                params.image_type.as_str(),
                params.index,
                served_variant
            );
            Ok(path)
        }
        Ok(None) => {
            warn!(
                "Unable to download image variant on-demand; variant record missing: {}/{}/{}/{} variant {}",
                params.media_type,
                params.media_id,
                params.image_type.as_str(),
                params.index,
                served_variant
            );
            IMAGE_RESP_404.fetch_add(1, Ordering::Relaxed);
            Err(StatusCode::NOT_FOUND)
        }
        Err(e) => {
            error!(
                "On-demand image download failed: {}/{}/{}/{} variant {}: {}",
                params.media_type,
                params.media_id,
                params.image_type.as_str(),
                params.index,
                served_variant,
                e
            );
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// Normalize a stored image path against the (already canonicalized) cache root.
///
/// Scanner instances historically persisted relative paths such as
/// `./cache/images/...`. The server only canonicalizes the cache directory once
/// during startup, so this helper must avoid re-normalizing the base and simply
/// map legacy relative variants into that absolute root.
fn normalize_image_path(original: &FsPath, cache_root: &FsPath) -> PathBuf {
    if original.is_absolute() {
        return original.to_path_buf();
    }

    debug_assert!(
        cache_root.is_absolute(),
        "cache_dir should have been canonicalized during startup"
    );

    let cache_basename = cache_root.file_name();
    let mut components = original.components().peekable();

    // Drop any leading `./` segments that were persisted by older scanners.
    while matches!(components.peek(), Some(Component::CurDir)) {
        components.next();
    }

    // Skip the first segment if it matches the cache directory name to avoid
    // producing `.../cache/cache/...` when we join below.
    if let Some(basename) = cache_basename {
        let drop_prefix = matches!(components.peek(), Some(Component::Normal(first)) if *first == basename);
        if drop_prefix {
            components.next();
        }
    }

    let mut relative = PathBuf::new();
    for component in components {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                // Stay within the cache root boundary.
                relative.pop();
            }
            Component::Normal(part) => relative.push(part),
            // Other component types (Prefix, RootDir) should not appear for
            // non-absolute inputs. If they do, fall back to returning the
            // cache root so we at least stay within a known-good directory.
            Component::Prefix(_) | Component::RootDir => {
                return cache_root.to_path_buf();
            }
        }
    }

    if relative.as_os_str().is_empty() {
        cache_root.to_path_buf()
    } else {
        cache_root.join(relative)
    }
}

#[derive(Debug, Clone)]
struct VariantPlan {
    requested_header: Option<String>,
    lookup_variant: Option<String>,
    ensure_variants: Vec<String>,
    target_width: Option<u32>,
}

fn determine_variant_plan(
    category: &MediaImageKind,
    query: &ImageQuery,
) -> VariantPlan {
    if let Some(w) = query.w.or(query.max_width) {
        let variant = map_width_to_tmdb_variant(category, w).to_string();
        return variant_plan_exact(variant, None);
    }

    if let Some(size_value) = query.size.as_ref().filter(|s| !s.is_empty()) {
        let trimmed = size_value.trim();
        let normalized = trimmed.to_ascii_lowercase();

        match normalized.as_str() {
            "quality" => {
                return auto_plan_for_category(category, Some("quality"));
            }
            "any" if matches!(category, MediaImageKind::Cast) => {
                return auto_plan_for_category(category, Some("any"));
            }
            _ => {
                if is_recognized_tmdb_variant(&normalized) {
                    // Normalize non-canonical poster size requests to reduce upstream 404s
                    if matches!(category, MediaImageKind::Poster)
                        && normalized == "w300"
                    {
                        return variant_plan_exact(
                            "w342".to_string(),
                            Some(trimmed.to_string()),
                        );
                    }

                    return variant_plan_exact(
                        normalized,
                        Some(trimmed.to_string()),
                    );
                }
            }
        }
    }

    auto_plan_for_category(category, None)
}

fn variant_plan_exact(variant: String, header: Option<String>) -> VariantPlan {
    let requested = header.unwrap_or_else(|| variant.clone());
    let normalized = variant.to_ascii_lowercase();

    VariantPlan {
        requested_header: Some(requested),
        lookup_variant: Some(normalized.clone()),
        ensure_variants: vec![normalized.clone()],
        target_width: variant_width_hint(&normalized),
    }
}

fn auto_plan_for_category(
    category: &MediaImageKind,
    label: Option<&str>,
) -> VariantPlan {
    match category {
        MediaImageKind::Poster => build_variant_plan(
            label,
            Some("w500"),
            &["w500", "w342", "w780", "original", "w185", "w154", "w92"],
            Some(500),
        ),
        MediaImageKind::Backdrop => build_variant_plan(
            label,
            // Prefer full-quality backdrops for now
            Some("original"),
            &["original"],
            None,
        ),
        MediaImageKind::Thumbnail => build_variant_plan(
            label,
            Some("w300"),
            &["w300", "w500", "w185"],
            Some(300),
        ),
        MediaImageKind::Logo => {
            build_variant_plan(label, Some("original"), &["original"], None)
        }
        MediaImageKind::Cast => build_variant_plan(
            label,
            Some("w185"),
            &["w185", "h632", "w45", "original"],
            Some(300),
        ),
        MediaImageKind::Other(_) => build_variant_plan(
            label,
            Some("w500"),
            &["w500", "original"],
            Some(500),
        ),
    }
}

fn build_variant_plan(
    label: Option<&str>,
    fallback: Option<&str>,
    ensures: &[&str],
    explicit_target: Option<u32>,
) -> VariantPlan {
    let fallback_norm = fallback.map(|s| s.to_ascii_lowercase());
    let mut ensure_variants: Vec<String> = Vec::new();

    if let Some(ref fallback_variant) = fallback_norm {
        push_unique(&mut ensure_variants, fallback_variant);
    }

    for candidate in ensures {
        let normalized = candidate.to_ascii_lowercase();
        push_unique(&mut ensure_variants, &normalized);
    }

    let target_width = explicit_target
        .or_else(|| fallback_norm.as_deref().and_then(variant_width_hint));

    VariantPlan {
        requested_header: label.map(|s| s.to_string()),
        lookup_variant: fallback_norm,
        ensure_variants,
        target_width,
    }
}

fn push_unique(vec: &mut Vec<String>, candidate: &str) {
    if !vec.iter().any(|existing| existing == candidate) {
        vec.push(candidate.to_string());
    }
}

fn is_recognized_tmdb_variant(value: &str) -> bool {
    TmdbImageSize::from_str(value).is_some()
}

fn variant_width_hint(variant: &str) -> Option<u32> {
    if variant.eq_ignore_ascii_case("original") {
        return Some(10_000);
    }
    variant
        .strip_prefix('w')
        .and_then(|digits| digits.parse::<u32>().ok())
}

/// Map desired width to a TMDB variant string for a given category
fn map_width_to_tmdb_variant(
    category: &MediaImageKind,
    w: u32,
) -> &'static str {
    // Choose nearest bucket >= requested; fall back to largest known or original
    match category {
        // Posters: 92, 154, 185, 342, 500, 780, original
        MediaImageKind::Poster => match w {
            0..=92 => "w92",
            93..=154 => "w154",
            155..=185 => "w185",
            186..=342 => "w342",
            343..=500 => "w500",
            501..=780 => "w780",
            _ => "original",
        },
        // Backdrops: 300, 780, 1280, original
        MediaImageKind::Backdrop => match w {
            0..=300 => "w300",
            301..=780 => "w780",
            781..=1280 => "w1280",
            _ => "original",
        },
        // Thumbnails: 92, 185, 300, 500, original
        MediaImageKind::Thumbnail => match w {
            0..=92 => "w92",
            93..=185 => "w185",
            186..=300 => "w300",
            301..=500 => "w500",
            _ => "original",
        },
        // Logos: prefer original to avoid artifacts
        MediaImageKind::Logo => "original",
        // Cast portraits: 45, 185, original
        MediaImageKind::Cast => match w {
            0..=45 => "w45",
            46..=185 => "w185",
            _ => "original",
        },
        MediaImageKind::Other(_) => "w500",
    }
}
