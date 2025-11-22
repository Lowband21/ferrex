//! Helpers for initializing and maintaining virtual carousels across views

use std::time::{Duration, Instant};

use crate::domains::metadata::demand_planner::DemandSnapshot;
use crate::domains::ui::views::virtual_carousel::{
    planner,
    types::{CarouselConfig, CarouselKey},
};
use crate::domains::ui::{
    messages::UiMessage,
    tabs::{TabId, TabState},
};
use crate::infra::api_types::LibraryType;
use crate::infra::api_types::Media;
use crate::state::State;
use ferrex_core::player_prelude::{MediaID, MovieID, PosterKind, SeriesID};
use iced::Task;
use iced::widget::{operation::scroll_to, scrollable::AbsoluteOffset};
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
            state.domains.ui.state.carousel_registry.ensure_default(
                key.clone(),
                total,
                width,
                CarouselConfig::poster_defaults(),
            );

            // Restore saved scroll position state (actual scroll_to happens in separate function)
            if let Some(saved_scroll) = state
                .domains
                .ui
                .state
                .scroll_manager
                .get_carousel_scroll(&key)
            {
                if let Some(vc) =
                    state.domains.ui.state.carousel_registry.get_mut(&key)
                {
                    // Restore the carousel state
                    vc.set_index_position(saved_scroll.index_position);
                    vc.set_reference_index(saved_scroll.reference_index);
                }
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
    poster_kind: Option<PosterKind>,
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
        let visible_ids: Vec<_> = (0..head).filter_map(|i| ids_fn(i)).collect();
        DemandSnapshot {
            visible_ids,
            prefetch_ids: Vec::new(),
            background_ids: Vec::new(),
            timestamp: Instant::now(),
            context: None,
            poster_kind,
        }
    } else {
        planner::snapshot_for_visible(vc, total, ids_fn, poster_kind, None)
    };

    handle.send(snap);
}

/// Build a DemandContext that maps the provided ids to their correct poster types
/// (Movie vs Series). This is useful for mixed lists like Continue Watching.
pub fn build_mixed_poster_context(
    state: &State,
    ids: &[Uuid],
) -> crate::domains::metadata::demand_planner::DemandContext {
    use crate::domains::metadata::demand_planner::{
        DemandContext, DemandRequestKind,
    };
    use ferrex_core::player_prelude::PosterSize;

    let mut ctx = DemandContext::default();
    let acc = &state.domains.ui.state.repo_accessor;
    for id in ids {
        // Try to detect series first; fall back to movie
        let is_series = acc
            .get(&MediaID::Series(SeriesID(*id)))
            .ok()
            .map(|m| matches!(m, Media::Series(_)))
            .unwrap_or(false);

        if is_series {
            ctx.override_request(
                *id,
                DemandRequestKind::Poster {
                    kind: PosterKind::Series,
                    size: PosterSize::Standard,
                },
            );
        } else {
            // Be explicit for movies too to avoid any fallback ambiguity
            ctx.override_request(
                *id,
                DemandRequestKind::Poster {
                    kind: PosterKind::Movie,
                    size: PosterSize::Standard,
                },
            );
        }
    }
    ctx
}

/// Emit initial DemandPlanner snapshots for each All-tab carousel so images load
/// even before any scroll events fire.
pub fn emit_initial_all_tab_snapshots(state: &mut State) {
    {
        for (lib_id, lib_type) in &state.tab_manager.library_info().clone() {
            // Ensure the corresponding library tab exists and has cached IDs
            if let Some(tab) =
                state.tab_manager.get_tab(TabId::Library(*lib_id))
            {
                if let TabState::Library(lib_state) = tab {
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
                    let poster_kind = match lib_type {
                        LibraryType::Movies => Some(PosterKind::Movie),
                        LibraryType::Series => Some(PosterKind::Series),
                    };
                    emit_snapshot_for_carousel_simple(
                        state,
                        &key,
                        total,
                        |i| ids.get(i).copied(),
                        poster_kind,
                    );
                }
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
