use axum::{
    body::{Body, Bytes},
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response, Sse},
};
use base64::{
    Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD,
};
use ferrex_core::{
    api::{
        routes::v1,
        types::{
            ImageManifestItem, ImageManifestRequest, ImageManifestResponse,
            ImageManifestResult, ImageManifestStatus,
        },
    },
    infra::{cache::ImageFileStore, image_service::CachePolicy},
};
use ferrex_flatbuffers::{
    FLATBUFFERS_MIME, conversions::image as fb_image,
    fb::common::ImageCategory as FbImageCategory,
};
use ferrex_model::ImageSize;
use ferrex_model::events::ImageSseEventType;
use httpdate::{fmt_http_date, parse_http_date};
use rkyv::util::AlignedVec;
use rkyv::{from_bytes, rancor::Error as RkyvError, to_bytes};
use std::{convert::Infallible, time::Duration};
use tokio_stream::{StreamExt, wrappers::BroadcastStream};
use tokio_util::io::ReaderStream;
use tracing::{error, warn};
use uuid::Uuid;

use crate::{
    handlers::media::image_validation::validate_magic_bytes,
    infra::{
        app_state::AppState,
        content_negotiation::{
            AcceptedFormat, RKYV_OCTET_STREAM_MIME, RequestBodyFormat,
            WireFormat,
        },
    },
};

const BLOB_CACHE_CONTROL: &str = "public, max-age=31536000, immutable";

/// POST /api/v1/images/manifest - Batch image readiness lookup.
pub async fn post_image_manifest_handler(
    State(state): State<AppState>,
    AcceptedFormat(response_format): AcceptedFormat,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let request = match parse_image_manifest_request(&headers, body) {
        Ok(request) => request,
        Err(status) => return status.into_response(),
    };

    let response = build_image_manifest_response(&state, request).await;

    match response_format {
        WireFormat::FlatBuffers => {
            let entries = response
                .results
                .iter()
                .map(|result| fb_image::ImageManifestEntry {
                    iid: result.iid,
                    category: image_size_to_category(result.imz),
                    status: match &result.status {
                        ImageManifestStatus::Ready { token, .. } => {
                            fb_image::ImageManifestEntryStatus::Ready { token }
                        }
                        ImageManifestStatus::Pending { retry_after_ms } => {
                            fb_image::ImageManifestEntryStatus::Pending {
                                retry_after_ms: *retry_after_ms,
                            }
                        }
                        ImageManifestStatus::Missing { reason } => {
                            fb_image::ImageManifestEntryStatus::Failed {
                                reason: Some(reason),
                            }
                        }
                    },
                })
                .collect::<Vec<_>>();
            let bytes = fb_image::serialize_image_manifest_response(&entries);
            (
                [(header::CONTENT_TYPE, FLATBUFFERS_MIME)],
                Bytes::from(bytes),
            )
                .into_response()
        }
        WireFormat::RkyvOctetStream => {
            let bytes = match to_bytes::<RkyvError>(&response) {
                Ok(bytes) => bytes,
                Err(err) => {
                    error!("image manifest encode failed: {err}");
                    return StatusCode::INTERNAL_SERVER_ERROR.into_response();
                }
            };

            (
                [(header::CONTENT_TYPE, RKYV_OCTET_STREAM_MIME)],
                Bytes::from(bytes.into_vec()),
            )
                .into_response()
        }
        WireFormat::Json => axum::Json(response).into_response(),
    }
}

fn parse_image_manifest_request(
    headers: &HeaderMap,
    body: Bytes,
) -> Result<ImageManifestRequest, StatusCode> {
    match RequestBodyFormat::from_headers(headers) {
        RequestBodyFormat::RkyvOctetStream => {
            // rkyv expects aligned bytes; axum's `Bytes` is not guaranteed to be aligned,
            // which can cause intermittent decode failures under load.
            let mut aligned: AlignedVec = AlignedVec::with_capacity(body.len());
            aligned.extend_from_slice(&body);

            from_bytes::<ImageManifestRequest, RkyvError>(&aligned).map_err(
                |err| {
                    warn!(
                        "image manifest rkyv decode failed: {err} (bytes={})",
                        body.len()
                    );
                    StatusCode::BAD_REQUEST
                },
            )
        }
        RequestBodyFormat::FlatBuffers => {
            let queries = fb_image::parse_image_manifest_request(&body)
                .map_err(|err| {
                    warn!(
                        "image manifest FlatBuffers decode failed: {err} (bytes={})",
                        body.len()
                    );
                    StatusCode::BAD_REQUEST
                })?;
            let mut requests = Vec::with_capacity(queries.len());
            for query in queries {
                let imz = image_category_to_size(query.category)
                    .ok_or(StatusCode::BAD_REQUEST)?;
                requests.push(ImageManifestItem {
                    iid: query.iid,
                    imz,
                });
            }
            Ok(ImageManifestRequest { requests })
        }
        RequestBodyFormat::Json => {
            serde_json::from_slice(&body).map_err(|err| {
                warn!(
                    "image manifest JSON decode failed: {err} (bytes={})",
                    body.len()
                );
                StatusCode::BAD_REQUEST
            })
        }
        RequestBodyFormat::Unsupported(content_type) => {
            warn!("unsupported image manifest content type: {content_type}");
            Err(StatusCode::BAD_REQUEST)
        }
    }
}

fn image_category_to_size(category: FbImageCategory) -> Option<ImageSize> {
    if category == FbImageCategory::Poster {
        Some(ImageSize::poster())
    } else if category == FbImageCategory::Backdrop {
        Some(ImageSize::backdrop())
    } else if category == FbImageCategory::Profile {
        Some(ImageSize::profile())
    } else if category == FbImageCategory::Episode {
        Some(ImageSize::thumbnail())
    } else {
        None
    }
}

fn image_size_to_category(size: ImageSize) -> FbImageCategory {
    match size {
        ImageSize::Poster(_) => FbImageCategory::Poster,
        ImageSize::Backdrop(_) => FbImageCategory::Backdrop,
        ImageSize::Profile(_) => FbImageCategory::Profile,
        ImageSize::Thumbnail(_) => FbImageCategory::Episode,
    }
}

async fn build_image_manifest_response(
    state: &AppState,
    request: ImageManifestRequest,
) -> ImageManifestResponse {
    let mut results = Vec::with_capacity(request.requests.len());

    for item in request.requests {
        let status = image_manifest_status(state, item.iid, item.imz).await;
        results.push(ImageManifestResult {
            iid: item.iid,
            imz: item.imz,
            status,
        });
    }

    ImageManifestResponse { results }
}

async fn image_manifest_status(
    state: &AppState,
    iid: Uuid,
    imz: ImageSize,
) -> ImageManifestStatus {
    #[cfg(feature = "demo")]
    {
        if matches!(
            imz.image_variant(),
            ferrex_model::image::ImageVariant::Thumbnail
        ) {
            return ImageManifestStatus::Missing {
                reason: "thumbnail images are not available in demo mode"
                    .to_string(),
            };
        }
    }

    let meta = match state
        .image_service()
        .read_cached_meta_by_key(iid, imz)
        .await
    {
        Ok(meta) => meta,
        Err(err) => {
            error!(
                "image manifest meta lookup failed: iid={}, imz={:?}, err={}",
                iid, imz, err
            );
            state
                .image_service()
                .enqueue_cache(iid, imz, CachePolicy::Ensure);
            return ImageManifestStatus::Pending {
                retry_after_ms: 1_000,
            };
        }
    };

    let Some(meta) = meta else {
        state
            .image_service()
            .enqueue_cache(iid, imz, CachePolicy::Ensure);
        return ImageManifestStatus::Pending {
            retry_after_ms: 1_000,
        };
    };

    let token =
        ImageFileStore::token_from_integrity(&meta.integrity.to_string());
    let blob_exists = match state.image_service().image_blob_path(&token) {
        Ok(path) => tokio::fs::try_exists(path).await.unwrap_or(false),
        Err(_) => false,
    };

    if blob_exists {
        return ImageManifestStatus::Ready {
            token,
            byte_len: meta.byte_len as u64,
        };
    }

    // Cached in `cacache`, but not yet materialized for the immutable blob path.
    state
        .image_service()
        .enqueue_cache(iid, imz, CachePolicy::Ensure);
    ImageManifestStatus::Pending {
        retry_after_ms: 1_000,
    }
}

/// GET /api/v1/images/iid/{iid} - Image-instance compatibility redirect.
pub async fn get_image_iid_handler(
    State(state): State<AppState>,
    Path(iid): Path<Uuid>,
) -> Response {
    match image_manifest_status(&state, iid, ImageSize::poster()).await {
        ImageManifestStatus::Ready { token, .. } => Response::builder()
            .status(StatusCode::TEMPORARY_REDIRECT)
            .header(
                header::LOCATION,
                v1::images::BLOB_ITEM.replace("{token}", &token),
            )
            .header(header::CACHE_CONTROL, "no-store")
            .body(Body::empty())
            .unwrap(),
        ImageManifestStatus::Pending { retry_after_ms } => Response::builder()
            .status(StatusCode::ACCEPTED)
            .header(header::RETRY_AFTER, "1")
            .header(header::CACHE_CONTROL, "no-store")
            .body(Body::from(
                serde_json::json!({
                    "status": "pending",
                    "retry_after_ms": retry_after_ms,
                })
                .to_string(),
            ))
            .unwrap(),
        ImageManifestStatus::Missing { .. } => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header(header::CACHE_CONTROL, "no-store")
            .body(Body::from(
                serde_json::json!({ "status": "missing" }).to_string(),
            ))
            .unwrap(),
    }
}

/// GET /api/v1/images/blob/{token} - Content-addressed immutable image blob.
pub async fn get_image_blob_handler(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> impl IntoResponse {
    let etag = format!("\"{token}\"");

    if let Some(if_none_match) = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        && if_none_match.split(',').any(|t| t.trim() == etag)
    {
        return Response::builder()
            .status(StatusCode::NOT_MODIFIED)
            .header(header::ETAG, etag)
            .header(header::CACHE_CONTROL, BLOB_CACHE_CONTROL)
            .body(Body::empty())
            .unwrap();
    }

    let path = match state.image_service().image_blob_path(&token) {
        Ok(path) => path,
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };

    let meta = match tokio::fs::metadata(&path).await {
        Ok(m) => m,
        Err(_) => return StatusCode::NOT_FOUND.into_response(),
    };

    let modified = meta.modified().unwrap_or(std::time::SystemTime::now());
    let last_modified = fmt_http_date(modified);

    if let Some(if_modified_since) = headers
        .get(header::IF_MODIFIED_SINCE)
        .and_then(|v| v.to_str().ok())
        && let Ok(since_time) = parse_http_date(if_modified_since)
        && modified <= since_time
    {
        return Response::builder()
            .status(StatusCode::NOT_MODIFIED)
            .header(header::ETAG, etag)
            .header(header::LAST_MODIFIED, last_modified)
            .header(header::CACHE_CONTROL, BLOB_CACHE_CONTROL)
            .body(Body::empty())
            .unwrap();
    }

    // Sniff content type via magic bytes (small read) without loading the whole file.
    let content_type = match tokio::fs::File::open(&path).await {
        Ok(mut f) => {
            use tokio::io::AsyncReadExt;
            let mut head = [0u8; 16];
            let n = f.read(&mut head).await.unwrap_or(0);
            validate_magic_bytes(&head[..n])
                .unwrap_or("application/octet-stream")
        }
        Err(_) => "application/octet-stream",
    };

    let file = match tokio::fs::File::open(&path).await {
        Ok(f) => f,
        Err(_) => return StatusCode::NOT_FOUND.into_response(),
    };
    let stream = ReaderStream::new(file);

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CONTENT_LENGTH, meta.len().to_string())
        .header(header::ETAG, etag)
        .header(header::LAST_MODIFIED, last_modified)
        .header(header::CACHE_CONTROL, BLOB_CACHE_CONTROL)
        .body(Body::from_stream(stream))
        .unwrap()
}

/// GET /api/v1/images/events - SSE stream for image readiness notifications.
pub async fn image_events_sse_handler(
    State(state): State<AppState>,
) -> Sse<
    impl tokio_stream::Stream<Item = Result<axum::response::sse::Event, Infallible>>,
> {
    use axum::response::sse::{Event, KeepAlive};

    let receiver = state.image_service().subscribe_image_events();

    let stream = async_stream::stream! {
        let mut live = BroadcastStream::new(receiver);
        while let Some(item) = live.next().await {
            match item {
                Ok(evt) => {
                    let Ok(bytes) = to_bytes::<RkyvError>(&evt) else {
                        continue;
                    };
                    let data = BASE64_STANDARD.encode(bytes.as_slice());
                    yield Ok::<Event, Infallible>(
                        Event::default()
                            .event(ImageSseEventType::Ready.event_name())
                            .data(data),
                    );
                }
                Err(err) => {
                    warn!("image SSE broadcast error: {err}");
                }
            }
        }
    };

    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    )
}
