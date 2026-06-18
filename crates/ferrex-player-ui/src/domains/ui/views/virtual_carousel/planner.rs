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

use ferrex_core::player_prelude::{EpisodeSize, PosterSize, ProfileSize};

use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CarouselDemandImageKind {
    Poster { size: PosterSize },
    EpisodeStill { size: EpisodeSize },
    Profile { size: ProfileSize },
}

impl CarouselDemandImageKind {
    fn poster_size(self, fallback: PosterSize) -> PosterSize {
        match self {
            Self::Poster { size } => size,
            Self::EpisodeStill { .. } | Self::Profile { .. } => fallback,
        }
    }

    fn request_kind(self) -> Option<DemandRequestKind> {
        match self {
            Self::Poster { .. } => None,
            Self::EpisodeStill { size } => {
                Some(DemandRequestKind::EpisodeStill { size })
            }
            Self::Profile { size } => Some(DemandRequestKind::Profile { size }),
        }
    }
}

/// Build a generic snapshot for a poster-based carousel.
///
/// - `ids_fn` must map item indices to Uuids (if present in range)
/// - `poster_size` sets the default `ImageSize::Poster` size when no
///   `DemandContext` override is provided.
///   Collect visible, prefetch, and background id lists using the state's ranges.
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
    poster_size: PosterSize,
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
        poster_size,
    }
}

pub fn snapshot_for_visible_with_image_kind<F>(
    state: &VirtualCarouselState,
    total_items: usize,
    ids_fn: F,
    image_kind: CarouselDemandImageKind,
    fallback_poster_size: PosterSize,
    rc: &RuntimeConfig,
) -> DemandSnapshot
where
    F: Fn(usize) -> Option<Uuid>,
{
    let (visible_ids, prefetch_ids, background_ids) =
        collect_ranges_ids(state, total_items, ids_fn, rc);
    let context = match image_kind {
        CarouselDemandImageKind::Profile { size } => {
            let image_ids = visible_ids
                .iter()
                .chain(prefetch_ids.iter())
                .chain(background_ids.iter())
                .copied()
                .collect::<Vec<_>>();
            Some(build_profile_context(&image_ids, size))
        }
        _ => image_kind.request_kind().map(|request_kind| {
            let mut context = DemandContext::default();
            for id in visible_ids
                .iter()
                .chain(prefetch_ids.iter())
                .chain(background_ids.iter())
            {
                context.override_request(*id, request_kind.clone());
            }
            context
        }),
    };

    DemandSnapshot {
        visible_ids,
        prefetch_ids,
        background_ids,
        timestamp: std::time::Instant::now(),
        context,
        poster_size: image_kind.poster_size(fallback_poster_size),
    }
}

/// Build a DemandContext overriding requests for the provided episode IDs to EpisodeStill.
pub fn build_episode_still_context(ids: &[Uuid]) -> DemandContext {
    let mut context = DemandContext::default();
    for id in ids {
        context.override_request(
            *id,
            DemandRequestKind::EpisodeStill {
                size: EpisodeSize::W512,
            },
        );
    }
    context
}

/// Build a DemandContext overriding requests for profile/cast images.
pub fn build_profile_context(ids: &[Uuid], size: ProfileSize) -> DemandContext {
    let mut context = DemandContext::default();
    for id in ids {
        context.override_request(*id, DemandRequestKind::Profile { size });
    }
    context
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::ui::views::virtual_carousel::types::CarouselConfig;

    fn uuid(value: u128) -> Uuid {
        Uuid::from_u128(value)
    }

    fn state_at_start() -> VirtualCarouselState {
        VirtualCarouselState::new_unscaled(
            8,
            320.0,
            CarouselConfig::detail_rail(100.0, 10.0),
        )
    }

    #[test]
    fn detail_profile_snapshot_overrides_all_priority_tiers() {
        let mut state = state_at_start();
        state.set_scroll_x(110.0);
        let ids = (1..=8).map(uuid).collect::<Vec<_>>();

        let snapshot = snapshot_for_visible_with_image_kind(
            &state,
            ids.len(),
            |i| ids.get(i).copied(),
            CarouselDemandImageKind::Profile {
                size: ProfileSize::W185,
            },
            PosterSize::W342,
            &RuntimeConfig::default(),
        );

        let context = snapshot.context.as_ref().expect("profile context");
        for id in snapshot
            .visible_ids
            .iter()
            .chain(snapshot.prefetch_ids.iter())
            .chain(snapshot.background_ids.iter())
        {
            assert!(matches!(
                context.request_kind(id),
                Some(DemandRequestKind::Profile {
                    size: ProfileSize::W185
                })
            ));
        }
    }

    #[test]
    fn detail_still_snapshot_uses_episode_still_overrides() {
        let state = state_at_start();
        let ids = (11..=18).map(uuid).collect::<Vec<_>>();

        let snapshot = snapshot_for_visible_with_image_kind(
            &state,
            ids.len(),
            |i| ids.get(i).copied(),
            CarouselDemandImageKind::EpisodeStill {
                size: EpisodeSize::W512,
            },
            PosterSize::W342,
            &RuntimeConfig::default(),
        );

        let context = snapshot.context.as_ref().expect("still context");
        assert!(snapshot.visible_ids.contains(&uuid(11)));
        for id in snapshot
            .visible_ids
            .iter()
            .chain(snapshot.prefetch_ids.iter())
        {
            assert!(matches!(
                context.request_kind(id),
                Some(DemandRequestKind::EpisodeStill {
                    size: EpisodeSize::W512
                })
            ));
        }
    }
}
