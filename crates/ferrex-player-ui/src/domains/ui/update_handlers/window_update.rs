use iced::{Size, Task};

use crate::domains::metadata::demand_planner::DemandSnapshot;
use crate::domains::ui::shell_ui::Scope;
use crate::domains::ui::views::detail::solve_detail_layout_from_runtime;
use crate::{domains::ui::messages::UiMessage, state::State};

fn detail_rail_viewport_width(state: &State, fallback: f32) -> f32 {
    let plan = solve_detail_layout_from_runtime(
        state.window_size.width,
        state.window_size.height,
        state.domains.ui.state.view.header_height().unwrap_or(0.0),
        state.interface_mode,
        &state.domains.ui.state.size_provider,
        &state.domains.ui.state.scaled_layout,
    );
    let stage_width = plan
        .foreground_stage(0.0)
        .stage
        .stage_width
        .min(plan.viewport_width);
    if stage_width > 0.0 {
        stage_width.max(1.0)
    } else {
        fallback.max(1.0)
    }
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn handle_window_resized(state: &mut State, size: Size) -> Task<UiMessage> {
    // log::trace!("Window resized to: {}x{}", size.width, size.height);

    // Grid state handling moved to ViewModels

    state.window_size = size;

    // Update all tab grids with new window width using current scale
    // This only updates column count - the scrollable widget will report actual viewport dimensions
    let scaled_layout = &state.domains.ui.state.scaled_layout;
    for tab_id in state.tab_manager.tab_ids() {
        if let Some(tab) = state.tab_manager.get_tab_mut(tab_id)
            && let Some(grid_state) = tab.grid_state_mut()
        {
            // Use resize_with_scale() to ensure columns are calculated with current scale
            // The scrollable widget will report actual viewport dimensions via TabGridScrolled
            grid_state.resize_with_scale(size.width, scaled_layout);
        }
    }

    // TODO: This is cumbersome, fix it
    let uuid = state
        .domains
        .ui
        .state
        .scope
        .lib_id()
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
    if let Some(handle) = state.domains.metadata.state.planner_handle.as_ref()
        && let crate::domains::ui::tabs::TabState::Library(lib_state) =
            state.tab_manager.active_tab()
    {
        let poster_size = state.domains.settings.display.library_poster_quality;
        let now = std::time::Instant::now();
        let mut visible_ids: Vec<uuid::Uuid> = Vec::new();
        let vr = lib_state.grid_state.visible_range.clone();
        if let Some(slice) = lib_state.cached_index_ids.get(vr) {
            visible_ids.extend(slice.iter().copied().filter_map(|id| {
                crate::domains::ui::utils::primary_poster_iid_for_library_media(
                    state,
                    lib_state.library_type,
                    id,
                )
            }));
        }

        let prefetch_rows = state.runtime_config.prefetch_rows_above();
        let pr = lib_state.grid_state.get_preload_range(prefetch_rows);
        let mut prefetch_ids: Vec<uuid::Uuid> = Vec::new();
        if let Some(slice) = lib_state.cached_index_ids.get(pr) {
            prefetch_ids.extend(slice.iter().copied().filter_map(|id| {
                crate::domains::ui::utils::primary_poster_iid_for_library_media(
                    state,
                    lib_state.library_type,
                    id,
                )
            }));
        }

        prefetch_ids.retain(|id| !visible_ids.contains(id));
        let br = lib_state.grid_state.get_background_range(
                prefetch_rows,
                crate::infra::constants::layout::virtual_grid::BACKGROUND_ROWS_BELOW,
            );
        let mut background_ids: Vec<uuid::Uuid> = Vec::new();
        if let Some(slice) = lib_state.cached_index_ids.get(br) {
            background_ids.extend(slice.iter().copied().filter_map(|id| {
                crate::domains::ui::utils::primary_poster_iid_for_library_media(
                    state,
                    lib_state.library_type,
                    id,
                )
            }));
        }
        background_ids.retain(|id| {
            !visible_ids.contains(id) && !prefetch_ids.contains(id)
        });

        let snapshot = DemandSnapshot {
            visible_ids,
            prefetch_ids,
            background_ids,
            timestamp: now,
            context: None,
            poster_size,
        };
        handle.send(snapshot);
    }

    // Update virtual carousels with new width. Detail rails use the solved
    // foreground-stage width until Iced reports the exact scroll viewport.
    let carousel_keys = state.domains.ui.state.carousel_registry.keys();
    let detail_width = detail_rail_viewport_width(state, size.width.max(1.0));
    {
        let reg = &mut state.domains.ui.state.carousel_registry;
        for key in &carousel_keys {
            let viewport_width = if key.is_detail_rail() {
                detail_width
            } else {
                size.width.max(1.0)
            };
            if let Some(vc) = reg.get_mut(key) {
                vc.update_dimensions(viewport_width);
            }
        }
    }

    // After resizing, re-emit snapshots for carousels to refresh visible/prefetch windows.
    // Covers All-tab (curated + per-library) and active detail carousels.
    // All view (Curated): re-emit combined snapshots so posters stay up to date after width change
    if matches!(state.domains.ui.state.scope, Scope::Home)
        && matches!(
            state.tab_manager.active_tab_id(),
            crate::domains::ui::tabs::TabId::Home
        )
    {
        super::home_tab::emit_initial_all_tab_snapshots_combined(state);
    }

    for key in carousel_keys {
        if key.is_detail_rail() {
            super::virtual_carousel_updates::maybe_send_snapshot_for_key(
                state, &key, true,
            );
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
) -> Task<UiMessage> {
    // Store the window position for later use (e.g., when spawning MPV)
    if let Some(position) = position {
        log::info!("Window moved to: ({}, {})", position.x, position.y);
        state.window_position = Some(position);
    }

    Task::none()
}
