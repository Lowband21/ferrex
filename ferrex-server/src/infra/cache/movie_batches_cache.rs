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
        MovieReferenceBatchBlob, MovieReferenceBatchBundleResponse,
        MovieReferenceBatchResponse,
    },
    application::unit_of_work::AppUnitOfWork,
    database::repository_ports::media_references::MovieBatchVersionRecord,
    types::{LibraryId, MovieBatchId},
};
use rayon::prelude::*;
use sha2::Digest;
use tokio::sync::Mutex;
use tracing::{debug, info};

#[derive(Debug, Clone, PartialEq, Eq)]
struct ManifestSignature([u8; 32]);

impl ManifestSignature {
    fn from_versions(versions: &[MovieBatchVersionRecord]) -> Self {
        let mut hasher = sha2::Sha256::new();
        for record in versions {
            hasher.update(record.batch_id.as_u32().to_be_bytes());
            hasher.update(record.version.to_be_bytes());
        }
        Self(hasher.finalize().into())
    }
}

#[derive(Debug, Clone)]
struct CachedMovieBatch {
    version: u64,
    #[allow(dead_code)]
    hash: u64,
    bytes: Bytes,
}

#[derive(Debug, Clone)]
struct CachedFullBundle {
    signature: ManifestSignature,
    bytes: Bytes,
}

#[derive(Debug, Default)]
struct LibraryCacheState {
    batches: HashMap<MovieBatchId, CachedMovieBatch>,
    full_bundle: Option<CachedFullBundle>,
}

/// Caches rkyv-serialized movie batch payloads to avoid rebuilding expensive
/// library bootstrap responses on every player startup.
///
/// This is an in-memory cache keyed by `(library_id, batch_id, version)` and
/// invalidated by comparing the server-side version manifest from
/// `list_movie_batch_versions_with_movies(library_id)`.
#[derive(Debug, Default)]
pub struct MovieBatchesCache {
    libraries: DashMap<LibraryId, Arc<Mutex<LibraryCacheState>>>,
}

impl MovieBatchesCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn get_library_bundle(
        &self,
        uow: Arc<AppUnitOfWork>,
        library_id: LibraryId,
    ) -> Result<Bytes, StatusCode> {
        let request_started = Instant::now();

        let versions = uow
            .media_refs
            .list_movie_batch_versions_with_movies(&library_id)
            .await
            .map_err(|_err| StatusCode::INTERNAL_SERVER_ERROR)?;

        let entry = self
            .libraries
            .entry(library_id)
            .or_insert_with(|| {
                Arc::new(Mutex::new(LibraryCacheState::default()))
            })
            .clone();

        let mut guard = entry.lock().await;

        if versions.is_empty() {
            let response = MovieReferenceBatchBundleResponse {
                library_id,
                batches: Vec::new(),
            };
            let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&response)
                .map_err(|_err| StatusCode::INTERNAL_SERVER_ERROR)?;
            let bytes = Bytes::from(bytes.into_vec());
            guard.batches.clear();
            guard.full_bundle = Some(CachedFullBundle {
                signature: ManifestSignature([0u8; 32]),
                bytes: bytes.clone(),
            });
            return Ok(bytes);
        }

        let signature = ManifestSignature::from_versions(&versions);
        if let Some(cached) = guard.full_bundle.as_ref()
            && cached.signature == signature
        {
            debug!(
                "movie batch bundle cache hit: library={} bytes={} elapsed={:?}",
                library_id,
                cached.bytes.len(),
                request_started.elapsed()
            );
            return Ok(cached.bytes.clone());
        }

        let mut rebuild_ids = Vec::new();
        let mut keep_ids = HashSet::with_capacity(versions.len());
        for record in &versions {
            keep_ids.insert(record.batch_id);
            let needs_rebuild = guard
                .batches
                .get(&record.batch_id)
                .is_none_or(|cached| cached.version != record.version);
            if needs_rebuild {
                rebuild_ids.push(record.batch_id);
            }
        }

        guard
            .batches
            .retain(|batch_id, _| keep_ids.contains(batch_id));
        guard.full_bundle = None;

        let rebuild_started = Instant::now();
        if !rebuild_ids.is_empty() {
            let rebuilt =
                build_movie_batches(Arc::clone(&uow), library_id, &rebuild_ids)
                    .await?;

            let mut versions_by_id = HashMap::with_capacity(versions.len());
            for record in &versions {
                versions_by_id.insert(record.batch_id, record.version);
            }

            for rebuilt in rebuilt {
                let version =
                    versions_by_id.get(&rebuilt.batch_id).copied().unwrap_or(1);
                guard.batches.insert(
                    rebuilt.batch_id,
                    CachedMovieBatch {
                        version,
                        hash: rebuilt.hash,
                        bytes: rebuilt.bytes,
                    },
                );
            }
        }

        let serialize_started = Instant::now();
        let mut batches = Vec::with_capacity(versions.len());
        for record in &versions {
            let Some(cached) = guard.batches.get(&record.batch_id) else {
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            };
            batches.push(MovieReferenceBatchBlob {
                batch_id: record.batch_id,
                bytes: cached.bytes.as_ref().to_vec(),
            });
        }

        let response = MovieReferenceBatchBundleResponse {
            library_id,
            batches,
        };

        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&response)
            .map_err(|_err| StatusCode::INTERNAL_SERVER_ERROR)?;
        let bytes = Bytes::from(bytes.into_vec());

        guard.full_bundle = Some(CachedFullBundle {
            signature,
            bytes: bytes.clone(),
        });

        info!(
            "Movie batches bundle cached: library={} batches={} bytes={} rebuilds={} rebuild_elapsed={:?} serialize_elapsed={:?} total_elapsed={:?}",
            library_id,
            versions.len(),
            bytes.len(),
            rebuild_ids.len(),
            rebuild_started.elapsed(),
            serialize_started.elapsed(),
            request_started.elapsed()
        );

        Ok(bytes)
    }

    pub async fn get_batch(
        &self,
        uow: Arc<AppUnitOfWork>,
        library_id: LibraryId,
        batch_id: MovieBatchId,
    ) -> Result<Bytes, StatusCode> {
        let versions = uow
            .media_refs
            .list_movie_batch_versions_with_movies(&library_id)
            .await
            .map_err(|_err| StatusCode::INTERNAL_SERVER_ERROR)?;

        let expected_version = versions
            .iter()
            .find(|record| record.batch_id == batch_id)
            .map(|record| record.version)
            .ok_or(StatusCode::NOT_FOUND)?;

        let entry = self
            .libraries
            .entry(library_id)
            .or_insert_with(|| {
                Arc::new(Mutex::new(LibraryCacheState::default()))
            })
            .clone();

        let mut guard = entry.lock().await;
        if let Some(cached) = guard.batches.get(&batch_id)
            && cached.version == expected_version
        {
            return Ok(cached.bytes.clone());
        }

        let rebuilt =
            build_movie_batches(Arc::clone(&uow), library_id, &[batch_id])
                .await?;
        let rebuilt = rebuilt
            .into_iter()
            .next()
            .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

        guard.batches.insert(
            batch_id,
            CachedMovieBatch {
                version: expected_version,
                hash: rebuilt.hash,
                bytes: rebuilt.bytes.clone(),
            },
        );
        guard.full_bundle = None;

        Ok(rebuilt.bytes)
    }

    pub async fn get_batch_subset(
        &self,
        uow: Arc<AppUnitOfWork>,
        library_id: LibraryId,
        mut batch_ids: Vec<MovieBatchId>,
    ) -> Result<Bytes, StatusCode> {
        if batch_ids.is_empty() {
            return Err(StatusCode::BAD_REQUEST);
        }

        batch_ids.sort_by_key(|id| id.as_u32());
        batch_ids.dedup();

        let versions = uow
            .media_refs
            .list_movie_batch_versions_with_movies(&library_id)
            .await
            .map_err(|_err| StatusCode::INTERNAL_SERVER_ERROR)?;

        let requested_set: HashSet<MovieBatchId> =
            batch_ids.iter().copied().collect();
        let mut requested_versions = HashMap::new();
        for record in versions {
            if requested_set.contains(&record.batch_id) {
                requested_versions.insert(record.batch_id, record.version);
            }
        }
        if requested_versions.len() != batch_ids.len() {
            return Err(StatusCode::NOT_FOUND);
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
        for batch_id in &batch_ids {
            let expected =
                requested_versions.get(batch_id).copied().unwrap_or(1);
            let needs_rebuild = guard
                .batches
                .get(batch_id)
                .is_none_or(|cached| cached.version != expected);
            if needs_rebuild {
                rebuild_ids.push(*batch_id);
            }
        }

        if !rebuild_ids.is_empty() {
            let rebuilt =
                build_movie_batches(Arc::clone(&uow), library_id, &rebuild_ids)
                    .await?;

            for rebuilt in rebuilt {
                let version = requested_versions
                    .get(&rebuilt.batch_id)
                    .copied()
                    .unwrap_or(1);
                guard.batches.insert(
                    rebuilt.batch_id,
                    CachedMovieBatch {
                        version,
                        hash: rebuilt.hash,
                        bytes: rebuilt.bytes,
                    },
                );
            }
            guard.full_bundle = None;
        }

        let mut batches = Vec::with_capacity(batch_ids.len());
        for batch_id in batch_ids {
            let Some(cached) = guard.batches.get(&batch_id) else {
                return Err(StatusCode::NOT_FOUND);
            };
            batches.push(MovieReferenceBatchBlob {
                batch_id,
                bytes: cached.bytes.as_ref().to_vec(),
            });
        }

        let response = MovieReferenceBatchBundleResponse {
            library_id,
            batches,
        };

        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&response)
            .map_err(|_err| StatusCode::INTERNAL_SERVER_ERROR)?;
        Ok(Bytes::from(bytes.into_vec()))
    }
}

#[derive(Debug)]
struct BuiltMovieBatch {
    batch_id: MovieBatchId,
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

async fn build_movie_batches(
    uow: Arc<AppUnitOfWork>,
    library_id: LibraryId,
    rebuild_ids: &[MovieBatchId],
) -> Result<Vec<BuiltMovieBatch>, StatusCode> {
    let batch_set: HashSet<MovieBatchId> =
        rebuild_ids.iter().copied().collect();

    let fetch_started = Instant::now();
    let movies = uow
        .media_refs
        .get_movie_references_for_batches(&library_id, rebuild_ids)
        .await
        .map_err(|_err| StatusCode::INTERNAL_SERVER_ERROR)?;
    debug!(
        "movie batches bulk fetch complete: library={} batches={} movies={} elapsed={:?}",
        library_id,
        rebuild_ids.len(),
        movies.len(),
        fetch_started.elapsed()
    );

    let mut movies_by_batch: HashMap<MovieBatchId, Vec<_>> = HashMap::new();
    for movie in movies {
        let Some(batch_id) = movie.batch_id else {
            continue;
        };
        if !batch_set.contains(&batch_id) {
            continue;
        }
        movies_by_batch.entry(batch_id).or_default().push(movie);
    }

    let mut build_inputs = Vec::with_capacity(rebuild_ids.len());
    for batch_id in rebuild_ids {
        build_inputs.push((
            *batch_id,
            movies_by_batch.remove(batch_id).unwrap_or_default(),
        ));
    }

    let built = tokio::task::spawn_blocking(move || {
        build_inputs
            .into_par_iter()
            .map(|(batch_id, movies)| {
                let response = MovieReferenceBatchResponse {
                    library_id,
                    batch_id,
                    movies,
                };

                let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&response)
                    .map_err(|_err| {
                        "movie batch serialize failed".to_string()
                    })?;
                let hash = stable_hash_u64(bytes.as_slice());
                Ok::<_, String>((batch_id, bytes.into_vec(), hash))
            })
            .collect::<Result<Vec<_>, String>>()
    })
    .await
    .map_err(|_err| StatusCode::INTERNAL_SERVER_ERROR)?;

    let built = built.map_err(|_err| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut out = Vec::with_capacity(built.len());
    for (batch_id, bytes, hash) in built {
        out.push(BuiltMovieBatch {
            batch_id,
            bytes: Bytes::from(bytes),
            hash,
        });
    }

    out.sort_by_key(|b| b.batch_id.as_u32());
    Ok(out)
}
