use axum::{
    extract::{Json, Path, State},
    http::{StatusCode, header},
    response::IntoResponse,
};
use ferrex_core::{
    api::types::{
        ApiResponse, MovieBatchFetchRequest, MovieBatchSyncRequest,
        MovieBatchSyncResponse, MovieBatchVersionManifestEntry,
    },
    application::unit_of_work::AppUnitOfWork,
    types::{LibraryId, MovieBatchId},
};
use sha2::Digest;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::infra::{app_state::AppState, demo_mode};

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
    Path((library_id, batch_id)): Path<(Uuid, u32)>,
) -> impl IntoResponse {
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
        "Fetching movie reference batch {} for library {}",
        batch_id, library_id
    );

    let bytes = state
        .movie_batches_cache
        .get_batch(uow, library_id, batch_id)
        .await?;

    Ok::<_, StatusCode>((
        [(header::CONTENT_TYPE, "application/octet-stream")],
        bytes,
    ))
}

pub async fn get_movie_reference_batch_bundle_handler(
    State(state): State<AppState>,
    Path(library_id): Path<Uuid>,
) -> impl IntoResponse {
    if demo_mode::is_demo_mode(&state)
        && !demo_mode::is_demo_library(&LibraryId(library_id))
    {
        return Err(StatusCode::NOT_FOUND);
    }

    let library_id = LibraryId(library_id);
    let uow = state.unit_of_work();

    info!("Fetching movie batch bundle for library {}", library_id);

    let bytes = state
        .movie_batches_cache
        .get_library_bundle(uow, library_id)
        .await?;

    Ok::<_, StatusCode>((
        [(header::CONTENT_TYPE, "application/octet-stream")],
        bytes,
    ))
}

pub async fn post_movie_reference_batch_sync_handler(
    State(state): State<AppState>,
    Path(library_id): Path<Uuid>,
    Json(request): Json<MovieBatchSyncRequest>,
) -> Result<Json<ApiResponse<MovieBatchSyncResponse>>, StatusCode> {
    if demo_mode::is_demo_mode(&state)
        && !demo_mode::is_demo_library(&LibraryId(library_id))
    {
        return Err(StatusCode::NOT_FOUND);
    }

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

    Ok(Json(ApiResponse::success(MovieBatchSyncResponse {
        library_id,
        updates,
        removals,
    })))
}

pub async fn post_movie_reference_batch_fetch_handler(
    State(state): State<AppState>,
    Path(library_id): Path<Uuid>,
    Json(request): Json<MovieBatchFetchRequest>,
) -> Result<impl IntoResponse, StatusCode> {
    if demo_mode::is_demo_mode(&state)
        && !demo_mode::is_demo_library(&LibraryId(library_id))
    {
        return Err(StatusCode::NOT_FOUND);
    }

    let library_id = LibraryId(library_id);
    let uow = state.unit_of_work();

    let batch_ids = request.batch_ids;

    let bytes = state
        .movie_batches_cache
        .get_batch_subset(uow, library_id, batch_ids)
        .await?;

    Ok(([(header::CONTENT_TYPE, "application/octet-stream")], bytes))
}
