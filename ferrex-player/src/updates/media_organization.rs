use iced::Task;
use std::collections::HashMap;

use crate::{
    api_types::SeriesReference,
    media_library::MediaFile,
    messages::library::Message,
    models::TvShow,
    state::{SortBy, SortOrder, State},
    view_models::ViewModel,
};

pub fn handle_media_organized(
    state: &mut State,
    _movies: Vec<MediaFile>,
    _tv_shows: HashMap<String, TvShow>,
) -> Task<Message> {
    // NEW ARCHITECTURE: This function is now a no-op
    // Media organization is handled by MediaStore and ViewModels
    // Legacy MediaFile format is no longer used

    log::info!("handle_media_organized called but legacy format no longer used");

    // Refresh ViewModels to ensure they're up to date
    state.all_view_model.refresh_from_store();
    state.movies_view_model.refresh_from_store();
    state.tv_view_model.refresh_from_store();

    Task::none()
}

pub fn handle_set_sort_by(state: &mut State, sort_by: SortBy) -> Task<Message> {
    log::info!("Setting sort by: {:?}", sort_by);
    state.sort_by = sort_by;

    // NEW ARCHITECTURE: ViewModels use state.sort_by and state.sort_order directly
    // Refresh ViewModels to apply new sort settings
    state.all_view_model.refresh_from_store();
    state.movies_view_model.refresh_from_store();
    state.tv_view_model.refresh_from_store();

    Task::none()
}

pub fn handle_toggle_sort_order(state: &mut State) -> Task<Message> {
    state.sort_order = match state.sort_order {
        SortOrder::Ascending => SortOrder::Descending,
        SortOrder::Descending => SortOrder::Ascending,
    };
    log::info!("Toggled sort order to: {:?}", state.sort_order);

    // NEW ARCHITECTURE: ViewModels use state.sort_by and state.sort_order directly
    // Refresh ViewModels to apply new sort settings
    state.all_view_model.refresh_from_store();
    state.movies_view_model.refresh_from_store();
    state.tv_view_model.refresh_from_store();

    Task::none()
}

pub fn handle_series_sorting_completed(
    state: &mut State,
    sorted_series: Vec<SeriesReference>,
) -> Task<Message> {
    log::info!(
        "Series sorting completed with {} items",
        sorted_series.len()
    );

    // Sorting now handled by ViewModels
    // Legacy code removed - series sorting is done internally by TvViewModel

    // Trigger UI update
    Task::none()
}

pub fn handle_aggregate_all_libraries(state: &mut State) -> Task<Message> {
    // NEW ARCHITECTURE: Aggregation is handled by ViewModels automatically
    // When library filter is None, ViewModels show all media from MediaStore
    log::info!(
        "handle_aggregate_all_libraries called but aggregation is automatic in new architecture"
    );

    // Ensure ViewModels are showing all libraries
    state.all_view_model.set_library_filter(None);
    state.movies_view_model.set_library_filter(None);
    state.tv_view_model.set_library_filter(None);

    // Start batch metadata fetching for all libraries if not already started
    if let Some(batch_fetcher) = &state.batch_metadata_fetcher {
        if !batch_fetcher.is_complete() {
            log::info!("[BatchMetadataFetcher] Starting batch processing for all libraries");

            // Collect all media references from all libraries
            let mut all_libraries_data: Vec<(uuid::Uuid, Vec<crate::api_types::MediaReference>)> =
                Vec::new();

            for (library_id, cache) in &state.library_media_cache {
                let mut media_refs = Vec::new();

                match cache {
                    crate::api_types::LibraryMediaCache::Movies { references } => {
                        for movie in references {
                            media_refs.push(crate::api_types::MediaReference::Movie(movie.clone()));
                        }
                    }
                    crate::api_types::LibraryMediaCache::TvShows {
                        series_references_sorted,
                        ..
                    } => {
                        for series in series_references_sorted {
                            media_refs
                                .push(crate::api_types::MediaReference::Series(series.clone()));
                        }
                    }
                }

                if !media_refs.is_empty() {
                    all_libraries_data.push((*library_id, media_refs));
                }
            }

            if !all_libraries_data.is_empty() {
                let fetcher = std::sync::Arc::clone(batch_fetcher);
                return Task::perform(
                    async move {
                        fetcher.process_libraries(all_libraries_data).await;
                    },
                    |_| Message::BatchMetadataComplete,
                );
            }
        }
    }

    state.loading = false;

    // No longer need to queue visible details - BatchMetadataFetcher handles all items
    Task::none()
}
