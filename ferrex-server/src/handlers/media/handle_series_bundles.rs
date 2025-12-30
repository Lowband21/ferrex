use std::{sync::Arc, time::Instant};

use axum::{
    body::Bytes,
    extract::{Json, Path, State},
    http::{StatusCode, header},
    response::IntoResponse,
};
use ferrex_core::{
    api::types::{
        ApiResponse, SeriesBundleFetchRequest, SeriesBundleResponse,
        SeriesBundleSyncRequest, SeriesBundleSyncResponse,
        SeriesBundleVersionManifestEntry,
    },
    error::MediaError,
    types::{LibraryId, SeriesID},
};
use sha2::Digest;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::infra::{app_state::AppState, demo_mode};

fn stable_hash_u64(bytes: &[u8]) -> u64 {
    let digest = sha2::Sha256::digest(bytes);
    u64::from_be_bytes(
        digest[..8]
            .try_into()
            .expect("sha256 digest must be at least 8 bytes"),
    )
}

pub async fn get_series_bundle_handler(
    State(state): State<AppState>,
    Path((library_id, series_id)): Path<(Uuid, Uuid)>,
) -> impl IntoResponse {
    if demo_mode::is_demo_mode(&state)
        && !demo_mode::is_demo_library(&LibraryId(library_id))
    {
        return Err(StatusCode::NOT_FOUND);
    }

    let request_started = Instant::now();
    let library_id = LibraryId(library_id);
    let series_id = SeriesID(series_id);

    info!(
        "Fetching series bundle for library {} series {}",
        library_id, series_id
    );

    let uow = state.unit_of_work();

    let mut series = uow
        .media_refs
        .get_series_reference(&series_id)
        .await
        .map_err(|err| match err {
            MediaError::NotFound(_) => StatusCode::NOT_FOUND,
            other => {
                error!(
                    "failed to fetch series reference {} for library {}: {}",
                    series_id, library_id, other
                );
                StatusCode::INTERNAL_SERVER_ERROR
            }
        })?;

    if series.library_id != library_id {
        warn!(
            "series bundle request library mismatch: requested library {} but series {} belongs to {}",
            library_id, series_id, series.library_id
        );
        return Err(StatusCode::NOT_FOUND);
    }

    let (seasons, episodes) = tokio::join!(
        uow.media_refs.get_series_seasons(&series_id),
        uow.media_refs.get_series_episodes(&series_id)
    );

    let seasons = seasons.map_err(|err| {
        error!(
            "failed to fetch seasons for library {} series {}: {}",
            library_id, series_id, err
        );
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let episodes = episodes.map_err(|err| {
        error!(
            "failed to fetch episodes for library {} series {}: {}",
            library_id, series_id, err
        );
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    series.details.available_seasons = Some(seasons.len() as u16);
    series.details.available_episodes = Some(episodes.len() as u16);

    let response = SeriesBundleResponse {
        library_id,
        series_id,
        series,
        seasons,
        episodes,
    };

    let bytes =
        rkyv::to_bytes::<rkyv::rancor::Error>(&response).map_err(|err| {
            error!(
                "failed to serialize SeriesBundleResponse for library {} series {}: {:?}",
                library_id, series_id, err
            );
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let hash = stable_hash_u64(bytes.as_slice());
    if let Err(err) = uow
        .media_refs
        .upsert_series_bundle_hash(&library_id, &series_id, hash)
        .await
    {
        error!(
            "series bundle hash upsert failed for library {} series {}: {}",
            library_id, series_id, err
        );
    }

    let total_elapsed = request_started.elapsed();
    info!(
        "Series bundle built: library={} series={} bytes={} total_elapsed={:?}",
        library_id,
        series_id,
        bytes.len(),
        total_elapsed
    );

    Ok::<_, StatusCode>((
        [(header::CONTENT_TYPE, "application/octet-stream")],
        Bytes::from(bytes.into_vec()),
    ))
}

pub async fn get_series_bundle_bundle_handler(
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

    info!("Fetching series bundle bundle for library {}", library_id);

    let bytes = state
        .series_bundles_cache
        .get_library_bundle(uow, library_id)
        .await?;

    Ok::<_, StatusCode>((
        [(header::CONTENT_TYPE, "application/octet-stream")],
        bytes,
    ))
}

pub async fn post_series_bundle_sync_handler(
    State(state): State<AppState>,
    Path(library_id): Path<Uuid>,
    Json(request): Json<SeriesBundleSyncRequest>,
) -> Result<Json<ApiResponse<SeriesBundleSyncResponse>>, StatusCode> {
    if demo_mode::is_demo_mode(&state)
        && !demo_mode::is_demo_library(&LibraryId(library_id))
    {
        return Err(StatusCode::NOT_FOUND);
    }

    let library_id = LibraryId(library_id);
    let uow = state.unit_of_work();

    // Defensive: if scan-driven finalization/versioning missed any series that
    // already have episodes indexed, repair the `series_bundle_versioning` table
    // before we compute the sync manifest. Otherwise the client can never learn
    // that those series exist (because it only asks for versions that we list).
    let series_with_episodes = uow
        .media_refs
        .list_library_series_ids_with_episodes(&library_id)
        .await
        .map_err(|err| {
            error!(
                "failed to list series ids with episodes for library {}: {}",
                library_id, err
            );
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let mut server_versions = uow
        .media_refs
        .list_finalized_series_bundle_versions(&library_id)
        .await
        .map_err(|err| {
            error!(
                "failed to list series bundle versions for library {}: {}",
                library_id, err
            );
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let expected_ids: std::collections::HashSet<_> =
        series_with_episodes.iter().copied().collect();
    let mut server_ids: std::collections::HashSet<_> =
        server_versions.iter().map(|r| r.series_id).collect();

    let missing_ids: Vec<_> = series_with_episodes
        .iter()
        .copied()
        .filter(|id| !server_ids.contains(id))
        .collect();

    if !missing_ids.is_empty() {
        info!(
            "repairing missing series bundle versioning rows: library={} missing={}",
            library_id,
            missing_ids.len()
        );

        state
            .series_bundles_cache
            .ensure_series_versioning(Arc::clone(&uow), library_id, missing_ids)
            .await?;

        // Refresh versions after repair.
        server_versions = uow
            .media_refs
            .list_finalized_series_bundle_versions(&library_id)
            .await
            .map_err(|err| {
                error!(
                    "failed to re-list series bundle versions for library {}: {}",
                    library_id, err
                );
                StatusCode::INTERNAL_SERVER_ERROR
            })?
            .into_iter()
            .filter(|record| expected_ids.contains(&record.series_id))
            .collect();
        server_ids = server_versions.iter().map(|r| r.series_id).collect();
    } else {
        // Restrict to expected ids (i.e. series that currently have episodes)
        // so orphan versioning rows don't leak into the client manifest.
        server_versions
            .retain(|record| expected_ids.contains(&record.series_id));
        server_ids = server_versions.iter().map(|r| r.series_id).collect();
    }

    let mut client_versions = std::collections::HashMap::new();
    for entry in request.bundles {
        client_versions.insert(entry.series_id, entry.version);
    }

    let mut updates = Vec::new();
    for record in server_versions {
        if !server_ids.contains(&record.series_id) {
            continue;
        }

        if client_versions.get(&record.series_id).copied()
            != Some(record.version)
        {
            updates.push(SeriesBundleVersionManifestEntry {
                series_id: record.series_id,
                version: record.version,
            });
        }
    }
    updates.sort_by_key(|e| e.series_id.to_uuid());

    let mut removals = Vec::new();
    for series_id in client_versions.keys() {
        if !server_ids.contains(series_id) {
            removals.push(*series_id);
        }
    }
    removals.sort_by_key(|id| id.to_uuid());

    Ok(Json(ApiResponse::success(SeriesBundleSyncResponse {
        library_id,
        updates,
        removals,
    })))
}

pub async fn post_series_bundle_fetch_handler(
    State(state): State<AppState>,
    Path(library_id): Path<Uuid>,
    Json(request): Json<SeriesBundleFetchRequest>,
) -> Result<impl IntoResponse, StatusCode> {
    if demo_mode::is_demo_mode(&state)
        && !demo_mode::is_demo_library(&LibraryId(library_id))
    {
        return Err(StatusCode::NOT_FOUND);
    }

    let library_id = LibraryId(library_id);
    let uow = state.unit_of_work();

    let mut series_ids = request.series_ids;
    series_ids.sort_by_key(|id| id.to_uuid());
    series_ids.dedup();

    if series_ids.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let bytes = state
        .series_bundles_cache
        .get_series_bundle_subset(uow, library_id, series_ids)
        .await?;

    Ok(([(header::CONTENT_TYPE, "application/octet-stream")], bytes))
}
