//! Curated carousels for the All tab
//!
//! Computes and initializes curated lists: Continue Watching, Recently Added
//! (Movies/Series), and Recently Released (Movies/Series). Separated from
//! generic virtual carousel helpers to keep responsibilities focused and
//! make future extensions straightforward.

use crate::{
    domains::{
        metadata::demand_planner::{
            DemandContext, DemandRequestKind, DemandSnapshot,
        },
        ui::{
            tabs::{TabId, TabState},
            views::virtual_carousel::{
                planner,
                types::{CarouselConfig, CarouselKey},
            },
        },
    },
    infra::constants::curated::{HEAD_WINDOW, MAX_CAROUSEL_ITEMS},
    state::State,
};

use ferrex_core::player_prelude::{
    LibraryId, PosterKind, PosterSize, SortBy, SortOrder, compare_media,
};
use ferrex_model::{
    EpisodeID, LibraryType, Media, MediaID, MovieID, SeasonID, SeriesID,
};
use log::info;

use std::cmp::Ordering;
use uuid::Uuid;

#[derive(Debug, Default)]
struct CuratedLists {
    continue_watching: Vec<Uuid>,
    recent_movies: Vec<Uuid>,
    recent_series: Vec<Uuid>,
    released_movies: Vec<Uuid>,
    released_series: Vec<Uuid>,
}

fn fetch_media_for_uuid(
    state: &State,
    lib_type: LibraryType,
    id: Uuid,
) -> Option<Media> {
    match lib_type {
        LibraryType::Movies => state
            .domains
            .ui
            .state
            .repo_accessor
            .get(&MediaID::Movie(MovieID(id)))
            .ok(),
        LibraryType::Series => state
            .domains
            .ui
            .state
            .repo_accessor
            .get(&MediaID::Series(SeriesID(id)))
            .ok(),
    }
}

fn k_way_merge_top(
    state: &State,
    lib_type: LibraryType,
    sort_by: SortBy,
    limit: usize,
) -> Vec<Uuid> {
    // Collect per-library sorted id lists
    let mut per_lib_ids: Vec<(&LibraryId, Vec<Uuid>, usize)> = Vec::new();
    for (lib_id, lt) in state.tab_manager.library_info() {
        if *lt != lib_type {
            continue;
        }
        let ids = state
            .domains
            .ui
            .state
            .repo_accessor
            .get_sorted_index_by_library(lib_id, sort_by, SortOrder::Descending)
            .unwrap_or_default();
        if !ids.is_empty() {
            per_lib_ids.push((lib_id, ids, 0));
        }
    }

    // Current head media for each library
    let mut head_media: Vec<Option<Media>> = vec![None; per_lib_ids.len()];
    let mut out: Vec<Uuid> = Vec::with_capacity(limit);

    while out.len() < limit {
        let mut best_idx: Option<usize> = None;
        let mut best_media: Option<Media> = None;
        for (i, (_lib_id, ids, ptr)) in per_lib_ids.iter_mut().enumerate() {
            if *ptr >= ids.len() {
                continue;
            }
            if head_media[i].is_none() {
                let id = ids[*ptr];
                head_media[i] = fetch_media_for_uuid(state, lib_type, id);
            }
            if let Some(ref media) = head_media[i] {
                match (&best_media, media) {
                    (None, m) => {
                        best_media = Some(m.clone());
                        best_idx = Some(i);
                    }
                    (Some(bm), m) => {
                        match compare_media(
                            bm,
                            m,
                            sort_by,
                            SortOrder::Descending,
                        ) {
                            Some(Ordering::Less) => {
                                best_media = Some(m.clone());
                                best_idx = Some(i);
                            }
                            Some(Ordering::Equal) | None => {
                                // Tiebreak by UUID desc for stability
                                let bm_id = match bm {
                                    Media::Movie(mr) => mr.id.to_uuid(),
                                    Media::Series(sr) => sr.id.to_uuid(),
                                    Media::Season(s) => s.id.to_uuid(),
                                    Media::Episode(e) => e.id.to_uuid(),
                                };
                                let m_id = match m {
                                    Media::Movie(mr) => mr.id.to_uuid(),
                                    Media::Series(sr) => sr.id.to_uuid(),
                                    Media::Season(s) => s.id.to_uuid(),
                                    Media::Episode(e) => e.id.to_uuid(),
                                };
                                if m_id > bm_id {
                                    best_media = Some(m.clone());
                                    best_idx = Some(i);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        if let Some(i) = best_idx {
            let id = per_lib_ids[i].1[per_lib_ids[i].2];
            out.push(id);
            per_lib_ids[i].2 += 1;
            head_media[i] = None; // advance
        } else {
            break;
        }
    }

    out
}

fn compute_curated_lists(state: &State) -> CuratedLists {
    // Continue Watching: movies and series only; map episodes to parent series and dedupe by most recent
    let mut cw_map: std::collections::HashMap<Uuid, i64> =
        std::collections::HashMap::new();
    // Helper: resolve a Media by unknown UUID by probing known ID kinds.
    let lookup_media = |uid: Uuid| -> Option<Media> {
        let acc = &state.domains.ui.state.repo_accessor;
        acc.get(&MediaID::Movie(MovieID(uid)))
            .ok()
            .or_else(|| acc.get(&MediaID::Series(SeriesID(uid))).ok())
            .or_else(|| acc.get(&MediaID::Season(SeasonID(uid))).ok())
            .or_else(|| acc.get(&MediaID::Episode(EpisodeID(uid))).ok())
    };

    if let Some(watch) = state.domains.media.state.get_watch_state() {
        for item in watch.in_progress.values() {
            let uid = item.media_id;
            // Get media by uuid (any type) and map accordingly
            if let Some(media) = lookup_media(uid) {
                match media {
                    Media::Movie(m) => {
                        let id = m.id.to_uuid();
                        cw_map
                            .entry(id)
                            .and_modify(|t| *t = (*t).max(item.last_watched))
                            .or_insert(item.last_watched);
                    }
                    Media::Series(s) => {
                        let id = s.id.to_uuid();
                        cw_map
                            .entry(id)
                            .and_modify(|t| *t = (*t).max(item.last_watched))
                            .or_insert(item.last_watched);
                    }
                    Media::Episode(e) => {
                        let id = e.series_id.to_uuid();
                        cw_map
                            .entry(id)
                            .and_modify(|t| *t = (*t).max(item.last_watched))
                            .or_insert(item.last_watched);
                    }
                    Media::Season(season) => {
                        // Treat season as series-level continue watching
                        let id = season.series_id.to_uuid();
                        cw_map
                            .entry(id)
                            .and_modify(|t| *t = (*t).max(item.last_watched))
                            .or_insert(item.last_watched);
                    }
                }
            }
        }
    }
    let mut continue_pairs: Vec<(i64, Uuid)> =
        cw_map.into_iter().map(|(id, t)| (t, id)).collect();
    continue_pairs.sort_by(|a, b| b.0.cmp(&a.0));
    let mut continue_ids: Vec<Uuid> =
        continue_pairs.into_iter().map(|(_, id)| id).collect();
    if continue_ids.len() > MAX_CAROUSEL_ITEMS {
        continue_ids.truncate(MAX_CAROUSEL_ITEMS);
    }

    // Recently added
    let recent_movies = k_way_merge_top(
        state,
        LibraryType::Movies,
        SortBy::DateAdded,
        MAX_CAROUSEL_ITEMS,
    );
    let recent_series = k_way_merge_top(
        state,
        LibraryType::Series,
        SortBy::DateAdded,
        MAX_CAROUSEL_ITEMS,
    );

    // Recently released
    let released_movies = k_way_merge_top(
        state,
        LibraryType::Movies,
        SortBy::ReleaseDate,
        MAX_CAROUSEL_ITEMS,
    );
    let released_series = k_way_merge_top(
        state,
        LibraryType::Series,
        SortBy::ReleaseDate,
        MAX_CAROUSEL_ITEMS,
    );

    CuratedLists {
        continue_watching: continue_ids,
        recent_movies,
        recent_series,
        released_movies,
        released_series,
    }
}

/// Recompute curated lists and ensure corresponding carousels are initialized
pub fn recompute_and_init_curated_carousels(state: &mut State) {
    // Ensure repository is initialized and libraries are known before first compute
    if !state.domains.ui.state.repo_accessor.is_initialized()
        || state.tab_manager.library_info().is_empty()
    {
        return;
    }
    let lists = compute_curated_lists(state);
    let width = state.window_size.width.max(1.0);

    // Apply to AllTabState in a short mutable borrow
    if let TabState::Home(all_state) =
        state.tab_manager.get_or_create_tab(TabId::Home)
    {
        all_state.continue_watching = lists.continue_watching.clone();
        all_state.recent_movies = lists.recent_movies.clone();
        all_state.recent_series = lists.recent_series.clone();
        all_state.released_movies = lists.released_movies.clone();
        all_state.released_series = lists.released_series.clone();
    }

    // Ensure registry states for each curated carousel
    let scale = state.domains.ui.state.scaled_layout.scale;
    state.domains.ui.state.carousel_registry.ensure_default(
        CarouselKey::Custom("ContinueWatching"),
        lists.continue_watching.len(),
        width,
        CarouselConfig::poster_defaults(),
        scale,
    );
    state.domains.ui.state.carousel_registry.ensure_default(
        CarouselKey::Custom("RecentlyAddedMovies"),
        lists.recent_movies.len(),
        width,
        CarouselConfig::poster_defaults(),
        scale,
    );
    state.domains.ui.state.carousel_registry.ensure_default(
        CarouselKey::Custom("RecentlyAddedSeries"),
        lists.recent_series.len(),
        width,
        CarouselConfig::poster_defaults(),
        scale,
    );
    state.domains.ui.state.carousel_registry.ensure_default(
        CarouselKey::Custom("RecentlyReleasedMovies"),
        lists.released_movies.len(),
        width,
        CarouselConfig::poster_defaults(),
        scale,
    );
    state.domains.ui.state.carousel_registry.ensure_default(
        CarouselKey::Custom("RecentlyReleasedSeries"),
        lists.released_series.len(),
        width,
        CarouselConfig::poster_defaults(),
        scale,
    );
}

/// Emit initial planner snapshots for curated carousels
pub fn emit_initial_curated_snapshots(state: &mut State) {
    let Some(handle) = state.domains.metadata.state.planner_handle.as_ref()
    else {
        return;
    };
    let all_state = match state.tab_manager.get_tab(TabId::Home) {
        Some(TabState::Home(s)) => s,
        _ => return,
    };

    // Continue Watching (mixed types)
    if let Some(vc) = state
        .domains
        .ui
        .state
        .carousel_registry
        .get(&CarouselKey::Custom("ContinueWatching"))
    {
        let ids = &all_state.continue_watching;
        let total = ids.len();
        if total > 0 {
            // If the VC hasn't measured yet and visible_range is empty,
            // fall back to a small head window to kick off image loads.
            if vc.visible_range.start == vc.visible_range.end {
                let head = usize::min(total, HEAD_WINDOW);
                let visible_ids: Vec<_> =
                    (0..head).filter_map(|i| ids.get(i).copied()).collect();
                // Build explicit context for both Movie/Series ids
                let ctx = crate::domains::ui::update_handlers::virtual_carousel_helpers::build_mixed_poster_context(
                    state,
                    &visible_ids,
                );

                let snap = DemandSnapshot {
                    visible_ids,
                    prefetch_ids: Vec::new(),
                    background_ids: Vec::new(),
                    timestamp: std::time::Instant::now(),
                    context: Some(ctx),
                    // Use explicit per-id overrides rather than a fallback kind
                    poster_kind: None,
                };
                handle.send(snap);
            } else {
                let (vis, mut pre, mut back) = planner::collect_ranges_ids(
                    vc,
                    total,
                    |i| ids.get(i).copied(),
                    &state.runtime_config,
                );
                pre.retain(|id| !vis.contains(id));
                back.retain(|id| !vis.contains(id) && !pre.contains(id));
                // Build explicit context for both Movie/Series ids in the union
                let mut all = vis.clone();
                all.extend(pre.iter().copied());
                all.extend(back.iter().copied());
                let ctx = crate::domains::ui::update_handlers::virtual_carousel_helpers::build_mixed_poster_context(
                    state,
                    &all,
                );

                info!(
                    "Sending snapshot request for {} visible posters, {} prefetch, and {} background.",
                    vis.len(),
                    pre.len(),
                    back.len()
                );

                let snap = DemandSnapshot {
                    visible_ids: vis,
                    prefetch_ids: pre,
                    background_ids: back,
                    timestamp: std::time::Instant::now(),
                    context: Some(ctx),
                    poster_kind: None,
                };
                handle.send(snap);
            }
        }
    }

    // Simple helpers for the remaining lists
    let send_simple = |key: &'static str,
                       poster: PosterKind,
                       ids: &Vec<Uuid>| {
        let carousel_key = CarouselKey::Custom(key);
        crate::domains::ui::update_handlers::virtual_carousel_helpers::emit_snapshot_for_carousel_simple(
            state,
            &carousel_key,
            ids.len(),
            |i| ids.get(i).copied(),
            Some(poster),
        );
    };

    send_simple(
        "RecentlyAddedMovies",
        PosterKind::Movie,
        &all_state.recent_movies,
    );
    send_simple(
        "RecentlyAddedSeries",
        PosterKind::Series,
        &all_state.recent_series,
    );
    send_simple(
        "RecentlyReleasedMovies",
        PosterKind::Movie,
        &all_state.released_movies,
    );
    send_simple(
        "RecentlyReleasedSeries",
        PosterKind::Series,
        &all_state.released_series,
    );
}
