use crate::infra::app_state::AppState;
use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use ferrex_core::database::traits::ImageLookupParams;
use ferrex_core::image::MediaImageKind;
use ferrex_core::image_service::TmdbImageSize;
use httpdate::{fmt_http_date, parse_http_date};
use serde::Deserialize;
use std::io::ErrorKind;
use std::path::{Component, Path as FsPath, PathBuf};
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
    Path((media_type, media_id, category, index)): Path<(
        String,
        String,
        String,
        u32,
    )>,
    Query(query): Query<ImageQuery>,
    headers: HeaderMap,
) -> impl IntoResponse {
    info!(
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
    let mut served_variant: Option<String> = None;

    for variant in plan.ensure_variants.iter() {
        let mut attempt_params = params.clone();
        attempt_params.variant = Some(variant.clone());
        match state
            .image_service()
            .ensure_variant_async(&attempt_params)
            .await
        {
            Ok(Some(path)) => {
                ready_path = Some(path);
                served_variant = Some(variant.clone());
                break;
            }
            Ok(None) => continue,
            Err(e) => {
                error!(
                    "ensure_variant_async failed for variant {}: {}",
                    variant, e
                );
            }
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
        match state
            .image_service()
            .pick_best_available(&params, plan.target_width)
            .await
        {
            Ok(Some((fallback_path, fallback_variant))) => {
                (fallback_path, fallback_variant)
            }
            Ok(None) => {
                warn!(
                    "No fallback available: {}/{}/{}/{}",
                    media_type,
                    media_id,
                    category_kind.as_str(),
                    index
                );
                return Err(StatusCode::NOT_FOUND);
            }
            Err(e) => {
                error!("Failed to select fallback: {}", e);
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            }
        }
    };

    let mut fetch_attempted = false;
    let (meta, data, resolved_path) = loop {
        let normalized =
            normalize_image_path(&image_path, state.config().cache_root());

        match tokio::fs::metadata(&normalized).await {
            Ok(meta) => match tokio::fs::read(&normalized).await {
                Ok(bytes) => break (meta, bytes, normalized),
                Err(e)
                    if e.kind() == ErrorKind::NotFound && !fetch_attempted =>
                {
                    fetch_attempted = true;
                    match redownload_variant(&state, &params, &served_variant)
                        .await
                    {
                        Ok(new_path) => {
                            image_path = new_path;
                            continue;
                        }
                        Err(status) => return Err(status),
                    }
                }
                Err(e) => {
                    error!("Failed to read image file {:?}: {}", normalized, e);
                    return Err(StatusCode::INTERNAL_SERVER_ERROR);
                }
            },
            Err(e) if e.kind() == ErrorKind::NotFound && !fetch_attempted => {
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
                error!("Failed to stat image file {:?}: {}", normalized, e);
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            }
        }
    };

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
        && if_none_match.split(',').any(|t| t.trim() == etag_value)
    {
        return Ok::<_, StatusCode>(
            Response::builder()
                .status(StatusCode::NOT_MODIFIED)
                .header(header::ETAG, etag_value)
                .header(header::LAST_MODIFIED, last_modified)
                .header(header::CACHE_CONTROL, "public, max-age=86400")
                .header("X-Variant-Requested", requested_header_value.as_str())
                .header("X-Variant-Served", served_variant.as_str())
                .body(Body::empty())
                .unwrap(),
        );
    }

    if let Some(if_modified_since) = headers
        .get(header::IF_MODIFIED_SINCE)
        .and_then(|v| v.to_str().ok())
        && let Ok(since_time) = parse_http_date(if_modified_since)
        && modified <= since_time
    {
        return Ok::<_, StatusCode>(
            Response::builder()
                .status(StatusCode::NOT_MODIFIED)
                .header(header::ETAG, etag_value)
                .header(header::LAST_MODIFIED, last_modified)
                .header(header::CACHE_CONTROL, "public, max-age=86400")
                .header("X-Variant-Requested", requested_header_value.as_str())
                .header("X-Variant-Served", served_variant.as_str())
                .body(Body::empty())
                .unwrap(),
        );
    }

    let resp = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CONTENT_LENGTH, data.len().to_string())
        .header(header::ETAG, etag_value)
        .header(header::LAST_MODIFIED, last_modified)
        .header(header::CACHE_CONTROL, "public, max-age=86400")
        .header("X-Variant-Served", served_variant)
        .header("X-Variant-Requested", requested_header_value.as_str());

    Ok::<_, StatusCode>(resp.body(Body::from(data)).unwrap())
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
            Some("w1280"),
            &["w780", "w1280", "original"],
            Some(1280),
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
