use crate::{
    domains::{
        metadata::demand_planner::{
            DemandContext, DemandRequestKind, DemandSnapshot,
        },
        ui::{
            messages as ui,
            motion_controller::{MotionController, MotionControllerConfig},
            scroll_manager::CarouselScrollState,
            tabs::{TabId, TabState},
            types::DisplayMode,
            views::virtual_carousel::{
                messages::VirtualCarouselMessage as VCM, planner,
                registry::MotionState, types::CarouselKey,
            },
        },
    },
    infra::{
        constants::virtual_carousel::{self, motion, snap},
        profiling_scopes,
    },
    state::State,
};
use std::time::Instant;

use ferrex_contracts::prelude::MediaLike;
use ferrex_core::{
    player_prelude::{LibraryID, MediaID, PosterKind, PosterSize},
    types::ids::SeriesID,
};

use ferrex_model::{MediaType, SeasonID};
use iced::{
    Task,
    widget::{operation::scroll_to, scrollable::AbsoluteOffset},
};
use std::time::Duration;

/// Handle virtual carousel messages
pub fn handle_virtual_carousel_message(
    state: &mut State,
    msg: VCM,
) -> Task<ui::Message> {
    match msg {
        VCM::ViewportChanged(key, viewport) => {
            #[cfg(any(
                feature = "profile-with-puffin",
                feature = "profile-with-tracy",
                feature = "profile-with-tracing",
            ))]
            profiling::scope!(profiling_scopes::scopes::CAROUSEL_CALC);

            // 1) Update viewport and check proximity to nearest aligned boundary
            let mut near_aligned: Option<f32> = None;
            if let Some(vc) =
                state.domains.ui.state.carousel_registry.get_mut(&key)
            {
                vc.update_scroll(viewport);
                // Persist horizontal scroll position for this carousel
                let saved = CarouselScrollState {
                    scroll_x: vc.scroll_x,
                    index_position: vc.index_position,
                    reference_index: vc.reference_index,
                    viewport_width: vc.viewport_width,
                };
                state
                    .domains
                    .ui
                    .state
                    .scroll_manager
                    .save_carousel_scroll(key.clone(), saved);

                // Compute nearest aligned boundary in pixel space and decide if it's close enough
                let stride = (vc.item_width + vc.item_spacing).max(1.0);
                let commit_threshold = stride * snap::SNAP_EPSILON_FRACTION;
                let i_floor = vc
                    .index_position
                    .floor()
                    .clamp(0.0, vc.max_start_index() as f32);
                let i_ceil = vc
                    .index_position
                    .ceil()
                    .clamp(0.0, vc.max_start_index() as f32);
                let x_floor = vc.index_to_scroll(i_floor);
                let x_ceil = vc.index_to_scroll(i_ceil);
                let df = (vc.scroll_x - x_floor).abs();
                let dc = (vc.scroll_x - x_ceil).abs();
                let (nearest_i, nearest_d) = if df <= dc {
                    (i_floor, df)
                } else {
                    (i_ceil, dc)
                };
                if nearest_d <= commit_threshold
                    && (vc.reference_index - nearest_i).abs() > 1e-4
                {
                    near_aligned = Some(nearest_i);
                }
            }

            // 2) Update settle tracking and commit only after a short stable dwell
            {
                let reg = &mut state.domains.ui.state.carousel_registry;
                match near_aligned {
                    Some(i) => {
                        let elapsed_ms =
                            reg.update_mouse_settle_candidate(&key, i);
                        let scroller_active = reg
                            .get_scroller(&key)
                            .map(|s| s.is_active())
                            .unwrap_or(false);
                        let animator_active = reg
                            .get_animator(&key)
                            .map(|a| a.is_active())
                            .unwrap_or(false);
                        let motion_idle =
                            matches!(reg.motion_state(&key), MotionState::Idle);
                        if !scroller_active
                            && !animator_active
                            && motion_idle
                            && (elapsed_ms as u64) >= snap::ANCHOR_SETTLE_MS
                        {
                            if let Some(vc) = reg.get_mut(&key) {
                                vc.set_reference_index(i);
                            }
                            // Reset settle tracking after commit
                            reg.clear_mouse_settle(&key);
                        }
                    }
                    None => {
                        // Not near a boundary; reset settle tracking
                        reg.clear_mouse_settle(&key);
                    }
                }
            }

            // 3) Save carousel scroll state to scroll manager for persistence
            if let Some(vc) = state.domains.ui.state.carousel_registry.get(&key)
            {
                let carousel_scroll_state = CarouselScrollState {
                    scroll_x: vc.scroll_x,
                    index_position: vc.index_position,
                    reference_index: vc.reference_index,
                    viewport_width: vc.viewport_width,
                };
                state
                    .domains
                    .ui
                    .state
                    .scroll_manager
                    .save_carousel_scroll(key.clone(), carousel_scroll_state);
            }

            match key {
                CarouselKey::ShowSeasons(series_uuid) => {
                    if let Some(handle) =
                        state.domains.metadata.state.planner_handle.as_ref()
                    {
                        let series_id = SeriesID::from(series_uuid);
                        if let Ok(seasons) = state
                            .domains
                            .ui
                            .state
                            .repo_accessor
                            .get_series_seasons(&series_id)
                        {
                            let total = seasons.len();
                            if let Some(vc) = state
                                .domains
                                .ui
                                .state
                                .carousel_registry
                                .get(&key)
                            {
                                #[cfg(any(
                                    feature = "profile-with-puffin",
                                    feature = "profile-with-tracy",
                                    feature = "profile-with-tracing",
                                ))]
                                profiling::scope!(
                                    profiling_scopes::scopes::CAROUSEL_SNAPSHOT
                                );
                                let snap = planner::snapshot_for_visible(
                                    vc,
                                    total,
                                    |i| seasons.get(i).map(|s| s.id.to_uuid()),
                                    Some(PosterKind::Season),
                                    None,
                                );
                                handle.send(snap);
                            }
                        }
                    }
                }
                CarouselKey::SeasonEpisodes(season_uuid) => {
                    if let Some(handle) =
                        state.domains.metadata.state.planner_handle.as_ref()
                    {
                        let season_id = SeasonID(season_uuid);
                        let episodes = state
                            .domains
                            .ui
                            .state
                            .repo_accessor
                            .get_season_episodes(&season_id)
                            .unwrap_or_else(|_| Vec::new());
                        let total = episodes.len();
                        if let Some(vc) =
                            state.domains.ui.state.carousel_registry.get(&key)
                        {
                            let (vis, mut pre, mut back) =
                                planner::collect_ranges_ids(vc, total, |i| {
                                    episodes.get(i).map(|e| e.id.to_uuid())
                                });
                            // Build context for all ids
                            // Combine and deduplicate
                            pre.retain(|id| !vis.contains(id));
                            back.retain(|id| {
                                !vis.contains(id) && !pre.contains(id)
                            });
                            let mut all = vis.clone();
                            all.extend(pre.iter().copied());
                            all.extend(back.iter().copied());
                            let ctx =
                                planner::build_episode_still_context(&all);
                            let snap = DemandSnapshot {
                                visible_ids: vis,
                                prefetch_ids: pre,
                                background_ids: back,
                                timestamp: Instant::now(),
                                context: Some(ctx),
                                poster_kind: None,
                            };
                            handle.send(snap);
                        }
                    }
                }
                CarouselKey::LibraryMovies(lib_uuid) => {
                    if let Some(handle) =
                        state.domains.metadata.state.planner_handle.as_ref()
                        && let Some(tab) = state
                            .tab_manager
                            .get_tab(TabId::Library(LibraryID(lib_uuid)))
                        && let TabState::Library(lib_state) = tab
                    {
                        let ids = &lib_state.cached_index_ids;
                        let total = ids.len();
                        if let Some(vc) =
                            state.domains.ui.state.carousel_registry.get(&key)
                        {
                            #[cfg(any(
                                feature = "profile-with-puffin",
                                feature = "profile-with-tracy",
                                feature = "profile-with-tracing",
                            ))]
                            profiling::scope!(
                                profiling_scopes::scopes::CAROUSEL_SNAPSHOT
                            );
                            let snap = planner::snapshot_for_visible(
                                vc,
                                total,
                                |i| ids.get(i).copied(),
                                Some(PosterKind::Movie),
                                None,
                            );
                            handle.send(snap);
                        }
                    }
                }
                CarouselKey::LibrarySeries(lib_uuid) => {
                    if let Some(handle) =
                        state.domains.metadata.state.planner_handle.as_ref()
                        && let Some(tab) = state
                            .tab_manager
                            .get_tab(TabId::Library(LibraryID(lib_uuid)))
                        && let TabState::Library(lib_state) = tab
                    {
                        let ids = &lib_state.cached_index_ids;
                        let total = ids.len();
                        if let Some(vc) =
                            state.domains.ui.state.carousel_registry.get(&key)
                        {
                            #[cfg(any(
                                feature = "profile-with-puffin",
                                feature = "profile-with-tracy",
                                feature = "profile-with-tracing",
                            ))]
                            profiling::scope!(
                                profiling_scopes::scopes::CAROUSEL_SNAPSHOT
                            );
                            let snap = planner::snapshot_for_visible(
                                vc,
                                total,
                                |i| ids.get(i).copied(),
                                Some(PosterKind::Series),
                                None,
                            );
                            handle.send(snap);
                        }
                    }
                }
                CarouselKey::Custom(name) => {
                    if let Some(handle) =
                        state.domains.metadata.state.planner_handle.as_ref()
                    {
                        // Access All tab curated lists
                        if let Some(TabState::All(all_state)) =
                            state.tab_manager.get_tab(TabId::All)
                        {
                            match name {
                                "ContinueWatching" => {
                                    let ids = &all_state.continue_watching;
                                    let total = ids.len();
                                    if let Some(vc) = state
                                        .domains
                                        .ui
                                        .state
                                        .carousel_registry
                                        .get(&key)
                                    {
                                        let (vis, mut pre, mut back) =
                                            planner::collect_ranges_ids(
                                                vc,
                                                total,
                                                |i| ids.get(i).copied(),
                                            );
                                        pre.retain(|id| !vis.contains(id));
                                        back.retain(|id| {
                                            !vis.contains(id)
                                                && !pre.contains(id)
                                        });

                                        let mut ctx = DemandContext::default();
                                        for id in vis.iter()
                                        //.chain(pre.iter())
                                        //.chain(back.iter())
                                        {
                                            if let Ok(media) = state
                                                .domains
                                                .ui
                                                .state
                                                .repo_accessor
                                                .get(&MediaID::Series(
                                                    SeriesID(*id),
                                                ))
                                                && matches!(
                                                    media.media_type(),
                                                    MediaType::Series
                                                )
                                            {
                                                ctx.override_request(
                                                        *id,
                                                        DemandRequestKind::Poster {
                                                            kind: PosterKind::Series,
                                                            size: PosterSize::Standard,
                                                        },
                                                    );
                                            }
                                        }
                                        let snap = DemandSnapshot {
                                            visible_ids: vis,
                                            prefetch_ids: pre,
                                            background_ids: back,
                                            timestamp: std::time::Instant::now(
                                            ),
                                            context: Some(ctx),
                                            poster_kind: Some(
                                                PosterKind::Movie,
                                            ),
                                        };
                                        handle.send(snap);
                                    }
                                }
                                "RecentlyAddedMovies" => {
                                    let ids = &all_state.recent_movies;
                                    let total = ids.len();
                                    if let Some(vc) = state
                                        .domains
                                        .ui
                                        .state
                                        .carousel_registry
                                        .get(&key)
                                    {
                                        let snap =
                                            planner::snapshot_for_visible(
                                                vc,
                                                total,
                                                |i| ids.get(i).copied(),
                                                Some(PosterKind::Movie),
                                                None,
                                            );
                                        handle.send(snap);
                                    }
                                }
                                "RecentlyAddedSeries" => {
                                    let ids = &all_state.recent_series;
                                    let total = ids.len();
                                    if let Some(vc) = state
                                        .domains
                                        .ui
                                        .state
                                        .carousel_registry
                                        .get(&key)
                                    {
                                        let snap =
                                            planner::snapshot_for_visible(
                                                vc,
                                                total,
                                                |i| ids.get(i).copied(),
                                                Some(PosterKind::Series),
                                                None,
                                            );
                                        handle.send(snap);
                                    }
                                }
                                "RecentlyReleasedMovies" => {
                                    let ids = &all_state.released_movies;
                                    let total = ids.len();
                                    if let Some(vc) = state
                                        .domains
                                        .ui
                                        .state
                                        .carousel_registry
                                        .get(&key)
                                    {
                                        let snap =
                                            planner::snapshot_for_visible(
                                                vc,
                                                total,
                                                |i| ids.get(i).copied(),
                                                Some(PosterKind::Movie),
                                                None,
                                            );
                                        handle.send(snap);
                                    }
                                }
                                "RecentlyReleasedSeries" => {
                                    let ids = &all_state.released_series;
                                    let total = ids.len();
                                    if let Some(vc) = state
                                        .domains
                                        .ui
                                        .state
                                        .carousel_registry
                                        .get(&key)
                                    {
                                        let snap =
                                            planner::snapshot_for_visible(
                                                vc,
                                                total,
                                                |i| ids.get(i).copied(),
                                                Some(PosterKind::Series),
                                                None,
                                            );
                                        handle.send(snap);
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
                _ => {}
            }
            Task::none()
        }
        VCM::NextPage(key) => start_snap_to_page(state, key, true),
        VCM::PrevPage(key) => start_snap_to_page(state, key, false),
        VCM::NextItem(key) => start_snap_to_step(state, key, true),
        VCM::PrevItem(key) => start_snap_to_step(state, key, false),
        VCM::FocusKey(key) => {
            // Only allow hover to change focus if there has been recent mouse movement
            let now = Instant::now();
            let allow =
                state.domains.ui.state.carousel_focus.has_recent_mouse_move(
                    now,
                    virtual_carousel::focus::HOVER_SWITCH_WINDOW_MS,
                );
            if !allow {
                return Task::none();
            }

            // Set this carousel as hovered and mark mouse as the last focus source
            state
                .domains
                .ui
                .state
                .carousel_focus
                .activate_hovered(key.clone());
            // Sync All view vertical focus so Up/Down starts from hovered row
            if matches!(
                state.domains.ui.state.display_mode,
                DisplayMode::Curated
            ) && matches!(state.tab_manager.active_tab_id(), TabId::All)
                && let Some(TabState::All(all_state)) =
                    state.tab_manager.get_tab_mut(TabId::All)
            {
                all_state.focus.active_carousel = Some(key);
            }
            Task::none()
        }
        VCM::BlurKey(_key) => {
            // Intentionally do not clear hover here to preserve stickiness when the cursor leaves the window.
            // Hover will switch immediately on the next FocusKey from a different carousel.
            Task::none()
        }
        VCM::NextPageActive => {
            let Some(key) = active_carousel_key(state) else {
                return Task::none();
            };
            start_snap_to_page(state, key, true)
        }
        VCM::PrevPageActive => {
            let Some(key) = active_carousel_key(state) else {
                return Task::none();
            };
            start_snap_to_page(state, key, false)
        }
        VCM::NextItemActive => {
            let Some(key) = active_carousel_key(state) else {
                return Task::none();
            };
            start_snap_to_step(state, key, true)
        }
        VCM::PrevItemActive => {
            let Some(key) = active_carousel_key(state) else {
                return Task::none();
            };
            start_snap_to_step(state, key, false)
        }
        VCM::StartRightActive => {
            let Some(key) = active_carousel_key(state) else {
                return Task::none();
            };
            let scroller = ensure_scroller_for_key(state, &key);
            scroller.start(1);

            // Cancel any active snap animation to avoid conflicting motions
            if let Some(anim) = state
                .domains
                .ui
                .state
                .carousel_registry
                .get_animator_mut(&key)
            {
                anim.cancel();
            }

            // Record hold start index and offset for displacement-based heuristics
            let maybe_vals = state
                .domains
                .ui
                .state
                .carousel_registry
                .get(&key)
                .map(|vc| (vc.index_position, vc.scroll_x));
            if let Some((idx, sx)) = maybe_vals {
                state
                    .domains
                    .ui
                    .state
                    .carousel_registry
                    .begin_hold_index_at(&key, idx);
                state
                    .domains
                    .ui
                    .state
                    .carousel_registry
                    .begin_hold_at(&key, sx);
            } else {
                state.domains.ui.state.carousel_registry.begin_hold(&key);
            }
            state
                .domains
                .ui
                .state
                .carousel_registry
                .set_motion_state(&key, MotionState::Kinetic(1));
            Task::none()
        }
        VCM::StartLeftActive => {
            let Some(key) = active_carousel_key(state) else {
                return Task::none();
            };
            let scroller = ensure_scroller_for_key(state, &key);
            scroller.start(-1);

            // Cancel any active snap animation to avoid conflicting motions
            if let Some(anim) = state
                .domains
                .ui
                .state
                .carousel_registry
                .get_animator_mut(&key)
            {
                anim.cancel();
            }

            let maybe_vals = state
                .domains
                .ui
                .state
                .carousel_registry
                .get(&key)
                .map(|vc| (vc.index_position, vc.scroll_x));

            if let Some((idx, sx)) = maybe_vals {
                state
                    .domains
                    .ui
                    .state
                    .carousel_registry
                    .begin_hold_index_at(&key, idx);
                state
                    .domains
                    .ui
                    .state
                    .carousel_registry
                    .begin_hold_at(&key, sx);
            } else {
                state.domains.ui.state.carousel_registry.begin_hold(&key);
            }

            state
                .domains
                .ui
                .state
                .carousel_registry
                .set_motion_state(&key, MotionState::Kinetic(-1));
            Task::none()
        }
        VCM::StopRightActive => {
            let Some(key) = active_carousel_key(state) else {
                return Task::none();
            };

            if let Some(scroller) = state
                .domains
                .ui
                .state
                .carousel_registry
                .get_scroller_mut(&key)
            {
                scroller.stop_holding(1);
                // Prevent kinetic decay from competing with the snap animation
                scroller.abort();
            }

            handle_release_snap_align(state, key, 1)
        }
        VCM::StopLeftActive => {
            let Some(key) = active_carousel_key(state) else {
                return Task::none();
            };

            if let Some(scroller) = state
                .domains
                .ui
                .state
                .carousel_registry
                .get_scroller_mut(&key)
            {
                scroller.stop_holding(-1);
                // Prevent kinetic decay from competing with the snap animation
                scroller.abort();
            }

            handle_release_snap_align(state, key, -1)
        }
        VCM::SetBoostActive(active) => {
            let Some(key) = active_carousel_key(state) else {
                return Task::none();
            };

            if let Some(scroller) = state
                .domains
                .ui
                .state
                .carousel_registry
                .get_scroller_mut(&key)
            {
                scroller.set_boost(active);
            }

            Task::none()
        }
        VCM::MotionTickActive => {
            let Some(key) = active_carousel_key(state) else {
                return Task::none();
            };

            // Gather immutable data first to avoid multiple mutable borrows
            let (stride, current_x, max_scroll, scroll_id);
            {
                let Some(vc) =
                    state.domains.ui.state.carousel_registry.get(&key)
                else {
                    return Task::none();
                };
                stride = (vc.item_width + vc.item_spacing).max(1.0);
                current_x = vc.scroll_x;
                max_scroll = vc.max_scroll;
                scroll_id = vc.scrollable_id.clone();
            }

            // Prefer active snap animator, else kinetic scroller
            let mut used_next_scroll: Option<f32> = None;
            let mut finished_snap = false;

            if let Some(anim) = state
                .domains
                .ui
                .state
                .carousel_registry
                .get_animator_mut(&key)
                && anim.is_active()
            {
                let n = anim.tick();
                finished_snap = n.is_some() && !anim.is_active();
                used_next_scroll = n;
            }

            if used_next_scroll.is_none()
                && let Some(scroller) = state
                    .domains
                    .ui
                    .state
                    .carousel_registry
                    .get_scroller_mut(&key)
            {
                // Integrate in pixel space (units = item stride in px)
                if let Some(next_x) =
                    scroller.tick(current_x, stride, max_scroll)
                {
                    if let Some(vc) =
                        state.domains.ui.state.carousel_registry.get_mut(&key)
                    {
                        vc.set_scroll_x(next_x);
                    }
                    return scroll_to::<ui::Message>(
                        scroll_id,
                        AbsoluteOffset { x: next_x, y: 0.0 },
                    );
                }

                // Kinetic inactive
                state
                    .domains
                    .ui
                    .state
                    .carousel_registry
                    .clear_motion_state(&key);
            }

            if let Some(next_scroll) = used_next_scroll {
                // Capture the target index before clearing the motion state
                let finished_target_index = if finished_snap {
                    match state
                        .domains
                        .ui
                        .state
                        .carousel_registry
                        .motion_state(&key)
                    {
                        MotionState::Snap { target_index, .. } => {
                            Some(target_index)
                        }
                        _ => None,
                    }
                } else {
                    None
                };

                if let Some(vc) =
                    state.domains.ui.state.carousel_registry.get_mut(&key)
                {
                    vc.set_scroll_x(next_scroll);
                    if let Some(ref_i) = finished_target_index {
                        vc.set_reference_index(ref_i);
                    }
                }

                if finished_snap {
                    state
                        .domains
                        .ui
                        .state
                        .carousel_registry
                        .clear_motion_state(&key);
                }

                maybe_send_snapshot_for_key(state, &key, finished_snap);
                return scroll_to::<ui::Message>(
                    scroll_id,
                    AbsoluteOffset {
                        x: next_scroll,
                        y: 0.0,
                    },
                );
            }
            Task::none()
        }
    }
}

fn ensure_scroller_for_key<'a>(
    state: &'a mut State,
    key: &CarouselKey,
) -> &'a mut MotionController {
    let cfg = MotionControllerConfig {
        tick_ns: motion::TICK_NS,
        accel_tau_ms: 0, // derive from ramp ratio
        accel_tau_to_ramp_ratio: 0.4,
        decay_tau_ms: motion::DECAY_TAU_MS,
        base_units_per_s: motion::BASE_ITEMS_PER_S,
        max_units_per_s: motion::MAX_ITEMS_PER_S,
        min_units_per_s_stop: 0.08,
        ramp_ms: motion::RAMP_MS,
        easing_kind: motion::EASING_KIND,
        boost_multiplier: motion::BOOST_MULTIPLIER,
    };

    state
        .domains
        .ui
        .state
        .carousel_registry
        .ensure_scroller_with_config(key, cfg)
}

fn active_carousel_key(state: &State) -> Option<CarouselKey> {
    // Priority 1: Prefer hover if last source was mouse, or there was recent mouse movement
    let now = Instant::now();
    if state.domains.ui.state.carousel_focus.should_prefer_hover(
        now,
        virtual_carousel::focus::HOVER_SWITCH_WINDOW_MS,
    ) && let Some(hovered_key) =
        &state.domains.ui.state.carousel_focus.hovered_key
    {
        return Some(hovered_key.clone());
    }

    // Priority 2: Check if a carousel has explicit keyboard focus
    // This is set by Up/Down arrow keys in All view, or as a side effect of chevron navigation
    if let Some(keyboard_key) =
        &state.domains.ui.state.carousel_focus.keyboard_active_key
    {
        return Some(keyboard_key.clone());
    }

    // Priority 3: Fall back to view-specific defaults
    match state.domains.ui.state.view.clone() {
        crate::domains::ui::types::ViewState::SeriesDetail {
            series_id,
            ..
        } => Some(CarouselKey::ShowSeasons(series_id.to_uuid())),
        crate::domains::ui::types::ViewState::SeasonDetail {
            season_id,
            ..
        } => Some(CarouselKey::SeasonEpisodes(season_id.to_uuid())),
        crate::domains::ui::types::ViewState::Library
            if matches!(
                state.domains.ui.state.display_mode,
                crate::domains::ui::types::DisplayMode::Curated
            ) =>
        {
            if let Some(crate::domains::ui::tabs::TabState::All(all_state)) =
                state
                    .tab_manager
                    .get_tab(crate::domains::ui::tabs::TabId::All)
            {
                all_state.focus.active_carousel.clone()
            } else {
                None
            }
        }
        _ => None,
    }
}

fn start_snap_to_page(
    state: &mut State,
    key: CarouselKey,
    to_right: bool,
) -> Task<ui::Message> {
    // Set keyboard focus to this carousel when user presses page chevrons
    state
        .domains
        .ui
        .state
        .carousel_focus
        .set_keyboard_active(Some(key.clone()));
    // Keep All view's vertical focus in sync so Up/Down moves from this row
    if matches!(
        state.domains.ui.state.display_mode,
        crate::domains::ui::types::DisplayMode::Curated
    ) && matches!(state.tab_manager.active_tab_id(), TabId::All)
        && let Some(TabState::All(all_state)) =
            state.tab_manager.get_tab_mut(TabId::All)
    {
        all_state.focus.active_carousel = Some(key.clone());
    }
    let (current_x, target_x, target_index): (f32, f32, f32);
    let (easing, duration_ms): (u8, u64);
    let (stride, max_scroll): (f32, f32);

    let _scroll_id: iced::widget::Id;
    {
        let Some(vc) = state.domains.ui.state.carousel_registry.get(&key)
        else {
            return Task::none();
        };
        current_x = vc.scroll_x;
        // Page calculations use a stable reference index to avoid drifting bases after holds.
        let ti = if to_right {
            vc.page_right_index_target()
        } else {
            vc.page_left_index_target()
        };
        target_index = ti;
        target_x = vc.index_to_scroll(ti);
        easing = snap::EASING_KIND;
        duration_ms = snap::PAGE_DURATION_MS;
        _scroll_id = vc.scrollable_id.clone();
        stride = (vc.item_width + vc.item_spacing).max(1.0);
        max_scroll = vc.max_scroll;
    }

    // Set up animator
    {
        // Allow tiny moves if they land on the end boundary
        let boundary_eps = stride * snap::SNAP_EPSILON_FRACTION;
        let near_end_target = (max_scroll - target_x).abs() <= boundary_eps;
        if (target_x - current_x).abs() <= boundary_eps && !near_end_target {
            return Task::none();
        }
    }

    // Always animate snaps across all views (All and Details)
    let anim = state
        .domains
        .ui
        .state
        .carousel_registry
        .ensure_animator(&key);
    anim.start(current_x, target_x, duration_ms, easing);
    state.domains.ui.state.carousel_registry.set_motion_state(
        &key,
        MotionState::Snap {
            target_index,
            target_x,
        },
    );
    maybe_send_snapshot_for_key(state, &key, true);
    Task::none()
}

fn start_snap_to_step(
    state: &mut State,
    key: CarouselKey,
    to_right: bool,
) -> Task<ui::Message> {
    // Set keyboard focus to this carousel when user presses chevron buttons
    state
        .domains
        .ui
        .state
        .carousel_focus
        .set_keyboard_active(Some(key.clone()));
    // Keep All view's vertical focus in sync so Up/Down moves from this row
    if matches!(state.domains.ui.state.display_mode, DisplayMode::Curated)
        && matches!(state.tab_manager.active_tab_id(), TabId::All)
        && let Some(TabState::All(all_state)) =
            state.tab_manager.get_tab_mut(TabId::All)
    {
        all_state.focus.active_carousel = Some(key.clone());
    }

    let current_x: f32;
    let target_x: f32;
    let target_index: f32;
    let easing: u8;
    let duration_ms: u64;
    let stride: f32;
    let near_end_target: bool;
    let _scroll_id: iced::widget::Id;
    {
        let Some(vc) = state.domains.ui.state.carousel_registry.get(&key)
        else {
            return Task::none();
        };
        current_x = vc.scroll_x;
        let motion =
            state.domains.ui.state.carousel_registry.motion_state(&key);
        let eps = 1e-4;
        stride = (vc.item_width + vc.item_spacing).max(1.0);
        let end_eps = stride * snap::SNAP_EPSILON_FRACTION;

        // If already at or targeting the end, ignore further right steps
        if to_right {
            let at_end = (vc.max_scroll - vc.scroll_x).abs() <= end_eps
                || matches!(motion, MotionState::Snap { target_x, .. } if (vc.max_scroll - target_x).abs() <= end_eps);
            if at_end {
                return Task::none();
            }
        }

        // Base steps on the last committed reference to avoid hold-induced drift.
        // If we're currently snapping, stack on the active snap's target.
        let base_index = match motion {
            MotionState::Snap { target_index, .. } => target_index,
            _ => vc.reference_index,
        };

        let tentative = if to_right {
            (base_index + 1.0).min(vc.max_start_index() as f32)
        } else {
            (base_index - 1.0).max(0.0)
        };

        target_index = tentative;

        // End-of-carousel nuance for single-item steps (state-driven)
        if to_right
            && (target_index - vc.max_start_index() as f32).abs() <= 1e-4
        {
            let max_aligned = vc.max_aligned_scroll();
            let remainder = (vc.max_scroll - max_aligned).max(0.0);

            match motion {
                MotionState::Snap {
                    target_x: prev_tx, ..
                } => {
                    if (vc.max_scroll - prev_tx).abs() <= end_eps {
                        return Task::none(); // already targeting end; no toggle
                    }
                    if (prev_tx - max_aligned).abs() <= eps {
                        // Second tap while snapping to last aligned -> decide end or stay
                        // Previous threshold used item_spacing, which prevented ever choosing
                        // the right-aligned end in common layouts. Any non-zero remainder
                        // means there is additional content to reveal to the right, so
                        // prefer max_scroll when remainder is meaningful (> eps).
                        target_x = if remainder > eps {
                            vc.max_scroll
                        } else {
                            max_aligned
                        };
                    } else {
                        // First time approaching the last aligned boundary
                        target_x = max_aligned;
                    }
                }
                _ => {
                    // Not snapping: compute from current alignment
                    let at_left_aligned_max =
                        (vc.scroll_x - max_aligned).abs() <= eps;
                    if at_left_aligned_max {
                        // If there's any remainder beyond the last aligned boundary,
                        // allow transitioning to the true right-aligned end.
                        target_x = if remainder > eps {
                            vc.max_scroll
                        } else {
                            max_aligned
                        };
                    } else {
                        target_x = max_aligned;
                    }
                }
            }
        } else {
            target_x = vc.index_to_scroll(target_index);
        }

        easing = snap::EASING_KIND;
        duration_ms = snap::ITEM_DURATION_MS;
        near_end_target = (vc.max_scroll - target_x).abs() <= end_eps;
        _scroll_id = vc.scrollable_id.clone();
    }

    // If there is effectively no movement and not targeting the end boundary, do nothing
    {
        let boundary_eps = stride * snap::SNAP_EPSILON_FRACTION;
        if (target_x - current_x).abs() <= boundary_eps && !near_end_target {
            return Task::none();
        }
    }

    // Always animate snaps across all views (All and Details)
    let anim = state
        .domains
        .ui
        .state
        .carousel_registry
        .ensure_animator(&key);
    anim.start(current_x, target_x, duration_ms, easing);
    state.domains.ui.state.carousel_registry.set_motion_state(
        &key,
        MotionState::Snap {
            target_index,
            target_x,
        },
    );
    maybe_send_snapshot_for_key(state, &key, true);
    Task::none()
}

fn handle_release_snap_align(
    state: &mut State,
    key: CarouselKey,
    dir: i32,
) -> Task<ui::Message> {
    let held_ms = state
        .domains
        .ui
        .state
        .carousel_registry
        .end_hold_elapsed_ms(&key)
        .unwrap_or(0);

    // Gather current state
    let (current_index, _, current_x, stride, max_scroll) = {
        let Some(vc) = state.domains.ui.state.carousel_registry.get(&key)
        else {
            return Task::none();
        };
        (
            vc.index_position,
            vc.max_start_index() as f32,
            vc.scroll_x,
            (vc.item_width + vc.item_spacing).max(1.0),
            vc.max_scroll,
        )
    };

    let end_eps = stride * snap::SNAP_EPSILON_FRACTION;

    // Displacement-based heuristic: consider how far we moved during the hold
    let moved_units = state
        .domains
        .ui
        .state
        .carousel_registry
        .end_hold_moved_units(&key, current_x, stride)
        .unwrap_or(0.0);

    if moved_units.abs() < 0.5 || (held_ms as u64) < snap::HOLD_TAP_THRESHOLD_MS
    {
        // Treat as tap: single step in the direction (robust to timing jitter)
        return start_snap_to_step(state, key, dir > 0);
    }

    // Align toward the direction of travel to avoid bounce/stutter with end nuance
    let (target_index, target_x) = {
        let Some(vc) = state.domains.ui.state.carousel_registry.get(&key)
        else {
            return Task::none();
        };
        let eps = 1e-4;
        if dir > 0 {
            if (max_scroll - current_x).abs() <= end_eps {
                return Task::none(); // already at end
            }
            let max_i = vc.max_start_index() as f32;
            if current_index >= max_i - eps {
                // Near end: decide end or last aligned based on remainder
                let max_aligned = vc.max_aligned_scroll();
                let remainder = (vc.max_scroll - max_aligned).max(0.0);
                // If there is any remainder beyond the last aligned boundary,
                // snap to the right-aligned end. Using item_spacing here
                // caused the carousel to stick left-aligned at the end.
                let tx = if remainder > eps {
                    vc.max_scroll
                } else {
                    max_aligned
                };
                (max_i, tx)
            } else {
                let ni = current_index.ceil().min(max_i);
                (ni, vc.index_to_scroll(ni))
            }
        } else if dir < 0 {
            let ni = current_index.floor().max(0.0);
            (ni, vc.index_to_scroll(ni))
        } else {
            let ni = current_index
                .round()
                .clamp(0.0, vc.max_start_index() as f32);
            (ni, vc.index_to_scroll(ni))
        }
    };

    let anim = state
        .domains
        .ui
        .state
        .carousel_registry
        .ensure_animator(&key);

    anim.start(
        current_x,
        target_x,
        snap::ITEM_DURATION_MS,
        snap::EASING_KIND,
    );

    state.domains.ui.state.carousel_registry.set_motion_state(
        &key,
        MotionState::Snap {
            target_index,
            target_x,
        },
    );

    maybe_send_snapshot_for_key(state, &key, true);
    Task::none()
}

fn maybe_send_snapshot_for_key(
    state: &mut State,
    key: &CarouselKey,
    force: bool,
) {
    if let Some(handle) = state.domains.metadata.state.planner_handle.as_ref() {
        let emit = if force {
            true
        } else {
            state
                .domains
                .ui
                .state
                .carousel_registry
                .should_emit_snapshot(
                    key,
                    Duration::from_millis(snap::SNAPSHOT_DEBOUNCE_MS),
                )
        };
        if !emit {
            return;
        }

        match key {
            CarouselKey::ShowSeasons(series_uuid) => {
                let series_id = SeriesID::from(*series_uuid);
                if let Ok(seasons) = state
                    .domains
                    .ui
                    .state
                    .repo_accessor
                    .get_series_seasons(&series_id)
                    && let Some(vc) =
                        state.domains.ui.state.carousel_registry.get(key)
                {
                    let total = seasons.len();
                    let snap = planner::snapshot_for_visible(
                        vc,
                        total,
                        |i| seasons.get(i).map(|s| s.id.to_uuid()),
                        Some(PosterKind::Season),
                        None,
                    );
                    handle.send(snap);
                }
            }
            CarouselKey::LibraryMovies(lib_uuid) => {
                if let Some(vc) =
                    state.domains.ui.state.carousel_registry.get(key)
                    && let Some(tab) = state
                        .tab_manager
                        .get_tab(TabId::Library(LibraryID(*lib_uuid)))
                    && let TabState::Library(lib_state) = tab
                {
                    let ids = &lib_state.cached_index_ids;
                    let total = ids.len();
                    #[cfg(any(
                        feature = "profile-with-puffin",
                        feature = "profile-with-tracy",
                        feature = "profile-with-tracing",
                    ))]
                    profiling::scope!(
                        profiling_scopes::scopes::CAROUSEL_SNAPSHOT
                    );
                    let snap = planner::snapshot_for_visible(
                        vc,
                        total,
                        |i| ids.get(i).copied(),
                        Some(PosterKind::Movie),
                        None,
                    );
                    handle.send(snap);
                }
            }
            CarouselKey::LibrarySeries(lib_uuid) => {
                if let Some(vc) =
                    state.domains.ui.state.carousel_registry.get(key)
                    && let Some(tab) = state
                        .tab_manager
                        .get_tab(TabId::Library(LibraryID(*lib_uuid)))
                    && let TabState::Library(lib_state) = tab
                {
                    let ids = &lib_state.cached_index_ids;
                    let total = ids.len();
                    #[cfg(any(
                        feature = "profile-with-puffin",
                        feature = "profile-with-tracy",
                        feature = "profile-with-tracing",
                    ))]
                    profiling::scope!(
                        profiling_scopes::scopes::CAROUSEL_SNAPSHOT
                    );
                    let snap = planner::snapshot_for_visible(
                        vc,
                        total,
                        |i| ids.get(i).copied(),
                        Some(PosterKind::Series),
                        None,
                    );
                    handle.send(snap);
                }
            }
            CarouselKey::SeasonEpisodes(season_uuid) => {
                let season_id = SeasonID(*season_uuid);
                let episodes = state
                    .domains
                    .ui
                    .state
                    .repo_accessor
                    .get_season_episodes(&season_id)
                    .unwrap_or_else(|_| Vec::new());
                if let Some(vc) =
                    state.domains.ui.state.carousel_registry.get(key)
                {
                    let total = episodes.len();
                    let (vis, mut pre, mut back) =
                        planner::collect_ranges_ids(vc, total, |i| {
                            episodes.get(i).map(|e| e.id.to_uuid())
                        });
                    pre.retain(|id| !vis.contains(id));
                    back.retain(|id| !vis.contains(id) && !pre.contains(id));
                    let mut all = vis.clone();
                    all.extend(pre.iter().copied());
                    all.extend(back.iter().copied());
                    let ctx = planner::build_episode_still_context(&all);
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
            _ => {}
        }
    }
}
