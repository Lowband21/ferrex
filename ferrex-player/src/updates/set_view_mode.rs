use ferrex_core::LibraryType;
use iced::{widget::scrollable, Task};

use crate::{
    messages::ui::Message,
    state::{State, ViewMode},
};

pub fn handle_set_view_mode(state: &mut State, mode: ViewMode) -> Task<Message> {
    log::info!("Setting view mode to: {:?}", mode);
    // NEW ARCHITECTURE: Use ViewModels to check data
    log::info!(
        "Current state: {} movies, {} TV shows, {} libraries",
        state.movies_view_model.all_movies().len(),
        state.tv_view_model.all_series().len(),
        state.libraries.len()
    );

    // Save current scroll position for the old view mode
    state.save_scroll_position();

    state.view_mode = mode;

    let mut tasks = Vec::new();

    // Clear data and fetch fresh data for the new view mode to ensure proper separation
    match mode {
        ViewMode::Movies => {
            // NEW ARCHITECTURE: Check if we have movie data in ViewModel
            let has_movie_data = !state.movies_view_model.all_movies().is_empty();

            if !has_movie_data
                && !state.loading
                && state
                    .libraries
                    .iter()
                    .any(|lib| lib.library_type == LibraryType::Movies && lib.enabled)
            {
                log::info!("No movies loaded, loading from first movie library");

                // Only set current_library_id if we don't have one already
                // This prevents changing libraries just by switching view modes
                if state.current_library_id.is_none() {
                    if let Some(movie_library) = state
                        .libraries
                        .iter()
                        .find(|lib| lib.library_type == LibraryType::Movies && lib.enabled)
                    {
                        state.current_library_id = Some(movie_library.id.clone());
                        let library_task = state.load_library_media_references(movie_library.id);
                        // Convert library task to UI message (discard result)
                        tasks.push(library_task.map(|_| Message::NoOp));
                    }
                } else {
                    // We have a current library but no data, load it
                    if let Some(library_id) = state.current_library_id.clone() {
                        let library_task = state.load_library_media_references(library_id);
                        // Convert library task to UI message (discard result)
                        tasks.push(library_task.map(|_| Message::NoOp));
                    }
                }
            } else if has_movie_data {
                log::info!(
                    "Using cached movie data with {} movies",
                    state.movies_view_model.all_movies().len()
                );
                // Don't change current_library_id - keep whatever was selected
            }

            // Restore scroll position
            if let Some(position) = state.movies_scroll_position {
                log::debug!("Restoring movies scroll position: {}", position);
                let scrollable_id = state.movies_view_model.grid_state().scrollable_id.clone();
                tasks.push(scrollable::scroll_to(
                    scrollable_id,
                    scrollable::AbsoluteOffset {
                        x: 0.0,
                        y: position,
                    },
                ));
            }
        }
        ViewMode::TvShows => {
            // NEW ARCHITECTURE: Check if we have TV show data in ViewModel
            let has_tv_data = !state.tv_view_model.all_series().is_empty();

            if !has_tv_data
                && !state.loading
                && state
                    .libraries
                    .iter()
                    .any(|lib| lib.library_type == LibraryType::TvShows && lib.enabled)
            {
                log::info!("No TV shows loaded, loading from first TV library");

                // Only set current_library_id if we don't have one already
                // This prevents changing libraries just by switching view modes
                if state.current_library_id.is_none() {
                    if let Some(tv_library) = state
                        .libraries
                        .iter()
                        .find(|lib| lib.library_type == LibraryType::TvShows && lib.enabled)
                    {
                        state.current_library_id = Some(tv_library.id.clone());
                        let library_task = state.load_library_media_references(tv_library.id);
                        // Convert library task to UI message (discard result)
                        tasks.push(library_task.map(|_| Message::NoOp));
                    }
                } else {
                    // We have a current library but no data, load it
                    if let Some(library_id) = state.current_library_id.clone() {
                        let library_task = state.load_library_media_references(library_id);
                        // Convert library task to UI message (discard result)
                        tasks.push(library_task.map(|_| Message::NoOp));
                    }
                }
            } else if has_tv_data {
                log::info!(
                    "Using cached TV show data with {} series",
                    state.tv_view_model.all_series().len()
                );
                // Don't change current_library_id - keep whatever was selected
            }

            // Restore scroll position
            if let Some(position) = state.tv_shows_scroll_position {
                log::debug!("Restoring TV shows scroll position: {}", position);
                let scrollable_id = state.tv_view_model.grid_state().scrollable_id.clone();
                tasks.push(scrollable::scroll_to(
                    scrollable_id,
                    scrollable::AbsoluteOffset {
                        x: 0.0,
                        y: position,
                    },
                ));
            }
        }
        ViewMode::All => {
            // NEW ARCHITECTURE: For All view, ensure ViewModels show all libraries
            state.all_view_model.set_library_filter(None);
            state.movies_view_model.set_library_filter(None);
            state.tv_view_model.set_library_filter(None);

            // Aggregate from all libraries if needed
            tasks.push(Task::perform(async {}, |_| Message::AggregateAllLibraries));

            // In All mode, restore movies scroll position
            if let Some(position) = state.movies_scroll_position {
                // Get scrollable ID from AllViewModel's internal state
                // For now, we'll skip scroll restoration as AllViewModel doesn't expose grid state
                log::debug!("Scroll restoration for All view not implemented yet");
            }
        }
    }

    // Mark visible items for loading in the new view
    state.mark_visible_posters_for_loading();

    // Metadata queueing no longer needed - batch fetching happens automatically
    // when libraries are loaded

    if tasks.is_empty() {
        Task::none()
    } else {
        Task::batch(tasks)
    }
}
