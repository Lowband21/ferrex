//! Helpers for initializing and maintaining virtual carousels across views

use crate::{
    domains::{
        metadata::demand_planner::DemandSnapshot,
        ui::{
            messages::UiMessage,
            tabs::{TabId, TabState},
            views::virtual_carousel::{
                planner,
                types::{CarouselConfig, CarouselKey},
            },
        },
    },
    infra::api_types::LibraryType,
    state::State,
};

use ferrex_core::player_prelude::PosterSize;
use iced::{
    Task,
    widget::{operation::scroll_to, scrollable::AbsoluteOffset},
};
use std::time::Instant;
use uuid::Uuid;

pub fn init_all_tab_virtual_carousels(state: &mut State) {
    let width = state.window_size.width.max(1.0);
    for (lib_id, lib_type) in &state.tab_manager.library_info().clone() {
        // Ensure a library tab exists for this library so we can use its cached sorted IDs
        let tab = state.tab_manager.get_or_create_tab(TabId::Library(*lib_id));

        // Align the library tab's sort with current UI sort and refresh caches if needed
        if let TabState::Library(lib_state) = tab {
            // Ensure sort matches global UI sort settings
            lib_state.set_sort(
                state.domains.ui.state.sort_by,
                state.domains.ui.state.sort_order,
            );
            if lib_state.needs_refresh {
                lib_state.refresh_from_repo();
            }

            let total = lib_state.cached_index_ids.len();
            let key = match lib_type {
                LibraryType::Movies => {
                    CarouselKey::LibraryMovies(lib_id.to_uuid())
                }
                LibraryType::Series => {
                    CarouselKey::LibrarySeries(lib_id.to_uuid())
                }
            };
            let scale = state.domains.ui.state.scaled_layout.scale;
            state.domains.ui.state.carousel_registry.ensure_default(
                key.clone(),
                total,
                width,
                CarouselConfig::poster_defaults(),
                scale,
            );

            // Restore saved scroll position state (actual scroll_to happens in separate function)
            if let Some(saved_scroll) = state
                .domains
                .ui
                .state
                .scroll_manager
                .get_carousel_scroll(&key)
                && let Some(vc) =
                    state.domains.ui.state.carousel_registry.get_mut(&key)
            {
                // Restore the carousel state
                vc.set_index_position(saved_scroll.index_position);
                vc.set_reference_index(saved_scroll.reference_index);
            }
        }
    }
}

// Note: single implementation of restore_all_tab_carousel_scroll_positions exists below.

/// Emit a snapshot for a carousel's current visible window.
///
/// - Uses planner::snapshot_for_visible when measurements are available.
/// - Falls back to a small head window when visible_range is empty.
/// - Debounced using the registry's `should_emit_snapshot` to reduce chatter.
pub fn emit_snapshot_for_carousel_simple<F>(
    state: &State,
    key: &CarouselKey,
    total: usize,
    ids_fn: F,
    poster_size: PosterSize,
) where
    F: Fn(usize) -> Option<Uuid> + Copy,
{
    let Some(handle) = state.domains.metadata.state.planner_handle.as_ref()
    else {
        return;
    };
    let Some(vc) = state.domains.ui.state.carousel_registry.get(key) else {
        return;
    };

    let snap = if vc.visible_range.start == vc.visible_range.end && total > 0 {
        // Head window fallback when no measurements yet
        let head = usize::min(total, 10);
        let visible_ids: Vec<_> = (0..head).filter_map(&ids_fn).collect();
        DemandSnapshot {
            visible_ids,
            prefetch_ids: Vec::new(),
            background_ids: Vec::new(),
            timestamp: Instant::now(),
            context: None,
            poster_size,
        }
    } else {
        planner::snapshot_for_visible(
            vc,
            total,
            ids_fn,
            poster_size,
            None,
            &state.runtime_config,
        )
    };

    handle.send(snap);
}

/// Emit initial DemandPlanner snapshots for each All-tab carousel so images load
/// even before any scroll events fire.
pub fn emit_initial_all_tab_snapshots(state: &mut State) {
    {
        let state_ref: &State = state;
        let poster_size = state.domains.settings.display.library_poster_quality;
        for (lib_id, lib_type) in &state.tab_manager.library_info().clone() {
            // Ensure the corresponding library tab exists and has cached IDs
            if let Some(tab) =
                state.tab_manager.get_tab(TabId::Library(*lib_id))
                && let TabState::Library(lib_state) = tab
            {
                let ids = &lib_state.cached_index_ids;
                let total = ids.len();
                let key = match lib_type {
                    LibraryType::Movies => {
                        CarouselKey::LibraryMovies(lib_id.to_uuid())
                    }
                    LibraryType::Series => {
                        CarouselKey::LibrarySeries(lib_id.to_uuid())
                    }
                };
                emit_snapshot_for_carousel_simple(
                    state_ref,
                    &key,
                    total,
                    |i| {
                        ids.get(i).copied().and_then(|id| {
                            crate::domains::ui::utils::primary_poster_iid_for_library_media(
                                state_ref,
                                *lib_type,
                                id,
                            )
                        })
                    },
                    poster_size,
                );
            }
        }
    }
}

/// Restore saved horizontal scroll positions for all All-tab carousels.
/// Returns a Task that batches scroll_to operations for each carousel with saved state.
pub fn restore_all_tab_carousel_scroll_positions(
    state: &mut State,
) -> Task<UiMessage> {
    let mut tasks: Vec<Task<UiMessage>> = Vec::new();

    // Use the ordered list of carousels currently in the All view to avoid touching unrelated keys
    let keys = crate::domains::ui::tabs::ordered_keys_for_home(state);
    for key in keys {
        // Skip if no state or no saved position
        let Some(vc) = state.domains.ui.state.carousel_registry.get_mut(&key)
        else {
            continue;
        };
        let Some(saved) = state
            .domains
            .ui
            .state
            .scroll_manager
            .get_carousel_scroll(&key)
        else {
            continue;
        };

        // Apply saved index position to state (authoritative index → scroll_x)
        vc.set_index_position(saved.index_position);
        vc.set_reference_index(saved.reference_index);

        // Schedule a scroll_to to apply the visual viewport position
        let id = vc.scrollable_id.clone();
        let x = vc.scroll_x;
        tasks.push(scroll_to::<UiMessage>(id, AbsoluteOffset { x, y: 0.0 }));
    }

    if tasks.is_empty() {
        Task::none()
    } else {
        Task::batch(tasks)
    }
}
