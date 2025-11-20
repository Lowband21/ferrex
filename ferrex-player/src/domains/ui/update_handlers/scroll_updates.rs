use std::time::{Duration, Instant};

use crate::{
    domains::ui::{messages::Message, scroll_manager::ScrollStateExt, tabs::TabState},
    infrastructure::constants::performance_config::scrolling::SCROLL_STOP_DEBOUNCE_MS,
    state_refactored::State,
};
use ferrex_core::{MediaID, MediaIDLike};
use iced::{widget::scrollable::Viewport, Task};

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn handle_detail_view_scrolled(state: &mut State, viewport: Viewport) -> Task<Message> {
    // Update scroll offset for fixed backdrop
    let scroll_offset = viewport.absolute_offset().y;
    log::debug!(
        "DetailViewScrolled: Updating background shader scroll offset to {}",
        scroll_offset
    );
    state.domains.ui.state.background_shader_state.scroll_offset = scroll_offset;

    // TODO: This is cumbersome, fix it
    let uuid = if let Some(library_id) = state.domains.library.state.current_library_id {
        Some(library_id.as_uuid())
    } else {
        None
    };

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
pub fn handle_tab_grid_scrolled(state: &mut State, viewport: Viewport) -> Task<Message> {
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
    let scroll_state = crate::domains::ui::scroll_manager::ScrollState::from_viewport(viewport);
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

    // Track scroll position and timing
    let current_position = viewport.absolute_offset().y;
    let now = Instant::now();

    // Keep UI rendering alive briefly after scroll to allow poster placeholders to swap to textures
    // This avoids visible stalls while atlas uploads complete
    {
        use std::time::Duration;
        let until = now
            + Duration::from_millis(
                (crate::infrastructure::constants::animation::DEFAULT_DURATION_MS as f64 * 1.25)
                    as u64,
            );
        let ui_until = &mut state.domains.ui.state.poster_anim_active_until;
        *ui_until = Some(ui_until.map(|u| u.max(until)).unwrap_or(until));
    }

    // Reset scroll stopped time when actively scrolling
    state.domains.ui.state.scroll_stopped_time = None;
    state.domains.ui.state.last_scroll_position = current_position;
    state.domains.ui.state.last_scroll_time = Some(now);

    // Rate-limit task creation: only create a new task if enough time has passed
    // This prevents flooding the subscription channel during rapid scrolling
    let should_create_task = state
        .domains
        .ui
        .state
        .last_check_task_created
        .map(|last| last.elapsed() >= Duration::from_millis(SCROLL_STOP_DEBOUNCE_MS / 2))
        .unwrap_or(true);

    if should_create_task {
        state.domains.ui.state.last_check_task_created = Some(now);

        // PoC yoke prefetch: limit frequency by the same gating used for debounced task creation
        // Prefetch only currently visible items to keep changes minimal
        let visible_items = state.tab_manager.get_active_tab_visible_items();
        let mut prefetched = 0usize;
        for archived_id in visible_items.iter() {
            // Deserialize archived ID to runtime MediaID
            if let Ok(media_id) =
                rkyv::deserialize::<ferrex_core::MediaID, rkyv::rancor::Error>(archived_id)
            {
                match media_id {
                    ferrex_core::MediaID::Movie(mid) => {
                        let uuid = mid.to_uuid();
                        // Skip if already cached
                        if state.domains.ui.state.movie_yoke_cache.contains_key(&uuid) {
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
                    ferrex_core::MediaID::Series(mid) => {
                        let uuid = mid.to_uuid();
                        if state.domains.ui.state.series_yoke_cache.contains_key(&uuid) {
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

        Task::perform(
            async move {
                tokio::time::sleep(Duration::from_millis(SCROLL_STOP_DEBOUNCE_MS)).await;
            },
            |_| Message::CheckScrollStopped,
        )
    } else {
        // Too soon since last task creation, skip to avoid channel overflow
        Task::none()
    }
}
