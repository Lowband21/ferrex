use crate::{
    domains::library::messages::Message, domains::ui::types::ViewState,
    domains::ui::view_models::ViewModel, infrastructure::api_types::LibraryMediaCache,
    infrastructure::api_types::MediaReference, state_refactored::State,
};
use iced::Task;

/// Handle media discovered from server events
#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn handle_media_discovered(
    state: &mut State,
    references: Vec<MediaReference>,
) -> Task<Message> {
    // NEW ARCHITECTURE: Add to MediaStore
    if let Ok(mut store) = state.domains.media.state.media_store.write() {
        for reference in &references {
            store.upsert(reference.clone());

            match reference {
                MediaReference::Movie(movie) => {
                    log::info!("Movie discovered: {}", movie.title.as_str());
                }
                MediaReference::Series(series) => {
                    log::info!("Series discovered: {}", series.title.as_str());
                }
                MediaReference::Season(season) => {
                    log::info!("Season discovered: S{}", season.season_number.value());
                }
                MediaReference::Episode(episode) => {
                    log::info!(
                        "Episode discovered: S{}E{}",
                        episode.season_number.value(),
                        episode.episode_number.value()
                    );
                }
            }
        }
    }

    // Add to current library's references if it matches
    if let Some(library_id) = &state.domains.library.state.current_library_id {
        if let Some(library) = state
            .domains
            .library
            .state
            .libraries
            .iter_mut()
            .find(|l| &l.id == library_id)
        {
            for reference in references {
                // Only add movies and series to library media list
                match reference {
                    MediaReference::Movie(_) | MediaReference::Series(_) => {
                        if let Some(media_vec) = &mut library.media {
                            media_vec.push(reference);
                        } else {
                            library.media = Some(vec![reference]);
                        }
                    }
                    _ => {} // Seasons and episodes are not stored at library level
                }
            }
        }
    }

    // NEW ARCHITECTURE: ViewModels notified via MediaStoreNotifier

    Task::none()
}

/// Handle media updated from server events
#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn handle_media_updated(state: &mut State, reference: MediaReference) -> Task<Message> {
    match &reference {
        MediaReference::Movie(movie) => {
            log::info!("Movie updated: {}", movie.title.as_str());
        }
        MediaReference::Series(series) => {
            log::info!("Series updated: {}", series.title.as_str());
        }
        MediaReference::Season(season) => {
            log::info!("Season updated: S{}", season.season_number.value());
        }
        MediaReference::Episode(episode) => {
            log::info!(
                "Episode updated: S{}E{}",
                episode.season_number.value(),
                episode.episode_number.value()
            );
        }
    }

    // NEW ARCHITECTURE: Update in MediaStore
    if let Ok(mut store) = state.domains.media.state.media_store.write() {
        store.upsert(reference.clone());
    }

    // Update in library cache if it exists
    if let Some(library_id) = &state.domains.library.state.current_library_id {
        if let Some(library) = state
            .domains
            .library
            .state
            .libraries
            .iter_mut()
            .find(|l| &l.id == library_id)
        {
            // Find and update the reference
            if let Some(media_vec) = &mut library.media {
                for media in media_vec.iter_mut() {
                    let should_update = match (&reference, &*media) {
                        (MediaReference::Movie(new), MediaReference::Movie(old)) => {
                            new.id == old.id
                        }
                        (MediaReference::Series(new), MediaReference::Series(old)) => {
                            new.id == old.id
                        }
                        _ => false,
                    };
                    if should_update {
                        *media = reference.clone();
                        break;
                    }
                }
            }
        }
    }

    // Also update in library_media_cache
    for (_, cache) in state.domains.library.state.library_media_cache.iter_mut() {
        match (cache, &reference) {
            (LibraryMediaCache::Movies { references }, MediaReference::Movie(movie)) => {
                if let Some(cached_movie) = references.iter_mut().find(|m| m.id == movie.id) {
                    *cached_movie = movie.clone();
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
            _ => {}
        }
    }

    // NEW ARCHITECTURE: Refresh affected ViewModels
    match &reference {
        MediaReference::Movie(_) => {
            // Direct refresh removed - ViewModels notified via MediaStoreNotifier
            // Direct refresh removed - ViewModels notified via MediaStoreNotifier
        }
        MediaReference::Series(_) | MediaReference::Season(_) | MediaReference::Episode(_) => {
            // Direct refresh removed - ViewModels notified via MediaStoreNotifier
            // Direct refresh removed - ViewModels notified via MediaStoreNotifier
        }
    }

    // No need to update current_show_details - MediaStore is the single source of truth

    Task::none()
}

/// Handle media deleted from server events
#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn handle_media_deleted(state: &mut State, file_id: String) -> Task<Message> {
    log::info!("Media file deleted: {}", file_id);

    // NEW ARCHITECTURE: Find and remove from MediaStore by file ID
    let media_ids_to_remove = if let Ok(store) = state.domains.media.state.media_store.read() {
        store.find_by_file_id(&file_id)
    } else {
        Vec::new()
    };

    // Remove the found media items
    if !media_ids_to_remove.is_empty() {
        if let Ok(mut store) = state.domains.media.state.media_store.write() {
            for media_id in media_ids_to_remove {
                log::debug!("Removing media with file_id {}: {:?}", file_id, media_id);
                store.remove(&media_id);
            }
        }
    }

    // Remove from current library's references
    if let Some(library_id) = &state.domains.library.state.current_library_id {
        if let Some(library) = state
            .domains
            .library
            .state
            .libraries
            .iter_mut()
            .find(|l| &l.id == library_id)
        {
            if let Some(media_vec) = &mut library.media {
                media_vec.retain(|media| match media {
                    MediaReference::Movie(m) => m.file.id.to_string() != file_id,
                    MediaReference::Episode(e) => e.file.id.to_string() != file_id,
                    _ => true, // Series and seasons don't have file IDs
                });
            }
        }
    }

    // Remove from library_media_cache
    for (_, cache) in state.domains.library.state.library_media_cache.iter_mut() {
        match cache {
            LibraryMediaCache::Movies { references } => {
                references.retain(|m| m.file.id.to_string() != file_id);
            }
            LibraryMediaCache::TvShows {
                episode_references, ..
            } => {
                // Remove episodes with this file ID from all seasons
                for (_, episodes) in episode_references.iter_mut() {
                    episodes.retain(|e| e.file.id.to_string() != file_id);
                }
            }
        }
    }

    // Clear detail view if it matches the deleted file
    match &state.domains.ui.state.view {
        ViewState::MovieDetail { movie, .. } => {
            if movie.file.id.to_string() == file_id {
                state.domains.ui.state.view = ViewState::Library;
            }
        }
        ViewState::EpisodeDetail { episode_id, .. } => {
            // Check if the episode with this ID has the matching file
            if let Ok(store) = state.domains.media.state.media_store.read() {
                if let Some(MediaReference::Episode(episode)) = store.get(
                    &ferrex_core::api_types::MediaId::Episode(episode_id.clone()),
                ) {
                    if episode.file.id.to_string() == file_id {
                        // Go back to TV show detail
                        state.domains.ui.state.view = ViewState::TvShowDetail {
                            series_id: episode.series_id.clone(),
                            backdrop_handle: None,
                        };
                    }
                }
            }
        }
        _ => {}
    }

    // NEW ARCHITECTURE: ViewModels notified via MediaStoreNotifier

    Task::none()
}
