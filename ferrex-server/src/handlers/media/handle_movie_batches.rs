use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Json, Response},
};
use ferrex_core::{
    api::types::{
        ApiResponse, MovieBatchFetchRequest, MovieBatchSyncRequest,
        MovieBatchSyncResponse, MovieBatchVersionManifestEntry,
    },
    application::unit_of_work::AppUnitOfWork,
    types::{LibraryId, MovieBatchId},
};
use ferrex_flatbuffers::{
    FLATBUFFERS_MIME, conversions::batch_sync as fb_batch_sync,
};
use sha2::Digest;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::infra::{
    app_state::AppState,
    cache::MovieBatchWireFormat,
    content_negotiation::{AcceptedFormat, RKYV_OCTET_STREAM_MIME, WireFormat},
    demo_mode,
    fb_request_parsing::parse_json_or_flatbuffers,
};

async fn refresh_unfinalized_movie_batch_hash(
    uow: &AppUnitOfWork,
    library_id: &LibraryId,
) -> Result<(), StatusCode> {
    let batch_id = uow
        .media_refs
        .get_unfinalized_movie_reference_batch_id(library_id)
        .await
        .map_err(|err| {
            error!(
                "failed to query unfinalized movie batch id for library {}: {}",
                library_id, err
            );
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let Some(batch_id) = batch_id else {
        return Ok(());
    };

    let existing_hash = uow
        .media_refs
        .get_movie_batch_hash(library_id, batch_id)
        .await
        .map_err(|err| {
            error!(
                "failed to fetch movie batch hash state for library {} batch {}: {}",
                library_id, batch_id, err
            );
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    if existing_hash.is_some() {
        return Ok(());
    }

    let movies = uow
        .media_refs
        .get_movie_references_by_batch(library_id, batch_id)
        .await
        .map_err(|err| {
            error!(
                "failed to fetch unfinalized movie batch for library {} batch {}: {}",
                library_id, batch_id, err
            );
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    if movies.is_empty() {
        return Ok(());
    }

    let batch_size = movies.len() as u32;

    let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(
        &ferrex_core::api::types::MovieReferenceBatchResponse {
            library_id: *library_id,
            batch_id,
            movies,
        },
    )
    .map_err(|err| {
        error!(
            "failed to serialize MovieReferenceBatchResponse for library {} batch {}: {:?}",
            library_id, batch_id, err
        );
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let digest = sha2::Sha256::digest(bytes.as_slice());
    let hash = u64::from_be_bytes(
        digest[..8]
            .try_into()
            .expect("sha256 digest must be at least 8 bytes"),
    );

    if let Err(err) = uow
        .media_refs
        .upsert_movie_batch_hash(library_id, &batch_id, hash, batch_size)
        .await
    {
        error!(
            "movie batch hash backfill failed for library {} batch {}: {}",
            library_id, batch_id, err
        );
    }

    Ok(())
}

pub async fn get_movie_reference_batch_handler(
    State(state): State<AppState>,
    AcceptedFormat(response_format): AcceptedFormat,
    Path((library_id, batch_id)): Path<(Uuid, u32)>,
) -> Result<Response, StatusCode> {
    if demo_mode::is_demo_mode(&state)
        && !demo_mode::is_demo_library(&LibraryId(library_id))
    {
        return Err(StatusCode::NOT_FOUND);
    }

    let batch_id = MovieBatchId::new(batch_id).map_err(|err| {
        warn!("invalid movie batch id {}: {}", batch_id, err);
        StatusCode::BAD_REQUEST
    })?;

    let library_id = LibraryId(library_id);
    let uow = state.unit_of_work();

    info!(
        "Fetching movie reference batch {} for library {} as {:?}",
        batch_id, library_id, response_format
    );

    let cache_format = binary_movie_batch_format(response_format);
    let bytes = state
        .movie_batches_cache
        .get_batch_with_format(uow, library_id, batch_id, cache_format)
        .await?;

    Ok(binary_response(cache_format, bytes))
}

pub async fn get_movie_reference_batch_bundle_handler(
    State(state): State<AppState>,
    AcceptedFormat(response_format): AcceptedFormat,
    Path(library_id): Path<Uuid>,
) -> Result<Response, StatusCode> {
    if demo_mode::is_demo_mode(&state)
        && !demo_mode::is_demo_library(&LibraryId(library_id))
    {
        return Err(StatusCode::NOT_FOUND);
    }

    let library_id = LibraryId(library_id);
    let uow = state.unit_of_work();

    info!(
        "Fetching movie batch bundle for library {} as {:?}",
        library_id, response_format
    );

    let cache_format = binary_movie_batch_format(response_format);
    let bytes = state
        .movie_batches_cache
        .get_library_bundle_with_format(uow, library_id, cache_format)
        .await?;

    Ok(binary_response(cache_format, bytes))
}

pub async fn post_movie_reference_batch_sync_handler(
    State(state): State<AppState>,
    AcceptedFormat(response_format): AcceptedFormat,
    Path(library_id): Path<Uuid>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, StatusCode> {
    if demo_mode::is_demo_mode(&state)
        && !demo_mode::is_demo_library(&LibraryId(library_id))
    {
        return Err(StatusCode::NOT_FOUND);
    }

    let request = parse_movie_batch_sync_request(&headers, body)?;

    let library_id = LibraryId(library_id);
    let uow = state.unit_of_work();

    refresh_unfinalized_movie_batch_hash(&uow, &library_id).await?;

    let server_versions = uow
        .media_refs
        .list_movie_batch_manifest_with_movies(&library_id)
        .await
        .map_err(|err| {
            error!(
                "failed to list movie batch versions for library {}: {}",
                library_id, err
            );
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let mut client_versions = std::collections::HashMap::new();
    for entry in request.batches {
        client_versions.insert(entry.batch_id, entry.version);
    }

    let mut server_ids = std::collections::HashSet::new();
    let mut updates = Vec::new();
    for record in server_versions {
        server_ids.insert(record.batch_id);
        if client_versions.get(&record.batch_id).copied()
            != Some(record.version)
        {
            updates.push(MovieBatchVersionManifestEntry {
                batch_id: record.batch_id,
                version: record.version,
                content_hash: record.content_hash,
            });
        }
    }
    updates.sort_by_key(|e| e.batch_id.as_u32());

    let mut removals = Vec::new();
    for batch_id in client_versions.keys() {
        if !server_ids.contains(batch_id) {
            removals.push(*batch_id);
        }
    }
    removals.sort_by_key(|id| id.as_u32());

    let response = MovieBatchSyncResponse {
        library_id,
        updates,
        removals,
    };

    Ok(movie_batch_sync_response(response_format, response))
}

pub async fn post_movie_reference_batch_fetch_handler(
    State(state): State<AppState>,
    AcceptedFormat(response_format): AcceptedFormat,
    Path(library_id): Path<Uuid>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, StatusCode> {
    if demo_mode::is_demo_mode(&state)
        && !demo_mode::is_demo_library(&LibraryId(library_id))
    {
        return Err(StatusCode::NOT_FOUND);
    }

    let request = parse_movie_batch_fetch_request(&headers, body)?;

    let library_id = LibraryId(library_id);
    let uow = state.unit_of_work();

    let cache_format = binary_movie_batch_format(response_format);
    let bytes = state
        .movie_batches_cache
        .get_batch_subset_with_format(
            uow,
            library_id,
            request.batch_ids,
            cache_format,
        )
        .await?;

    Ok(binary_response(cache_format, bytes))
}

fn binary_movie_batch_format(format: WireFormat) -> MovieBatchWireFormat {
    match format {
        WireFormat::FlatBuffers => MovieBatchWireFormat::FlatBuffers,
        WireFormat::Json | WireFormat::RkyvOctetStream => {
            MovieBatchWireFormat::Rkyv
        }
    }
}

fn binary_response(format: MovieBatchWireFormat, bytes: Bytes) -> Response {
    let content_type = match format {
        MovieBatchWireFormat::Rkyv => RKYV_OCTET_STREAM_MIME,
        MovieBatchWireFormat::FlatBuffers => FLATBUFFERS_MIME,
    };
    ([(header::CONTENT_TYPE, content_type)], bytes).into_response()
}

fn movie_batch_sync_response(
    format: WireFormat,
    response: MovieBatchSyncResponse,
) -> Response {
    match format {
        WireFormat::FlatBuffers => {
            let stale_batch_ids = response
                .updates
                .iter()
                .map(|entry| entry.batch_id.as_u32())
                .collect::<Vec<_>>();
            let deleted_batch_ids = response
                .removals
                .iter()
                .map(|id| id.as_u32())
                .collect::<Vec<_>>();
            let server_versions = response
                .updates
                .iter()
                .map(|entry| fb_batch_sync::BatchVersion {
                    batch_id: entry.batch_id.as_u32(),
                    version: entry.version,
                })
                .collect::<Vec<_>>();
            let bytes = fb_batch_sync::serialize_batch_sync_response(
                &stale_batch_ids,
                &deleted_batch_ids,
                &server_versions,
            );
            (
                [(header::CONTENT_TYPE, FLATBUFFERS_MIME)],
                Bytes::from(bytes),
            )
                .into_response()
        }
        WireFormat::Json | WireFormat::RkyvOctetStream => {
            Json(ApiResponse::success(response)).into_response()
        }
    }
}

fn parse_movie_batch_sync_request(
    headers: &HeaderMap,
    body: Bytes,
) -> Result<MovieBatchSyncRequest, StatusCode> {
    parse_json_or_flatbuffers(headers, body, |bytes| {
        fb_batch_sync::parse_batch_sync_request(bytes)
            .map_err(|err| err.to_string())?
            .into_iter()
            .map(|entry| {
                Ok(MovieBatchVersionManifestEntry {
                    batch_id: MovieBatchId::new(entry.batch_id)
                        .map_err(|err| err.to_string())?,
                    version: entry.version,
                    content_hash: None,
                })
            })
            .collect::<Result<Vec<_>, String>>()
            .map(|batches| MovieBatchSyncRequest { batches })
    })
    .map_err(|err| {
        warn!("invalid movie batch sync request: {}", err);
        StatusCode::BAD_REQUEST
    })
}

fn parse_movie_batch_fetch_request(
    headers: &HeaderMap,
    body: Bytes,
) -> Result<MovieBatchFetchRequest, StatusCode> {
    parse_json_or_flatbuffers(headers, body, |bytes| {
        fb_batch_sync::parse_batch_fetch_request(bytes)
            .map_err(|err| err.to_string())?
            .into_iter()
            .map(|batch_id| {
                MovieBatchId::new(batch_id).map_err(|err| err.to_string())
            })
            .collect::<Result<Vec<_>, String>>()
            .map(|batch_ids| MovieBatchFetchRequest { batch_ids })
    })
    .map_err(|err| {
        warn!("invalid movie batch fetch request: {}", err);
        StatusCode::BAD_REQUEST
    })
}
