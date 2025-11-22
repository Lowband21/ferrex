use iced::Task;

use crate::{
    common::messages::{DomainMessage, DomainUpdateResult},
    domains::{
        metadata::demand_planner::DemandSnapshot,
        ui::{
            library_ui::LibraryUiMessage,
            tabs::{TabId, TabState},
            utils::bump_keep_alive,
        },
    },
    infra::{api_types::LibraryType, constants::layout},
    state::State,
};

use ferrex_core::{
    player_prelude::{
        MediaTypeFilter, PosterKind, SortOrder, UiResolution, UiWatchStatus,
    },
    query::filtering::{
        FilterRequestParams, build_filter_indices_request, hash_filter_spec,
    },
};

pub fn update_library_ui(
    state: &mut State,
    message: LibraryUiMessage,
) -> DomainUpdateResult {
    match message {
        LibraryUiMessage::SetSortBy(sort_by) => {
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
                LibraryUiMessage::RequestFilteredPositions.into(),
            ));
            if let Some(handle) =
                state.domains.metadata.state.planner_handle.as_ref()
                && let TabState::Library(lib_state) =
                    state.tab_manager.active_tab()
            {
                let now = std::time::Instant::now();
                let mut visible_ids: Vec<uuid::Uuid> = Vec::new();
                let vr = lib_state.grid_state.visible_range.clone();
                if let Some(slice) = lib_state.cached_index_ids.get(vr) {
                    visible_ids.extend(slice.iter().copied());
                }
                let pr = lib_state.grid_state.get_preload_range(
                    layout::virtual_grid::PREFETCH_ROWS_ABOVE,
                );
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
                    !visible_ids.contains(id) && !prefetch_ids.contains(id)
                });
                let poster_kind = match lib_state.library_type {
                    LibraryType::Movies => Some(PosterKind::Movie),
                    LibraryType::Series => Some(PosterKind::Series),
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
            DomainUpdateResult::task(fetch_task)
        }
        LibraryUiMessage::ToggleSortOrder => {
            bump_keep_alive(state);
            // Toggle sort order and refresh the active tab immediately so the grid stays visible.
            state.domains.ui.state.sort_order =
                match state.domains.ui.state.sort_order {
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
                LibraryUiMessage::RequestFilteredPositions.into(),
            ));
            if let Some(handle) =
                state.domains.metadata.state.planner_handle.as_ref()
                && let TabState::Library(lib_state) =
                    state.tab_manager.active_tab()
            {
                let now = std::time::Instant::now();
                let mut visible_ids: Vec<uuid::Uuid> = Vec::new();
                let vr = lib_state.grid_state.visible_range.clone();
                if let Some(slice) = lib_state.cached_index_ids.get(vr) {
                    visible_ids.extend(slice.iter().copied());
                }
                let pr = lib_state.grid_state.get_preload_range(
                    layout::virtual_grid::PREFETCH_ROWS_ABOVE,
                );
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
                    !visible_ids.contains(id) && !prefetch_ids.contains(id)
                });
                let poster_kind = match lib_state.library_type {
                    LibraryType::Movies => Some(PosterKind::Movie),
                    LibraryType::Series => Some(PosterKind::Series),
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
            DomainUpdateResult::task(fetch_task)
        }
        LibraryUiMessage::ApplyFilteredPositions(
            library_id,
            cache_key,
            positions,
        ) => {
            bump_keep_alive(state);
            // Apply server-provided positions directly to the active library tab.
            // Do NOT call refresh_active_tab() here; it would clear the applied positions
            // and briefly reset the grid, causing it to appear empty.
            let mut applied = false;
            if let Some(tab) =
                state.tab_manager.get_tab_mut(TabId::Library(library_id))
                && let TabState::Library(lib_state) = tab
            {
                lib_state.apply_sorted_positions(&positions, Some(cache_key));
                applied = true;
            }
            if applied {
                if let Some(handle) =
                    state.domains.metadata.state.planner_handle.as_ref()
                    && let TabState::Library(lib_state) =
                        state.tab_manager.active_tab()
                {
                    let now = std::time::Instant::now();
                    let mut visible_ids: Vec<uuid::Uuid> = Vec::new();
                    let vr = lib_state.grid_state.visible_range.clone();
                    if let Some(slice) = lib_state.cached_index_ids.get(vr) {
                        visible_ids.extend(slice.iter().copied());
                    }
                    let pr = lib_state.grid_state.get_preload_range(
                        layout::virtual_grid::PREFETCH_ROWS_ABOVE,
                    );
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
                        !visible_ids.contains(id) && !prefetch_ids.contains(id)
                    });
                    let poster_kind = match lib_state.library_type {
                        LibraryType::Movies => Some(PosterKind::Movie),
                        LibraryType::Series => Some(PosterKind::Series),
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

                DomainUpdateResult::task(Task::none())
            } else {
                DomainUpdateResult::task(Task::none())
            }
        }
        LibraryUiMessage::RequestFilteredPositions => {
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
                        let has_resolution =
                            ui.selected_resolution != UiResolution::Any;
                        let has_watch =
                            ui.selected_watch_status != UiWatchStatus::Any;
                        let has_search = !ui.search_query.trim().is_empty();
                        has_genres
                            || has_decade
                            || has_resolution
                            || has_watch
                            || has_search
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
                        watch_status: state
                            .domains
                            .ui
                            .state
                            .selected_watch_status,
                        search,
                        sort: core_sort,
                        order: core_order,
                    };
                    let spec = build_filter_indices_request(params);
                    let spec_hash = hash_filter_spec(&spec);
                    let spec_for_request = spec.clone();

                    if let TabState::Library(lib_state) =
                        state.tab_manager.get_active_tab()
                        && let Some(cached) =
                            lib_state.cached_positions_for_hash(spec_hash)
                    {
                        let cached_positions = cached.clone();
                        return DomainUpdateResult::task(Task::done(
                            DomainMessage::Ui(
                                LibraryUiMessage::ApplySortedPositions(
                                    lib_id,
                                    Some(spec_hash),
                                    cached_positions,
                                )
                                .into(),
                            ),
                        ));
                    }

                    let task = Task::perform(
                        async move {
                            match api
                                .fetch_filtered_indices(
                                    lib_id.to_uuid(),
                                    &spec_for_request,
                                )
                                .await
                            {
                                Ok(positions) => {
                                    LibraryUiMessage::ApplyFilteredPositions(
                                        lib_id, spec_hash, positions,
                                    )
                                }
                                Err(e) => LibraryUiMessage::SortedIndexFailed(
                                    e.to_string(),
                                ),
                            }
                        },
                        |msg| DomainMessage::Ui(msg.into()),
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
        LibraryUiMessage::ToggleFilterPanel => {
            state.domains.ui.state.show_filter_panel =
                !state.domains.ui.state.show_filter_panel;
            DomainUpdateResult::task(Task::none())
        }
        LibraryUiMessage::ToggleFilterGenre(g) => {
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
        LibraryUiMessage::SetFilterDecade(d) => {
            state.domains.ui.state.selected_decade = Some(d);
            DomainUpdateResult::task(Task::none())
        }
        LibraryUiMessage::ClearFilterDecade => {
            state.domains.ui.state.selected_decade = None;
            DomainUpdateResult::task(Task::none())
        }
        LibraryUiMessage::SetFilterResolution(r) => {
            state.domains.ui.state.selected_resolution = r;
            DomainUpdateResult::task(Task::none())
        }
        LibraryUiMessage::SetFilterWatchStatus(ws) => {
            state.domains.ui.state.selected_watch_status = ws;
            DomainUpdateResult::task(Task::none())
        }
        LibraryUiMessage::ApplyFilters => {
            bump_keep_alive(state);
            // Reuse RequestFilteredPositions path; it will read current UI fields
            DomainUpdateResult::task(Task::done(DomainMessage::Ui(
                LibraryUiMessage::RequestFilteredPositions.into(),
            )))
        }
        LibraryUiMessage::ClearFilters => {
            bump_keep_alive(state);
            state.domains.ui.state.selected_genres.clear();
            state.domains.ui.state.selected_decade = None;
            state.domains.ui.state.selected_resolution = UiResolution::Any;
            state.domains.ui.state.selected_watch_status = UiWatchStatus::Any;
            DomainUpdateResult::task(Task::done(DomainMessage::Ui(
                LibraryUiMessage::RequestFilteredPositions.into(),
            )))
        }
        LibraryUiMessage::ApplySortedPositions(
            library_id,
            cache_key,
            positions,
        ) => {
            bump_keep_alive(state);

            if let Some(tab) =
                state.tab_manager.get_tab_mut(TabId::Library(library_id))
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
        LibraryUiMessage::SortedIndexFailed(err) => {
            log::warn!("Sorted index fetch failed: {}", err);
            state.domains.ui.state.error_message =
                Some(format!("Unable to apply sort/filter: {}", err));
            DomainUpdateResult::task(Task::none())
        }
    }
}
