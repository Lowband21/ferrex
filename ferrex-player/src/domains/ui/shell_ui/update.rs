use crate::{
    common::messages::{CrossDomainEvent, DomainMessage, DomainUpdateResult},
    domains::{
        metadata::demand_planner::DemandSnapshot,
        ui::{
            tabs::{self, TabId, TabState},
            types::ViewState,
            update_handlers::{
                home_focus,
                home_tab::{
                    emit_initial_all_tab_snapshots_combined, init_all_tab_view,
                },
                navigation_updates, search_updates, virtual_carousel_helpers,
            },
            utils::bump_keep_alive,
            windows,
        },
    },
    infra::{api_types::LibraryType, constants::layout},
    state::State,
};
use ferrex_core::player_prelude::{LibraryId, PosterKind};
use iced::{
    Task,
    widget::{operation::scroll_to, scrollable::AbsoluteOffset},
};
use std::time::Instant;

use super::{Scope, UiShellMessage};

#[cfg(feature = "demo")]
use crate::domains::ui::update_handlers::demo_controls;

/// Helper to build a demand snapshot for a library tab
fn build_library_demand_snapshot(
    lib_state: &tabs::state::LibraryTabState,
) -> DemandSnapshot {
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
    background_ids
        .retain(|id| !visible_ids.contains(id) && !prefetch_ids.contains(id));

    let poster_kind = match lib_state.library_type {
        LibraryType::Movies => Some(PosterKind::Movie),
        LibraryType::Series => Some(PosterKind::Series),
    };

    DemandSnapshot {
        visible_ids,
        prefetch_ids,
        background_ids,
        timestamp: std::time::Instant::now(),
        context: None,
        poster_kind,
    }
}

/// Helper to restore scroll position for a library tab
fn restore_library_tab_scroll(
    state: &State,
    library_id: LibraryId,
) -> Task<DomainMessage> {
    if let Some(TabState::Library(lib_state)) =
        state.tab_manager.get_tab(TabId::Library(library_id))
    {
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
}

pub fn update_shell_ui(
    state: &mut State,
    message: UiShellMessage,
) -> DomainUpdateResult {
    match message {
        UiShellMessage::SelectScope(scope) => {
            // 1. State guard - prevent redundant work
            let current_scope = match state.domains.ui.state.scope {
                Scope::Home
                    if state.domains.ui.state.current_library_id.is_none() =>
                {
                    Scope::Home
                }
                Scope::Library(id) => {
                    match state.domains.ui.state.current_library_id {
                        Some(_) => Scope::Library(id),
                        None => Scope::Home,
                    }
                }
                _ => Scope::Home, // Default to Curated for any other state
            };

            if scope == current_scope {
                log::debug!(
                    "SelectScope: Already in {:?}, short-circuiting",
                    scope
                );
                return DomainUpdateResult::task(Task::none());
            }

            state.domains.ui.state.scope = scope;
            state.domains.ui.state.current_library_id = scope.lib_id();

            // 3. Tab management with scroll restoration
            let tab_id = scope.to_tab_id();
            let scaled_layout = &state.domains.ui.state.scaled_layout;
            state.tab_manager.set_active_tab_with_scroll(
                tab_id,
                &mut state.domains.ui.state.scroll_manager,
                state.window_size.width,
                scaled_layout,
            );
            state.tab_manager.set_active_sort(
                state.domains.ui.state.sort_by,
                state.domains.ui.state.sort_order,
            );

            // 4. Scope-specific initialization
            let mut tasks = vec![];
            let mut events = vec![];

            match scope {
                Scope::Home => {
                    log::info!("Scope changed to Curated (Home libraries)");

                    // Initialize Home tab view
                    init_all_tab_view(state);
                    emit_initial_all_tab_snapshots_combined(state);

                    // Build focus order for Home view
                    let ordered = tabs::ordered_keys_for_home(state);
                    if let Some(TabState::Home(all_state)) =
                        state.tab_manager.get_tab_mut(TabId::Home)
                    {
                        all_state.focus.ordered_keys = ordered;
                        if all_state.focus.active_carousel.is_none() {
                            all_state.focus.active_carousel =
                                all_state.focus.ordered_keys.first().cloned();
                        }
                        // Sync carousel keyboard focus
                        if let Some(key) =
                            all_state.focus.active_carousel.clone()
                        {
                            state
                                .domains
                                .ui
                                .state
                                .carousel_focus
                                .set_keyboard_active(Some(key));
                        }
                    }

                    // Restore carousel scroll positions
                    let restore_task = virtual_carousel_helpers::
                        restore_all_tab_carousel_scroll_positions(state)
                        .map(DomainMessage::Ui);
                    tasks.push(restore_task);

                    // Emit cross-domain event
                    events.push(CrossDomainEvent::LibrarySelectHome);

                    // Keep UI alive for poster fetching
                    bump_keep_alive(state);
                }

                Scope::Library(lib_id) => {
                    log::info!("Scope changed to Library({})", lib_id);

                    // Emit demand snapshot for visible items
                    if let Some(handle) =
                        state.domains.metadata.state.planner_handle.as_ref()
                        && let Some(TabState::Library(lib_state)) =
                            state.tab_manager.get_tab(TabId::Library(lib_id))
                    {
                        let snapshot = build_library_demand_snapshot(lib_state);
                        handle.send(snapshot);
                    }

                    // Restore tab scroll position
                    let scroll_task = restore_library_tab_scroll(state, lib_id);
                    tasks.push(scroll_task);

                    // Emit cross-domain event
                    events.push(CrossDomainEvent::LibrarySelected(lib_id));
                }
            }

            // 5. Refresh active tab content
            state.tab_manager.refresh_active_tab();

            // 6. Trigger view model update
            tasks.push(Task::done(DomainMessage::Ui(
                crate::domains::ui::view_model_ui::ViewModelMessage::UpdateViewModelFilters.into()
            )));

            // 7. Log diagnostic
            log::trace!(
                "SelectScope completed: tab={:?}, events={:?}",
                tab_id,
                events
            );

            DomainUpdateResult::with_events(Task::batch(tasks), events)
        }
        UiShellMessage::OpenSearchWindow => {
            windows::controller::open_search(state, None)
        }
        UiShellMessage::OpenSearchWindowWithSeed(seed) => {
            windows::controller::open_search(state, Some(seed))
        }
        UiShellMessage::SearchWindowOpened(id) => {
            state.search_window_id = Some(id);
            windows::controller::on_search_opened(state, id)
        }
        UiShellMessage::MainWindowOpened(id) => {
            state.windows.set(windows::WindowKind::Main, id);
            DomainUpdateResult::task(Task::none())
        }
        UiShellMessage::MainWindowFocused => {
            // When regaining focus, re-emit initial snapshots to ensure images load
            init_all_tab_view(state);
            emit_initial_all_tab_snapshots_combined(state);
            bump_keep_alive(state);
            DomainUpdateResult::task(Task::none())
        }
        UiShellMessage::MainWindowUnfocused => {
            // No special handling currently; keep behavior simple
            DomainUpdateResult::task(Task::none())
        }
        UiShellMessage::RawWindowClosed(id) => {
            windows::controller::on_raw_window_closed(state, id)
        }
        UiShellMessage::FocusSearchWindow => {
            windows::controller::focus_search(state)
        }
        UiShellMessage::FocusSearchInput => {
            windows::controller::focus_search_input(state)
        }
        UiShellMessage::CloseSearchWindow => {
            windows::controller::close_search(state)
        }
        UiShellMessage::ToggleFullscreen => DomainUpdateResult::with_events(
            Task::none(),
            vec![CrossDomainEvent::MediaToggleFullscreen],
        ),
        UiShellMessage::SelectLibraryAndMode(library_id) => {
            log::warn!(
                "Legacy SelectLibraryAndMode called - migrating to SelectScope"
            );
            update_shell_ui(
                state,
                UiShellMessage::SelectScope(Scope::Library(library_id)),
            )
        }
        UiShellMessage::ViewMovieDetails(movie_ref) => {
            let task =
                navigation_updates::handle_view_movie_details(state, movie_ref);
            DomainUpdateResult::task(task.map(DomainMessage::Ui))
        }
        UiShellMessage::ViewTvShow(series_id) => {
            let task = navigation_updates::handle_view_series(state, series_id);
            DomainUpdateResult::task(task.map(DomainMessage::Ui))
        }
        UiShellMessage::ViewSeason(series_id, season_id) => {
            let task = navigation_updates::handle_view_season(
                state, series_id, season_id,
            );
            DomainUpdateResult::task(task.map(DomainMessage::Ui))
        }
        UiShellMessage::ViewEpisode(episode_id) => {
            let task =
                navigation_updates::handle_view_episode(state, episode_id);
            DomainUpdateResult::task(task.map(DomainMessage::Ui))
        }
        UiShellMessage::NavigateHome => {
            // Clear navigation history when going home
            state.domains.ui.state.navigation_history.clear();

            // Reset UI state to library view
            state.domains.ui.state.view = ViewState::Library;

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
                    None, // Curated view has no specific library
                );

            // Delegate all scope change logic to SelectScope
            update_shell_ui(state, UiShellMessage::SelectScope(Scope::Home))
        }
        UiShellMessage::NavigateBack => {
            // Navigate to the previous view in history
            let library_id =
                state.domains.ui.state.scope.lib_id().map(|id| id.to_uuid());

            match state.domains.ui.state.navigation_history.pop() {
                Some(ref previous_view) => {
                    let _current_view = state.domains.ui.state.view.clone();
                    if matches!(previous_view, _current_view) {
                        log::warn!(
                            "NavigateBack popped the same view as current: {:?}",
                            previous_view
                        );
                    }
                    state.domains.ui.state.view = previous_view.clone();

                    // Restore scroll state when returning to views
                    match &previous_view {
                        ViewState::Library => {
                            // Determine library context based on display mode
                            let library_id = match state.domains.ui.state.scope
                            {
                                Scope::Library(lib_id) => Some(lib_id),
                                Scope::Home => None,
                            };

                            // Restore scroll state through TabManager with ScrollPositionManager
                            let tab_id = if let Some(lib_id) = library_id {
                                TabId::Library(lib_id)
                            } else {
                                TabId::Home
                            };

                            // Use the scroll-aware tab switching which automatically restores position
                            let scaled_layout =
                                &state.domains.ui.state.scaled_layout;
                            state.tab_manager.set_active_tab_with_scroll(
                                tab_id,
                                &mut state.domains.ui.state.scroll_manager,
                                state.window_size.width,
                                scaled_layout,
                            );
                            state.tab_manager.set_active_sort(
                                state.domains.ui.state.sort_by,
                                state.domains.ui.state.sort_order,
                            );

                            state.tab_manager.refresh_active_tab();

                            // Explicitly restore scroll position after tab switch
                            let scroll_task = if let Some(tab) =
                                state.tab_manager.get_tab(tab_id)
                            {
                                if let Some(grid_state) = tab.grid_state() {
                                    let scroll_position =
                                        grid_state.scroll_position;
                                    let scrollable_id =
                                        grid_state.scrollable_id.clone();
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
                                .reset_to_view_colors(previous_view);

                            let library_id = state
                                .domains
                                .ui
                                .state
                                .scope
                                .lib_id()
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
                            log::debug!(
                                "Navigated back to view: {:?}",
                                previous_view
                            );
                        }
                    }

                    // Reset colors if returning to a non-detail view
                    state
                        .domains
                        .ui
                        .state
                        .background_shader_state
                        .reset_to_view_colors(previous_view);

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
                    let library_id = match state.domains.ui.state.scope {
                        Scope::Library(id) => Some(id),
                        Scope::Home => None,
                    };

                    log::debug!(
                        "NavigateBack with no history: preserving display mode {:?}",
                        state.domains.ui.state.scope
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
        UiShellMessage::UpdateSearchQuery(query) => {
            let mut result = search_updates::update_search_query(state, query);

            if state.windows.get(windows::WindowKind::Search).is_none() {
                let open = windows::controller::open_search(state, None);
                result.task = Task::batch([result.task, open.task]);
                result.events.extend(open.events);
            }

            result
        }
        UiShellMessage::BeginSearchFromKeyboard(seed) => {
            search_updates::begin_search_from_keyboard(state, seed)
        }
        UiShellMessage::ExecuteSearch => {
            // Forward directly to search domain
            DomainUpdateResult::task(Task::done(DomainMessage::Search(
                crate::domains::search::messages::SearchMessage::ExecuteSearch,
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrex_core::player_prelude::LibraryId;
    use uuid::Uuid;

    #[test]
    fn test_scope_to_tab_id() {
        assert_eq!(Scope::Home.to_tab_id(), TabId::Home);

        let lib_id = LibraryId(Uuid::new_v4());
        assert_eq!(Scope::Library(lib_id).to_tab_id(), TabId::Library(lib_id));
    }

    #[test]
    fn test_scope_equality() {
        // Test Home equality
        let home1 = Scope::Home;
        let home2 = Scope::Home;
        assert_eq!(home1, home2);

        // Test Library equality with same ID
        let lib_id = LibraryId(Uuid::new_v4());
        let library1 = Scope::Library(lib_id);
        let library2 = Scope::Library(lib_id);
        assert_eq!(library1, library2);

        // Test inequality with different IDs
        let lib_id2 = LibraryId(Uuid::new_v4());
        let library3 = Scope::Library(lib_id2);
        assert_ne!(library1, library3);

        // Test inequality between Home and Library
        assert_ne!(home1, library1);
    }

    #[test]
    fn test_scope_debug_format() {
        let home = Scope::Home;
        assert_eq!(format!("{:?}", home), "Home");

        let lib_id = LibraryId(Uuid::new_v4());
        let library = Scope::Library(lib_id);
        let debug_str = format!("{:?}", library);
        assert!(debug_str.starts_with("Library("));
        assert!(debug_str.contains(&lib_id.to_string()));
    }
}
