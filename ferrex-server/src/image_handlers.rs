use crate::AppState;
use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use ferrex_core::database::traits::ImageLookupParams;
use serde::Deserialize;
use tracing::{debug, error, info, warn};

#[derive(Debug, Deserialize)]
pub struct ImageQuery {
    size: Option<String>,
}

/// Serve cached images with metadata
/// Path format: /images/{type}/{id}/{category}/{index}
/// Example: /images/movie/12345/poster/0
pub async fn serve_image_handler(
    State(state): State<AppState>,
    Path((media_type, media_id, category, index)): Path<(String, String, String, u32)>,
    Query(query): Query<ImageQuery>,
) -> Result<Response, StatusCode> {
    info!(
        "Image request: type={}, id={}, category={}, index={}, size={:?}",
        media_type, media_id, category, index, query.size
    );
    
    // Enhanced logging to debug 500 errors
    debug!("Raw media_id for image lookup: '{}'", media_id);
    debug!("Media ID length: {}", media_id.len());
    debug!("Media ID chars: {:?}", media_id.chars().collect::<Vec<_>>());

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

    // Create lookup parameters
    let params = ImageLookupParams {
        media_type: media_type.clone(),
        media_id: media_id.clone(),
        image_type: category.clone(),
        index,
        variant: query.size.clone().or_else(|| Some("w500".to_string())),
    };

    // Look up image variant in database
    let (image_path, image_metadata) =
        match state.image_service.get_or_download_variant(&params).await {
            Ok(Some(path)) => {
                // Get metadata from database
                let metadata = state
                    .db
                    .backend()
                    .lookup_image_variant(&params)
                    .await
                    .ok()
                    .flatten();
                (path, metadata)
            }
            Ok(None) => {
                warn!(
                    "Image not found: {}/{}/{}/{}",
                    media_type, media_id, category, index
                );
                return Err(StatusCode::NOT_FOUND);
            }
            Err(e) => {
                error!("Failed to get image: {}", e);
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            }
        };

    // Read the image file
    debug!("Attempting to read image from path: {:?}", image_path);
    debug!("Current working directory: {:?}", std::env::current_dir());
    debug!("Image path exists: {}", image_path.exists());
    
    let image_data = match tokio::fs::read(&image_path).await {
        Ok(data) => data,
        Err(e) => {
            error!("Failed to read image file {:?}: {}", image_path, e);
            error!("Absolute path: {:?}", image_path.canonicalize());
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // Determine content type based on file extension or metadata
    let content_type = if let Some((image, _)) = &image_metadata {
        match image.format.as_deref() {
            Some("jpg") | Some("jpeg") => "image/jpeg",
            Some("png") => "image/png",
            Some("webp") => "image/webp",
            _ => "image/jpeg",
        }
    } else {
        match image_path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_lowercase())
            .as_deref()
        {
            Some("jpg") | Some("jpeg") => "image/jpeg",
            Some("png") => "image/png",
            Some("webp") => "image/webp",
            _ => "image/jpeg",
        }
    };

    // Build response with headers
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static(content_type),
    );
    headers.insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("public, max-age=31536000"), // Cache for 1 year
    );

    // Add CORS headers for images
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        header::HeaderValue::from_static("*"),
    );

    // Add image metadata headers if available
    if let Some((image, variant)) = &image_metadata {
        let width = variant
            .as_ref()
            .and_then(|v| v.width)
            .or(image.width)
            .unwrap_or(0);
        let height = variant
            .as_ref()
            .and_then(|v| v.height)
            .or(image.height)
            .unwrap_or(0);
        let aspect_ratio = if height > 0 {
            width as f32 / height as f32
        } else {
            0.0
        };

        headers.insert(
            "X-Image-Width",
            header::HeaderValue::from_str(&width.to_string())
                .unwrap_or_else(|_| header::HeaderValue::from_static("0")),
        );
        headers.insert(
            "X-Image-Height",
            header::HeaderValue::from_str(&height.to_string())
                .unwrap_or_else(|_| header::HeaderValue::from_static("0")),
        );
        headers.insert(
            "X-Image-Aspect-Ratio",
            header::HeaderValue::from_str(&format!("{:.3}", aspect_ratio))
                .unwrap_or_else(|_| header::HeaderValue::from_static("0")),
        );
    }

    Ok((headers, image_data).into_response())
}
