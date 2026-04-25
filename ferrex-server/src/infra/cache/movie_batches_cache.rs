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

/// Wire format selector for cache consumers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireFormat {
    /// rkyv zero-copy — existing desktop/web player path.
    Rkyv,
    /// FlatBuffers — mobile client path.
    FlatBuffers,
}

#[derive(Clone)]
struct CachedMovieBatch {
    version: u64,
    #[allow(dead_code)]
    hash: u64,
    /// Pre-serialized rkyv bytes for this batch.
    rkyv_bytes: Bytes,
    /// Source movie references — kept in memory so FlatBuffers responses
    /// can be assembled without re-querying the database.
    movies: Arc<Vec<ferrex_model::MovieReference>>,
}

impl std::fmt::Debug for CachedMovieBatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CachedMovieBatch")
            .field("version", &self.version)
            .field("hash", &self.hash)
            .field("rkyv_bytes_len", &self.rkyv_bytes.len())
            .field("movies_count", &self.movies.len())
            .finish()
    }
}

#[derive(Debug, Clone)]
struct CachedFullBundle {
    signature: ManifestSignature,
    rkyv_bytes: Option<Bytes>,
    fb_bytes: Option<Bytes>,
}

#[derive(Debug, Default)]
struct LibraryCacheState {
    batches: HashMap<MovieBatchId, CachedMovieBatch>,
    full_bundle: Option<CachedFullBundle>,
}

/// Caches movie-batch payloads in both rkyv and FlatBuffers wire formats.
///
/// Per-batch data is cached as:
/// - **rkyv bytes** — pre-serialized, used directly for rkyv responses.
/// - **source `MovieReference` structs** — used to build FlatBuffers responses
///   on demand without re-querying the database.
///
/// Full-library bundles are cached as assembled bytes in **both** formats.
///
/// Keyed by `(library_id, batch_id, version)` and invalidated by comparing
/// the server-side version manifest.
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
        format: WireFormat,
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
            let bytes = match format {
                WireFormat::Rkyv => {
                    let b = serialize_empty_rkyv_bundle(library_id)?;
                    Bytes::from(b)
                }
                WireFormat::FlatBuffers => {
                    let b = ferrex_flatbuffers::conversions::batch_data::serialize_batch_fetch_response(&[]);
                    Bytes::from(b)
                }
            };
            guard.batches.clear();
            guard.full_bundle = Some(CachedFullBundle {
                signature: ManifestSignature([0u8; 32]),
                rkyv_bytes: match format {
                    WireFormat::Rkyv => Some(bytes.clone()),
                    _ => None,
                },
                fb_bytes: match format {
                    WireFormat::FlatBuffers => Some(bytes.clone()),
                    _ => None,
                },
            });
            return Ok(bytes);
        }

        let signature = ManifestSignature::from_versions(&versions);
        if let Some(cached) = guard.full_bundle.as_ref()
            && cached.signature == signature
        {
            let hit = match format {
                WireFormat::Rkyv => cached.rkyv_bytes.as_ref(),
                WireFormat::FlatBuffers => cached.fb_bytes.as_ref(),
            };
            if let Some(bytes) = hit {
                debug!(
                    "movie batch bundle cache hit: library={} format={:?} bytes={} elapsed={:?}",
                    library_id,
                    format,
                    bytes.len(),
                    request_started.elapsed()
                );
                return Ok(bytes.clone());
            }
            // Signature matches but this format slot hasn't been built yet.
            // Fall through to build it.
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
                        rkyv_bytes: rebuilt.rkyv_bytes,
                        movies: rebuilt.movies,
                    },
                );
            }
        }

        let serialize_started = Instant::now();

        // Build only the requested format.
        let result = match format {
            WireFormat::Rkyv => {
                assemble_rkyv_bundle(library_id, &versions, &guard.batches)?
            }
            WireFormat::FlatBuffers => {
                assemble_fb_bundle(&versions, &guard.batches)
            }
        };

        // Update the bundle cache — preserve the other format if it was
        // already cached under the same signature.
        let existing = guard.full_bundle.take();
        let (prev_rkyv, prev_fb) = existing
            .filter(|c| c.signature == signature)
            .map(|c| (c.rkyv_bytes, c.fb_bytes))
            .unwrap_or((None, None));

        guard.full_bundle = Some(CachedFullBundle {
            signature,
            rkyv_bytes: match format {
                WireFormat::Rkyv => Some(result.clone()),
                _ => prev_rkyv,
            },
            fb_bytes: match format {
                WireFormat::FlatBuffers => Some(result.clone()),
                _ => prev_fb,
            },
        });

        info!(
            "Movie batches bundle cached: library={} format={:?} batches={} bytes={} rebuilds={} rebuild_elapsed={:?} serialize_elapsed={:?} total_elapsed={:?}",
            library_id,
            format,
            versions.len(),
            result.len(),
            rebuild_ids.len(),
            rebuild_started.elapsed(),
            serialize_started.elapsed(),
            request_started.elapsed()
        );

        Ok(result)
    }

    pub async fn get_batch(
        &self,
        uow: Arc<AppUnitOfWork>,
        library_id: LibraryId,
        batch_id: MovieBatchId,
        format: WireFormat,
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
            return Ok(match format {
                WireFormat::Rkyv => cached.rkyv_bytes.clone(),
                WireFormat::FlatBuffers => {
                    let fb = ferrex_flatbuffers::conversions::batch_data::serialize_batch_fetch_response(
                        &[ferrex_flatbuffers::conversions::batch_data::BatchInput {
                            batch_id: batch_id.as_u32(),
                            version: cached.version,
                            movies: &cached.movies,
                        }],
                    );
                    Bytes::from(fb)
                }
            });
        }

        let rebuilt =
            build_movie_batches(Arc::clone(&uow), library_id, &[batch_id])
                .await?;
        let rebuilt = rebuilt
            .into_iter()
            .next()
            .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

        let result = match format {
            WireFormat::Rkyv => rebuilt.rkyv_bytes.clone(),
            WireFormat::FlatBuffers => {
                let fb = ferrex_flatbuffers::conversions::batch_data::serialize_batch_fetch_response(
                    &[ferrex_flatbuffers::conversions::batch_data::BatchInput {
                        batch_id: batch_id.as_u32(),
                        version: expected_version,
                        movies: &rebuilt.movies,
                    }],
                );
                Bytes::from(fb)
            }
        };

        guard.batches.insert(
            batch_id,
            CachedMovieBatch {
                version: expected_version,
                hash: rebuilt.hash,
                rkyv_bytes: rebuilt.rkyv_bytes,
                movies: rebuilt.movies,
            },
        );
        guard.full_bundle = None;

        Ok(result)
    }

    pub async fn get_batch_subset(
        &self,
        uow: Arc<AppUnitOfWork>,
        library_id: LibraryId,
        mut batch_ids: Vec<MovieBatchId>,
        format: WireFormat,
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
                        rkyv_bytes: rebuilt.rkyv_bytes,
                        movies: rebuilt.movies,
                    },
                );
            }
            guard.full_bundle = None;
        }

        match format {
            WireFormat::Rkyv => {
                let mut batches = Vec::with_capacity(batch_ids.len());
                for batch_id in batch_ids {
                    let Some(cached) = guard.batches.get(&batch_id) else {
                        return Err(StatusCode::NOT_FOUND);
                    };
                    batches.push(MovieReferenceBatchBlob {
                        batch_id,
                        bytes: cached.rkyv_bytes.as_ref().to_vec(),
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
            WireFormat::FlatBuffers => {
                let fb_batches: Vec<_> = batch_ids
                    .iter()
                    .filter_map(|batch_id| {
                        let cached = guard.batches.get(batch_id)?;
                        Some(ferrex_flatbuffers::conversions::batch_data::BatchInput {
                            batch_id: batch_id.as_u32(),
                            version: cached.version,
                            movies: &cached.movies,
                        })
                    })
                    .collect();

                if fb_batches.len() != batch_ids.len() {
                    return Err(StatusCode::NOT_FOUND);
                }

                let bytes = ferrex_flatbuffers::conversions::batch_data::serialize_batch_fetch_response(
                    &fb_batches,
                );
                Ok(Bytes::from(bytes))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

struct BuiltMovieBatch {
    batch_id: MovieBatchId,
    rkyv_bytes: Bytes,
    movies: Arc<Vec<ferrex_model::MovieReference>>,
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

/// Serialize an empty rkyv bundle.
fn serialize_empty_rkyv_bundle(
    library_id: LibraryId,
) -> Result<Vec<u8>, StatusCode> {
    let response = MovieReferenceBatchBundleResponse {
        library_id,
        batches: Vec::new(),
    };
    let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&response)
        .map_err(|_err| StatusCode::INTERNAL_SERVER_ERROR)?
        .into_vec();
    Ok(bytes)
}

/// Assemble a full rkyv bundle from per-batch cached rkyv bytes.
fn assemble_rkyv_bundle(
    library_id: LibraryId,
    versions: &[MovieBatchVersionRecord],
    batches: &HashMap<MovieBatchId, CachedMovieBatch>,
) -> Result<Bytes, StatusCode> {
    let mut blobs = Vec::with_capacity(versions.len());
    for record in versions {
        let Some(cached) = batches.get(&record.batch_id) else {
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        };
        blobs.push(MovieReferenceBatchBlob {
            batch_id: record.batch_id,
            bytes: cached.rkyv_bytes.as_ref().to_vec(),
        });
    }

    let response = MovieReferenceBatchBundleResponse {
        library_id,
        batches: blobs,
    };

    let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&response)
        .map_err(|_err| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Bytes::from(bytes.into_vec()))
}

/// Assemble a full FlatBuffers bundle from per-batch cached source structs.
fn assemble_fb_bundle(
    versions: &[MovieBatchVersionRecord],
    batches: &HashMap<MovieBatchId, CachedMovieBatch>,
) -> Bytes {
    use ferrex_flatbuffers::conversions::batch_data::{
        BatchInput, serialize_batch_fetch_response,
    };

    let inputs: Vec<BatchInput<'_>> = versions
        .iter()
        .filter_map(|record| {
            let cached = batches.get(&record.batch_id)?;
            Some(BatchInput {
                batch_id: record.batch_id.as_u32(),
                version: record.version,
                movies: &cached.movies,
            })
        })
        .collect();

    Bytes::from(serialize_batch_fetch_response(&inputs))
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

    let mut build_inputs: Vec<(
        MovieBatchId,
        Vec<ferrex_model::MovieReference>,
    )> = Vec::with_capacity(rebuild_ids.len());
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
                // Serialize to rkyv for the existing path.
                let response = MovieReferenceBatchResponse {
                    library_id,
                    batch_id,
                    movies: movies.clone(),
                };

                let rkyv_bytes = rkyv::to_bytes::<rkyv::rancor::Error>(
                    &response,
                )
                .map_err(|_err| "movie batch serialize failed".to_string())?;
                let hash = stable_hash_u64(rkyv_bytes.as_slice());

                Ok::<_, String>(BuiltMovieBatch {
                    batch_id,
                    rkyv_bytes: Bytes::from(rkyv_bytes.into_vec()),
                    movies: Arc::new(movies),
                    hash,
                })
            })
            .collect::<Result<Vec<_>, String>>()
    })
    .await
    .map_err(|_err| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut out = built.map_err(|_err| StatusCode::INTERNAL_SERVER_ERROR)?;
    out.sort_by_key(|b| b.batch_id.as_u32());
    Ok(out)
}
