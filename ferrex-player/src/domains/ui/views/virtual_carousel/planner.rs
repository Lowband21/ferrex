//! Planner integration helpers (scaffold)
//!
//! Provides helpers to build DemandSnapshot data from a carousel state.
//! The concrete mapping of media IDs to ImageRequests depends on context
//! (movies/series vs episodes/cast) and will be wired by callsites.

use super::state::VirtualCarouselState;
use crate::{
    domains::metadata::demand_planner::{
        DemandContext, DemandRequestKind, DemandSnapshot,
    },
    infra::runtime_config::RuntimeConfig,
};

use ferrex_core::player_prelude::{EpisodeStillSize, PosterKind};

use uuid::Uuid;

/// Build a generic snapshot for a poster-based carousel.
///
/// - `ids_fn` must map item indices to Uuids (if present in range)
/// - `poster_kind` sets the default PosterKind when no DemandContext override
///   is provided (e.g., Movie/Series/Season)
/// Collect visible, prefetch, and background id lists using the state's ranges.
pub fn collect_ranges_ids<F>(
    state: &VirtualCarouselState,
    _total_items: usize,
    ids_fn: F,
    rc: &RuntimeConfig,
) -> (Vec<Uuid>, Vec<Uuid>, Vec<Uuid>)
where
    F: Fn(usize) -> Option<Uuid>,
{
    let vr = state.visible_range.clone();

    let mut visible_ids: Vec<Uuid> = Vec::new();
    for i in vr.clone() {
        if let Some(id) = ids_fn(i) {
            visible_ids.push(id);
        }
    }

    let prefetch_items = rc.carousel_prefetch_items();
    let prefetch = state.prefetch_range(prefetch_items);
    let mut prefetch_ids: Vec<Uuid> = Vec::new();
    for i in prefetch.clone() {
        if let Some(id) = ids_fn(i) {
            prefetch_ids.push(id);
        }
    }
    prefetch_ids.retain(|id| !visible_ids.contains(id));

    let background_items = rc.carousel_background_items();
    let background = state.background_range(prefetch_items, background_items);
    let mut background_ids: Vec<Uuid> = Vec::new();
    for i in background.clone() {
        if let Some(id) = ids_fn(i) {
            background_ids.push(id);
        }
    }
    background_ids
        .retain(|id| !visible_ids.contains(id) && !prefetch_ids.contains(id));

    (visible_ids, prefetch_ids, background_ids)
}

pub fn snapshot_for_visible<F>(
    state: &VirtualCarouselState,
    total_items: usize,
    ids_fn: F,
    poster_kind: Option<PosterKind>,
    context: Option<DemandContext>,
    rc: &RuntimeConfig,
) -> DemandSnapshot
where
    F: Fn(usize) -> Option<Uuid>,
{
    let (visible_ids, prefetch_ids, background_ids) =
        collect_ranges_ids(state, total_items, ids_fn, rc);

    DemandSnapshot {
        visible_ids,
        prefetch_ids,
        background_ids,
        timestamp: std::time::Instant::now(),
        context,
        poster_kind,
    }
}

/// Build a DemandContext overriding requests for the provided episode IDs to EpisodeStill.
pub fn build_episode_still_context(ids: &[Uuid]) -> DemandContext {
    let mut context = DemandContext::default();
    for id in ids {
        context.override_request(
            *id,
            DemandRequestKind::EpisodeStill {
                size: EpisodeStillSize::Standard,
            },
        );
    }
    context
}
