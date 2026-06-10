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
use ferrex_flatbuffers::conversions::batch_data as fb_batch_data;
use rayon::prelude::*;
use sha2::Digest;
use tokio::sync::Mutex;
use tracing::{debug, info};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MovieBatchWireFormat {
    Rkyv,
    FlatBuffers,
}

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
    bytes_by_format: HashMap<MovieBatchWireFormat, Bytes>,
}

#[derive(Debug, Clone)]
struct CachedFullBundle {
    signature: ManifestSignature,
    bytes: Bytes,
}

#[derive(Debug, Default)]
struct LibraryCacheState {
    batches: HashMap<MovieBatchId, CachedMovieBatch>,
    full_bundles: HashMap<MovieBatchWireFormat, CachedFullBundle>,
}

/// Caches serialized movie batch payloads to avoid rebuilding expensive
/// library bootstrap responses on every player startup.
///
/// The desktop player consumes rkyv bytes (`application/octet-stream`) while
/// mobile clients consume FlatBuffers. The cache keeps those encodings separate
/// even when they share the same `(library_id, batch_id, version)` identity so a
/// FlatBuffers request can never be served rkyv bytes (or vice versa).
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
        self.get_library_bundle_with_format(
            uow,
            library_id,
            MovieBatchWireFormat::Rkyv,
        )
        .await
    }

    pub async fn get_library_bundle_with_format(
        &self,
        uow: Arc<AppUnitOfWork>,
        library_id: LibraryId,
        format: MovieBatchWireFormat,
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
            let bytes = serialize_empty_bundle(library_id, format)?;
            guard.batches.clear();
            guard.full_bundles.clear();
            guard.full_bundles.insert(
                format,
                CachedFullBundle {
                    signature: ManifestSignature([0u8; 32]),
                    bytes: bytes.clone(),
                },
            );
            return Ok(bytes);
        }

        let signature = ManifestSignature::from_versions(&versions);
        if let Some(cached) = guard.full_bundles.get(&format)
            && cached.signature == signature
        {
            debug!(
                "movie batch bundle cache hit: library={} format={:?} bytes={} elapsed={:?}",
                library_id,
                format,
                cached.bytes.len(),
                request_started.elapsed()
            );
            return Ok(cached.bytes.clone());
        }

        let versions_by_id = versions_by_id(&versions);

        if format == MovieBatchWireFormat::FlatBuffers {
            let batch_ids = versions
                .iter()
                .map(|record| record.batch_id)
                .collect::<Vec<_>>();
            let bytes = build_movie_batch_fetch_response(
                Arc::clone(&uow),
                library_id,
                &batch_ids,
                &versions_by_id,
            )
            .await?;

            guard.full_bundles.insert(
                format,
                CachedFullBundle {
                    signature,
                    bytes: bytes.clone(),
                },
            );

            info!(
                "Movie batches FlatBuffers bundle cached: library={} batches={} bytes={} total_elapsed={:?}",
                library_id,
                versions.len(),
                bytes.len(),
                request_started.elapsed()
            );

            return Ok(bytes);
        }

        let mut rebuild_ids = Vec::new();
        let mut keep_ids = HashSet::with_capacity(versions.len());
        for record in &versions {
            keep_ids.insert(record.batch_id);
            let needs_rebuild =
                guard.batches.get(&record.batch_id).is_none_or(|cached| {
                    cached.version != record.version
                        || !cached.bytes_by_format.contains_key(&format)
                });
            if needs_rebuild {
                rebuild_ids.push(record.batch_id);
            }
        }

        guard
            .batches
            .retain(|batch_id, _| keep_ids.contains(batch_id));
        guard.full_bundles.clear();

        let rebuild_started = Instant::now();
        if !rebuild_ids.is_empty() {
            let rebuilt = build_movie_batches(
                Arc::clone(&uow),
                library_id,
                &rebuild_ids,
                format,
                &versions_by_id,
            )
            .await?;

            for rebuilt in rebuilt {
                let version =
                    versions_by_id.get(&rebuilt.batch_id).copied().unwrap_or(1);
                upsert_cached_batch(
                    &mut guard,
                    rebuilt.batch_id,
                    version,
                    rebuilt.hash,
                    format,
                    rebuilt.bytes,
                );
            }
        }

        let serialize_started = Instant::now();
        let mut batches = Vec::with_capacity(versions.len());
        for record in &versions {
            let Some(cached) = guard.batches.get(&record.batch_id) else {
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            };
            let Some(bytes) = cached.bytes_by_format.get(&format) else {
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            };
            batches.push(MovieReferenceBatchBlob {
                batch_id: record.batch_id,
                bytes: bytes.as_ref().to_vec(),
            });
        }

        let response = MovieReferenceBatchBundleResponse {
            library_id,
            batches,
        };

        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&response)
            .map_err(|_err| StatusCode::INTERNAL_SERVER_ERROR)?;
        let bytes = Bytes::from(bytes.into_vec());

        guard.full_bundles.insert(
            format,
            CachedFullBundle {
                signature,
                bytes: bytes.clone(),
            },
        );

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
        self.get_batch_with_format(
            uow,
            library_id,
            batch_id,
            MovieBatchWireFormat::Rkyv,
        )
        .await
    }

    pub async fn get_batch_with_format(
        &self,
        uow: Arc<AppUnitOfWork>,
        library_id: LibraryId,
        batch_id: MovieBatchId,
        format: MovieBatchWireFormat,
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
        let versions_by_id = versions_by_id(&versions);

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
            && let Some(bytes) = cached.bytes_by_format.get(&format)
        {
            return Ok(bytes.clone());
        }

        let rebuilt = build_movie_batches(
            Arc::clone(&uow),
            library_id,
            &[batch_id],
            format,
            &versions_by_id,
        )
        .await?;
        let rebuilt = rebuilt
            .into_iter()
            .next()
            .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

        let bytes = rebuilt.bytes.clone();
        upsert_cached_batch(
            &mut guard,
            batch_id,
            expected_version,
            rebuilt.hash,
            format,
            rebuilt.bytes,
        );
        guard.full_bundles.clear();

        Ok(bytes)
    }

    pub async fn get_batch_subset(
        &self,
        uow: Arc<AppUnitOfWork>,
        library_id: LibraryId,
        batch_ids: Vec<MovieBatchId>,
    ) -> Result<Bytes, StatusCode> {
        self.get_batch_subset_with_format(
            uow,
            library_id,
            batch_ids,
            MovieBatchWireFormat::Rkyv,
        )
        .await
    }

    pub async fn get_batch_subset_with_format(
        &self,
        uow: Arc<AppUnitOfWork>,
        library_id: LibraryId,
        mut batch_ids: Vec<MovieBatchId>,
        format: MovieBatchWireFormat,
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

        if format == MovieBatchWireFormat::FlatBuffers {
            return build_movie_batch_fetch_response(
                uow,
                library_id,
                &batch_ids,
                &requested_versions,
            )
            .await;
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
            let needs_rebuild =
                guard.batches.get(batch_id).is_none_or(|cached| {
                    cached.version != expected
                        || !cached.bytes_by_format.contains_key(&format)
                });
            if needs_rebuild {
                rebuild_ids.push(*batch_id);
            }
        }

        if !rebuild_ids.is_empty() {
            let rebuilt = build_movie_batches(
                Arc::clone(&uow),
                library_id,
                &rebuild_ids,
                format,
                &requested_versions,
            )
            .await?;

            for rebuilt in rebuilt {
                let version = requested_versions
                    .get(&rebuilt.batch_id)
                    .copied()
                    .unwrap_or(1);
                upsert_cached_batch(
                    &mut guard,
                    rebuilt.batch_id,
                    version,
                    rebuilt.hash,
                    format,
                    rebuilt.bytes,
                );
            }
            guard.full_bundles.clear();
        }

        let mut batches = Vec::with_capacity(batch_ids.len());
        for batch_id in batch_ids {
            let Some(cached) = guard.batches.get(&batch_id) else {
                return Err(StatusCode::NOT_FOUND);
            };
            let Some(bytes) = cached.bytes_by_format.get(&format) else {
                return Err(StatusCode::NOT_FOUND);
            };
            batches.push(MovieReferenceBatchBlob {
                batch_id,
                bytes: bytes.as_ref().to_vec(),
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

fn versions_by_id(
    versions: &[MovieBatchVersionRecord],
) -> HashMap<MovieBatchId, u64> {
    let mut out = HashMap::with_capacity(versions.len());
    for record in versions {
        out.insert(record.batch_id, record.version);
    }
    out
}

fn serialize_empty_bundle(
    library_id: LibraryId,
    format: MovieBatchWireFormat,
) -> Result<Bytes, StatusCode> {
    match format {
        MovieBatchWireFormat::Rkyv => {
            let response = MovieReferenceBatchBundleResponse {
                library_id,
                batches: Vec::new(),
            };
            let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&response)
                .map_err(|_err| StatusCode::INTERNAL_SERVER_ERROR)?;
            Ok(Bytes::from(bytes.into_vec()))
        }
        MovieBatchWireFormat::FlatBuffers => Ok(Bytes::from(
            fb_batch_data::serialize_batch_fetch_response(&[]),
        )),
    }
}

fn upsert_cached_batch(
    guard: &mut LibraryCacheState,
    batch_id: MovieBatchId,
    version: u64,
    hash: u64,
    format: MovieBatchWireFormat,
    bytes: Bytes,
) {
    let cached =
        guard
            .batches
            .entry(batch_id)
            .or_insert_with(|| CachedMovieBatch {
                version,
                hash,
                bytes_by_format: HashMap::new(),
            });

    if cached.version != version {
        cached.bytes_by_format.clear();
    }

    cached.version = version;
    cached.hash = hash;
    cached.bytes_by_format.insert(format, bytes);
}

async fn build_movie_batches(
    uow: Arc<AppUnitOfWork>,
    library_id: LibraryId,
    rebuild_ids: &[MovieBatchId],
    format: MovieBatchWireFormat,
    versions_by_id: &HashMap<MovieBatchId, u64>,
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
            versions_by_id.get(batch_id).copied().unwrap_or(1),
            movies_by_batch.remove(batch_id).unwrap_or_default(),
        ));
    }

    let built = tokio::task::spawn_blocking(move || {
        build_inputs
            .into_par_iter()
            .map(|(batch_id, version, movies)| {
                let bytes = match format {
                    MovieBatchWireFormat::Rkyv => {
                        let response = MovieReferenceBatchResponse {
                            library_id,
                            batch_id,
                            movies,
                        };

                        rkyv::to_bytes::<rkyv::rancor::Error>(&response)
                            .map_err(|_err| {
                                "movie batch serialize failed".to_string()
                            })?
                            .into_vec()
                    }
                    MovieBatchWireFormat::FlatBuffers => {
                        fb_batch_data::serialize_movie_batch_data(
                            &fb_batch_data::MovieBatch {
                                batch_id: batch_id.as_u32(),
                                version,
                                movies: &movies,
                            },
                        )
                    }
                };
                let hash = stable_hash_u64(&bytes);
                Ok::<_, String>((batch_id, bytes, hash))
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

async fn build_movie_batch_fetch_response(
    uow: Arc<AppUnitOfWork>,
    library_id: LibraryId,
    batch_ids: &[MovieBatchId],
    versions_by_id: &HashMap<MovieBatchId, u64>,
) -> Result<Bytes, StatusCode> {
    let batch_set: HashSet<MovieBatchId> = batch_ids.iter().copied().collect();

    let movies = uow
        .media_refs
        .get_movie_references_for_batches(&library_id, batch_ids)
        .await
        .map_err(|_err| StatusCode::INTERNAL_SERVER_ERROR)?;

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

    let mut build_inputs = Vec::with_capacity(batch_ids.len());
    for batch_id in batch_ids {
        build_inputs.push((
            *batch_id,
            versions_by_id.get(batch_id).copied().unwrap_or(1),
            movies_by_batch.remove(batch_id).unwrap_or_default(),
        ));
    }

    let bytes = tokio::task::spawn_blocking(move || {
        let batches = build_inputs
            .iter()
            .map(|(batch_id, version, movies)| fb_batch_data::MovieBatch {
                batch_id: batch_id.as_u32(),
                version: *version,
                movies,
            })
            .collect::<Vec<_>>();
        fb_batch_data::serialize_batch_fetch_response(&batches)
    })
    .await
    .map_err(|_err| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Bytes::from(bytes))
}
