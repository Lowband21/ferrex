use std::collections::HashMap;
use std::sync::Arc;

use super::super::LibraryMediaCache;
use crate::{
    domains::library::messages::Message,
    domains::media::library::fetch_library_media_references,
    domains::media::models::ReferenceOrganizer,
    infrastructure::api_types::{
        LibraryMediaResponse, LibraryType, MediaDetailsOption, MediaReference,
    },
    state_refactored::State,
};
use ferrex_core::MovieReference;
use ferrex_core::SeriesReference;
use iced::Task;
use uuid::Uuid;

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::all_functions
)]
impl State {
    /// Load media references for a library using the new API
    pub fn load_library_media_references(&mut self, library_id: Uuid) -> Task<Message> {
        self.loading = true;
        self.domains.ui.state.error_message = None;

        // Global fetcher is already initialized on startup

        let server_url = self.server_url.clone();

        Task::perform(
            async move { fetch_library_media_references(server_url, library_id).await },
            |result| match result {
                Ok(response) => Message::LibraryMediaReferencesLoaded(Ok(response)),
                Err(e) => Message::LibraryMediaReferencesLoaded(Err(e.to_string())),
            },
        )
    }

    /// Process loaded media references and organize them
    pub fn process_media_references(
        &mut self,
        response: LibraryMediaResponse,
    ) -> Vec<Task<Message>> {
        // Legacy field removed - not needed with new architecture

        // Get library ID and type for caching
        let library_id = response.library.id;
        let library_type = response.library.library_type;

        // Per user guidance: "We should basically never be clearing data"
        // Only clear if this is an explicit refresh operation
        // For now, we'll never clear and just update/add references
        log::info!(
            "Loading library {} ({:?}) without clearing existing data",
            library_id,
            library_type
        );

        // NEW ARCHITECTURE: Use BatchCoordinator for efficient initial load
        // Initialize tasks list that will be returned
        let mut tasks = Vec::new();

        // Process initial load asynchronously - this populates the MediaStore with ALL media types
        let media_store = Arc::clone(&self.domains.media.state.media_store);
        let items_to_load = response.media.clone();
        let items_count = items_to_load.len();

        let batch_task = Task::perform(
            async move {
                let coordinator = crate::domains::media::store::BatchCoordinator::new(media_store);

                // Process as initial load - preserve server sorting
                // Use smaller chunks for better UI responsiveness
                let config = crate::domains::media::store::BatchConfig {
                    is_initial_load: true, // Skip client-side sorting
                    preserve_order: true,  // Keep server's sort order
                    max_batch_size: 250,   // Smaller chunks for better responsiveness
                    use_background_thread: true,
                };

                // Convert to library data format - includes movies, series, seasons, and episodes
                let library_data = vec![(library_id, items_to_load)];

                match coordinator.process_initial_load(library_data).await {
                    Ok(_) => {
                        log::info!(
                            "MediaStore successfully loaded with {} items from library {}",
                            items_count,
                            library_id
                        );
                        true
                    }
                    Err(e) => {
                        log::error!("CRITICAL: Failed to load library into MediaStore: {}", e);
                        false
                    }
                }
            },
            |success| {
                if !success {
                    log::error!("MediaStore population failed - UI may not display all media");
                }
                Message::NoOp
            },
        );

        tasks.push(batch_task);

        // Log the counts for debugging
        let mut endpoint_count = 0;
        let mut details_count = 0;
        let mut movies_count = 0;
        let mut series_count = 0;
        let mut seasons_count = 0;
        let mut episodes_count = 0;

        for media_ref in &response.media {
            match media_ref {
                MediaReference::Movie(movie) => {
                    movies_count += 1;
                    match &movie.details {
                        MediaDetailsOption::Endpoint(_) => endpoint_count += 1,
                        MediaDetailsOption::Details(_) => details_count += 1,
                    }
                }
                MediaReference::Series(series) => {
                    series_count += 1;
                    match &series.details {
                        MediaDetailsOption::Endpoint(_) => endpoint_count += 1,
                        MediaDetailsOption::Details(_) => details_count += 1,
                    }
                }
                MediaReference::Season(season) => {
                    seasons_count += 1;
                    log::trace!(
                        "Received season {} (ID: {}) for series {}",
                        season.season_number.value(),
                        season.id.as_str(),
                        season.series_id.as_str()
                    );
                }
                MediaReference::Episode(episode) => {
                    episodes_count += 1;
                }
            }
        }

        log::info!(
            "Library {} contains {} movies, {} series, {} seasons, {} episodes ({} endpoint refs, {} detail refs)",
            library_id, movies_count, series_count, seasons_count, episodes_count, endpoint_count, details_count
        );

        // Log summary for debugging
        log::info!(
            "Passing {} total items to MediaStore (including {} seasons)",
            items_count,
            seasons_count
        );

        // ViewModels will be refreshed automatically when MediaStore batch ends
        // DO NOT refresh here - it causes partial data to be shown
        log::debug!(
            "MediaStore loaded with {} items - ViewModels will refresh when batch completes",
            items_count
        );

        // Clone media references for immediate queueing
        let media_refs_for_queue = response.media.clone();

        // Update the current library's media field
        if let Some(library_id) = &self.domains.library.state.current_library_id {
            if let Some(library) = self
                .domains
                .library
                .state
                .libraries
                .iter_mut()
                .find(|l| &l.id == library_id)
            {
                // Store only MovieReference and SeriesReference in the library
                library.media = Some(
                    response
                        .media
                        .iter()
                        .filter_map(|media_ref| {
                            // Only include movies and series, skip seasons and episodes
                            match media_ref.media_type() {
                                "movie" | "series" => Some(media_ref.clone()),
                                _ => None,
                            }
                        })
                        .collect(),
                );
                log::info!(
                    "Updated library {}",
                    library.name,
                    //library.media.len()
                );
            }
        }

        // MediaStore population is handled by the batch_task created above
        // It will asynchronously insert all media references (movies, series, seasons, episodes)

        // Store in library cache for instant switching
        let media_refs = response.media.clone();
        match library_type {
            LibraryType::Movies => {
                let movie_refs: Vec<MovieReference> = media_refs
                    .into_iter()
                    .filter_map(|m| m.as_movie().cloned())
                    .collect();
                let cache = LibraryMediaCache::Movies {
                    references: movie_refs,
                };
                self.domains
                    .library
                    .state
                    .library_media_cache
                    .insert(library_id, cache);
            }
            LibraryType::TvShows => {
                // For TV shows, organize into proper structure
                let (_, tv_shows) = ReferenceOrganizer::organize_references(response.media);

                let mut series_map = HashMap::new();
                let mut season_map = HashMap::new();
                let mut episode_map = HashMap::new();

                for (series_id, (series, seasons, episodes)) in tv_shows {
                    series_map.insert(series_id, series);
                    season_map.insert(series_id, seasons);

                    for (season_id, eps) in episodes {
                        episode_map.insert(season_id, eps);
                    }
                }

                // Sort series for display
                let mut sorted_series: Vec<SeriesReference> =
                    series_map.values().cloned().collect();
                sorted_series.sort_by(|a, b| a.title.as_str().cmp(&b.title.as_str()));
                let sorted_indices: Vec<String> = sorted_series
                    .iter()
                    .map(|s| s.id.as_str().to_string())
                    .collect();

                let cache = LibraryMediaCache::TvShows {
                    series_references: series_map,
                    series_references_sorted: sorted_series,
                    series_indices_sorted: sorted_indices,
                    season_references: season_map,
                    episode_references: episode_map,
                };
                self.domains
                    .library
                    .state
                    .library_media_cache
                    .insert(library_id, cache);
            }
        }

        // After MediaStore is updated via the batch_task, emit event to trigger metadata fetching
        // This replaces the direct ViewModel refresh that violated domain boundaries
        // [MediaStoreNotifier] ViewModels refresh no longer needed - MediaStore notifies automatically
        // tasks.push(Task::done(Message::_EmitCrossDomainEvent(
        //     crate::common::messages::CrossDomainEvent::RequestViewModelRefresh,
        // )));

        // Sorting is now handled by ViewModels

        // Grid states and carousels are managed by ViewModels now

        // Library cache was already updated above after organizing references

        // NEW: Emit event to trigger batch metadata fetching through proper channels
        // This ensures the batch fetcher's tasks are executed and BatchMetadataReady events are emitted
        if let Some(fetcher) = &self.batch_metadata_fetcher {
            // Check if batch metadata fetching is already complete to avoid re-fetching
            if !fetcher.is_complete() {
                log::info!(
                    "[BatchMetadataFetcher] Triggering batch fetch for library {} ({} items)",
                    library_id,
                    media_refs_for_queue.len()
                );

                // Create the library data tuple for batch fetching
                let library_data = vec![(library_id, media_refs_for_queue)];

                // Note: Cross-domain event emission for batch metadata fetching
                // is now handled in update.rs when LibraryMediaReferencesLoaded is processed.
                // This ensures proper separation of concerns and clean event emission.
                log::debug!("[BatchMetadataFetcher] Media references prepared for batch fetching");
            } else {
                log::info!(
                    "[BatchMetadataFetcher] Skipping batch fetch for library {} - already complete",
                    library_id
                );
            }
        } else {
            log::warn!("No batch metadata fetcher available");
        }

        // No longer needed - metadata is fetched in batches regardless of visibility

        //// Get posters to load for items that already have metadata
        //let posters_to_load = self
        //    .domains
        //    .media
        //    .state
        //    .get_posters_to_load_for_references();
        //if !posters_to_load.is_empty() {
        //    log::info!(
        //        "Found {} posters to load from existing metadata",
        //        posters_to_load.len()
        //    );
        //    for media_id in &posters_to_load {
        //        log::debug!("Creating LoadPoster task for media_id: {}", media_id);
        //    }
        //    // Images are loaded on-demand through UnifiedImageService
        //} Note: Poster loading is now handled through UnifiedImageService on-demand

        self.loading = false;

        tasks
    }

    /// Handle media details update from background fetcher
    pub fn handle_media_details_update(&mut self, media: MediaReference) -> Vec<String> {
        let posters_to_load = Vec::new();

        // Log which media is being updated
        if let Some(movie) = media.as_movie() {
            log::debug!(
                "Updating movie: {} ({})",
                movie.title.as_str(),
                movie.id.as_str()
            )
        } else if let Some(series) = media.as_series() {
            log::debug!(
                "Updating series: {} ({})",
                series.title.as_str(),
                series.id.as_str()
            )
        }

        // NEW ARCHITECTURE: Update MediaStore
        if let Ok(mut store) = self.domains.media.state.media_store.write() {
            // Note: Direct cross-domain state access - MediaStore is shared between domains
            // This is acceptable as MediaStore acts as a central repository for media data
            store.upsert(media.clone());
            log::debug!("MediaStore updated with new media reference");
        } else {
            log::error!("Failed to acquire write lock on MediaStore");
        }

        // Also update library_media_cache with the new details
        // This is important because the UI might be reading from this cache
        for (library_id, cache) in self.domains.library.state.library_media_cache.iter_mut() {
            match (cache, &media) {
                (LibraryMediaCache::Movies { references }, MediaReference::Movie(movie)) => {
                    // Find and update the movie in the cache
                    if let Some(cached_movie) = references.iter_mut().find(|m| m.id == movie.id) {
                        *cached_movie = movie.clone();
                        log::debug!(
                            "Updated movie {} in library {} cache",
                            movie.id.as_str(),
                            library_id
                        );
                    }
                }
                (
                    LibraryMediaCache::TvShows {
                        series_references,
                        series_references_sorted,
                        ..
                    },
                    MediaReference::Series(series),
                ) => {
                    // Update in the map
                    let series_id = series.id;
                    let series_id_str = series_id.as_str();

                    if series_references.contains_key(series_id.as_ref()) {
                        series_references.insert(series_id.as_uuid(), series.clone());
                        // Also update in the sorted list
                        if let Some(sorted_series) = series_references_sorted
                            .iter_mut()
                            .find(|s| s.id == series.id)
                        {
                            *sorted_series = series.clone();
                        }
                        log::debug!(
                            "Updated series {} in library {} cache",
                            series_id_str,
                            library_id
                        );
                    }
                }
                _ => {}
            }
        }

        // ViewModels will be refreshed when metadata arrives via RefreshViewModels message
        // No need to mark individual items - batch updates will trigger a full refresh
        // All media types (movies, series, seasons, episodes) will be refreshed via RefreshViewModels

        // ViewModels will be refreshed via the RefreshViewModels message after batch updates

        // Legacy code removed - poster loading is handled by ViewModels now

        posters_to_load
    }

    /// Handle a batch of media details updates efficiently
    pub fn handle_media_details_batch(&mut self, batch: Vec<MediaReference>) -> Task<Message> {
        use std::time::Instant;

        let start = Instant::now();
        log::info!("[Batch] Processing {} media updates", batch.len());

        // Process all updates without refreshing ViewModels
        {
            let mut store_write = match self.domains.media.state.media_store.try_write() {
                // Note: Direct cross-domain state access - MediaStore is shared between domains
                Ok(store) => store,
                Err(_) => {
                    log::error!("Failed to acquire write lock on MediaStore - queuing for retry");
                    // Queue the batch for retry
                    return Task::perform(
                        async move {
                            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                            batch
                        },
                        Message::MediaDetailsBatch,
                    );
                }
            };

            // Update MediaStore in batch
            for media in &batch {
                store_write.upsert(media.clone());
            }
        } // Release write lock

        // Update library caches efficiently
        let mut movies_updated = 0;
        let mut series_updated = 0;

        for media in batch {
            if let Some(movie) = media.as_movie() {
                movies_updated += 1;
                // Update in all library caches that contain this movie
                for (_, cache) in self.domains.library.state.library_media_cache.iter_mut() {
                    if let LibraryMediaCache::Movies { references } = cache {
                        if let Some(cached_movie) = references.iter_mut().find(|m| m.id == movie.id)
                        {
                            *cached_movie = movie.clone();
                        }
                    }
                }
            } else if let Some(series) = media.as_series() {
                series_updated += 1;
                // Update in all library caches that contain this series
                for (_, cache) in self.domains.library.state.library_media_cache.iter_mut() {
                    if let LibraryMediaCache::TvShows {
                        series_references,
                        series_references_sorted,
                        ..
                    } = cache
                    {
                        if series_references.contains_key(series.id.as_ref()) {
                            series_references.insert(series.id.as_uuid(), series.clone());
                            // Also update in sorted list
                            if let Some(sorted_series) = series_references_sorted
                                .iter_mut()
                                .find(|s| s.id == series.id)
                            {
                                *sorted_series = series.clone();
                            }
                        }
                    }
                }
            }
            // Skip seasons/episodes for now
        }

        log::info!(
            "[Batch] Processed {} movies and {} series in {:?}",
            movies_updated,
            series_updated,
            start.elapsed()
        );

        // Queue a single refresh with debouncing
        self.queue_viewmodel_refresh()
    }

    /// Queue a debounced refresh of ViewModels
    fn queue_viewmodel_refresh(&mut self) -> Task<Message> {
        // [MediaStoreNotifier] Refresh no longer needed - MediaStore notifies automatically
        // ViewModels will be refreshed via MediaStoreNotifier subscription
        Task::none()
    }
}
