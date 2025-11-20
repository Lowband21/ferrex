use crate::{
    messages::library::Message,
    state::{State, ViewMode},
};
use ferrex_core::{LibraryMediaCache, MediaReference};
use iced::Task;
use uuid::Uuid;

pub fn handle_select_library(state: &mut State, library_id: Option<Uuid>) -> Task<Message> {
    //let server_url = state.server_url.clone();

    if library_id.is_none() {
        // Special case: show all media files from all libraries
        state.current_library_id = None;
        // Switch to All view mode to show carousel
        state.view_mode = ViewMode::All;

        // Check if we have any cached libraries
        let has_cached_libraries = !state.library_media_cache.is_empty();

        if has_cached_libraries {
            log::info!(
                "Aggregating media from {} cached libraries",
                state.library_media_cache.len()
            );

            // NEW ARCHITECTURE: Clear and repopulate MediaStore
            if let Ok(mut store) = state.media_store.write() {
                store.begin_batch();
                store.clear(); // Clear all data

                // Aggregate from all cached libraries
                for (lib_id, cache) in &state.library_media_cache {
                    match cache {
                        LibraryMediaCache::Movies { references } => {
                            log::info!(
                                "Adding {} movies from library {} to MediaStore",
                                references.len(),
                                lib_id
                            );
                            for movie in references {
                                store.upsert(MediaReference::Movie(movie.clone()));
                            }
                        }
                        LibraryMediaCache::TvShows {
                            series_references,
                            season_references,
                            episode_references,
                            ..
                        } => {
                            log::info!(
                                "Adding {} series from library {} to MediaStore",
                                series_references.len(),
                                lib_id
                            );
                            for series in series_references.values() {
                                store.upsert(MediaReference::Series(series.clone()));
                            }
                            for seasons in season_references.values() {
                                for season in seasons {
                                    store.upsert(MediaReference::Season(season.clone()));
                                }
                            }
                            for episodes in episode_references.values() {
                                for episode in episodes {
                                    store.upsert(MediaReference::Episode(episode.clone()));
                                }
                            }
                        }
                    }
                }

                store.end_batch();
            }

            // Update ViewModels to show all (no library filter)
            state.all_view_model.set_library_filter(None);
            state.movies_view_model.set_library_filter(None);
            state.tv_view_model.set_library_filter(None);

            // Legacy view data clearing removed - using MediaStore now

            // Legacy aggregation removed - MediaStore handles this

            // Re-sort TV shows after aggregation - handled by ViewModels
            state.update_sorted_series_references();

            // Grid and carousel counts now handled by ViewModels

            state.loading = false;
            Task::none()
        } else {
            log::info!("No cached media found for 'All' libraries, loading from server");
            // For "All" view, we need to clear everything since we're loading from all libraries
            // Legacy clearing removed - MediaStore handles this

            state.loading = true;

            // Load media references from all enabled libraries
            let mut tasks = Vec::new();

            let ids: Vec<Uuid> = state
                .libraries
                .iter()
                .filter(|lib| lib.enabled)
                .map(|lib| lib.id)
                .collect();

            for id in ids {
                tasks.push(state.load_library_media_references(id));
            }

            if tasks.is_empty() {
                state.loading = false;
                Task::none()
            } else {
                Task::batch(tasks)
            }
        }
    } else {
        let library_id = library_id.unwrap();
        log::info!("Selecting library: {}", library_id);
        // Select specific library
        state.current_library_id = Some(library_id.clone());

        // Check library type and set appropriate view mode
        if let Some(library) = state.libraries.iter().find(|l| l.id == library_id) {
            // Set view mode based on library type
            use crate::api_types::LibraryType;
            match library.library_type {
                LibraryType::Movies => state.view_mode = ViewMode::Movies,
                LibraryType::TvShows => state.view_mode = ViewMode::TvShows,
            }

            // Check if we have this specific library cached
            if let Some(cache) = state.library_media_cache.get(&library_id) {
                log::info!(
                    "Found cached data for library {}, loading from cache",
                    library_id
                );

                // NEW ARCHITECTURE: Update MediaStore with cached data
                if let Ok(mut store) = state.media_store.write() {
                    store.begin_batch();
                    store.clear_library(library_id); // Clear existing data for this library

                    match cache {
                        LibraryMediaCache::Movies { references } => {
                            log::info!(
                                "Loading {} movies from cache into MediaStore",
                                references.len()
                            );
                            for movie in references {
                                store.upsert(MediaReference::Movie(movie.clone()));
                            }
                        }
                        LibraryMediaCache::TvShows {
                            series_references,
                            season_references,
                            episode_references,
                            ..
                        } => {
                            log::info!(
                                "Loading {} series from cache into MediaStore",
                                series_references.len()
                            );
                            // Add series
                            for series in series_references.values() {
                                store.upsert(MediaReference::Series(series.clone()));
                            }
                            // Add seasons
                            for seasons in season_references.values() {
                                for season in seasons {
                                    store.upsert(MediaReference::Season(season.clone()));
                                }
                            }
                            // Add episodes
                            for episodes in episode_references.values() {
                                for episode in episodes {
                                    store.upsert(MediaReference::Episode(episode.clone()));
                                }
                            }
                        }
                    }

                    store.end_batch();
                }

                // Update ViewModels library filter
                state.all_view_model.set_library_filter(Some(library_id));
                state.movies_view_model.set_library_filter(Some(library_id));
                state.tv_view_model.set_library_filter(Some(library_id));

                // Legacy copying removed - MediaStore handles all data

                state.loading = false;
                Task::none()
            } else {
                log::info!(
                    "No cached media found for library {}, loading from server",
                    library_id
                );
                // No cached data, need to fetch from server
                state.loading = true;

                // Legacy data clearing removed - MediaStore handles this

                // Use the new reference-based API
                state.load_library_media_references(library.id)
            }
        } else {
            // Library not found, just return
            log::error!("Library {} not found", library_id);
            Task::none()
        }
    }
}

// Legacy handle_library_selected removed - using reference-based API now
