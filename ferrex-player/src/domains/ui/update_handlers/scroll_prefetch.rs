use crate::{
    domains::{metadata::demand_planner::DemandSnapshot, ui::tabs::TabState},
    infra::constants,
    state::State,
};

use ferrex_core::player_prelude::{MediaID, MovieID, SeriesID};

use std::{
    collections::HashSet,
    time::{Duration, Instant},
};
use uuid::Uuid;

const PREFETCH_THROTTLE_FLOOR_MS: u64 = 33; // ~30Hz, avoids work every frame
const MAX_SNAPSHOT_IDS_PER_BUCKET: usize = 192;
const MAX_YOKES_PER_PASS_WHEN_IDLE: usize = 48;
const MAX_YOKES_PER_PASS_WHEN_KINETIC: usize = 16;

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn maybe_run_grid_scroll_prefetch(state: &mut State, now: Instant) {
    let debounce_ms = state.runtime_config.scroll_debounce_ms();
    let throttle_ms = (debounce_ms / 2).max(PREFETCH_THROTTLE_FLOOR_MS);

    let should_run = state
        .domains
        .ui
        .state
        .last_prefetch_tick
        .map(|last| {
            now.saturating_duration_since(last)
                >= Duration::from_millis(throttle_ms)
        })
        .unwrap_or(true);

    if !should_run {
        return;
    }
    state.domains.ui.state.last_prefetch_tick = Some(now);

    let active_tab_id = state.tab_manager.active_tab_id();
    let Some(TabState::Library(lib_state)) =
        state.tab_manager.get_tab(active_tab_id)
    else {
        return;
    };

    let grid = &lib_state.grid_state;
    if grid.total_items == 0 || grid.columns == 0 {
        return;
    }

    // Snapshot ranges are based on the tab's last computed visible range.
    let vr = grid.visible_range.clone();
    let prefetch_rows = state.runtime_config.prefetch_rows_above();
    let pr = grid.get_preload_range(prefetch_rows);
    let br = grid.get_background_range(
        prefetch_rows,
        constants::layout::virtual_grid::BACKGROUND_ROWS_BELOW,
    );

    // ========= DEMAND SNAPSHOT (IMAGE PREFETCH PLANNER) =========
    if let Some(handle) = state.domains.metadata.state.planner_handle.as_ref() {
        let visible_slice: &[Uuid] =
            lib_state.cached_index_ids.get(vr).unwrap_or(&[]);
        let prefetch_slice: &[Uuid] =
            lib_state.cached_index_ids.get(pr).unwrap_or(&[]);
        let background_slice: &[Uuid] =
            lib_state.cached_index_ids.get(br).unwrap_or(&[]);

        let mut visible_ids: Vec<Uuid> =
            Vec::with_capacity(visible_slice.len().min(64));
        let mut prefetch_ids: Vec<Uuid> =
            Vec::with_capacity(prefetch_slice.len().min(64));
        let mut background_ids: Vec<Uuid> =
            Vec::with_capacity(background_slice.len().min(64));

        let mut visible_set: HashSet<Uuid> =
            HashSet::with_capacity(visible_slice.len());
        let mut prefetch_set: HashSet<Uuid> =
            HashSet::with_capacity(prefetch_slice.len());

        for &media_uuid in
            visible_slice.iter().take(MAX_SNAPSHOT_IDS_PER_BUCKET)
        {
            if let Some(iid) = crate::domains::ui::utils::primary_poster_iid_for_library_media_cached(
                state,
                lib_state.library_type,
                media_uuid,
            ) && visible_set.insert(iid)
            {
                visible_ids.push(iid);
            }
        }

        for &media_uuid in
            prefetch_slice.iter().take(MAX_SNAPSHOT_IDS_PER_BUCKET)
        {
            if let Some(iid) = crate::domains::ui::utils::primary_poster_iid_for_library_media_cached(
                state,
                lib_state.library_type,
                media_uuid,
            ) && !visible_set.contains(&iid)
                && prefetch_set.insert(iid)
            {
                prefetch_ids.push(iid);
            }
        }

        for &media_uuid in
            background_slice.iter().take(MAX_SNAPSHOT_IDS_PER_BUCKET)
        {
            if let Some(iid) = crate::domains::ui::utils::primary_poster_iid_for_library_media_cached(
                state,
                lib_state.library_type,
                media_uuid,
            ) && !visible_set.contains(&iid)
                && !prefetch_set.contains(&iid)
            {
                background_ids.push(iid);
            }
        }

        let poster_size = state.domains.settings.display.library_poster_quality;
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

    // ========= UI YOKE PREFETCH (AVOIDS VIEW-TIME REPO READS) =========
    // During kinetic scrolling, prioritize keeping frames smooth; do less per pass.
    let max_yokes = if state.domains.ui.state.motion_controller.is_active() {
        MAX_YOKES_PER_PASS_WHEN_KINETIC
    } else {
        MAX_YOKES_PER_PASS_WHEN_IDLE
    };

    // Prefer priming the currently-visible range first, then the preload range.
    let visible_slice: &[Uuid] = lib_state
        .cached_index_ids
        .get(grid.visible_range.clone())
        .unwrap_or(&[]);
    let preload_slice: &[Uuid] = lib_state
        .cached_index_ids
        .get(grid.get_preload_range(prefetch_rows))
        .unwrap_or(&[]);

    let mut prefetched = 0usize;
    for &media_uuid in visible_slice.iter().chain(preload_slice.iter()) {
        if prefetched >= max_yokes {
            break;
        }

        match lib_state.library_type {
            crate::infra::api_types::LibraryType::Movies => {
                if state
                    .domains
                    .ui
                    .state
                    .movie_yoke_cache
                    .contains_key(&media_uuid)
                {
                    continue;
                }
                if let Ok(yoke) = state
                    .domains
                    .ui
                    .state
                    .repo_accessor
                    .get_movie_yoke(&MediaID::Movie(MovieID(media_uuid)))
                {
                    state
                        .domains
                        .ui
                        .state
                        .movie_yoke_cache
                        .insert(media_uuid, std::sync::Arc::new(yoke));
                    prefetched += 1;
                }
            }
            crate::infra::api_types::LibraryType::Series => {
                if state
                    .domains
                    .ui
                    .state
                    .series_yoke_cache
                    .contains_key(&media_uuid)
                {
                    continue;
                }
                if let Ok(yoke) = state
                    .domains
                    .ui
                    .state
                    .repo_accessor
                    .get_series_yoke(&MediaID::Series(SeriesID(media_uuid)))
                {
                    state
                        .domains
                        .ui
                        .state
                        .series_yoke_cache
                        .insert(media_uuid, std::sync::Arc::new(yoke));
                    prefetched += 1;
                }
            }
        }
    }
}
