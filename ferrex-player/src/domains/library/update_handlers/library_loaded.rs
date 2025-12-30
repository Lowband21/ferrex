use crate::{
    domains::{
        auth::types::AuthenticationFlow,
        library::{
            LibrariesLoadState,
            messages::LibraryMessage,
            repo_snapshot::{decode_repo_snapshot, encode_repo_snapshot},
            types::{
                LibrariesBootstrapPayload, MovieBatchInstallCart,
                SeriesBundleInstallCart,
            },
        },
        ui::{
            update_handlers::{
                emit_initial_all_tab_snapshots_combined, init_all_tab_view,
            },
            utils::bump_keep_alive,
        },
    },
    infra::{
        cache::{PlayerDiskMediaRepoCache, content_hash_u64_from_integrity},
        repository::media_repo::MediaRepo,
        services::api::ApiService,
    },
    state::State,
};

use ferrex_core::player_prelude::{
    Library, LibraryId, MovieBatchFetchRequest, MovieBatchId,
    MovieBatchSyncRequest, MovieBatchVersionManifestEntry,
    MovieReferenceBatchBundleResponse, SeriesBundleBundleResponse,
    SeriesBundleFetchRequest, SeriesBundleSyncRequest,
    SeriesBundleVersionManifestEntry, SeriesID,
};
use ferrex_model::LibraryType;
use futures::{StreamExt, stream};
use iced::Task;
use rkyv::rancor::Error as RkyvError;
use rkyv::util::AlignedVec;
use sha2::Digest;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

fn encode_libraries_seed_snapshot(
    libraries: &[Library],
) -> Result<AlignedVec, RkyvError> {
    // The libraries list is metadata-only, but be explicit: the MediaRepo seed
    // should not embed any media references (those are supplied by batches/bundles).
    let mut seed = Vec::with_capacity(libraries.len());
    for library in libraries.iter().cloned() {
        let mut library = library;
        library.media = None;
        seed.push(library);
    }

    rkyv::to_bytes::<RkyvError>(&seed)
}

/// Fetch libraries list (metadata only) from the server.
///
/// This is intentionally separated from the heavier media cache bootstrap so the UI
/// can render library-scoped navigation (tabs) as soon as library metadata is known.
pub async fn fetch_libraries_list(
    api_service: Arc<dyn ApiService>,
) -> anyhow::Result<Vec<Library>> {
    let now = Instant::now();
    let libraries = api_service.fetch_libraries().await?;

    log::info!(
        "[Library] Fetched libraries list: libraries={} elapsed={:?}",
        libraries.len(),
        now.elapsed()
    );

    Ok(libraries)
}

/// Fetch all libraries
pub async fn fetch_libraries(
    api_service: Arc<dyn ApiService>,
    media_repo_cache: Option<Arc<PlayerDiskMediaRepoCache>>,
) -> anyhow::Result<LibrariesBootstrapPayload> {
    let libraries = fetch_libraries_list(api_service.clone()).await?;
    fetch_libraries_bootstrap(api_service, media_repo_cache, libraries).await
}

/// Bootstrap local media cache overlays (movie batches + series bundles) for the provided libraries.
///
/// This performs the heavier snapshot/cache sync work and is safe to run after
/// `fetch_libraries_list` has already updated UI navigation state.
pub async fn fetch_libraries_bootstrap(
    api_service: Arc<dyn ApiService>,
    media_repo_cache: Option<Arc<PlayerDiskMediaRepoCache>>,
    libraries: Vec<Library>,
) -> anyhow::Result<LibrariesBootstrapPayload> {
    let now = Instant::now();

    let movie_library_ids = libraries
        .iter()
        .filter(|library| {
            library.enabled && library.library_type == LibraryType::Movies
        })
        .map(|library| library.id)
        .collect::<Vec<_>>();

    let series_library_ids = libraries
        .iter()
        .filter(|library| {
            library.enabled && library.library_type == LibraryType::Series
        })
        .map(|library| library.id)
        .collect::<Vec<_>>();

    let parallelism: usize = 4;

    let mut used_repo_snapshot = false;
    let mut movie_batches: Vec<MovieBatchInstallCart>;
    let mut series_bundles: Vec<SeriesBundleInstallCart>;

    if let Some(cache) = media_repo_cache.clone()
        && let Some(snapshot_bytes) = cache.read_repo_snapshot().await
    {
        log::debug!(
            "[Library] media repo snapshot loaded: bytes={}",
            snapshot_bytes.len()
        );
        match decode_repo_snapshot(&snapshot_bytes) {
            Ok(snapshot) => {
                used_repo_snapshot = true;
                log::info!(
                    "[Library] media repo snapshot decoded: movie_batches={} series_bundles={}",
                    snapshot.movie_batches.len(),
                    snapshot.series_bundles.len()
                );

                let mut batches_by_library: HashMap<
                    LibraryId,
                    Vec<MovieBatchInstallCart>,
                > = HashMap::new();
                for batch in snapshot.movie_batches {
                    batches_by_library
                        .entry(batch.library_id)
                        .or_default()
                        .push(batch);
                }

                let mut bundles_by_library: HashMap<
                    LibraryId,
                    Vec<SeriesBundleInstallCart>,
                > = HashMap::new();
                for bundle in snapshot.series_bundles {
                    bundles_by_library
                        .entry(bundle.library_id)
                        .or_default()
                        .push(bundle);
                }

                let movie_seed = movie_library_ids
                    .iter()
                    .copied()
                    .map(|library_id| {
                        let seed = batches_by_library
                            .remove(&library_id)
                            .unwrap_or_default();
                        (library_id, seed)
                    })
                    .collect::<Vec<_>>();
                let series_seed = series_library_ids
                    .iter()
                    .copied()
                    .map(|library_id| {
                        let seed = bundles_by_library
                            .remove(&library_id)
                            .unwrap_or_default();
                        (library_id, seed)
                    })
                    .collect::<Vec<_>>();

                let movie_batches_fut = stream::iter(movie_seed.into_iter())
                    .map(|(library_id, seed)| {
                        let api_service = api_service.clone();
                        let cache = cache.clone();
                        async move {
                            sync_movie_batches_from_seed(
                                api_service,
                                cache,
                                library_id,
                                seed,
                            )
                            .await
                        }
                    })
                    .buffer_unordered(parallelism)
                    .collect::<Vec<_>>();

                let series_bundles_fut = stream::iter(series_seed.into_iter())
                    .map(|(library_id, seed)| {
                        let api_service = api_service.clone();
                        let cache = cache.clone();
                        async move {
                            sync_series_bundles_from_seed(
                                api_service,
                                cache,
                                library_id,
                                seed,
                            )
                            .await
                        }
                    })
                    .buffer_unordered(parallelism)
                    .collect::<Vec<_>>();

                let (movie_batches_res, series_bundles_res) =
                    tokio::join!(movie_batches_fut, series_bundles_fut);

                movie_batches = movie_batches_res
                    .into_iter()
                    .collect::<Result<Vec<_>, _>>()?
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>();
                series_bundles = series_bundles_res
                    .into_iter()
                    .collect::<Result<Vec<_>, _>>()?
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>();
            }
            Err(err) => {
                log::warn!(
                    "[Library] Media repo snapshot decode failed; err={}; falling back to per-bundle bootstrap",
                    err
                );
                cache.invalidate_repo_snapshot().await;
                movie_batches = Vec::new();
                series_bundles = Vec::new();
            }
        }
    } else {
        movie_batches = Vec::new();
        series_bundles = Vec::new();
    }

    if !used_repo_snapshot {
        let movie_batches_fut = stream::iter(movie_library_ids.into_iter())
            .map(|library_id| {
                let api_service = api_service.clone();
                let cache = media_repo_cache.clone();
                async move {
                    bootstrap_movie_batches_for_library(
                        api_service,
                        cache,
                        library_id,
                    )
                    .await
                }
            })
            .buffer_unordered(parallelism)
            .collect::<Vec<_>>();

        let series_bundles_fut = stream::iter(series_library_ids.into_iter())
            .map(|library_id| {
                let api_service = api_service.clone();
                let cache = media_repo_cache.clone();
                async move {
                    bootstrap_series_bundles_for_library(
                        api_service,
                        cache,
                        library_id,
                    )
                    .await
                }
            })
            .buffer_unordered(parallelism)
            .collect::<Vec<_>>();

        let (movie_batches_res, series_bundles_res) =
            tokio::join!(movie_batches_fut, series_bundles_fut);

        movie_batches = movie_batches_res
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        series_bundles = series_bundles_res
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
    }

    log::info!(
        "[Library] Bootstrapped overlays: movie_batches={} series_bundles={} elapsed={:?}",
        movie_batches.len(),
        series_bundles.len(),
        now.elapsed(),
    );

    if let Some(cache) = media_repo_cache.as_ref() {
        let snapshot_bytes =
            encode_repo_snapshot(&movie_batches, &series_bundles);
        if let Err(err) = cache.put_repo_snapshot(&snapshot_bytes).await {
            log::warn!(
                "[Library] media repo snapshot cache write failed; err={}",
                err
            );
        }
    }

    Ok(LibrariesBootstrapPayload {
        libraries,
        movie_batches,
        series_bundles,
    })
}

/// Handles LibrariesLoaded message
#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn handle_libraries_loaded(
    state: &mut State,
    result: Result<LibrariesBootstrapPayload, String>,
) -> Task<LibraryMessage> {
    match result {
        Ok(payload) => {
            let LibrariesBootstrapPayload {
                libraries,
                movie_batches,
                series_bundles,
            } = payload;

            state.domains.library.state.libraries = libraries.clone();
            state.tab_manager.set_libraries(&libraries);

            // Create and populate MediaRepo with library metadata first so admin
            // views can render library cards, while media content comes from
            // movie batches / series bundles.
            let media_repo = match encode_libraries_seed_snapshot(&libraries)
                .and_then(MediaRepo::new)
            {
                Ok(repo) => repo,
                Err(err) => {
                    log::error!(
                        "[Library] Failed to seed MediaRepo with libraries metadata: {err}"
                    );
                    state.domains.library.state.load_state =
                        LibrariesLoadState::Failed {
                            last_error: err.to_string(),
                        };
                    state.loading = false;
                    return Task::none();
                }
            };

            // Store MediaRepo in State
            {
                let mut repo_lock = state.media_repo.write();
                *repo_lock = Some(media_repo);
                log::info!(
                    "Initialized MediaRepo seed + overlays (libraries={} movie_batches={} series_bundles={})",
                    libraries.len(),
                    movie_batches.len(),
                    series_bundles.len()
                );
            }

            // Install movie batch payloads before initializing Home/carousels so
            // the initial UI snapshots include movies.
            for batch in movie_batches {
                if let Err(err) = state
                    .domains
                    .library
                    .state
                    .repo_accessor
                    .install_movie_reference_batch(
                        batch.library_id,
                        batch.batch_id,
                        batch.cart,
                    )
                {
                    log::warn!(
                        "[Library] Failed to install movie batch (library {} batch {} v{}): {}",
                        batch.library_id,
                        batch.batch_id,
                        batch.version,
                        err
                    );
                }
            }

            // Install series bundle payloads before initializing Home/carousels so
            // the initial UI snapshots include series.
            for bundle in series_bundles {
                if let Err(err) = state
                    .domains
                    .library
                    .state
                    .repo_accessor
                    .install_series_bundle(
                        bundle.library_id,
                        bundle.series_id,
                        bundle.cart,
                    )
                {
                    log::warn!(
                        "[Library] Failed to install series bundle (library {} series {} v{}): {}",
                        bundle.library_id,
                        bundle.series_id,
                        bundle.version,
                        err
                    );
                }
            }

            // Initialize All-tab (curated + per-library) and emit initial snapshots
            init_all_tab_view(state);
            emit_initial_all_tab_snapshots_combined(state);
            // Keep UI active briefly to ensure initial poster loads/animations are processed
            bump_keep_alive(state);

            // Refresh the All tab
            state.tab_manager.refresh_active_tab();

            // Mark load succeeded for the current session
            let user_id = match &state.domains.auth.state.auth_flow {
                AuthenticationFlow::Authenticated { user, .. } => Some(user.id),
                _ => None,
            };
            state.domains.library.state.load_state =
                LibrariesLoadState::Succeeded {
                    user_id,
                    server_url: state.server_url.clone(),
                };

            state.loading = false;
            Task::none()
        }
        Err(e) => {
            log::error!(
                "[Library] Failed to load libraries (server_url={}): {}",
                state.server_url,
                e
            );
            state.domains.library.state.load_state =
                LibrariesLoadState::Failed { last_error: e };
            state.loading = false;
            Task::none()
        }
    }
}

const FULL_BUNDLE_FETCH_THRESHOLD: usize = 256;
const CACHE_BLOB_READ_PARALLELISM: usize = 4;

async fn bootstrap_movie_batches_for_library(
    api_service: Arc<dyn ApiService>,
    cache: Option<Arc<PlayerDiskMediaRepoCache>>,
    library_id: LibraryId,
) -> anyhow::Result<Vec<MovieBatchInstallCart>> {
    if let Some(cache) = cache {
        bootstrap_movie_batches_with_cache(api_service, cache, library_id).await
    } else {
        let cart = api_service
            .fetch_movie_reference_batch_bundle(library_id)
            .await?;
        extract_movie_batches_from_bundle_cart(library_id, 0, &cart)
    }
}

async fn sync_movie_batches_from_seed(
    api_service: Arc<dyn ApiService>,
    cache: Arc<PlayerDiskMediaRepoCache>,
    library_id: LibraryId,
    mut installed: Vec<MovieBatchInstallCart>,
) -> anyhow::Result<Vec<MovieBatchInstallCart>> {
    let mut manifest = Vec::with_capacity(installed.len());
    for entry in &installed {
        manifest.push(MovieBatchVersionManifestEntry {
            batch_id: entry.batch_id,
            version: entry.version,
            content_hash: None,
        });
    }

    let sync_started = Instant::now();
    let sync = api_service
        .sync_movie_reference_batches(
            library_id,
            MovieBatchSyncRequest { batches: manifest },
        )
        .await?;
    log::debug!(
        "[Library] movie batch sync complete; library={} removals={} updates={} elapsed={:?}",
        library_id,
        sync.removals.len(),
        sync.updates.len(),
        sync_started.elapsed()
    );

    let mut dirty_index = false;
    if !sync.removals.is_empty() {
        cache.remove_movie_batches(library_id, &sync.removals).await;
        installed.retain(|c| !sync.removals.contains(&c.batch_id));
        dirty_index = true;
    }

    let total_updates = sync.updates.len();
    let mut fetch_updates_by_id: HashMap<MovieBatchId, u64> = HashMap::new();

    if total_updates > 0 {
        let mut installed_pos_by_id = HashMap::with_capacity(installed.len());
        for (idx, entry) in installed.iter().enumerate() {
            installed_pos_by_id.insert(entry.batch_id, idx);
        }

        for update in sync.updates {
            let should_skip_fetch = update.content_hash.and_then(|hash| {
                installed_pos_by_id
                    .get(&update.batch_id)
                    .copied()
                    .map(|idx| {
                        content_hash_u64(installed[idx].cart.as_slice()) == hash
                    })
            }) == Some(true);

            if should_skip_fetch {
                if let Some(idx) =
                    installed_pos_by_id.get(&update.batch_id).copied()
                {
                    installed[idx].version = update.version;
                }
                if cache
                    .set_movie_batch_version(
                        library_id,
                        update.batch_id,
                        update.version,
                    )
                    .await
                {
                    dirty_index = true;
                }
                continue;
            }

            fetch_updates_by_id.insert(update.batch_id, update.version);
        }
    }

    let skipped_by_hash =
        total_updates.saturating_sub(fetch_updates_by_id.len());
    if total_updates > 0 {
        log::debug!(
            "[Library] movie batch updates resolved; library={} updates_total={} skipped_by_hash={} to_fetch={}",
            library_id,
            total_updates,
            skipped_by_hash,
            fetch_updates_by_id.len()
        );
    }

    if !fetch_updates_by_id.is_empty() {
        installed.retain(|c| !fetch_updates_by_id.contains_key(&c.batch_id));

        let update_ids =
            fetch_updates_by_id.keys().copied().collect::<Vec<_>>();
        let fetched = if update_ids.len() >= FULL_BUNDLE_FETCH_THRESHOLD {
            let full_fetch_started = Instant::now();
            let cart = api_service
                .fetch_movie_reference_batch_bundle(library_id)
                .await?;
            log::debug!(
                "[Library] movie batch full bundle fetch complete; library={} bytes={} elapsed={:?}",
                library_id,
                cart.len(),
                full_fetch_started.elapsed()
            );
            extract_movie_batches_from_bundle_cart_filtered(
                library_id,
                &fetch_updates_by_id,
                &cart,
            )?
        } else {
            let partial_fetch_started = Instant::now();
            let cart = api_service
                .fetch_movie_reference_batches(
                    library_id,
                    MovieBatchFetchRequest {
                        batch_ids: update_ids,
                    },
                )
                .await?;
            log::debug!(
                "[Library] movie batch partial fetch complete; library={} bytes={} elapsed={:?}",
                library_id,
                cart.len(),
                partial_fetch_started.elapsed()
            );
            extract_movie_batches_from_bundle_cart_filtered(
                library_id,
                &fetch_updates_by_id,
                &cart,
            )?
        };

        for batch in &fetched {
            if cache
                .put_movie_batch(
                    library_id,
                    batch.batch_id,
                    batch.version,
                    batch.cart.as_slice(),
                )
                .await
                .is_ok()
            {
                dirty_index = true;
            }
        }

        installed.extend(fetched);
    }

    installed.sort_by_key(|c| c.batch_id.as_u32());

    if dirty_index && let Err(err) = cache.persist_index().await {
        log::warn!(
            "[Library] media repo cache index persist failed (movie batches); err={}",
            err
        );
    }

    Ok(installed)
}

async fn bootstrap_movie_batches_with_cache(
    api_service: Arc<dyn ApiService>,
    cache: Arc<PlayerDiskMediaRepoCache>,
    library_id: LibraryId,
) -> anyhow::Result<Vec<MovieBatchInstallCart>> {
    let cached_entries = cache.list_movie_batches_for_library(library_id).await;

    let mut dirty_index = false;
    let mut installed = Vec::new();

    let mut manifest = Vec::with_capacity(cached_entries.len());
    let mut cached_versions_by_id =
        HashMap::with_capacity(cached_entries.len());
    for entry in &cached_entries {
        manifest.push(MovieBatchVersionManifestEntry {
            batch_id: entry.batch_id,
            version: entry.version,
            content_hash: None,
        });
        cached_versions_by_id.insert(entry.batch_id, entry.version);
    }

    let sync_started = Instant::now();
    let sync = api_service
        .sync_movie_reference_batches(
            library_id,
            MovieBatchSyncRequest { batches: manifest },
        )
        .await?;
    log::debug!(
        "[Library] movie batch sync complete; library={} removals={} updates={} elapsed={:?}",
        library_id,
        sync.removals.len(),
        sync.updates.len(),
        sync_started.elapsed()
    );

    if !sync.removals.is_empty() {
        cache.remove_movie_batches(library_id, &sync.removals).await;
        dirty_index = true;
    }

    let total_updates = sync.updates.len();
    let mut bumped_versions_by_id: HashMap<MovieBatchId, u64> = HashMap::new();
    let mut fetch_updates_by_id: HashMap<MovieBatchId, u64> = HashMap::new();

    if total_updates > 0 {
        let mut cached_integrity_hash_by_id =
            HashMap::with_capacity(cached_versions_by_id.len());
        for entry in &cached_entries {
            if let Some(hash) =
                content_hash_u64_from_integrity(&entry.integrity)
            {
                cached_integrity_hash_by_id.insert(entry.batch_id, hash);
            }
        }

        for update in sync.updates {
            let should_skip_fetch = update.content_hash.and_then(|hash| {
                cached_integrity_hash_by_id
                    .get(&update.batch_id)
                    .copied()
                    .map(|local_hash| local_hash == hash)
            }) == Some(true);

            if should_skip_fetch {
                bumped_versions_by_id.insert(update.batch_id, update.version);
                if cache
                    .set_movie_batch_version(
                        library_id,
                        update.batch_id,
                        update.version,
                    )
                    .await
                {
                    dirty_index = true;
                }
                continue;
            }

            fetch_updates_by_id.insert(update.batch_id, update.version);
        }
    }

    let mut skip_cached_reads = HashSet::new();
    skip_cached_reads.extend(sync.removals.iter().copied());
    skip_cached_reads.extend(fetch_updates_by_id.keys().copied());

    let skipped_by_hash =
        total_updates.saturating_sub(fetch_updates_by_id.len());
    if total_updates > 0 {
        log::debug!(
            "[Library] movie batch updates resolved; library={} updates_total={} skipped_by_hash={} to_fetch={}",
            library_id,
            total_updates,
            skipped_by_hash,
            fetch_updates_by_id.len()
        );
    }

    let cached_reads_started = Instant::now();
    let cached_read_results = stream::iter(
        cached_entries
            .into_iter()
            .filter(|entry| !skip_cached_reads.contains(&entry.batch_id)),
    )
    .map(|entry| {
        let cache = cache.clone();
        let bumped_version =
            bumped_versions_by_id.get(&entry.batch_id).copied();
        async move {
            let bytes = cache.read_hash(&entry.integrity).await;
            (
                entry.batch_id,
                bumped_version.unwrap_or(entry.version),
                bytes,
            )
        }
    })
    .buffer_unordered(CACHE_BLOB_READ_PARALLELISM)
    .collect::<Vec<_>>()
    .await;

    let mut read_failures = Vec::new();
    let mut cached_bytes_total: usize = 0;
    for (batch_id, cached_version, bytes) in cached_read_results {
        match bytes {
            Ok(bytes) => {
                cached_bytes_total =
                    cached_bytes_total.saturating_add(bytes.len());
                installed.push(MovieBatchInstallCart {
                    library_id,
                    batch_id,
                    version: cached_version,
                    cart: aligned_from_slice(&bytes),
                });
            }
            Err(err) => {
                log::warn!(
                    "[Library] movie batch cache read failed; library={} batch={} err={}",
                    library_id,
                    batch_id,
                    err
                );
                read_failures.push(batch_id);
                let expected_version = cached_versions_by_id
                    .get(&batch_id)
                    .copied()
                    .unwrap_or(cached_version);
                let expected_version = bumped_versions_by_id
                    .get(&batch_id)
                    .copied()
                    .unwrap_or(expected_version);
                fetch_updates_by_id
                    .entry(batch_id)
                    .or_insert(expected_version);
                dirty_index = true;
            }
        }
    }

    if !read_failures.is_empty() {
        cache.remove_movie_batches(library_id, &read_failures).await;
    }

    log::debug!(
        "[Library] movie batch cache reads complete; library={} installed_cached={} cached_bytes={} read_failures={} elapsed={:?}",
        library_id,
        installed.len(),
        cached_bytes_total,
        read_failures.len(),
        cached_reads_started.elapsed()
    );

    if !fetch_updates_by_id.is_empty() {
        let update_ids =
            fetch_updates_by_id.keys().copied().collect::<Vec<_>>();
        let fetched = if update_ids.len() >= FULL_BUNDLE_FETCH_THRESHOLD {
            let full_fetch_started = Instant::now();
            let cart = api_service
                .fetch_movie_reference_batch_bundle(library_id)
                .await?;
            log::debug!(
                "[Library] movie batch full bundle fetch complete; library={} bytes={} elapsed={:?}",
                library_id,
                cart.len(),
                full_fetch_started.elapsed()
            );
            extract_movie_batches_from_bundle_cart_filtered(
                library_id,
                &fetch_updates_by_id,
                &cart,
            )?
        } else {
            let partial_fetch_started = Instant::now();
            let cart = api_service
                .fetch_movie_reference_batches(
                    library_id,
                    MovieBatchFetchRequest {
                        batch_ids: update_ids,
                    },
                )
                .await?;
            log::debug!(
                "[Library] movie batch partial fetch complete; library={} bytes={} elapsed={:?}",
                library_id,
                cart.len(),
                partial_fetch_started.elapsed()
            );
            extract_movie_batches_from_bundle_cart_filtered(
                library_id,
                &fetch_updates_by_id,
                &cart,
            )?
        };

        for batch in &fetched {
            if cache
                .put_movie_batch(
                    library_id,
                    batch.batch_id,
                    batch.version,
                    batch.cart.as_slice(),
                )
                .await
                .is_ok()
            {
                dirty_index = true;
            }
        }

        installed.extend(fetched);
    }

    installed.sort_by_key(|c| c.batch_id.as_u32());

    if dirty_index && let Err(err) = cache.persist_index().await {
        log::warn!(
            "[Library] media repo cache index persist failed (movie batches); err={}",
            err
        );
    }

    Ok(installed)
}

async fn bootstrap_series_bundles_for_library(
    api_service: Arc<dyn ApiService>,
    cache: Option<Arc<PlayerDiskMediaRepoCache>>,
    library_id: LibraryId,
) -> anyhow::Result<Vec<SeriesBundleInstallCart>> {
    if let Some(cache) = cache {
        bootstrap_series_bundles_with_cache(api_service, cache, library_id)
            .await
    } else {
        let cart = api_service.fetch_series_bundle_bundle(library_id).await?;
        extract_series_bundles_from_bundle_cart(library_id, 0, &cart)
    }
}

async fn sync_series_bundles_from_seed(
    api_service: Arc<dyn ApiService>,
    cache: Arc<PlayerDiskMediaRepoCache>,
    library_id: LibraryId,
    mut installed: Vec<SeriesBundleInstallCart>,
) -> anyhow::Result<Vec<SeriesBundleInstallCart>> {
    let mut manifest = Vec::with_capacity(installed.len());
    for entry in &installed {
        manifest.push(SeriesBundleVersionManifestEntry {
            series_id: entry.series_id,
            version: entry.version,
        });
    }

    let sync_started = Instant::now();
    let sync = api_service
        .sync_series_bundles(
            library_id,
            SeriesBundleSyncRequest { bundles: manifest },
        )
        .await?;
    log::debug!(
        "[Library] series bundle sync complete; library={} removals={} updates={} elapsed={:?}",
        library_id,
        sync.removals.len(),
        sync.updates.len(),
        sync_started.elapsed()
    );

    let mut dirty_index = false;
    if !sync.removals.is_empty() {
        cache
            .remove_series_bundles(library_id, &sync.removals)
            .await;
        installed.retain(|c| !sync.removals.contains(&c.series_id));
        dirty_index = true;
    }

    let mut updates_by_id = HashMap::new();
    for update in sync.updates {
        updates_by_id.insert(update.series_id, update.version);
    }

    if !updates_by_id.is_empty() {
        installed.retain(|c| !updates_by_id.contains_key(&c.series_id));

        let update_ids = updates_by_id.keys().copied().collect::<Vec<_>>();
        let partial_fetch_started = Instant::now();
        let cart = api_service
            .fetch_series_bundles(
                library_id,
                SeriesBundleFetchRequest {
                    series_ids: update_ids,
                },
            )
            .await?;
        log::debug!(
            "[Library] series bundle partial fetch complete; library={} bytes={} elapsed={:?}",
            library_id,
            cart.len(),
            partial_fetch_started.elapsed()
        );
        let fetched = extract_series_bundles_from_bundle_cart_filtered(
            library_id,
            &updates_by_id,
            &cart,
        )?;

        for bundle in &fetched {
            if cache
                .put_series_bundle(
                    library_id,
                    bundle.series_id,
                    bundle.version,
                    bundle.cart.as_slice(),
                )
                .await
                .is_ok()
            {
                dirty_index = true;
            }
        }

        installed.extend(fetched);
    }

    installed.sort_by_key(|c| c.series_id.to_uuid());

    if dirty_index && let Err(err) = cache.persist_index().await {
        log::warn!(
            "[Library] media repo cache index persist failed (series bundles); err={}",
            err
        );
    }

    Ok(installed)
}

async fn bootstrap_series_bundles_with_cache(
    api_service: Arc<dyn ApiService>,
    cache: Arc<PlayerDiskMediaRepoCache>,
    library_id: LibraryId,
) -> anyhow::Result<Vec<SeriesBundleInstallCart>> {
    let cached_entries =
        cache.list_series_bundles_for_library(library_id).await;

    let mut dirty_index = false;
    let mut installed = Vec::new();

    let mut manifest = Vec::with_capacity(cached_entries.len());
    let mut cached_versions_by_id =
        HashMap::with_capacity(cached_entries.len());
    for entry in &cached_entries {
        manifest.push(SeriesBundleVersionManifestEntry {
            series_id: entry.series_id,
            version: entry.version,
        });
        cached_versions_by_id.insert(entry.series_id, entry.version);
    }

    let sync_started = Instant::now();
    let sync = api_service
        .sync_series_bundles(
            library_id,
            SeriesBundleSyncRequest { bundles: manifest },
        )
        .await?;
    log::debug!(
        "[Library] series bundle sync complete; library={} removals={} updates={} elapsed={:?}",
        library_id,
        sync.removals.len(),
        sync.updates.len(),
        sync_started.elapsed()
    );

    if !sync.removals.is_empty() {
        cache
            .remove_series_bundles(library_id, &sync.removals)
            .await;
        dirty_index = true;
    }

    let mut updates_by_id = HashMap::new();
    for update in sync.updates {
        updates_by_id.insert(update.series_id, update.version);
    }

    let mut skip_cached_reads = HashSet::new();
    skip_cached_reads.extend(sync.removals.iter().copied());
    skip_cached_reads.extend(updates_by_id.keys().copied());

    let cached_reads_started = Instant::now();
    let cached_read_results = stream::iter(
        cached_entries
            .into_iter()
            .filter(|entry| !skip_cached_reads.contains(&entry.series_id)),
    )
    .map(|entry| {
        let cache = cache.clone();
        async move {
            let bytes = cache.read_hash(&entry.integrity).await;
            (entry.series_id, entry.version, bytes)
        }
    })
    .buffer_unordered(CACHE_BLOB_READ_PARALLELISM)
    .collect::<Vec<_>>()
    .await;

    let mut read_failures = Vec::new();
    let mut cached_bytes_total: usize = 0;
    for (series_id, cached_version, bytes) in cached_read_results {
        match bytes {
            Ok(bytes) => {
                cached_bytes_total =
                    cached_bytes_total.saturating_add(bytes.len());
                installed.push(SeriesBundleInstallCart {
                    library_id,
                    series_id,
                    version: cached_version,
                    cart: aligned_from_slice(&bytes),
                });
            }
            Err(err) => {
                log::warn!(
                    "[Library] series bundle cache read failed; library={} series={} err={}",
                    library_id,
                    series_id,
                    err
                );
                read_failures.push(series_id);
                let expected_version = cached_versions_by_id
                    .get(&series_id)
                    .copied()
                    .unwrap_or(cached_version);
                updates_by_id.entry(series_id).or_insert(expected_version);
                dirty_index = true;
            }
        }
    }

    if !read_failures.is_empty() {
        cache
            .remove_series_bundles(library_id, &read_failures)
            .await;
    }

    log::debug!(
        "[Library] series bundle cache reads complete; library={} installed_cached={} cached_bytes={} read_failures={} elapsed={:?}",
        library_id,
        installed.len(),
        cached_bytes_total,
        read_failures.len(),
        cached_reads_started.elapsed()
    );

    if !updates_by_id.is_empty() {
        let update_ids = updates_by_id.keys().copied().collect::<Vec<_>>();
        let partial_fetch_started = Instant::now();
        let cart = api_service
            .fetch_series_bundles(
                library_id,
                SeriesBundleFetchRequest {
                    series_ids: update_ids,
                },
            )
            .await?;
        log::debug!(
            "[Library] series bundle partial fetch complete; library={} bytes={} elapsed={:?}",
            library_id,
            cart.len(),
            partial_fetch_started.elapsed()
        );
        let fetched = extract_series_bundles_from_bundle_cart_filtered(
            library_id,
            &updates_by_id,
            &cart,
        )?;

        for bundle in &fetched {
            if cache
                .put_series_bundle(
                    library_id,
                    bundle.series_id,
                    bundle.version,
                    bundle.cart.as_slice(),
                )
                .await
                .is_ok()
            {
                dirty_index = true;
            }
        }

        installed.extend(fetched);
    }

    installed.sort_by_key(|c| c.series_id.to_uuid());

    if dirty_index && let Err(err) = cache.persist_index().await {
        log::warn!(
            "[Library] media repo cache index persist failed (series bundles); err={}",
            err
        );
    }

    Ok(installed)
}

fn extract_movie_batches_from_bundle_cart(
    expected_library_id: LibraryId,
    default_version: u64,
    cart: &AlignedVec,
) -> anyhow::Result<Vec<MovieBatchInstallCart>> {
    extract_movie_batches_from_bundle_cart_filtered(
        expected_library_id,
        &HashMap::new(),
        cart,
    )
    .map(|mut items| {
        for item in &mut items {
            if item.version == 0 {
                item.version = default_version;
            }
        }
        items
    })
}

fn extract_movie_batches_from_bundle_cart_filtered(
    expected_library_id: LibraryId,
    versions: &HashMap<MovieBatchId, u64>,
    cart: &AlignedVec,
) -> anyhow::Result<Vec<MovieBatchInstallCart>> {
    let archived = rkyv::access::<
        rkyv::Archived<MovieReferenceBatchBundleResponse>,
        RkyvError,
    >(cart)?;

    if archived.library_id.as_uuid() != expected_library_id.to_uuid() {
        anyhow::bail!(
            "Movie batch bundle library_id mismatch: expected {} got {}",
            expected_library_id,
            archived.library_id.as_uuid()
        );
    }

    let mut out = Vec::new();
    for batch in archived.batches.iter() {
        let batch_id = MovieBatchId::new(batch.batch_id.0.into())?;
        if !versions.is_empty() && !versions.contains_key(&batch_id) {
            continue;
        }

        let version = versions.get(&batch_id).copied().unwrap_or(0);
        let aligned = aligned_from_slice(batch.bytes.as_slice());
        out.push(MovieBatchInstallCart {
            library_id: expected_library_id,
            batch_id,
            version,
            cart: aligned,
        });
    }
    Ok(out)
}

fn extract_series_bundles_from_bundle_cart(
    expected_library_id: LibraryId,
    default_version: u64,
    cart: &AlignedVec,
) -> anyhow::Result<Vec<SeriesBundleInstallCart>> {
    extract_series_bundles_from_bundle_cart_filtered(
        expected_library_id,
        &HashMap::new(),
        cart,
    )
    .map(|mut items| {
        for item in &mut items {
            if item.version == 0 {
                item.version = default_version;
            }
        }
        items
    })
}

fn extract_series_bundles_from_bundle_cart_filtered(
    expected_library_id: LibraryId,
    versions: &HashMap<SeriesID, u64>,
    cart: &AlignedVec,
) -> anyhow::Result<Vec<SeriesBundleInstallCart>> {
    let archived = rkyv::access::<
        rkyv::Archived<SeriesBundleBundleResponse>,
        RkyvError,
    >(cart)?;

    if archived.library_id.as_uuid() != expected_library_id.to_uuid() {
        anyhow::bail!(
            "Series bundle bundle library_id mismatch: expected {} got {}",
            expected_library_id,
            archived.library_id.as_uuid()
        );
    }

    let mut out = Vec::new();
    for entry in archived.bundles.iter() {
        let series_id = SeriesID(entry.0.series_id.to_uuid());
        if !versions.is_empty() && !versions.contains_key(&series_id) {
            continue;
        }

        let version = versions.get(&series_id).copied().unwrap_or(0);
        let aligned = aligned_from_slice(entry.0.bytes.as_slice());
        out.push(SeriesBundleInstallCart {
            library_id: expected_library_id,
            series_id,
            version,
            cart: aligned,
        });
    }

    Ok(out)
}

fn content_hash_u64(bytes: &[u8]) -> u64 {
    let digest = sha2::Sha256::digest(bytes);
    u64::from_be_bytes(
        digest[..8]
            .try_into()
            .expect("sha256 digest must be at least 8 bytes"),
    )
}

fn aligned_from_slice(bytes: &[u8]) -> AlignedVec {
    let mut aligned = AlignedVec::with_capacity(bytes.len());
    aligned.extend_from_slice(bytes);
    if aligned.capacity() > aligned.len() * 2 {
        aligned.shrink_to_fit();
    }
    aligned
}
