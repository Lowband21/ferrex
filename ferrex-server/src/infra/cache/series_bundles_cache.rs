use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Instant,
};

use axum::body::Bytes;
use axum::http::StatusCode;
use dashmap::DashMap;
use ferrex_core::{
    api::types::{
        SeriesBundleBlob, SeriesBundleBundleResponse, SeriesBundleResponse,
    },
    application::unit_of_work::AppUnitOfWork,
    database::repository_ports::media_references::SeriesBundleVersionRecord,
    types::{LibraryId, SeriesID},
};
use futures::{StreamExt, TryStreamExt};
use rayon::prelude::*;
use sha2::Digest;
use tokio::sync::Mutex;
use tracing::{debug, info};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BundleSignature {
    series_count: usize,
    version_sum: u64,
}

impl BundleSignature {
    fn from_versions(versions: &[SeriesBundleVersionRecord]) -> Self {
        let mut version_sum: u64 = 0;
        for record in versions {
            version_sum = version_sum.saturating_add(record.version);
        }

        Self {
            series_count: versions.len(),
            version_sum,
        }
    }

    fn from_manifest(
        series_ids: &[SeriesID],
        versions_by_id: &HashMap<SeriesID, u64>,
    ) -> Self {
        let mut version_sum: u64 = 0;
        for series_id in series_ids {
            let version = versions_by_id.get(series_id).copied().unwrap_or(0);
            version_sum = version_sum.saturating_add(version);
        }

        Self {
            series_count: series_ids.len(),
            version_sum,
        }
    }
}

#[derive(Debug, Clone)]
struct CachedSeriesBundle {
    version: u64,
    hash: u64,
    bytes: Bytes,
}

#[derive(Debug, Clone)]
struct CachedFullBundle {
    signature: BundleSignature,
    bytes: Bytes,
}

#[derive(Debug, Default)]
struct LibraryCacheState {
    series: HashMap<SeriesID, CachedSeriesBundle>,
    full_bundle: Option<CachedFullBundle>,
}

/// Caches rkyv-serialized series bundle payloads to avoid rebuilding
/// expensive library bootstrap responses on every player startup.
///
/// This is an in-memory cache keyed by `(library_id, series_id, version)` and
/// invalidated by comparing the server-side version manifest.
#[derive(Debug, Default)]
pub struct SeriesBundlesCache {
    libraries: DashMap<LibraryId, Arc<Mutex<LibraryCacheState>>>,
}

impl SeriesBundlesCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn get_library_bundle(
        &self,
        uow: Arc<AppUnitOfWork>,
        library_id: LibraryId,
    ) -> Result<Bytes, StatusCode> {
        let request_started = Instant::now();

        let mut versions = uow
            .media_refs
            .list_finalized_series_bundle_versions(&library_id)
            .await
            .map_err(|_err| StatusCode::INTERNAL_SERVER_ERROR)?;

        // Scope bundle manifests to series that actually have episode references.
        // This prevents orphan `series` rows (e.g. from transient mis-matches) from
        // leaking into player-visible bundles.
        let active_series_ids = uow
            .media_refs
            .list_library_series_ids_with_episodes(&library_id)
            .await
            .map_err(|_err| StatusCode::INTERNAL_SERVER_ERROR)?;
        let active_ids: HashSet<SeriesID> =
            active_series_ids.iter().copied().collect();
        versions.retain(|record| active_ids.contains(&record.series_id));

        let entry = self
            .libraries
            .entry(library_id)
            .or_insert_with(|| {
                Arc::new(Mutex::new(LibraryCacheState::default()))
            })
            .clone();

        let mut guard = entry.lock().await;

        // Defensive: if the versioning table is incomplete (e.g. some series
        // are not finalized yet), rebuild and backfill so the bundle response
        // includes every series with episodes in the library.
        if active_series_ids.len() != versions.len() {
            let bytes = backfill_versioning_and_seed_cache(
                Arc::clone(&uow),
                library_id,
                active_series_ids,
                &mut guard,
            )
            .await?;

            let signature = guard
                .full_bundle
                .as_ref()
                .map(|entry| entry.signature)
                .unwrap_or(BundleSignature {
                    series_count: 0,
                    version_sum: 0,
                });

            info!(
                "Series bundles bundle versioning repaired: library={} series={} bytes={} total_elapsed={:?}",
                library_id,
                signature.series_count,
                bytes.len(),
                request_started.elapsed()
            );
            return Ok(bytes);
        }

        let signature = BundleSignature::from_versions(&versions);

        if let Some(cached) = guard.full_bundle.as_ref()
            && cached.signature == signature
        {
            debug!(
                "series bundle bundle cache hit: library={} bytes={} elapsed={:?}",
                library_id,
                cached.bytes.len(),
                request_started.elapsed()
            );
            return Ok(cached.bytes.clone());
        }

        // Ensure per-series caches match the current server manifest.
        let mut rebuild_ids = Vec::new();
        let mut keep_ids = HashSet::with_capacity(versions.len());
        for record in &versions {
            keep_ids.insert(record.series_id);
            let needs_rebuild = guard
                .series
                .get(&record.series_id)
                .is_none_or(|cached| cached.version != record.version);
            if needs_rebuild {
                rebuild_ids.push(record.series_id);
            }
        }

        // Drop stale entries (e.g. series deleted from the library).
        guard
            .series
            .retain(|series_id, _| keep_ids.contains(series_id));
        guard.full_bundle = None;

        let rebuild_started = Instant::now();
        if !rebuild_ids.is_empty() {
            let rebuilt = build_series_bundles(
                Arc::clone(&uow),
                library_id,
                &rebuild_ids,
                versions.len(),
            )
            .await?;

            // Map versions for quick lookup.
            let mut versions_by_id = HashMap::with_capacity(versions.len());
            for record in &versions {
                versions_by_id.insert(record.series_id, record.version);
            }

            for rebuilt in rebuilt {
                let version = versions_by_id
                    .get(&rebuilt.series_id)
                    .copied()
                    .unwrap_or(1);
                guard.series.insert(
                    rebuilt.series_id,
                    CachedSeriesBundle {
                        version,
                        hash: rebuilt.hash,
                        bytes: rebuilt.bytes,
                    },
                );
            }
        }

        // Build full bundle bytes from cached per-series payloads.
        let serialize_started = Instant::now();
        let mut bundles = Vec::with_capacity(versions.len());
        for record in &versions {
            let Some(cached) = guard.series.get(&record.series_id) else {
                // Shouldn't happen: rebuild_ids should include all missing series.
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            };

            bundles.push((
                SeriesBundleBlob {
                    series_id: record.series_id,
                    bytes: cached.bytes.as_ref().to_vec(),
                },
                cached.hash,
            ));
        }

        let response = SeriesBundleBundleResponse {
            library_id,
            bundles,
        };

        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&response)
            .map_err(|_err| StatusCode::INTERNAL_SERVER_ERROR)?;
        let bytes = Bytes::from(bytes.into_vec());

        let cached = CachedFullBundle {
            signature,
            bytes: bytes.clone(),
        };
        guard.full_bundle = Some(cached);

        info!(
            "Series bundles bundle cached: library={} series={} bytes={} rebuilds={} rebuild_elapsed={:?} serialize_elapsed={:?} total_elapsed={:?}",
            library_id,
            signature.series_count,
            bytes.len(),
            rebuild_ids.len(),
            rebuild_started.elapsed(),
            serialize_started.elapsed(),
            request_started.elapsed()
        );

        Ok(bytes)
    }

    pub async fn get_series_bundle(
        &self,
        uow: Arc<AppUnitOfWork>,
        library_id: LibraryId,
        series_id: SeriesID,
    ) -> Result<Bytes, StatusCode> {
        // Fast-path: serve from cache when present.
        let cached_entry = self
            .libraries
            .get(&library_id)
            .map(|entry| Arc::clone(entry.value()));
        if let Some(entry) = cached_entry {
            let guard = entry.lock().await;
            if let Some(cached) = guard.series.get(&series_id) {
                return Ok(cached.bytes.clone());
            }
        }

        // Fallback to building the single bundle and caching it.
        let rebuilt =
            build_single_series_bundle(Arc::clone(&uow), library_id, series_id)
                .await?;

        // Best-effort: keep the server-side versioning table up to date so
        // sync manifests remain accurate.
        if let Err(_err) = uow
            .media_refs
            .upsert_series_bundle_hash(&library_id, &series_id, rebuilt.hash)
            .await
        {
            debug!(
                "series bundle hash upsert failed (cache path): library={} series={}",
                library_id, series_id
            );
        }

        let versions = uow
            .media_refs
            .list_finalized_series_bundle_versions(&library_id)
            .await
            .map_err(|_err| StatusCode::INTERNAL_SERVER_ERROR)?;
        let version = versions
            .into_iter()
            .find(|record| record.series_id == series_id)
            .map(|record| record.version)
            .unwrap_or(1);

        let entry = self
            .libraries
            .entry(library_id)
            .or_insert_with(|| {
                Arc::new(Mutex::new(LibraryCacheState::default()))
            })
            .clone();

        let mut guard = entry.lock().await;
        guard.series.insert(
            series_id,
            CachedSeriesBundle {
                version,
                hash: rebuilt.hash,
                bytes: rebuilt.bytes.clone(),
            },
        );
        // Invalidate full bundle cache - this series may have been updated.
        guard.full_bundle = None;

        Ok(rebuilt.bytes)
    }

    /// Ensures the server-side `series_bundle_versioning` table has a finalized
    /// entry (and content hash) for each series id in `series_ids`.
    ///
    /// This is a defensive reconciliation path intended to repair situations
    /// where scan-driven finalization missed an entry (e.g. process restart,
    /// event loss, or legacy data). It is safe to call repeatedly.
    pub async fn ensure_series_versioning(
        &self,
        uow: Arc<AppUnitOfWork>,
        library_id: LibraryId,
        mut series_ids: Vec<SeriesID>,
    ) -> Result<(), StatusCode> {
        series_ids.sort_by_key(|id| id.to_uuid());
        series_ids.dedup();

        if series_ids.is_empty() {
            return Ok(());
        }

        let rebuilt = build_series_bundles_bulk(
            Arc::clone(&uow),
            library_id,
            &series_ids,
        )
        .await?;

        // Backfill versioning hashes in bounded parallelism.
        let parallelism: usize = 8;
        let upserts = rebuilt
            .iter()
            .map(|bundle| (bundle.series_id, bundle.hash))
            .collect::<Vec<_>>();
        futures::stream::iter(upserts.into_iter())
            .map(|(series_id, hash)| {
                let uow = Arc::clone(&uow);
                async move {
                    uow.media_refs
                        .upsert_series_bundle_hash(
                            &library_id,
                            &series_id,
                            hash,
                        )
                        .await
                        .map_err(|_err| StatusCode::INTERNAL_SERVER_ERROR)
                }
            })
            .buffer_unordered(parallelism)
            .try_collect::<Vec<_>>()
            .await?;

        // Keep cache coherent for subsequent fetches.
        let versions = uow
            .media_refs
            .list_finalized_series_bundle_versions(&library_id)
            .await
            .map_err(|_err| StatusCode::INTERNAL_SERVER_ERROR)?;
        let mut versions_by_id = HashMap::with_capacity(versions.len());
        for record in &versions {
            versions_by_id.insert(record.series_id, record.version);
        }

        let entry = self
            .libraries
            .entry(library_id)
            .or_insert_with(|| {
                Arc::new(Mutex::new(LibraryCacheState::default()))
            })
            .clone();
        let mut guard = entry.lock().await;
        for bundle in rebuilt {
            let version =
                versions_by_id.get(&bundle.series_id).copied().unwrap_or(1);
            guard.series.insert(
                bundle.series_id,
                CachedSeriesBundle {
                    version,
                    hash: bundle.hash,
                    bytes: bundle.bytes,
                },
            );
        }
        guard.full_bundle = None;

        Ok(())
    }

    pub async fn get_series_bundle_subset(
        &self,
        uow: Arc<AppUnitOfWork>,
        library_id: LibraryId,
        series_ids: Vec<SeriesID>,
    ) -> Result<Bytes, StatusCode> {
        if series_ids.is_empty() {
            return Err(StatusCode::BAD_REQUEST);
        }

        let mut deduped = series_ids;
        deduped.sort_by_key(|id| id.to_uuid());
        deduped.dedup();

        let versions = uow
            .media_refs
            .list_finalized_series_bundle_versions(&library_id)
            .await
            .map_err(|_err| StatusCode::INTERNAL_SERVER_ERROR)?;

        let requested_ids: HashSet<SeriesID> =
            deduped.iter().copied().collect();
        let mut requested_versions: HashMap<SeriesID, u64> = HashMap::new();
        for record in versions {
            if requested_ids.contains(&record.series_id) {
                requested_versions.insert(record.series_id, record.version);
            }
        }

        // Defensive: if versioning is missing for any of the requested series,
        // build+upsert those bundles so the caller can fetch them.
        if requested_versions.len() != deduped.len() {
            let missing_ids: Vec<SeriesID> = deduped
                .iter()
                .copied()
                .filter(|id| !requested_versions.contains_key(id))
                .collect();

            let rebuilt = build_series_bundles(
                Arc::clone(&uow),
                library_id,
                &missing_ids,
                deduped.len(),
            )
            .await?;

            let parallelism: usize = 8;
            let upserts = rebuilt
                .iter()
                .map(|bundle| (bundle.series_id, bundle.hash))
                .collect::<Vec<_>>();
            futures::stream::iter(upserts.into_iter())
                .map(|(series_id, hash)| {
                    let uow = Arc::clone(&uow);
                    async move {
                        uow.media_refs
                            .upsert_series_bundle_hash(
                                &library_id,
                                &series_id,
                                hash,
                            )
                            .await
                            .map_err(|_err| StatusCode::INTERNAL_SERVER_ERROR)
                    }
                })
                .buffer_unordered(parallelism)
                .try_collect::<Vec<_>>()
                .await?;

            let refreshed = uow
                .media_refs
                .list_finalized_series_bundle_versions(&library_id)
                .await
                .map_err(|_err| StatusCode::INTERNAL_SERVER_ERROR)?;

            requested_versions.clear();
            for record in refreshed {
                if requested_ids.contains(&record.series_id) {
                    requested_versions.insert(record.series_id, record.version);
                }
            }

            if requested_versions.len() != deduped.len() {
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            }

            let entry = self
                .libraries
                .entry(library_id)
                .or_insert_with(|| {
                    Arc::new(Mutex::new(LibraryCacheState::default()))
                })
                .clone();

            let mut guard = entry.lock().await;
            for rebuilt in rebuilt {
                let version = requested_versions
                    .get(&rebuilt.series_id)
                    .copied()
                    .unwrap_or(1);
                guard.series.insert(
                    rebuilt.series_id,
                    CachedSeriesBundle {
                        version,
                        hash: rebuilt.hash,
                        bytes: rebuilt.bytes,
                    },
                );
            }
            guard.full_bundle = None;
        }

        let entry = self
            .libraries
            .entry(library_id)
            .or_insert_with(|| {
                Arc::new(Mutex::new(LibraryCacheState::default()))
            })
            .clone();

        let mut guard = entry.lock().await;

        let mut rebuild_ids = Vec::new();
        for series_id in &deduped {
            let expected_version =
                requested_versions.get(series_id).copied().unwrap_or(1);
            let needs_rebuild = guard
                .series
                .get(series_id)
                .is_none_or(|cached| cached.version != expected_version);
            if needs_rebuild {
                rebuild_ids.push(*series_id);
            }
        }

        if !rebuild_ids.is_empty() {
            let rebuilt = build_series_bundles(
                Arc::clone(&uow),
                library_id,
                &rebuild_ids,
                deduped.len(),
            )
            .await?;

            for rebuilt in rebuilt {
                let version = requested_versions
                    .get(&rebuilt.series_id)
                    .copied()
                    .unwrap_or(1);
                guard.series.insert(
                    rebuilt.series_id,
                    CachedSeriesBundle {
                        version,
                        hash: rebuilt.hash,
                        bytes: rebuilt.bytes,
                    },
                );
            }

            guard.full_bundle = None;
        }

        let mut bundles = Vec::with_capacity(deduped.len());
        for series_id in deduped {
            let Some(cached) = guard.series.get(&series_id) else {
                return Err(StatusCode::NOT_FOUND);
            };

            bundles.push((
                SeriesBundleBlob {
                    series_id,
                    bytes: cached.bytes.as_ref().to_vec(),
                },
                cached.hash,
            ));
        }

        let response = SeriesBundleBundleResponse {
            library_id,
            bundles,
        };

        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&response)
            .map_err(|_err| StatusCode::INTERNAL_SERVER_ERROR)?;
        Ok(Bytes::from(bytes.into_vec()))
    }
}

async fn backfill_versioning_and_seed_cache(
    uow: Arc<AppUnitOfWork>,
    library_id: LibraryId,
    mut series_ids: Vec<SeriesID>,
    guard: &mut LibraryCacheState,
) -> Result<Bytes, StatusCode> {
    series_ids.sort_by_key(|id| id.to_uuid());
    series_ids.dedup();

    if series_ids.is_empty() {
        let response = SeriesBundleBundleResponse {
            library_id,
            bundles: Vec::new(),
        };
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&response)
            .map_err(|_err| StatusCode::INTERNAL_SERVER_ERROR)?;
        let bytes = Bytes::from(bytes.into_vec());
        guard.series.clear();
        guard.full_bundle = Some(CachedFullBundle {
            signature: BundleSignature {
                series_count: 0,
                version_sum: 0,
            },
            bytes: bytes.clone(),
        });
        return Ok(bytes);
    }

    let rebuilt =
        build_series_bundles_bulk(Arc::clone(&uow), library_id, &series_ids)
            .await?;

    // Backfill versioning hashes in bounded parallelism.
    let parallelism: usize = 8;
    let upserts = rebuilt
        .iter()
        .map(|bundle| (bundle.series_id, bundle.hash))
        .collect::<Vec<_>>();
    futures::stream::iter(upserts.into_iter())
        .map(|(series_id, hash)| {
            let uow = Arc::clone(&uow);
            async move {
                uow.media_refs
                    .upsert_series_bundle_hash(&library_id, &series_id, hash)
                    .await
                    .map_err(|_err| StatusCode::INTERNAL_SERVER_ERROR)
            }
        })
        .buffer_unordered(parallelism)
        .try_collect::<Vec<_>>()
        .await?;

    let versions = uow
        .media_refs
        .list_finalized_series_bundle_versions(&library_id)
        .await
        .map_err(|_err| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut versions_by_id = HashMap::with_capacity(versions.len());
    for record in &versions {
        versions_by_id.insert(record.series_id, record.version);
    }

    guard.series.clear();
    for bundle in rebuilt {
        let version =
            versions_by_id.get(&bundle.series_id).copied().unwrap_or(1);
        guard.series.insert(
            bundle.series_id,
            CachedSeriesBundle {
                version,
                hash: bundle.hash,
                bytes: bundle.bytes,
            },
        );
    }

    // Build the response from the requested manifest, not from the version list.
    //
    // If `series_bundle_versioning` is partially populated (or temporarily empty),
    // relying on `list_finalized_series_bundle_versions` can produce an empty bundle
    // response even though we just rebuilt the series bundles successfully.
    let mut bundles = Vec::with_capacity(series_ids.len());
    for series_id in &series_ids {
        let Some(cached) = guard.series.get(series_id) else {
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        };

        bundles.push((
            SeriesBundleBlob {
                series_id: *series_id,
                bytes: cached.bytes.as_ref().to_vec(),
            },
            cached.hash,
        ));
    }

    let response = SeriesBundleBundleResponse {
        library_id,
        bundles,
    };

    let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&response)
        .map_err(|_err| StatusCode::INTERNAL_SERVER_ERROR)?;
    let bytes = Bytes::from(bytes.into_vec());

    let signature =
        BundleSignature::from_manifest(&series_ids, &versions_by_id);
    guard.full_bundle = Some(CachedFullBundle {
        signature,
        bytes: bytes.clone(),
    });

    Ok(bytes)
}

#[derive(Debug)]
struct BuiltSeriesBundle {
    series_id: SeriesID,
    bytes: Bytes,
    hash: u64,
}

fn stable_hash_u64(bytes: &[u8]) -> u64 {
    let digest = sha2::Sha256::digest(bytes);
    u64::from_be_bytes(
        digest[..8]
            .try_into()
            .expect("sha256 digest must be at least 8 bytes"),
    )
}

async fn build_single_series_bundle(
    uow: Arc<AppUnitOfWork>,
    library_id: LibraryId,
    series_id: SeriesID,
) -> Result<BuiltSeriesBundle, StatusCode> {
    let mut series = uow
        .media_refs
        .get_series_reference(&series_id)
        .await
        .map_err(|err| match err {
            ferrex_core::error::MediaError::NotFound(_) => {
                StatusCode::NOT_FOUND
            }
            _other => StatusCode::INTERNAL_SERVER_ERROR,
        })?;

    if series.library_id != library_id {
        return Err(StatusCode::NOT_FOUND);
    }

    let (seasons, episodes) = tokio::join!(
        uow.media_refs.get_series_seasons(&series_id),
        uow.media_refs.get_series_episodes(&series_id)
    );

    let seasons = seasons.map_err(|_err| StatusCode::INTERNAL_SERVER_ERROR)?;
    let episodes =
        episodes.map_err(|_err| StatusCode::INTERNAL_SERVER_ERROR)?;

    series.details.available_seasons = Some(seasons.len() as u16);
    series.details.available_episodes = Some(episodes.len() as u16);

    let response = SeriesBundleResponse {
        library_id,
        series_id,
        series,
        seasons,
        episodes,
    };

    let built = tokio::task::spawn_blocking(move || {
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&response)
            .map_err(|_err| "series bundle serialize failed".to_string())?;
        let hash = stable_hash_u64(bytes.as_slice());
        Ok::<_, String>((Bytes::from(bytes.into_vec()), hash))
    })
    .await
    .map_err(|_err| StatusCode::INTERNAL_SERVER_ERROR)?;

    let (bytes, hash) =
        built.map_err(|_err| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(BuiltSeriesBundle {
        series_id,
        bytes,
        hash,
    })
}

async fn build_series_bundles(
    uow: Arc<AppUnitOfWork>,
    library_id: LibraryId,
    rebuild_ids: &[SeriesID],
    total_series: usize,
) -> Result<Vec<BuiltSeriesBundle>, StatusCode> {
    const BULK_THRESHOLD: usize = 64;

    if rebuild_ids.len() >= BULK_THRESHOLD || rebuild_ids.len() == total_series
    {
        build_series_bundles_bulk(uow, library_id, rebuild_ids).await
    } else {
        build_series_bundles_targeted(uow, library_id, rebuild_ids).await
    }
}

async fn build_series_bundles_targeted(
    uow: Arc<AppUnitOfWork>,
    library_id: LibraryId,
    rebuild_ids: &[SeriesID],
) -> Result<Vec<BuiltSeriesBundle>, StatusCode> {
    let parallelism: usize = 8;
    let mut bundles: Vec<BuiltSeriesBundle> =
        futures::stream::iter(rebuild_ids.iter().copied())
            .map(|series_id| {
                let uow = Arc::clone(&uow);
                async move {
                    build_single_series_bundle(uow, library_id, series_id).await
                }
            })
            .buffer_unordered(parallelism)
            .try_collect()
            .await?;

    bundles.sort_by_key(|bundle| bundle.series_id.to_uuid());
    Ok(bundles)
}

async fn build_series_bundles_bulk(
    uow: Arc<AppUnitOfWork>,
    library_id: LibraryId,
    rebuild_ids: &[SeriesID],
) -> Result<Vec<BuiltSeriesBundle>, StatusCode> {
    let rebuild_set: HashSet<SeriesID> = rebuild_ids.iter().copied().collect();

    let fetch_started = Instant::now();
    let (series, seasons, episodes) = tokio::join!(
        uow.media_refs.get_library_series(&library_id),
        uow.media_refs.get_library_seasons(&library_id),
        uow.media_refs.get_library_episodes(&library_id)
    );

    let series = series.map_err(|_err| StatusCode::INTERNAL_SERVER_ERROR)?;
    let seasons = seasons.map_err(|_err| StatusCode::INTERNAL_SERVER_ERROR)?;
    let episodes =
        episodes.map_err(|_err| StatusCode::INTERNAL_SERVER_ERROR)?;
    debug!(
        "series bundle cache bulk hydrate complete: library={} series={} seasons={} episodes={} elapsed={:?}",
        library_id,
        series.len(),
        seasons.len(),
        episodes.len(),
        fetch_started.elapsed()
    );

    let mut seasons_by_series: HashMap<uuid::Uuid, Vec<_>> = HashMap::new();
    for season in seasons {
        seasons_by_series
            .entry(season.series_id.to_uuid())
            .or_default()
            .push(season);
    }
    for seasons in seasons_by_series.values_mut() {
        seasons.sort_by_key(|s| s.season_number.value());
    }

    let mut episodes_by_series: HashMap<uuid::Uuid, Vec<_>> = HashMap::new();
    for episode in episodes {
        episodes_by_series
            .entry(episode.series_id.to_uuid())
            .or_default()
            .push(episode);
    }
    for episodes in episodes_by_series.values_mut() {
        episodes.sort_by_key(|e| {
            (e.season_number.value(), e.episode_number.value())
        });
    }

    let build_inputs = {
        let mut out = Vec::new();
        for mut series_ref in series {
            let series_id = series_ref.id;
            if !rebuild_set.contains(&series_id) {
                continue;
            }

            let series_uuid = series_id.to_uuid();
            let seasons =
                seasons_by_series.remove(&series_uuid).unwrap_or_default();
            let episodes =
                episodes_by_series.remove(&series_uuid).unwrap_or_default();
            series_ref.details.available_seasons = Some(seasons.len() as u16);
            series_ref.details.available_episodes = Some(episodes.len() as u16);

            out.push((series_ref, seasons, episodes));
        }
        out
    };

    let built = tokio::task::spawn_blocking(move || {
        build_inputs
            .into_par_iter()
            .map(|(series_ref, seasons, episodes)| {
                let series_id = series_ref.id;
                let library_id = series_ref.library_id;
                let response = SeriesBundleResponse {
                    library_id,
                    series_id,
                    series: series_ref,
                    seasons,
                    episodes,
                };

                let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&response)
                    .map_err(|_err| {
                        "series bundle serialize failed".to_string()
                    })?;

                let hash = stable_hash_u64(bytes.as_slice());
                Ok::<_, String>((series_id, bytes.into_vec(), hash))
            })
            .collect::<Result<Vec<_>, String>>()
    })
    .await
    .map_err(|_err| StatusCode::INTERNAL_SERVER_ERROR)?;

    let built = built.map_err(|_err| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut out = Vec::with_capacity(built.len());
    for (series_id, bytes, hash) in built {
        out.push(BuiltSeriesBundle {
            series_id,
            bytes: Bytes::from(bytes),
            hash,
        });
    }

    out.sort_by_key(|bundle| bundle.series_id.to_uuid());
    Ok(out)
}
