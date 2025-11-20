use std::collections::HashMap;
use std::sync::Arc;

use crate::{
    api_types::{LibraryMediaCache, LibraryMediaResponse, LibraryType, MediaReference},
    media_library::fetch_library_media_references,
    messages::{library::Message, metadata, ui, DomainMessage},
    models::ReferenceOrganizer,
    state::State,
    view_models::ViewModel,
};
use ferrex_core::SeriesReference;
use ferrex_core::{api_types::MediaId, MovieReference};
use iced::Task;
use uuid::Uuid;

impl State {
    /// Load media references for a library using the new API
    pub fn load_library_media_references(&mut self, library_id: Uuid) -> Task<Message> {
        self.loading = true;
        self.error_message = None;

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
            "Loading library {} without clearing existing data",
            library_id
        );

        // NEW ARCHITECTURE: Begin batch update on MediaStore
        log::info!(
            "Starting to populate MediaStore with {} items from library {}",
            response.media.len(),
            library_id
        );
        if let Ok(mut store) = self.media_store.write() {
            log::info!("MediaStore before update: {} items", store.len());
            store.begin_batch();

            // Add all media references to the store
            let mut endpoint_count = 0;
            let mut details_count = 0;

            for media_ref in &response.media {
                // Debug: Check what type of details we have
                match media_ref {
                    MediaReference::Movie(movie) => match &movie.details {
                        crate::api_types::MediaDetailsOption::Endpoint(_) => endpoint_count += 1,
                        crate::api_types::MediaDetailsOption::Details(_) => details_count += 1,
                    },
                    MediaReference::Series(series) => match &series.details {
                        crate::api_types::MediaDetailsOption::Endpoint(_) => endpoint_count += 1,
                        crate::api_types::MediaDetailsOption::Details(_) => details_count += 1,
                    },
                    _ => {}
                }

                store.upsert(media_ref.clone());
            }

            log::info!(
                "MediaStore loaded with {} endpoint refs and {} detail refs",
                endpoint_count,
                details_count
            );

            // End batch - this will notify all subscribers
            store.end_batch();

            log::info!("MediaStore after update: {} total items", store.len());
        }

        // NEW ARCHITECTURE: Update ViewModels library filter and refresh
        log::info!("Setting library filter {} on ViewModels", library_id);
        self.all_view_model.set_library_filter(Some(library_id));
        self.movies_view_model.set_library_filter(Some(library_id));
        self.tv_view_model.set_library_filter(Some(library_id));

        // Debug: Check what ViewModels see
        log::info!(
            "After setting library filter - AllViewModel has {} movies, {} series",
            self.all_view_model.all_movies().len(),
            self.all_view_model.all_series().len()
        );

        // Clone media references for immediate queueing
        let media_refs_for_queue = response.media.clone();

        // Update the current library's media field
        if let Some(library_id) = &self.current_library_id {
            if let Some(library) = self.libraries.iter_mut().find(|l| &l.id == library_id) {
                // Store only MovieReference and SeriesReference in the library
                library.media = response
                    .media
                    .iter()
                    .filter_map(|media_ref| match media_ref {
                        MediaReference::Movie(_) | MediaReference::Series(_) => {
                            Some(media_ref.clone())
                        }
                        _ => None, // Skip Season and Episode references
                    })
                    .collect();
                log::info!(
                    "Updated library {} with {} media references",
                    library.name,
                    library.media.len()
                );
            }
        }

        // Legacy reference organization removed - MediaStore handles all storage now
        // The MediaStore has already been populated above with all references

        // Store in library cache for instant switching
        let media_refs = response.media.clone();
        match library_type {
            LibraryType::Movies => {
                let movie_refs: Vec<MovieReference> = media_refs
                    .into_iter()
                    .filter_map(|m| match m {
                        MediaReference::Movie(movie) => Some(movie),
                        _ => None,
                    })
                    .collect();
                let cache = LibraryMediaCache::Movies {
                    references: movie_refs,
                };
                self.library_media_cache.insert(library_id, cache);
            }
            LibraryType::TvShows => {
                // For TV shows, organize into proper structure
                let (_, tv_shows) = ReferenceOrganizer::organize_references(response.media);

                let mut series_map = HashMap::new();
                let mut season_map = HashMap::new();
                let mut episode_map = HashMap::new();

                for (series_id, (series, seasons, episodes)) in tv_shows {
                    series_map.insert(series_id.clone(), series);
                    season_map.insert(series_id.clone(), seasons);

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
                self.library_media_cache.insert(library_id, cache);
            }
        }

        // Initialize tasks list
        let mut tasks = Vec::new();

        // Sorting is now handled by ViewModels

        // Grid states and carousels are managed by ViewModels now

        // Library cache was already updated above after organizing references

        // NEW SIMPLIFIED APPROACH: Start batch metadata fetching immediately
        // No queuing, no priorities, just direct batch processing
        if let Some(batch_fetcher) = &self.batch_metadata_fetcher {
            let fetcher = Arc::clone(batch_fetcher);
            let media_refs_clone = media_refs_for_queue.clone();

            // Launch async task to process this library's metadata in batches
            tasks.push(Task::perform(
                async move {
                    fetcher.process_library(library_id, media_refs_clone).await;
                },
                |_| Message::NoOp, // No need for a callback, updates come via MediaDetailsUpdated
            ));

            log::info!(
                "[BatchMetadataFetcher] Started task for library {} ({} items)",
                library_id,
                media_refs_for_queue.len()
            );
        } else {
            log::warn!("No batch metadata fetcher available");
        }

        // No longer needed - metadata is fetched in batches regardless of visibility

        // Get posters to load for items that already have metadata
        let posters_to_load = self.get_posters_to_load_for_references();
        if !posters_to_load.is_empty() {
            log::info!(
                "Found {} posters to load from existing metadata",
                posters_to_load.len()
            );
            for media_id in &posters_to_load {
                log::debug!("Creating LoadPoster task for media_id: {}", media_id);
            }
            // Images are loaded on-demand through UnifiedImageService
        }

        self.loading = false;

        tasks
    }

    /// Convert new references to legacy MediaFile format for backward compatibility
    #[allow(dead_code)]
    pub fn convert_references_to_legacy(&mut self) {
        // TODO: Remove this method once all UI components use MediaReference types
        // Legacy fields have been removed - this method is now a no-op
    }

    /// Handle media details update from background fetcher
    pub fn handle_media_details_update(&mut self, media: MediaReference) -> Vec<String> {
        let posters_to_load = Vec::new();

        // Log which media is being updated
        match &media {
            MediaReference::Movie(m) => {
                log::debug!("Updating movie: {} ({})", m.title.as_str(), m.id.as_str())
            }
            MediaReference::Series(s) => {
                log::debug!("Updating series: {} ({})", s.title.as_str(), s.id.as_str())
            }
            _ => {}
        }

        // NEW ARCHITECTURE: Update MediaStore
        if let Ok(mut store) = self.media_store.write() {
            store.upsert(media.clone());
            log::debug!("MediaStore updated with new media reference");
        } else {
            log::error!("Failed to acquire write lock on MediaStore");
        }

        // Also update library_media_cache with the new details
        // This is important because the UI might be reading from this cache
        for (library_id, cache) in self.library_media_cache.iter_mut() {
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
                    let series_id_str = series.id.as_str();
                    if series_references.contains_key(series_id_str) {
                        series_references.insert(series_id_str.to_string(), series.clone());
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

        // NEW ARCHITECTURE: Mark affected ViewModels as needing refresh
        match &media {
            MediaReference::Movie(_) => {
                self.all_view_model.mark_needs_refresh();
                self.movies_view_model.mark_needs_refresh();
            }
            MediaReference::Series(_) => {
                self.all_view_model.mark_needs_refresh();
                self.tv_view_model.mark_needs_refresh();
            }
            _ => {
                // Season/Episode updates - refresh TV view
                self.tv_view_model.mark_needs_refresh();
            }
        }

        // NEW ARCHITECTURE: Refresh affected ViewModels
        match &media {
            MediaReference::Movie(_) => {
                log::debug!("Refreshing all_view_model and movies_view_model after movie update");
                self.all_view_model.refresh_from_store();
                self.movies_view_model.refresh_from_store();
            }
            MediaReference::Series(_) => {
                log::debug!("Refreshing all_view_model and tv_view_model after series update");
                self.all_view_model.refresh_from_store();
                self.tv_view_model.refresh_from_store();
            }
            _ => {
                // Season/Episode updates - refresh TV view
                log::debug!("Refreshing tv_view_model after season/episode update");
                self.tv_view_model.refresh_from_store();
            }
        }

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
            let mut store_write = match self.media_store.try_write() {
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
            match &media {
                MediaReference::Movie(movie) => {
                    movies_updated += 1;
                    // Update in all library caches that contain this movie
                    for (_, cache) in self.library_media_cache.iter_mut() {
                        if let LibraryMediaCache::Movies { references } = cache {
                            if let Some(cached_movie) =
                                references.iter_mut().find(|m| m.id == movie.id)
                            {
                                *cached_movie = movie.clone();
                            }
                        }
                    }
                }
                MediaReference::Series(series) => {
                    series_updated += 1;
                    // Update in all library caches that contain this series
                    for (_, cache) in self.library_media_cache.iter_mut() {
                        if let LibraryMediaCache::TvShows {
                            series_references,
                            series_references_sorted,
                            ..
                        } = cache
                        {
                            let series_id_str = series.id.as_str();
                            if series_references.contains_key(series_id_str) {
                                series_references.insert(series_id_str.to_string(), series.clone());
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
                _ => {} // Skip seasons/episodes for now
            }
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
        use iced::Task;
        use std::time::Duration;

        // Mark ViewModels as needing refresh
        self.all_view_model.mark_needs_refresh();
        self.movies_view_model.mark_needs_refresh();
        self.tv_view_model.mark_needs_refresh();

        // Return a task that will trigger refresh after a delay
        Task::perform(
            async move {
                tokio::time::sleep(Duration::from_millis(100)).await;
            },
            |_| Message::RefreshViewModels,
        )
    }
}
