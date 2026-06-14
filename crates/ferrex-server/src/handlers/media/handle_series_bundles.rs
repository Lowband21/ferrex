use std::{collections::HashMap, sync::Arc, time::Instant};

use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Json, Response},
};
use ferrex_core::{
    api::types::{
        ApiResponse, SeriesBundleFetchRequest, SeriesBundleResponse,
        SeriesBundleSyncRequest, SeriesBundleSyncResponse,
        SeriesBundleVersionManifestEntry,
    },
    application::unit_of_work::AppUnitOfWork,
    error::MediaError,
    types::{EpisodeReference, LibraryId, SeasonReference, Series, SeriesID},
};
use ferrex_flatbuffers::{
    FLATBUFFERS_MIME,
    conversions::{batch_data as fb_batch_data, batch_sync as fb_batch_sync},
};
use futures::{StreamExt, TryStreamExt};
use sha2::Digest;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::infra::{
    app_state::AppState,
    content_negotiation::{AcceptedFormat, RKYV_OCTET_STREAM_MIME, WireFormat},
    demo_mode,
    fb_request_parsing::parse_json_or_flatbuffers,
};

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
    AcceptedFormat(response_format): AcceptedFormat,
    Path((library_id, series_id)): Path<(Uuid, Uuid)>,
) -> Result<Response, StatusCode> {
    if demo_mode::is_demo_mode(&state)
        && !demo_mode::is_demo_library(&LibraryId(library_id))
    {
        return Err(StatusCode::NOT_FOUND);
    }

    let request_started = Instant::now();
    let library_id = LibraryId(library_id);
    let series_id = SeriesID(series_id);

    info!(
        "Fetching series bundle for library {} series {} as {:?}",
        library_id, series_id, response_format
    );

    let uow = state.unit_of_work();
    let (series, seasons, episodes) =
        load_series_bundle_parts(&uow, library_id, series_id).await?;

    let response = SeriesBundleResponse {
        library_id,
        series_id,
        series: series.clone(),
        seasons: seasons.clone(),
        episodes: episodes.clone(),
    };

    let rkyv_bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&response).map_err(
        |err| {
            error!(
                "failed to serialize SeriesBundleResponse for library {} series {}: {:?}",
                library_id, series_id, err
            );
            StatusCode::INTERNAL_SERVER_ERROR
        },
    )?;

    let hash = stable_hash_u64(rkyv_bytes.as_slice());
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

    let wire = binary_series_bundle_format(response_format);
    let response = match wire {
        SeriesBundleWireFormat::Rkyv => (
            [(header::CONTENT_TYPE, RKYV_OCTET_STREAM_MIME)],
            Bytes::from(rkyv_bytes.into_vec()),
        )
            .into_response(),
        SeriesBundleWireFormat::FlatBuffers => {
            let version =
                fetch_series_bundle_version(&uow, library_id, series_id)
                    .await?
                    .unwrap_or(1);
            let bytes = fb_batch_data::serialize_series_bundle_data(
                &fb_batch_data::SeriesBundle {
                    version,
                    series: &series,
                    seasons: &seasons,
                    episodes: &episodes,
                },
            );
            (
                [(header::CONTENT_TYPE, FLATBUFFERS_MIME)],
                Bytes::from(bytes),
            )
                .into_response()
        }
    };

    let total_elapsed = request_started.elapsed();
    info!(
        "Series bundle built: library={} series={} format={:?} total_elapsed={:?}",
        library_id, series_id, wire, total_elapsed
    );

    Ok(response)
}

pub async fn get_series_bundle_bundle_handler(
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
        "Fetching series bundle bundle for library {} as {:?}",
        library_id, response_format
    );

    match binary_series_bundle_format(response_format) {
        SeriesBundleWireFormat::Rkyv => {
            let bytes = state
                .series_bundles_cache
                .get_library_bundle(uow, library_id)
                .await?;
            Ok(([(header::CONTENT_TYPE, RKYV_OCTET_STREAM_MIME)], bytes)
                .into_response())
        }
        SeriesBundleWireFormat::FlatBuffers => {
            let mut series_ids = uow
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
            series_ids.sort_by_key(|id| id.to_uuid());

            if !series_ids.is_empty() {
                state
                    .series_bundles_cache
                    .ensure_series_versioning(
                        Arc::clone(&uow),
                        library_id,
                        series_ids.clone(),
                    )
                    .await?;
            }

            let bytes = build_series_bundle_fetch_response(
                Arc::clone(&uow),
                library_id,
                series_ids,
            )
            .await?;
            Ok(([(header::CONTENT_TYPE, FLATBUFFERS_MIME)], bytes)
                .into_response())
        }
    }
}

pub async fn post_series_bundle_sync_handler(
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

    let request = parse_series_bundle_sync_request(&headers, body)?;

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

    let response = SeriesBundleSyncResponse {
        library_id,
        updates,
        removals,
    };

    Ok(series_bundle_sync_response(response_format, response))
}

pub async fn post_series_bundle_fetch_handler(
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

    let request = parse_series_bundle_fetch_request(&headers, body)?;

    let library_id = LibraryId(library_id);
    let uow = state.unit_of_work();

    let mut series_ids = request.series_ids;
    series_ids.sort_by_key(|id| id.to_uuid());
    series_ids.dedup();

    if series_ids.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    match binary_series_bundle_format(response_format) {
        SeriesBundleWireFormat::Rkyv => {
            let bytes = state
                .series_bundles_cache
                .get_series_bundle_subset(uow, library_id, series_ids)
                .await?;
            Ok(([(header::CONTENT_TYPE, RKYV_OCTET_STREAM_MIME)], bytes)
                .into_response())
        }
        SeriesBundleWireFormat::FlatBuffers => {
            state
                .series_bundles_cache
                .ensure_series_versioning(
                    Arc::clone(&uow),
                    library_id,
                    series_ids.clone(),
                )
                .await?;
            let bytes = build_series_bundle_fetch_response(
                Arc::clone(&uow),
                library_id,
                series_ids,
            )
            .await?;
            Ok(([(header::CONTENT_TYPE, FLATBUFFERS_MIME)], bytes)
                .into_response())
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SeriesBundleWireFormat {
    Rkyv,
    FlatBuffers,
}

fn binary_series_bundle_format(format: WireFormat) -> SeriesBundleWireFormat {
    match format {
        WireFormat::FlatBuffers => SeriesBundleWireFormat::FlatBuffers,
        WireFormat::Json | WireFormat::RkyvOctetStream => {
            SeriesBundleWireFormat::Rkyv
        }
    }
}

fn series_bundle_sync_response(
    format: WireFormat,
    response: SeriesBundleSyncResponse,
) -> Response {
    match format {
        WireFormat::FlatBuffers => {
            let stale_series_ids = response
                .updates
                .iter()
                .map(|entry| entry.series_id.to_uuid())
                .collect::<Vec<_>>();
            let deleted_series_ids = response
                .removals
                .iter()
                .map(|id| id.to_uuid())
                .collect::<Vec<_>>();
            let server_versions = response
                .updates
                .iter()
                .map(|entry| fb_batch_sync::SeriesBundleVersion {
                    series_id: entry.series_id.to_uuid(),
                    version: entry.version,
                })
                .collect::<Vec<_>>();
            let bytes = fb_batch_sync::serialize_series_bundle_sync_response(
                &stale_series_ids,
                &deleted_series_ids,
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

fn parse_series_bundle_sync_request(
    headers: &HeaderMap,
    body: Bytes,
) -> Result<SeriesBundleSyncRequest, StatusCode> {
    parse_json_or_flatbuffers(headers, body, |bytes| {
        fb_batch_sync::parse_series_bundle_sync_request(bytes)
            .map_err(|err| err.to_string())
            .map(|versions| SeriesBundleSyncRequest {
                bundles: versions
                    .into_iter()
                    .map(|entry| SeriesBundleVersionManifestEntry {
                        series_id: SeriesID(entry.series_id),
                        version: entry.version,
                    })
                    .collect(),
            })
    })
    .map_err(|err| {
        warn!("invalid series bundle sync request: {}", err);
        StatusCode::BAD_REQUEST
    })
}

fn parse_series_bundle_fetch_request(
    headers: &HeaderMap,
    body: Bytes,
) -> Result<SeriesBundleFetchRequest, StatusCode> {
    parse_json_or_flatbuffers(headers, body, |bytes| {
        fb_batch_sync::parse_series_bundle_fetch_request(bytes)
            .map_err(|err| err.to_string())
            .map(|series_ids| SeriesBundleFetchRequest {
                series_ids: series_ids.into_iter().map(SeriesID).collect(),
            })
    })
    .map_err(|err| {
        warn!("invalid series bundle fetch request: {}", err);
        StatusCode::BAD_REQUEST
    })
}

async fn load_series_bundle_parts(
    uow: &AppUnitOfWork,
    library_id: LibraryId,
    series_id: SeriesID,
) -> Result<(Series, Vec<SeasonReference>, Vec<EpisodeReference>), StatusCode> {
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

    Ok((series, seasons, episodes))
}

async fn fetch_series_bundle_version(
    uow: &AppUnitOfWork,
    library_id: LibraryId,
    series_id: SeriesID,
) -> Result<Option<u64>, StatusCode> {
    let versions = uow
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
    Ok(versions
        .into_iter()
        .find(|record| record.series_id == series_id)
        .map(|record| record.version))
}

#[derive(Debug)]
struct OwnedSeriesBundle {
    version: u64,
    series: Series,
    seasons: Vec<SeasonReference>,
    episodes: Vec<EpisodeReference>,
}

async fn build_series_bundle_fetch_response(
    uow: Arc<AppUnitOfWork>,
    library_id: LibraryId,
    series_ids: Vec<SeriesID>,
) -> Result<Bytes, StatusCode> {
    if series_ids.is_empty() {
        return Ok(Bytes::from(
            fb_batch_data::serialize_series_bundle_fetch_response(&[]),
        ));
    }

    let versions = uow
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
    let version_by_id = versions
        .into_iter()
        .map(|record| (record.series_id, record.version))
        .collect::<HashMap<_, _>>();

    let parallelism: usize = 8;
    let mut bundles: Vec<OwnedSeriesBundle> = futures::stream::iter(series_ids)
        .map(|series_id| {
            let uow = Arc::clone(&uow);
            let version_by_id = version_by_id.clone();
            async move {
                let (series, seasons, episodes) =
                    load_series_bundle_parts(&uow, library_id, series_id)
                        .await?;
                let version =
                    version_by_id.get(&series_id).copied().unwrap_or(1);
                Ok::<_, StatusCode>(OwnedSeriesBundle {
                    version,
                    series,
                    seasons,
                    episodes,
                })
            }
        })
        .buffer_unordered(parallelism)
        .try_collect()
        .await?;

    bundles.sort_by_key(|bundle| bundle.series.id.to_uuid());

    let bytes = tokio::task::spawn_blocking(move || {
        let borrowed = bundles
            .iter()
            .map(|bundle| fb_batch_data::SeriesBundle {
                version: bundle.version,
                series: &bundle.series,
                seasons: &bundle.seasons,
                episodes: &bundle.episodes,
            })
            .collect::<Vec<_>>();
        fb_batch_data::serialize_series_bundle_fetch_response(&borrowed)
    })
    .await
    .map_err(|_err| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Bytes::from(bytes))
}
