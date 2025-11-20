use iced::Task;
use std::collections::HashMap;

use crate::{
    domains::{
        media::{library::MediaFile, messages::Message, models::TvShow},
        ui::{
            types::{SortBy, SortOrder},
            view_models::ViewModel,
        },
    },
    infrastructure::api_types::SeriesReference,
    state_refactored::State,
};

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
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
    // Direct refresh removed - ViewModels notified via MediaStoreNotifier
    // Direct refresh removed - ViewModels notified via MediaStoreNotifier
    // Direct refresh removed - ViewModels notified via MediaStoreNotifier

    Task::none()
}

pub fn handle_set_sort_by(state: &mut State, sort_by: SortBy) -> Task<Message> {
    log::info!("Setting sort by: {:?}", sort_by);
    state.domains.ui.state.sort_by = sort_by;

    // NEW ARCHITECTURE: ViewModels use state.sort_by and state.sort_order directly
    // Refresh ViewModels to apply new sort settings
    // Direct refresh removed - ViewModels notified via MediaStoreNotifier
    // Direct refresh removed - ViewModels notified via MediaStoreNotifier
    // Direct refresh removed - ViewModels notified via MediaStoreNotifier

    Task::none()
}

pub fn handle_toggle_sort_order(state: &mut State) -> Task<Message> {
    state.domains.ui.state.sort_order = match state.domains.ui.state.sort_order {
        SortOrder::Ascending => SortOrder::Descending,
        SortOrder::Descending => SortOrder::Ascending,
    };
    log::info!(
        "Toggled sort order to: {:?}",
        state.domains.ui.state.sort_order
    );

    // NEW ARCHITECTURE: ViewModels use state.sort_by and state.sort_order directly
    // Refresh ViewModels to apply new sort settings
    // Direct refresh removed - ViewModels notified via MediaStoreNotifier
    // Direct refresh removed - ViewModels notified via MediaStoreNotifier
    // Direct refresh removed - ViewModels notified via MediaStoreNotifier

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

// NOTE: handle_aggregate_all_libraries has been moved to the library domain
// at src/domains/library/update_handlers/aggregate_libraries.rs
// since it handles a library domain message (AggregateAllLibraries)
