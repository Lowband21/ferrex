use std::time::{Duration, Instant};

use crate::domains::metadata::demand_planner::DemandSnapshot;
use crate::infra::api_types::LibraryType;
use crate::{
    domains::ui::{messages::UiMessage, tabs::TabState},
    infra::constants::performance_config::scrolling::SCROLL_STOP_DEBOUNCE_MS,
    state::State,
};
use ferrex_core::player_prelude::PosterKind;
use ferrex_core::player_prelude::{MediaID, MediaIDLike};
use iced::{Task, widget::scrollable::Viewport};

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn handle_detail_view_scrolled(
    state: &mut State,
    viewport: Viewport,
) -> Task<UiMessage> {
    // Update scroll offset for fixed backdrop
    let scroll_offset = viewport.absolute_offset().y;
    log::debug!(
        "DetailViewScrolled: Updating background shader scroll offset to {}",
        scroll_offset
    );
    state.domains.ui.state.background_shader_state.scroll_offset =
        scroll_offset;

    // TODO: This is cumbersome, fix it
    let uuid = state
        .domains
        .library
        .state
        .current_library_id
        .map(|library_id| library_id.to_uuid());

    // Update depth lines to move with the scrolled content
    state
        .domains
        .ui
        .state
        .background_shader_state
        .update_depth_lines(
            &state.domains.ui.state.view,
            state.window_size.width,
            state.window_size.height,
            uuid,
        );

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
pub fn handle_tab_grid_scrolled(
    state: &mut State,
    viewport: Viewport,
) -> Task<UiMessage> {
    // Get the active tab and update its scroll state
    let active_tab_id = state.tab_manager.active_tab_id();

    // Update scroll position in the active tab's grid state (for virtual scrolling)
    if let Some(tab) = state.tab_manager.get_tab_mut(active_tab_id) {
        match tab {
            TabState::Library(lib_state) => {
                // Update the grid state for virtual scrolling calculations
                lib_state.update_scroll(viewport);
            }
            TabState::All(_all_state) => {
                // All tab uses carousel, not virtual grid - no grid state to update
            }
        }
    }

    // Save scroll position to ScrollPositionManager for persistence
    let scroll_state =
        crate::domains::ui::scroll_manager::ScrollState::from_viewport(
            viewport,
        );
    state
        .domains
        .ui
        .state
        .scroll_manager
        .save_tab_scroll(&active_tab_id, scroll_state);

    log::trace!(
        "Tab {:?} scroll saved to manager at position {}",
        active_tab_id,
        viewport.absolute_offset().y
    );

    // Track timing for planner snapshots and yoke prefetch throttling
    let now = Instant::now();

    // Emit a planner snapshot for the active library tab (visible + prefetch)
    if let Some(handle) = state.domains.metadata.state.planner_handle.as_ref() {
        if let crate::domains::ui::tabs::TabState::Library(lib_state) =
            state.tab_manager.active_tab()
        {
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

            // Deduplicate
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

    // Rate-limit yoke prefetch work to avoid hammering the repo accessor
    let should_prefetch = state
        .domains
        .ui
        .state
        .last_prefetch_tick
        .map(|last| {
            last.elapsed() >= Duration::from_millis(SCROLL_STOP_DEBOUNCE_MS / 2)
        })
        .unwrap_or(true);

    if should_prefetch {
        state.domains.ui.state.last_prefetch_tick = Some(now);

        // PoC yoke prefetch: limit frequency by the same gating used for debounced task creation
        // Prefetch only currently visible items to keep changes minimal
        let visible_items = state.tab_manager.get_active_tab_visible_items();
        let mut prefetched = 0usize;
        for archived_id in visible_items.iter() {
            // Deserialize archived ID to runtime MediaID
            if let Ok(media_id) =
                rkyv::deserialize::<MediaID, rkyv::rancor::Error>(archived_id)
            {
                match media_id {
                    MediaID::Movie(mid) => {
                        let uuid = mid.to_uuid();
                        // Skip if already cached
                        if state
                            .domains
                            .ui
                            .state
                            .movie_yoke_cache
                            .contains_key(&uuid)
                        {
                            continue;
                        }
                        // Fetch and insert into cache
                        if let Ok(yoke) = state
                            .domains
                            .ui
                            .state
                            .repo_accessor
                            .get_movie_yoke(&MediaID::Movie(mid))
                        {
                            state
                                .domains
                                .ui
                                .state
                                .movie_yoke_cache
                                .insert(uuid, std::sync::Arc::new(yoke));
                            prefetched += 1;
                            if prefetched >= 64 {
                                // cap per cycle
                                break;
                            }
                        }
                    }
                    MediaID::Series(mid) => {
                        let uuid = mid.to_uuid();
                        if state
                            .domains
                            .ui
                            .state
                            .series_yoke_cache
                            .contains_key(&uuid)
                        {
                            continue;
                        }
                        if let Ok(yoke) = state
                            .domains
                            .ui
                            .state
                            .repo_accessor
                            .get_series_yoke(&MediaID::Series(mid))
                        {
                            state
                                .domains
                                .ui
                                .state
                                .series_yoke_cache
                                .insert(uuid, std::sync::Arc::new(yoke));
                            prefetched += 1;
                            if prefetched >= 64 {
                                break;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        Task::none()
    } else {
        // Too soon since last prefetch pass, skip to avoid redundant work
        Task::none()
    }
}
