use iced::{Size, Task};

use crate::domains::metadata::demand_planner::DemandSnapshot;
use crate::infra::api_types::LibraryType;
use crate::{domains::ui::messages::Message, state::State};
use ferrex_core::player_prelude::PosterKind;

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn handle_window_resized(state: &mut State, size: Size) -> Task<Message> {
    log::debug!("Window resized to: {}x{}", size.width, size.height);

    // Grid state handling moved to ViewModels

    state.window_size = size;

    // Update all tab grids with new window width
    // This only updates column count - the scrollable widget will report actual viewport dimensions
    for tab_id in state.tab_manager.tab_ids() {
        if let Some(tab) = state.tab_manager.get_tab_mut(tab_id)
            && let Some(grid_state) = tab.grid_state_mut()
        {
            // Use resize() which only updates columns based on width
            // The scrollable widget will report actual viewport dimensions via TabGridScrolled
            grid_state.resize(size.width);
        }
    }

    // TODO: This is cumbersome, fix it
    let uuid = state
        .domains
        .library
        .state
        .current_library_id
        .map(|library_id| library_id.to_uuid());

    // Update depth regions for the current view with new window size
    state
        .domains
        .ui
        .state
        .background_shader_state
        .update_depth_lines(
            &state.domains.ui.state.view,
            size.width,
            size.height,
            uuid,
        );

    // Emit snapshot for active library tab after columns update.
    if let Some(handle) = state.domains.metadata.state.planner_handle.as_ref() {
        if let crate::domains::ui::tabs::TabState::Library(lib_state) =
            state.tab_manager.active_tab()
        {
            let now = std::time::Instant::now();
            let mut visible_ids: Vec<uuid::Uuid> = Vec::new();
            let vr = lib_state.grid_state.visible_range.clone();
            if let Some(slice) = lib_state.cached_index_ids.get(vr) {
                visible_ids.extend(slice.iter().copied());
            }

            let pr = lib_state
                .grid_state
                .get_preload_range(crate::infra::constants::layout::virtual_grid::PREFETCH_ROWS_ABOVE);
            let mut prefetch_ids: Vec<uuid::Uuid> = Vec::new();
            if let Some(slice) = lib_state.cached_index_ids.get(pr) {
                prefetch_ids.extend(slice.iter().copied());
            }

            prefetch_ids.retain(|id| !visible_ids.contains(id));
            let br = lib_state.grid_state.get_background_range(
                crate::infra::constants::layout::virtual_grid::PREFETCH_ROWS_ABOVE,
                crate::infra::constants::layout::virtual_grid::BACKGROUND_ROWS_BELOW,
            );
            let mut background_ids: Vec<uuid::Uuid> = Vec::new();
            if let Some(slice) = lib_state.cached_index_ids.get(br) {
                background_ids.extend(slice.iter().copied());
            }
            background_ids.retain(|id| {
                !visible_ids.contains(id) && !prefetch_ids.contains(id)
            });

            let poster_kind = match lib_state.library_type {
                LibraryType::Movies => Some(PosterKind::Movie),
                LibraryType::Series => Some(PosterKind::Series),
            };

            let snapshot = DemandSnapshot {
                visible_ids,
                prefetch_ids,
                background_ids,
                timestamp: now,
                context: None,
                poster_kind,
            };
            handle.send(snapshot);
        }
    }

    Task::none()
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn handle_window_moved(
    state: &mut State,
    position: Option<iced::Point>,
) -> Task<Message> {
    // Store the window position for later use (e.g., when spawning MPV)
    if let Some(position) = position {
        log::info!("Window moved to: ({}, {})", position.x, position.y);
        state.window_position = Some(position);
    }

    Task::none()
}
