use crate::{
    common::messages::{CrossDomainEvent, DomainMessage, DomainUpdateResult},
    domains::{
        library,
        metadata::demand_planner::DemandSnapshot,
        settings::messages::Message as SettingsMessage,
        ui::{
            messages::Message as UiMessage,
            tabs::{TabId, TabState},
            types::{BackdropAspectMode, DisplayMode, ViewState},
            update_handlers::{
                emit_initial_all_tab_snapshots_combined,
                handle_virtual_carousel_message, init_all_tab_view,
            },
            utils::bump_keep_alive,
            windows,
        },
    },
    infra::{api_types::LibraryType, constants::layout},
    state::State,
};

use ferrex_core::{
    player_prelude::{
        EpisodeLike, Media, MediaTypeFilter, MovieLike, PosterKind, SortOrder,
        UiResolution, UiWatchStatus,
    },
    query::filtering::{
        FilterRequestParams, build_filter_indices_request, hash_filter_spec,
    },
};

use iced::{
    Task,
    widget::{operation::scroll_to, scrollable::AbsoluteOffset},
};
use std::time::Instant;

#[cfg(feature = "demo")]
use crate::domains::ui::update_handlers::demo_controls;

pub fn update_ui(state: &mut State, message: UiMessage) -> DomainUpdateResult {
    match message {
        UiMessage::OpenSearchWindow => {
            windows::controller::open_search(state, None)
        }
        UiMessage::OpenSearchWindowWithSeed(seed) => {
            windows::controller::open_search(state, Some(seed))
        }
        UiMessage::SearchWindowOpened(id) => {
            state.search_window_id = Some(id);
            windows::controller::on_search_opened(state, id)
        }
        UiMessage::MainWindowOpened(id) => {
            state
                .windows
                .set(windows::WindowKind::Main, id);
            DomainUpdateResult::task(Task::none())
        }
        UiMessage::MainWindowFocused => {
            // When regaining focus, re-emit initial snapshots to ensure images load
            super::update_handlers::all_tab::init_all_tab_view(state);
            super::update_handlers::all_tab::emit_initial_all_tab_snapshots_combined(state);
            bump_keep_alive(state);
            DomainUpdateResult::task(Task::none())
        }
        UiMessage::MainWindowUnfocused => {
            // No special handling currently; keep behavior simple
            DomainUpdateResult::task(Task::none())
        }
        UiMessage::RawWindowClosed(id) => {
            windows::controller::on_raw_window_closed(state, id)
        }
        UiMessage::FocusSearchWindow => {
            windows::controller::focus_search(state)
        }
        UiMessage::FocusSearchInput => {
            windows::controller::focus_search_input(state)
        }
        UiMessage::CloseSearchWindow => {
            windows::controller::close_search(state)
        }
        UiMessage::SetDisplayMode(display_mode) => {
            state.domains.ui.state.display_mode = display_mode;

            match display_mode {
                DisplayMode::Curated => {
                    state.tab_manager.set_active_tab_with_scroll(
                        TabId::All,
                        &mut state.domains.ui.state.scroll_manager,
                        state.window_size.width,
                    );
                    state.tab_manager.set_active_sort(
                        state.domains.ui.state.sort_by,
                        state.domains.ui.state.sort_order,
                    );
                    log::info!("Tab activated: All (Curated mode)");

                    // Show all libraries in curated view
                    state.domains.ui.state.current_library_id = None;

                    // Initialize All tab (curated + per-library) and emit initial snapshots
                    init_all_tab_view(state);
                    emit_initial_all_tab_snapshots_combined(state);

                    // Build focus order and set initial active carousel (no mutable borrow)
                    let ordered = super::tabs::ordered_keys_for_all_view(state);
                    if let Some(TabState::All(all_state)) = state.tab_manager.get_tab_mut(TabId::All) {
                        all_state.focus.ordered_keys = ordered;
                        if all_state.focus.active_carousel.is_none() {
                            all_state.focus.active_carousel = all_state.focus.ordered_keys.first().cloned();
                        }
                        // Initialize carousel keyboard focus to match the initial All focus, so highlight/keys work without hover
                        if state.domains.ui.state.carousel_focus.get_active_key().is_none() {
                            if let Some(k) = all_state.focus.active_carousel.clone() {
                                state.domains.ui.state.carousel_focus.set_keyboard_active(Some(k));
                            }
                        }
                    }

                    // Keep UI alive while initial poster fetch/uploads start
                    bump_keep_alive(state);

                    // Restore horizontal scroll positions for All-tab carousels (if saved)
                    let restore_task = super::update_handlers::virtual_carousel_helpers::restore_all_tab_carousel_scroll_positions(state)
                        .map(DomainMessage::Ui);
                    return DomainUpdateResult::task(restore_task);

                }
                DisplayMode::Library => {
                    // Show current library
                    let library_id = state.domains.ui.state.current_library_id;

                    if let Some(lib_id) = library_id {
                        state.tab_manager.set_active_tab_with_scroll(
                            TabId::Library(lib_id),
                            &mut state.domains.ui.state.scroll_manager,
                            state.window_size.width,
                        );
                        state.tab_manager.set_active_sort(
                            state.domains.ui.state.sort_by,
                            state.domains.ui.state.sort_order,
                        );
                        log::info!("Tab activated: Library({}) (Library mode)", lib_id);

                        if let Some(handle) =
                            state.domains.metadata.state.planner_handle.as_ref()
                            && let TabState::Library(
                                lib_state,
                            ) = state.tab_manager.active_tab()
                            {
                                let now = std::time::Instant::now();
                                let mut visible_ids: Vec<uuid::Uuid> = Vec::new();
                                let vr =
                                    lib_state.grid_state.visible_range.clone();
                                if let Some(slice) =
                                    lib_state.cached_index_ids.get(vr)
                                {
                                    visible_ids
                                        .extend(slice.iter().copied());
                                }
                                let pr = lib_state.grid_state
                                    .get_preload_range(layout::virtual_grid::PREFETCH_ROWS_ABOVE);
                                let mut prefetch_ids: Vec<uuid::Uuid> =
                                    Vec::new();
                                if let Some(slice) =
                                    lib_state.cached_index_ids.get(pr)
                                {
                                    prefetch_ids
                                        .extend(slice.iter().copied());
                                }
                                prefetch_ids
                                    .retain(|id| !visible_ids.contains(id));
                                let br = lib_state.grid_state.get_background_range(
                                    layout::virtual_grid::PREFETCH_ROWS_ABOVE,
                                    layout::virtual_grid::BACKGROUND_ROWS_BELOW,
                                );
                                let mut background_ids: Vec<uuid::Uuid> =
                                    Vec::new();
                                if let Some(slice) =
                                    lib_state.cached_index_ids.get(br)
                                {
                                    background_ids
                                        .extend(slice.iter().copied());
                                }
                                background_ids.retain(|id| {
                                    !visible_ids.contains(id)
                                        && !prefetch_ids.contains(id)
                                });
                                let poster_kind = match lib_state.library_type {
                                    LibraryType::Movies => {
                                        Some(PosterKind::Movie)
                                    }
                                    LibraryType::Series => {
                                        Some(PosterKind::Series)
                                    }
                                };
                                handle.send(DemandSnapshot {
                                    visible_ids,
                                    prefetch_ids,
                                    background_ids,
                                    timestamp: now,
                                    context: None,
                                    poster_kind,
                                });
                            }

                        return DomainUpdateResult::task(Task::none());
                    }
                }
                _ => {
                    // Other modes not implemented yet
                    log::info!("Display mode {:?} not implemented yet", display_mode);
                }
            }

            // Refresh views
            //state.all_view_model.refresh_from_store();

            // NEW ARCHITECTURE: Also refresh the active tab
            state.tab_manager.refresh_active_tab();

            // Only broadcast scope changes appropriate to the selected mode.
            // - Curated: tell other domains we're in the global (all libraries) scope.
            // - Library: do NOT emit LibrarySelectAll here or it will immediately
            //   reset selection back to All and flip the tab, causing a flicker.
            match display_mode {
                DisplayMode::Curated => {
                    DomainUpdateResult::with_events(
                        Task::none(),
                        vec![CrossDomainEvent::LibrarySelectAll],
                    )
                }
                DisplayMode::Library => {
                    // Library scope is set via SelectLibraryAndMode/LibrarySelected flow.
                    // Emitting LibrarySelectAll here would clobber selection.
                    DomainUpdateResult::task(Task::none())
                }
                _ => DomainUpdateResult::task(Task::none()),
            }

        }
        UiMessage::SelectLibraryAndMode(library_id) => {

            state.tab_manager.set_active_tab_with_scroll(
                TabId::Library(library_id),
                &mut state.domains.ui.state.scroll_manager,
                state.window_size.width,
            );
            state.tab_manager.set_active_sort(
                state.domains.ui.state.sort_by,
                state.domains.ui.state.sort_order,
            );
            log::info!("Tab activated: Library({})", library_id);

            // Create scroll restoration task for the newly active tab
            // Note: We ignore the scroll_to result since it returns () and we just need to trigger the scroll
            let scroll_task = {
                let tab_id = TabId::Library(library_id);
                if let Some(tab) = state.tab_manager.get_tab(tab_id) {
                    if let TabState::Library(lib_state) = tab {
                        // Restore scroll position (or snap to 0 if no position stored)
                        let scroll_position = lib_state.grid_state.scroll_position;
                        let scrollable_id = lib_state.grid_state.scrollable_id.clone();
                        log::debug!(
                            "Restoring scroll position {} for library {}",
                            scroll_position,
                            library_id
                        );
                        scroll_to::<DomainMessage>(
                            scrollable_id,
                            AbsoluteOffset {
                                x: 0.0,
                                y: scroll_position,
                            },
                        )
                    } else {
                        Task::none()
                    }
                } else {
                    // Tab doesn't exist yet, will be created with scroll position 0
                    Task::none()
                }
            };

            // Don't change display mode yet - wait for library domain to update
            // The library domain will emit LibraryChanged event after updating its state,
            // which will trigger the display mode change and UpdateViewModelFilters
            DomainUpdateResult::with_events(
                scroll_task,
                vec![CrossDomainEvent::LibrarySelected(library_id)],
            )
        }
        UiMessage::ViewDetails(media) => {
            let task =
                super::update_handlers::navigation_updates::handle_view_details(state, media);
            DomainUpdateResult::task(task.map(DomainMessage::Ui))
        }
        UiMessage::ViewMovieDetails(movie_ref) => {
            let task = super::update_handlers::navigation_updates::handle_view_movie_details(
                state, movie_ref,
            );
            DomainUpdateResult::task(task.map(DomainMessage::Ui))
        }
        UiMessage::ViewTvShow(series_id) => {
            let task =
                super::update_handlers::navigation_updates::handle_view_series(state, series_id);
            DomainUpdateResult::task(task.map(DomainMessage::Ui))
        }
        UiMessage::ViewSeason(series_id, season_id) => {
            let task = super::update_handlers::navigation_updates::handle_view_season(
                state, series_id, season_id,
            );
            DomainUpdateResult::task(task.map(DomainMessage::Ui))
        }
        UiMessage::ViewEpisode(episode_id) => {
            let task =
                super::update_handlers::navigation_updates::handle_view_episode(state, episode_id);
            DomainUpdateResult::task(task.map(DomainMessage::Ui))
        }
        UiMessage::SetSortBy(sort_by) => {
            bump_keep_alive(state);
            // Update UI sort state and immediately refresh the active tab
            // to keep the grid populated while filtered indices are fetched.
            state.domains.ui.state.sort_by = sort_by;
            state
                .tab_manager
                .set_active_sort(sort_by, state.domains.ui.state.sort_order);

            // Ensure the grid reflects the new sort right away
            state.tab_manager.refresh_active_tab();


            let fetch_task = Task::done(DomainMessage::Ui(
                UiMessage::RequestFilteredPositions,
            ));
            if let Some(handle) = state.domains.metadata.state.planner_handle.as_ref()
                && let TabState::Library(lib_state) = state.tab_manager.active_tab() {
                    let now = std::time::Instant::now();
                    let mut visible_ids: Vec<uuid::Uuid> = Vec::new();
                    let vr = lib_state.grid_state.visible_range.clone();
                    if let Some(slice) = lib_state.cached_index_ids.get(vr) {
                        visible_ids.extend(slice.iter().copied());
                    }
                    let pr = lib_state
                        .grid_state
                        .get_preload_range(layout::virtual_grid::PREFETCH_ROWS_ABOVE);
                    let mut prefetch_ids: Vec<uuid::Uuid> = Vec::new();
                    if let Some(slice) = lib_state.cached_index_ids.get(pr) {
                        prefetch_ids.extend(slice.iter().copied());
                    }
                    prefetch_ids.retain(|id| !visible_ids.contains(id));
                    let br = lib_state.grid_state.get_background_range(
                        layout::virtual_grid::PREFETCH_ROWS_ABOVE,
                        layout::virtual_grid::BACKGROUND_ROWS_BELOW,
                    );
                    let mut background_ids: Vec<uuid::Uuid> = Vec::new();
                    if let Some(slice) = lib_state.cached_index_ids.get(br) {
                        background_ids.extend(slice.iter().copied());
                    }
                    background_ids.retain(|id| {
                        !visible_ids.contains(id)
                            && !prefetch_ids.contains(id)
                    });
                    let poster_kind = match lib_state.library_type {
                        LibraryType::Movies => Some(PosterKind::Movie),
                        LibraryType::Series => Some(PosterKind::Series),
                    };
                    handle.send(DemandSnapshot { visible_ids, prefetch_ids, background_ids, timestamp: now, context: None, poster_kind });
                }
            DomainUpdateResult::task(fetch_task)
        }
        UiMessage::ToggleSortOrder => {
            bump_keep_alive(state);
            // Toggle sort order and refresh the active tab immediately so the grid stays visible.
            state.domains.ui.state.sort_order = match state.domains.ui.state.sort_order {
                SortOrder::Ascending => SortOrder::Descending,
                SortOrder::Descending => SortOrder::Ascending,
            };
            state.tab_manager.set_active_sort(
                state.domains.ui.state.sort_by,
                state.domains.ui.state.sort_order,
            );

            // Keep the current grid populated while we fetch filtered indices
            state.tab_manager.refresh_active_tab();


            let fetch_task = Task::done(DomainMessage::Ui(
                UiMessage::RequestFilteredPositions,
            ));
            if let Some(handle) = state.domains.metadata.state.planner_handle.as_ref()
                && let TabState::Library(lib_state) = state.tab_manager.active_tab() {
                    let now = std::time::Instant::now();
                    let mut visible_ids: Vec<uuid::Uuid> = Vec::new();
                    let vr = lib_state.grid_state.visible_range.clone();
                    if let Some(slice) = lib_state.cached_index_ids.get(vr) {
                        visible_ids.extend(slice.iter().copied());
                    }
                    let pr = lib_state
                        .grid_state
                        .get_preload_range(layout::virtual_grid::PREFETCH_ROWS_ABOVE);
                    let mut prefetch_ids: Vec<uuid::Uuid> = Vec::new();
                    if let Some(slice) = lib_state.cached_index_ids.get(pr) {
                        prefetch_ids.extend(slice.iter().copied());
                    }
                    prefetch_ids.retain(|id| !visible_ids.contains(id));
                    let br = lib_state.grid_state.get_background_range(
                        layout::virtual_grid::PREFETCH_ROWS_ABOVE,
                        layout::virtual_grid::BACKGROUND_ROWS_BELOW,
                    );
                    let mut background_ids: Vec<uuid::Uuid> = Vec::new();
                    if let Some(slice) = lib_state.cached_index_ids.get(br) {
                        background_ids.extend(slice.iter().copied());
                    }
                    background_ids.retain(|id| {
                        !visible_ids.contains(id)
                            && !prefetch_ids.contains(id)
                    });
                    let poster_kind = match lib_state.library_type {
                        LibraryType::Movies => Some(PosterKind::Movie),
                        LibraryType::Series => Some(PosterKind::Series),
                    };
                    handle.send(DemandSnapshot { visible_ids, prefetch_ids, background_ids, timestamp: now, context: None, poster_kind });
                }
            DomainUpdateResult::task(fetch_task)
        }
        UiMessage::ApplyFilteredPositions(library_id, cache_key, positions) => {
            bump_keep_alive(state);
            // Apply server-provided positions directly to the active library tab.
            // Do NOT call refresh_active_tab() here; it would clear the applied positions
            // and briefly reset the grid, causing it to appear empty.
            let mut applied = false;
            if let Some(tab) = state
                .tab_manager
                .get_tab_mut(TabId::Library(library_id))
                && let TabState::Library(lib_state) = tab
            {
                lib_state.apply_sorted_positions(&positions, Some(cache_key));
                applied = true;
            }
            if applied {
                if let Some(handle) = state.domains.metadata.state.planner_handle.as_ref()
                    && let TabState::Library(lib_state) = state.tab_manager.active_tab() {
                        let now = std::time::Instant::now();
                        let mut visible_ids: Vec<uuid::Uuid> = Vec::new();
                        let vr = lib_state.grid_state.visible_range.clone();
                        if let Some(slice) = lib_state.cached_index_ids.get(vr) {
                            visible_ids.extend(slice.iter().copied());
                        }
                        let pr = lib_state
                            .grid_state
                            .get_preload_range(layout::virtual_grid::PREFETCH_ROWS_ABOVE);
                        let mut prefetch_ids: Vec<uuid::Uuid> = Vec::new();
                        if let Some(slice) = lib_state.cached_index_ids.get(pr) {
                            prefetch_ids.extend(slice.iter().copied());
                        }
                        prefetch_ids.retain(|id| !visible_ids.contains(id));
                        let br = lib_state.grid_state.get_background_range(
                            layout::virtual_grid::PREFETCH_ROWS_ABOVE,
                            layout::virtual_grid::BACKGROUND_ROWS_BELOW,
                        );
                        let mut background_ids: Vec<uuid::Uuid> =
                            Vec::new();
                        if let Some(slice) =
                            lib_state.cached_index_ids.get(br)
                        {
                            background_ids.extend(slice.iter().copied());
                        }
                        background_ids.retain(|id| {
                            !visible_ids.contains(id)
                                && !prefetch_ids.contains(id)
                        });
                        let poster_kind = match lib_state.library_type {
                            LibraryType::Movies => Some(PosterKind::Movie),
                            LibraryType::Series => Some(PosterKind::Series),
                        };
                        handle.send(DemandSnapshot { visible_ids, prefetch_ids, background_ids, timestamp: now, context: None, poster_kind });
                    }

                DomainUpdateResult::task(Task::none())
            } else {
                DomainUpdateResult::task(Task::none())
            }
        }
        UiMessage::RequestFilteredPositions => {
            bump_keep_alive(state);
            // Build a FilterIndicesRequest from current UI filters (Phase 1: movies only)
            let api = state.api_service.clone();
            let active_lib = state.tab_manager.active_tab_id().library_id();
            let active_type = state.tab_manager.active_tab_type().cloned();
            if let (Some(lib_id), Some(lib_type)) = (active_lib, active_type) {
                if matches!(lib_type, LibraryType::Movies) {
                    // If no filters are active and search is empty, skip the server call.
                    // Local repo already applied the new sort via refresh_active_tab().
                    let has_active_filters = {
                        let ui = &state.domains.ui.state;
                        let has_genres = !ui.selected_genres.is_empty();
                        let has_decade = ui.selected_decade.is_some();
                        let has_resolution = ui.selected_resolution != UiResolution::Any;
                        let has_watch = ui.selected_watch_status != UiWatchStatus::Any;
                        let has_search = !ui.search_query.trim().is_empty();
                        has_genres || has_decade || has_resolution || has_watch || has_search
                    };
                    if !has_active_filters {
                        log::debug!(
                            "RequestFilteredPositions: no active filters; using local sort only"
                        );
                        return DomainUpdateResult::task(Task::none());
                    }

                    // Use core sort directly
                    let core_sort = state.domains.ui.state.sort_by;
                    let core_order = state.domains.ui.state.sort_order;
                    let genre_names: Vec<String> = state
                        .domains
                        .ui
                        .state
                        .selected_genres
                        .iter()
                        .map(|g| g.api_name().to_string())
                        .collect();

                    let search = state.domains.ui.state.search_query.as_str();
                    let trimmed = search.trim();
                    let search = if trimmed.is_empty() {
                        None
                    } else {
                        Some(trimmed)
                    };

                    let params = FilterRequestParams {
                        media_type: Some(MediaTypeFilter::Movie),
                        genres: &genre_names,
                        decade: state.domains.ui.state.selected_decade,
                        explicit_year_range: None,
                        rating: None,
                        resolution: state.domains.ui.state.selected_resolution,
                        watch_status: state.domains.ui.state.selected_watch_status,
                        search,
                        sort: core_sort,
                        order: core_order,
                    };
                    let spec = build_filter_indices_request(params);
                    let spec_hash = hash_filter_spec(&spec);
                    let spec_for_request = spec.clone();

                    if let TabState::Library(lib_state) =
                        state.tab_manager.get_active_tab()
                        && let Some(cached) = lib_state.cached_positions_for_hash(spec_hash)
                    {
                        let cached_positions = cached.clone();
                        return DomainUpdateResult::task(Task::done(DomainMessage::Ui(
                            UiMessage::ApplySortedPositions(
                                lib_id,
                                Some(spec_hash),
                                cached_positions,
                            ),
                        )));
                    }

                    let task = Task::perform(
                        async move {
                            match api
                                .fetch_filtered_indices(lib_id.to_uuid(), &spec_for_request)
                                .await
                            {
                                Ok(positions) => UiMessage::ApplyFilteredPositions(
                                    lib_id, spec_hash, positions,
                                ),
                                Err(e) => UiMessage::SortedIndexFailed(e.to_string()),
                            }
                        },
                        DomainMessage::Ui,
                    );
                    DomainUpdateResult::task(task)
                } else {
                    log::debug!(
                        "Skip filtered indices request for non-movie library {:?}",
                        lib_type
                    );
                    DomainUpdateResult::task(Task::none())
                }
            } else {
                DomainUpdateResult::task(Task::none())
            }
        }
        UiMessage::ToggleFilterPanel => {
            state.domains.ui.state.show_filter_panel = !state.domains.ui.state.show_filter_panel;
            DomainUpdateResult::task(Task::none())
        }
        UiMessage::ToggleFilterGenre(g) => {
            if let Some(pos) = state
                .domains
                .ui
                .state
                .selected_genres
                .iter()
                .position(|x| x == &g)
            {
                state.domains.ui.state.selected_genres.remove(pos);
            } else {
                state.domains.ui.state.selected_genres.push(g);
            }
            DomainUpdateResult::task(Task::none())
        }
        UiMessage::SetFilterDecade(d) => {
            state.domains.ui.state.selected_decade = Some(d);
            DomainUpdateResult::task(Task::none())
        }
        UiMessage::ClearFilterDecade => {
            state.domains.ui.state.selected_decade = None;
            DomainUpdateResult::task(Task::none())
        }
        UiMessage::SetFilterResolution(r) => {
            state.domains.ui.state.selected_resolution = r;
            DomainUpdateResult::task(Task::none())
        }
        UiMessage::SetFilterWatchStatus(ws) => {
            state.domains.ui.state.selected_watch_status = ws;
            DomainUpdateResult::task(Task::none())
        }
        UiMessage::ApplyFilters => {
            bump_keep_alive(state);
            // Reuse RequestFilteredPositions path; it will read current UI fields
            DomainUpdateResult::task(Task::done(DomainMessage::Ui(
                UiMessage::RequestFilteredPositions,
            )))
        }
        UiMessage::ClearFilters => {
            bump_keep_alive(state);
            state.domains.ui.state.selected_genres.clear();
            state.domains.ui.state.selected_decade = None;
            state.domains.ui.state.selected_resolution = UiResolution::Any;
            state.domains.ui.state.selected_watch_status = UiWatchStatus::Any;
            DomainUpdateResult::task(Task::done(DomainMessage::Ui(
                UiMessage::RequestFilteredPositions,
            )))
        }
        UiMessage::ShowAdminDashboard => {
            state.domains.ui.state.view = ViewState::AdminDashboard;
            DomainUpdateResult::task(Task::none())
        }
        UiMessage::ApplySortedPositions(library_id, cache_key, positions) => {
            bump_keep_alive(state);

            if let Some(tab) = state
                .tab_manager
                .get_tab_mut(TabId::Library(library_id))
                && let TabState::Library(lib_state) = tab
            {
                let count = positions.len();
                let first = positions.first().copied();
                let last = positions.last().copied();
                log::debug!(
                    "ApplySortedPositions: library {} received {} positions (first={:?}, last={:?})",
                    library_id,
                    count,
                    first,
                    last
                );
                lib_state.apply_sorted_positions(&positions, cache_key);
            }
            // Do not refresh here; keep applied positions intact.
            DomainUpdateResult::task(Task::none())
        }
        UiMessage::SortedIndexFailed(err) => {
            log::warn!("Sorted index fetch failed: {}", err);
            state.domains.ui.state.error_message =
                Some(format!("Unable to apply sort/filter: {}", err));
            DomainUpdateResult::task(Task::none())
        }
        UiMessage::HideAdminDashboard => {
            state.domains.ui.state.view = ViewState::Library;
            DomainUpdateResult::task(Task::none())
        }
        UiMessage::ShowUserManagement => {
            // Save current view to navigation history
            state
                .domains
                .ui
                .state
                .navigation_history
                .push(state.domains.ui.state.view.clone());

            state.domains.ui.state.view = ViewState::AdminUsers;

            // Trigger load of users from the user management domain
            let task = Task::done(DomainMessage::UserManagement(
                crate::domains::user_management::messages::Message::LoadUsers,
            ));
            DomainUpdateResult::task(task)
        }
        UiMessage::HideUserManagement => {
            // Return to Admin Dashboard
            state.domains.ui.state.view = ViewState::AdminDashboard;
            DomainUpdateResult::task(Task::none())
        }
        UiMessage::UserAdminDelete(user_id) => {
            // Proxy to user_management domain delete confirm action
            let task = Task::done(DomainMessage::UserManagement(
                crate::domains::user_management::messages::Message::DeleteUserConfirm(user_id),
            ));
            DomainUpdateResult::task(task)
        }
        UiMessage::ShowLibraryManagement => {
            // Save current view to navigation history
            state
                .domains
                .ui
                .state
                .navigation_history
                .push(state.domains.ui.state.view.clone());

            state.domains.library.state.library_form_success = None;
            state.domains.ui.state.view = ViewState::LibraryManagement;
            state.domains.library.state.show_library_management = true;

            let fetch_scans_task = Task::done(DomainMessage::Library(
                library::messages::Message::FetchActiveScans,
            ));

            let mut tasks = vec![fetch_scans_task];

            #[cfg(feature = "demo")]
            {
                tasks = demo_controls::augment_show_library_management_tasks(state, tasks);
            }

            let combined_task = Task::batch(tasks);

            // Request library refresh if needed
            if !state.domains.ui.state.repo_accessor.is_initialized() {
                DomainUpdateResult::with_events(
                    combined_task,
                    vec![CrossDomainEvent::RequestLibraryRefresh],
                )
            } else {
                DomainUpdateResult::task(combined_task)
            }
        }
        UiMessage::HideLibraryManagement => {
            state.domains.ui.state.view = ViewState::Library;
            state.domains.library.state.show_library_management = false;
            state.domains.library.state.library_form_data = None; // Clear form when leaving management view
            state.domains.library.state.library_form_success = None;
            DomainUpdateResult::task(Task::none())
        }
        #[cfg(feature = "demo")]
        UiMessage::DemoMoviesTargetChanged(value) => {
            demo_controls::handle_movies_input(state, value)
        }
        #[cfg(feature = "demo")]
        UiMessage::DemoSeriesTargetChanged(value) => {
            demo_controls::handle_series_input(state, value)
        }
        #[cfg(feature = "demo")]
        UiMessage::DemoApplySizing => demo_controls::handle_apply_sizing(state),
        #[cfg(feature = "demo")]
        UiMessage::DemoRefreshStatus => demo_controls::handle_refresh_status(state),
        UiMessage::ShowClearDatabaseConfirm => {
            state.domains.ui.state.show_clear_database_confirm = true;
            DomainUpdateResult::task(Task::none())
        }
        UiMessage::HideClearDatabaseConfirm => {
            state.domains.ui.state.show_clear_database_confirm = false;
            DomainUpdateResult::task(Task::none())
        }
        UiMessage::ClearDatabase => {
            let task = crate::common::clear_database::handle_clear_database(state);
            DomainUpdateResult::task(task)
        }
        UiMessage::DatabaseCleared(result) => {
            let task = crate::common::clear_database::handle_database_cleared(state, result);
            DomainUpdateResult::task(task)
        }
        UiMessage::ClearError => {
            state.domains.ui.state.error_message = None;
            DomainUpdateResult::task(Task::none())
        }
        UiMessage::TabGridScrolled(viewport) => {
            bump_keep_alive(state);
            let task = super::update_handlers::scroll_updates::handle_tab_grid_scrolled(state, viewport);
            DomainUpdateResult::task(task.map(DomainMessage::Ui))
        }
        UiMessage::KineticScroll(inner) => {
            let task = super::motion_controller::update::update(state, inner);
            DomainUpdateResult::task(task.map(DomainMessage::Ui))
        }
        UiMessage::DetailViewScrolled(viewport) => DomainUpdateResult::task(
            super::update_handlers::scroll_updates::handle_detail_view_scrolled(state, viewport)
                .map(DomainMessage::Ui),
        ),
        UiMessage::AllViewScrolled(viewport) => DomainUpdateResult::task(
            super::update_handlers::all_focus::handle_all_view_scrolled(state, viewport)
                .map(DomainMessage::Ui),
        ),
        UiMessage::AllFocusNext => DomainUpdateResult::task(
            super::update_handlers::all_focus::handle_all_focus_next(state)
                .map(DomainMessage::Ui),
        ),
        UiMessage::AllFocusPrev => DomainUpdateResult::task(
            super::update_handlers::all_focus::handle_all_focus_prev(state)
                .map(DomainMessage::Ui),
        ),
        UiMessage::AllFocusTick => DomainUpdateResult::task(
            super::update_handlers::all_focus::handle_all_focus_tick(state)
                .map(DomainMessage::Ui),
        ),
        UiMessage::WindowResized(size) => DomainUpdateResult::task(
            super::update_handlers::window_update::handle_window_resized(state, size)
                .map(DomainMessage::Ui),
        ),
        UiMessage::WindowMoved(position) => DomainUpdateResult::task(
            super::update_handlers::window_update::handle_window_moved(state, position)
                .map(DomainMessage::Ui),
        ),
        UiMessage::MouseMoved => {
            state
                .domains
                .ui
                .state
                .carousel_focus
                .record_mouse_move(Instant::now());
            DomainUpdateResult::task(Task::none())
        }
        UiMessage::MediaHovered(media_id) => {
            state.domains.ui.state.hovered_media_id = Some(media_id);
            DomainUpdateResult::task(Task::none())
        }
        UiMessage::MediaUnhovered(media_id) => {
            // Only clear hover state if it matches the media being unhovered
            // This prevents race conditions when quickly moving between posters
            if state.domains.ui.state.hovered_media_id.as_ref() == Some(&media_id) {
                state.domains.ui.state.hovered_media_id = None;
            }
            DomainUpdateResult::task(Task::none())
        }
        UiMessage::NavigateHome => {
            let library_id = state
                .domains
                .library
                .state
                .current_library_id
                .map(|id| id.to_uuid());
            state.domains.ui.state.view = ViewState::Library;
            state.domains.ui.state.display_mode = DisplayMode::Curated;

            // REMOVED: No longer clearing duplicate state fields
            // MediaStore is the single source of truth

            // For curated mode (All libraries), activate the All tab with scroll restoration
            state.tab_manager.set_active_tab_with_scroll(
                TabId::All,
                &mut state.domains.ui.state.scroll_manager,
                state.window_size.width,
            );
            state.tab_manager.set_active_sort(
                state.domains.ui.state.sort_by,
                state.domains.ui.state.sort_order,
            );
            state.tab_manager.refresh_active_tab();
            log::debug!("NavigateHome: Activated All tab for curated view");

            // Clear navigation history when going home
            state.domains.ui.state.navigation_history.clear();

            // Reset theme colors to library defaults
            state
                .domains
                .ui
                .state
                .background_shader_state
                .reset_to_library_colors();

            // Update background shader depth regions for library view
            state
                .domains
                .ui
                .state
                .background_shader_state
                .update_depth_lines(
                    &state.domains.ui.state.view,
                    state.window_size.width,
                    state.window_size.height,
                    library_id,
                );

            // Initialize All tab (curated + per-library) and emit initial snapshots
            init_all_tab_view(state);
            emit_initial_all_tab_snapshots_combined(state);

            // Build focus order and set initial active carousel
            let ordered = super::tabs::ordered_keys_for_all_view(state);
            if let Some(super::tabs::TabState::All(all_state)) = state.tab_manager.get_tab_mut(super::tabs::TabId::All) {
                all_state.focus.ordered_keys = ordered;
                if all_state.focus.active_carousel.is_none() {
                    all_state.focus.active_carousel = all_state.focus.ordered_keys.first().cloned();
                }
            }

            // Keep UI alive while initial poster fetch/uploads start
            bump_keep_alive(state);

            DomainUpdateResult::task(Task::none())
        }
        UiMessage::NavigateBack => {
            // Navigate to the previous view in history
            let library_id = state
                .domains
                .library
                .state
                .current_library_id
                .map(|id| id.to_uuid());

            match state.domains.ui.state.navigation_history.pop() {
                Some(previous_view) => {
                    state.domains.ui.state.view = previous_view.clone();

                    // Restore scroll state when returning to views
                    match &previous_view {
                        ViewState::Library => {
                            // Determine library context based on display mode
                            let library_id = match state.domains.ui.state.display_mode {
                                DisplayMode::Library => {
                                    state.domains.library.state.current_library_id
                                }
                                DisplayMode::Curated => None, // All libraries
                                _ => None,
                            };

                            // Restore scroll state through TabManager with ScrollPositionManager
                            let tab_id = if let Some(lib_id) = library_id {
                                TabId::Library(lib_id)
                            } else {
                                TabId::All
                            };

                            // Use the scroll-aware tab switching which automatically restores position
                            state.tab_manager.set_active_tab_with_scroll(
                                tab_id,
                                &mut state.domains.ui.state.scroll_manager,
                                state.window_size.width,
                            );
                            state.tab_manager.set_active_sort(
                                state.domains.ui.state.sort_by,
                                state.domains.ui.state.sort_order,
                            );

                            state.tab_manager.refresh_active_tab();

                            // Explicitly restore scroll position after tab switch
                            let scroll_task = if let Some(tab) = state.tab_manager.get_tab(tab_id) {
                                if let Some(grid_state) = tab.grid_state() {
                                    let scroll_position = grid_state.scroll_position;
                                    let scrollable_id = grid_state.scrollable_id.clone();
                                    log::debug!(
                                        "NavigateBack: Restoring scroll position {} for tab {:?}",
                                        scroll_position,
                                        tab_id
                                    );
                                    scroll_to::<DomainMessage>(
                                        scrollable_id,
                                        AbsoluteOffset {
                                            x: 0.0,
                                            y: scroll_position,
                                        },
                                    )
                                } else {
                                    Task::none()
                                }
                            } else {
                                Task::none()
                            };

                            log::debug!(
                                "NavigateBack: Restored tab state for library {:?}",
                                library_id
                            );

                            // Reset colors and update depth regions for library view
                            state
                                .domains
                                .ui
                                .state
                                .background_shader_state
                                .reset_to_view_colors(&previous_view);

                            let library_id = state
                                .domains
                                .library
                                .state
                                .current_library_id
                                .map(|id| id.to_uuid());

                            state
                                .domains
                                .ui
                                .state
                                .background_shader_state
                                .update_depth_lines(
                                    &state.domains.ui.state.view,
                                    state.window_size.width,
                                    state.window_size.height,
                                    library_id,
                                );

                            return DomainUpdateResult::task(scroll_task);
                        }
                        _ => {
                            // Detail views don't have scrollable content in current implementation
                            log::debug!("Navigated back to view: {:?}", previous_view);
                        }
                    }

                    // Reset colors if returning to a non-detail view
                    state
                        .domains
                        .ui
                        .state
                        .background_shader_state
                        .reset_to_view_colors(&previous_view);

                    // Update background shader depth regions for the restored view
                    state
                        .domains
                        .ui
                        .state
                        .background_shader_state
                        .update_depth_lines(
                            &state.domains.ui.state.view,
                            state.window_size.width,
                            state.window_size.height,
                            library_id,
                        );

                    DomainUpdateResult::task(Task::none())
                }
                _ => {
                    // No history - return to library view preserving current display mode
                    // This handles the case where a user plays a video directly from a library grid
                    // and then exits - we want to return to that library grid, not the home carousel
                    state.domains.ui.state.view = ViewState::Library;

                    // Preserve the current display mode and library context
                    let library_id = match state.domains.ui.state.display_mode {
                        DisplayMode::Library => state.domains.library.state.current_library_id,
                        DisplayMode::Curated => None,
                        _ => None,
                    };

                    log::debug!(
                        "NavigateBack with no history: preserving display mode {:?}",
                        state.domains.ui.state.display_mode
                    );

                    // Reset colors and update depth regions for library view
                    state
                        .domains
                        .ui
                        .state
                        .background_shader_state
                        .reset_to_library_colors();

                    let library_id = library_id.map(|id| id.to_uuid());

                    state
                        .domains
                        .ui
                        .state
                        .background_shader_state
                        .update_depth_lines(
                            &state.domains.ui.state.view,
                            state.window_size.width,
                            state.window_size.height,
                            library_id,
                        );

                    DomainUpdateResult::task(Task::none())
                }
            }
        }
        UiMessage::UpdateSearchQuery(query) => {
            let mut result = super::update_handlers::update_search_query(state, query);

            if state
                .windows
                .get(windows::WindowKind::Search)
                .is_none()
            {
                let open = windows::controller::open_search(state, None);
                result.task = Task::batch([result.task, open.task]);
                result.events.extend(open.events);
            }

            result
        }
        UiMessage::BeginSearchFromKeyboard(seed) => {
            windows::controller::open_search(state, Some(seed))
        }
        UiMessage::ExecuteSearch => {
            // Forward directly to search domain
            DomainUpdateResult::task(Task::done(DomainMessage::Search(
                crate::domains::search::messages::Message::ExecuteSearch,
            )))
        }
        UiMessage::ShowLibraryMenu => {
            state.domains.ui.state.show_library_menu = !state.domains.ui.state.show_library_menu;
            DomainUpdateResult::task(Task::none())
        }
        UiMessage::ShowAllLibrariesMenu => {
            state.domains.ui.state.show_library_menu = !state.domains.ui.state.show_library_menu;
            state.domains.ui.state.library_menu_target = None;
            DomainUpdateResult::task(Task::none())
        }
        UiMessage::ShowProfile => {
            // Save current view to navigation history
            state
                .domains
                .ui
                .state
                .navigation_history
                .push(state.domains.ui.state.view.clone());

            state.domains.ui.state.view = ViewState::UserSettings;

            // Load auto-login preference when showing settings
            let svc = state.domains.auth.state.auth_service.clone();

            DomainUpdateResult::task(
                Task::perform(
                    async move {
                        svc.is_current_user_auto_login_enabled()
                            .await
                            .unwrap_or(false)
                    },
                    |enabled| UiMessage::AutoLoginToggled(Ok(enabled)),
                )
                .map(DomainMessage::Ui),
            )
        }

        UiMessage::ShowUserProfile => {
            state.domains.settings.current_view =
                crate::domains::settings::state::SettingsView::Profile;
            DomainUpdateResult::task(Task::none())
        }

        UiMessage::ShowUserPreferences => {
            state.domains.settings.current_view =
                crate::domains::settings::state::SettingsView::Preferences;
            DomainUpdateResult::task(Task::none())
        }

        UiMessage::ShowUserSecurity => {
            state.domains.settings.current_view =
                crate::domains::settings::state::SettingsView::Security;

            DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                SettingsMessage::CheckUserHasPin,
            )))
        }

        UiMessage::ShowDeviceManagement => {
            state.domains.settings.current_view =
                crate::domains::settings::state::SettingsView::DeviceManagement;
            // Load devices when the view is shown - send direct message to Settings domain
            DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                crate::domains::settings::messages::Message::LoadDevices,
            )))
        }

        UiMessage::BackToSettings => {
            state.domains.ui.state.view = ViewState::UserSettings;
            state.domains.settings.current_view =
                crate::domains::settings::state::SettingsView::Main;
            // Clear any security settings state
            state.domains.settings.security = Default::default();
            DomainUpdateResult::task(Task::none())
        }

        // Security settings handlers - emit cross-domain events to Settings domain
        UiMessage::ShowChangePassword => {
            DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                crate::domains::settings::messages::Message::ShowChangePassword,
            )))
        }

        UiMessage::UpdatePasswordCurrent(value) => {
            DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                crate::domains::settings::messages::Message::UpdatePasswordCurrent(value),
            )))
        }

        UiMessage::UpdatePasswordNew(value) => {
            DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                crate::domains::settings::messages::Message::UpdatePasswordNew(value),
            )))
        }

        UiMessage::UpdatePasswordConfirm(value) => {
            DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                crate::domains::settings::messages::Message::UpdatePasswordConfirm(value),
            )))
        }

        UiMessage::TogglePasswordVisibility => {
            DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                crate::domains::settings::messages::Message::TogglePasswordVisibility,
            )))
        }

        UiMessage::SubmitPasswordChange => {
            DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                crate::domains::settings::messages::Message::SubmitPasswordChange,
            )))
        }

        // TODO: PASSWORD CHANGE UNIMPLEMENTED
        UiMessage::PasswordChangeResult(_result) => {
            // UI handles displaying the result
            DomainUpdateResult::task(Task::none())
        }

        UiMessage::CancelPasswordChange => {
            DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                crate::domains::settings::messages::Message::CancelPasswordChange,
            )))
        }

        UiMessage::ShowSetPin => DomainUpdateResult::task(Task::done(DomainMessage::Settings(
            crate::domains::settings::messages::Message::ShowSetPin,
        ))),

        UiMessage::ShowChangePin => DomainUpdateResult::task(Task::done(
            DomainMessage::Settings(crate::domains::settings::messages::Message::ShowChangePin),
        )),

        UiMessage::UpdatePinCurrent(value) => {
            DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                crate::domains::settings::messages::Message::UpdatePinCurrent(value),
            )))
        }

        UiMessage::UpdatePinNew(value) => {
            DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                crate::domains::settings::messages::Message::UpdatePinNew(value),
            )))
        }

        UiMessage::UpdatePinConfirm(value) => {
            DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                crate::domains::settings::messages::Message::UpdatePinConfirm(value),
            )))
        }

        UiMessage::SubmitPinChange => DomainUpdateResult::task(Task::done(
            DomainMessage::Settings(crate::domains::settings::messages::Message::SubmitPinChange),
        )),

        // TODO: PIN CHANGE UNIMPLEMENTED
        UiMessage::PinChangeResult(_result) => {
            DomainUpdateResult::task(Task::none())
        }

        UiMessage::CancelPinChange => DomainUpdateResult::task(Task::done(
            DomainMessage::Settings(SettingsMessage::CancelPinChange),
        )),

        UiMessage::EnableAdminPinUnlock => {
            DomainUpdateResult::task(Task::done(DomainMessage::Auth(
                crate::domains::auth::messages::Message::EnableAdminPinUnlock,
            )))
        }

        UiMessage::DisableAdminPinUnlock => {
            DomainUpdateResult::task(Task::done(DomainMessage::Auth(
                crate::domains::auth::messages::Message::DisableAdminPinUnlock,
            )))
        }


        UiMessage::ToggleAutoLogin(enabled) => {
            DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                SettingsMessage::ToggleAutoLogin(enabled),
            )))
        }

        // TODO: DEAD MESSAGE VARIANT?
        UiMessage::AutoLoginToggled(_result) => {
            DomainUpdateResult::task(Task::none())
        }

        // TODO: LOAD DEVICES NOT WORKING
        UiMessage::LoadDevices => {
            DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                SettingsMessage::LoadDevices,
            )))
        }

        // TODO: DEAD MESSAGE VARIANT
        UiMessage::DevicesLoaded(_result) => {
            // This message should now come from settings domain, but kept for compatibility
            log::warn!("DevicesLoaded should now come from settings domain via cross-domain event");
            DomainUpdateResult::task(Task::none())
        }

        UiMessage::RefreshDevices => {
            DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                crate::domains::settings::messages::Message::RefreshDevices,
            )))
        }

        UiMessage::RevokeDevice(device_id) => {
            DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                crate::domains::settings::messages::Message::RevokeDevice(device_id),
            )))
        }

        // TODO: DEAD MESSAGE VARIANT
        UiMessage::DeviceRevoked(_result) => {
            log::warn!("DeviceRevoked should now come from settings domain via cross-domain event");
            DomainUpdateResult::task(Task::none())
        }

        // Logout: delegate to Auth domain so it performs a secure logout
        // flow and then reloads users. This avoids getting stuck on the
        // auth view's fallback "loading users" state without any tasks.
        UiMessage::Logout => {
            use crate::domains::auth::messages as auth;
            DomainUpdateResult::task(Task::done(DomainMessage::Auth(
                auth::Message::Logout,
            )))
        }

        UiMessage::VirtualCarousel(vc_msg) => {
            bump_keep_alive(state);
            DomainUpdateResult::task(
                handle_virtual_carousel_message(state, vc_msg)
                    .map(DomainMessage::Ui),
            )
        }
        UiMessage::UpdateTransitions => {
            let ui_state = &mut state.domains.ui.state;
            let now = Instant::now();

            let poster_anim_active = match ui_state.poster_anim_active_until {
                Some(until) if until > now => true,
                Some(_) => {
                    ui_state.poster_anim_active_until = None;
                    false
                }
                None => false,
            };

            let shader_state = &mut ui_state.background_shader_state;
            let transitions_active = shader_state.color_transitions.is_transitioning()
                || shader_state.backdrop_transitions.is_transitioning()
                || shader_state.gradient_transitions.is_transitioning();

            if !poster_anim_active && !transitions_active {
                return DomainUpdateResult::task(Task::none());
            }

            shader_state.color_transitions.update();
            shader_state.backdrop_transitions.update();
            shader_state.gradient_transitions.update();

            // Update the actual colors based on transition progress
            let (primary, secondary) = shader_state.color_transitions.get_interpolated_colors();
            shader_state.primary_color = primary;
            shader_state.secondary_color = secondary;

            // Update the gradient center based on transition progress
            shader_state.gradient_center =
                shader_state.gradient_transitions.get_interpolated_center();

            DomainUpdateResult::task(Task::none())
        }
        UiMessage::ToggleBackdropAspectMode => {
            state
                .domains
                .ui
                .state
                .background_shader_state
                .backdrop_aspect_mode = match state
                .domains
                .ui
                .state
                .background_shader_state
                .backdrop_aspect_mode
            {
                BackdropAspectMode::Auto => {
                    BackdropAspectMode::Force21x9
                }
                BackdropAspectMode::Force21x9 => {
                    BackdropAspectMode::Auto
                }
            };
            DomainUpdateResult::task(Task::none())
        }
        UiMessage::UpdateBackdropHandle(_handle) => {
            // Deprecated - backdrops are now pulled reactively from image service
            // This message handler kept for compatibility but does nothing
            DomainUpdateResult::task(Task::none())
        }
        UiMessage::CheckMediaStoreRefresh => {
            // Check if MediaStore notifier indicates a refresh is needed
            /*
            if state.media_store_notifier.should_refresh() {
                log::debug!(
                    "[MediaStoreNotifier] ViewModels refresh needed - triggering RefreshViewModels"
                );
                DomainUpdateResult::task(
                    Task::done(UiMessage::RefreshViewModels).map(DomainMessage::Ui),
                )
            } else { */
            DomainUpdateResult::task(Task::none())
            //}
        }
        UiMessage::RefreshViewModels => {
            bump_keep_alive(state);
            // Refresh view models - pull latest data from MediaStore
            log::info!(
                "[MediaStoreNotifier] RefreshViewModels triggered - updating view models with latest MediaStore data"
            );

            // Update library filters based on current display mode
            let library_filter = match state.domains.ui.state.display_mode {
                DisplayMode::Curated => None, // Show all libraries
                DisplayMode::Library => state.domains.ui.state.current_library_id,
                _ => None, // Other modes show all content for now
            };

            // Sync UI domain's library ID with the determined filter
            // This ensures UI domain state matches what ViewModels will use
            // TODO: This should not be necessary once we properly handle current library ID
            if matches!(state.domains.ui.state.display_mode, DisplayMode::Library)
                && library_filter != state.domains.ui.state.current_library_id
            {
                log::warn!(
                    "UI: Syncing UI domain library ID from {:?} to {:?}",
                    state.domains.ui.state.current_library_id,
                    library_filter
                );
                state.domains.ui.state.current_library_id = library_filter;
            }

            // The view models now have the latest sorted data
            //log::info!(
            //    "UI: View models refreshed with {} movies, {} series in AllViewModel",
            //    state.all_view_model.all_movies().len(),
            //    state.all_view_model.all_series().len()
            //);

            // NEW ARCHITECTURE: Refresh TabManager tabs with sorted data
            // This ensures the tab-based views show the newly sorted content
            // Refresh only the active tab for better performance
            state
                .tab_manager
                .mark_tab_needs_refresh(state.tab_manager.active_tab_id());
            state.tab_manager.refresh_active_tab();
            log::info!("TabManager: All tabs refreshed with sorted data from MediaStore");

            // After view models refresh, if we're in All (Curated) mode,
            // (re)initialize carousels and emit initial snapshots so images load immediately.
            if matches!(state.domains.ui.state.display_mode, DisplayMode::Curated)
                && matches!(state.tab_manager.active_tab_id(), TabId::All)
            {
                super::update_handlers::all_tab::init_all_tab_view(state);
                super::update_handlers::all_tab::emit_initial_all_tab_snapshots_combined(state);
            }

            DomainUpdateResult::task(Task::none())
        }
        UiMessage::UpdateViewModelFilters => {
            bump_keep_alive(state);
            // Lightweight update - just change filters without re-reading from MediaStore
            let library_filter = match state.domains.ui.state.display_mode {
                DisplayMode::Library => state.domains.ui.state.current_library_id,
                DisplayMode::Curated => None, // Always show all in curated mode
                _ => None,
            };

            log::info!(
                "UI: UpdateViewModelFilters called - library_filter = {:?}, display_mode = {:?}, ui.current_library_id = {:?}, library.current_library_id = {:?}",
                library_filter,
                state.domains.ui.state.display_mode,
                state.domains.ui.state.current_library_id,
                state.domains.library.state.current_library_id
            );

            //// Always update AllViewModel as it handles both types
            //state.all_view_model.set_library_filter(library_filter);

            //log::info!(
            //    "UI: Filter updated - All: {} movies + {} series",
            //    state.all_view_model.all_movies().len(),
            //    state.all_view_model.all_series().len()
            //);

            DomainUpdateResult::task(Task::none()) // View will update on next frame
        }

        UiMessage::QueueVisibleDetailsForFetch => {
            // TODO: Implement queue visible details for fetch
            log::debug!("Queue visible details for fetch requested");
            DomainUpdateResult::task(Task::none())
        }

        // Cross-domain proxy messages
        UiMessage::ToggleFullscreen => {
            // Forward to media domain
            DomainUpdateResult::with_events(
                Task::none(),
                vec![CrossDomainEvent::MediaToggleFullscreen],
            )
        }
        UiMessage::SelectLibrary(library_id) => {
            // Forward to library domain via cross-domain event
            log::info!(
                "UI: SelectLibrary({:?}) - emitting cross-domain event",
                library_id
            );
            if let Some(id) = library_id {
                DomainUpdateResult::with_events(
                    Task::none(),
                    vec![CrossDomainEvent::LibrarySelected(id)],
                )
            } else {
                // None means show all libraries - forward to library domain
                DomainUpdateResult::with_events(
                    Task::none(),
                    vec![CrossDomainEvent::LibrarySelectAll],
                )
            }
        }
        UiMessage::PlayMediaWithId(media_id) => {
            match state.domains.ui.state.repo_accessor.get(&media_id) {
                Ok(media) => match media {
                    Media::Movie(movie) => DomainUpdateResult::with_events(
                        Task::none(),
                        vec![CrossDomainEvent::MediaPlayWithId(movie.file(), media_id)],
                    ),
                    Media::Episode(episode) => DomainUpdateResult::with_events(
                        Task::none(),
                        vec![CrossDomainEvent::MediaPlayWithId(episode.file(), media_id)],
                    ),
                    _ => {
                        log::error!("Media not playable type {}", media_id);
                        DomainUpdateResult::task(Task::none())
                    }
                },
                Err(_) => {
                    log::error!("Failed to get media with id {}", media_id);
                    DomainUpdateResult::task(Task::none())
                }
            }
        }
        UiMessage::PlayMediaWithIdInMpv(media_id) => {
            match state.domains.ui.state.repo_accessor.get(&media_id) {
                Ok(media) => {
                    // Extract the concrete media file for playback
                    let media_file = match media {
                        Media::Movie(movie) => movie.file(),
                        Media::Episode(episode) => episode.file(),
                        _ => {
                            log::error!("Media not playable type {}", media_id);
                            return DomainUpdateResult::task(Task::none());
                        }
                    };

                    // Seed resume/duration hints similarly to CrossDomainEvent::MediaPlayWithId
                    let mut resume_opt: Option<f32> = None;
                    let mut watch_duration_hint: Option<f64> = None;
                    if let Some(watch_state) =
                        &state.domains.media.state.user_watch_state
                        && let Some(item) =
                            watch_state.get_by_media_id(media_id.as_uuid())
                    {
                        if item.position > 0.0 && item.duration > 0.0 {
                            resume_opt = Some(item.position);
                        }
                        if item.duration > 0.0 {
                            watch_duration_hint = Some(item.duration as f64);
                        }
                    }

                    let metadata_duration_hint = media_file
                        .media_file_metadata
                        .as_ref()
                        .and_then(|meta| meta.duration)
                        .filter(|d| *d > 0.0);

                    let duration_hint = watch_duration_hint.or(metadata_duration_hint);

                    state.domains.player.state.last_valid_position =
                        resume_opt.map(|pos| pos as f64).unwrap_or(0.0);
                    state.domains.player.state.last_valid_duration =
                        duration_hint.unwrap_or(0.0);
                    state.domains.media.state.pending_resume_position = resume_opt;
                    state.domains.player.state.pending_resume_position = resume_opt;

                    // First seed the player with PlayMediaWithId, then switch to external player
                    let tasks = Task::batch(vec![
                        Task::done(DomainMessage::Player(
                            crate::domains::player::messages::Message::PlayMediaWithId(
                                media_file,
                                media_id,
                            ),
                        )),
                        Task::done(DomainMessage::Player(
                            crate::domains::player::messages::Message::PlayExternal,
                        )),
                    ]);

                    DomainUpdateResult::task(tasks)
                }
                Err(_) => {
                    log::error!("Failed to get media with id {}", media_id);
                    DomainUpdateResult::task(Task::none())
                }
            }
        }
        UiMessage::PlaySeriesNextEpisode(_series_id) => {
            /*
            // Use series progress service to find next episode
            use crate::domains::media::services::SeriesProgressService;

            let media_store = state.domains.media.state.media_store.clone();
            let service = SeriesProgressService::new(media_store);

            // Get watch state and find next episode
            let watch_state = state.domains.media.state.user_watch_state.as_ref();

            if let Some((episode, resume_position)) =
                service.get_next_episode_for_series(&series_id, watch_state)
            {
                // Convert episode to MediaFile
                let media_file =
                    crate::domains::media::library::MediaFile::from(episode.file.clone());
                let media_id = MediaID::Episode(episode.id.clone());

                // Store resume position if available
                if let Some(resume_pos) = resume_position {
                    state.domains.media.state.pending_resume_position = Some(resume_pos);
                    log::info!("Series will resume episode at position: {:.1}s", resume_pos);
                }

                // Play the episode
                DomainUpdateResult::with_events(
                    Task::none(),
                    vec![CrossDomainEvent::MediaPlayWithId(media_file, media_id)],
                )
            } else {
                log::info!(
                    "No unwatched episodes found for series {}",
                    series_id.as_str()
                );
                // Could show a message or navigate to series details
                DomainUpdateResult::task(Task::none())
            }*/
            DomainUpdateResult::task(Task::none())
        }

        // Library management proxies
        UiMessage::ShowLibraryForm(library) => {
            // Send direct message to library domain
            DomainUpdateResult::task(Task::done(DomainMessage::Library(
                library::messages::Message::ShowLibraryForm(library),
            )))
        }
        UiMessage::HideLibraryForm => {
            // Send direct message to library domain
            DomainUpdateResult::task(Task::done(DomainMessage::Library(
                library::messages::Message::HideLibraryForm,
            )))
        }
        UiMessage::ScanLibrary(library_id) => {
            // Send direct message to library domain
            DomainUpdateResult::task(Task::done(DomainMessage::Library(
                library::messages::Message::ScanLibrary(library_id),
            )))
        }
        UiMessage::DeleteLibrary(library_id) => {
            // Send direct message to library domain
            DomainUpdateResult::task(Task::done(DomainMessage::Library(
                library::messages::Message::DeleteLibrary(library_id),
            )))
        }
        UiMessage::UpdateLibraryFormName(name) => {
            // Send direct message to library domain
            DomainUpdateResult::task(Task::done(DomainMessage::Library(
                library::messages::Message::UpdateLibraryFormName(name),
            )))
        }
        UiMessage::UpdateLibraryFormType(library_type) => {
            // Send direct message to library domain
            DomainUpdateResult::task(Task::done(DomainMessage::Library(
                library::messages::Message::UpdateLibraryFormType(library_type),
            )))
        }
        UiMessage::UpdateLibraryFormPaths(paths) => {
            // Send direct message to library domain
            DomainUpdateResult::task(Task::done(DomainMessage::Library(
                library::messages::Message::UpdateLibraryFormPaths(paths),
            )))
        }
        UiMessage::UpdateLibraryFormScanInterval(interval) => {
            // Send direct message to library domain
            DomainUpdateResult::task(Task::done(DomainMessage::Library(
                library::messages::Message::UpdateLibraryFormScanInterval(interval),
            )))
        }
        UiMessage::ToggleLibraryFormEnabled => {
            // Send direct message to library domain
            DomainUpdateResult::task(Task::done(DomainMessage::Library(
                library::messages::Message::ToggleLibraryFormEnabled,
            )))
        }
        UiMessage::ToggleLibraryFormStartScan => {
            // Send direct message to library domain
            DomainUpdateResult::task(Task::done(DomainMessage::Library(
                library::messages::Message::ToggleLibraryFormStartScan,
            )))
        }
        UiMessage::SubmitLibraryForm => {
            // Send direct message to library domain
            DomainUpdateResult::task(Task::done(DomainMessage::Library(
                library::messages::Message::SubmitLibraryForm,
            )))
        }
        UiMessage::LibraryMediaRoot(message) => DomainUpdateResult::task(
            Task::done(DomainMessage::Library(
                library::messages::Message::MediaRootBrowser(message),
            )),
        ),
        UiMessage::PauseLibraryScan(library_id, scan_id) => DomainUpdateResult::task(Task::done(
            DomainMessage::Library(library::messages::Message::PauseScan {
                library_id,
                scan_id,
            }),
        )),
        UiMessage::ResumeLibraryScan(library_id, scan_id) => {
            DomainUpdateResult::task(Task::done(DomainMessage::Library(
                library::messages::Message::ResumeScan {
                    library_id,
                    scan_id,
                },
            )))
        }
        UiMessage::CancelLibraryScan(library_id, scan_id) => {
            DomainUpdateResult::task(Task::done(DomainMessage::Library(
                library::messages::Message::CancelScan {
                    library_id,
                    scan_id,
                },
            )))
        }
        UiMessage::FetchScanMetrics => DomainUpdateResult::task(Task::done(
            DomainMessage::Library(library::messages::Message::FetchScanMetrics),
        )),
        UiMessage::ResetLibrary(library_id) => DomainUpdateResult::task(Task::done(
            DomainMessage::Library(library::messages::Message::ResetLibrary(library_id)),
        )),

        // Aggregate all libraries
        UiMessage::AggregateAllLibraries => {
            // Emit cross-domain event to trigger library aggregation
            DomainUpdateResult::with_events(
                Task::none(),
                vec![CrossDomainEvent::RequestLibraryRefresh],
            )
        }

        // No-op
        UiMessage::NoOp => DomainUpdateResult::task(Task::none()),
    }
}
