use crate::{
    common::messages::{CrossDomainEvent, DomainMessage, DomainUpdateResult},
    domains::library,
    domains::ui::messages as ui,
    domains::ui::scroll_manager::ScrollStateExt,
    domains::ui::types::{DisplayMode, ViewState},
    domains::ui::view_models::ViewModel,
    domains::ui::views::carousel::CarouselMessage,
    state_refactored::State,
};
use iced::Task;
use std::sync::Arc;

/// Check if user has PIN - returns a task that sends a Settings message
fn check_user_has_pin() -> DomainUpdateResult {
    // Send message to Settings domain to check if user has PIN
    DomainUpdateResult::task(Task::done(DomainMessage::Settings(
        crate::domains::settings::messages::Message::CheckUserHasPin,
    )))
}

/// Handle UI domain messages
/// Returns a DomainUpdateResult containing both the task and any events to emit
pub fn update_ui(state: &mut State, message: ui::Message) -> DomainUpdateResult {
    #[cfg(any(feature = "profile-with-puffin", feature = "profile-with-tracy", feature = "profile-with-tracing"))]
    profiling::scope!(crate::infrastructure::profiling_scopes::scopes::UI_UPDATE);
    
    match message {
        ui::Message::SetDisplayMode(display_mode) => {
            state.domains.ui.state.display_mode = display_mode;

            // NEW ARCHITECTURE: Activate appropriate tab in TabManager
            match display_mode {
                DisplayMode::Curated => {
                    // Activate the All tab with scroll restoration
                    state
                        .tab_manager
                        .set_active_tab_with_scroll(
                            crate::domains::ui::tabs::TabId::All,
                            &mut state.domains.ui.state.scroll_manager
                        );
                    log::info!("Tab activated: All (Curated mode)");

                    // Note: All tab uses carousel view, not scrollable grid, so no scroll restoration needed

                    // Show all libraries in curated view
                    state.domains.ui.state.current_library_id = None;

                    // Update AllViewModel for curated view
                    state.all_view_model.set_library_filter(None);
                }
                DisplayMode::Library => {
                    // Show current library
                    let library_id = state.domains.ui.state.current_library_id;

                    // Activate the library tab if we have a library selected
                    if let Some(lib_id) = library_id {
                        state
                            .tab_manager
                            .set_active_tab_with_scroll(
                                crate::domains::ui::tabs::TabId::Library(lib_id),
                                &mut state.domains.ui.state.scroll_manager
                            );
                        log::info!("Tab activated: Library({}) (Library mode)", lib_id);
                    }

                    // Update AllViewModel with library filter
                    state.all_view_model.set_library_filter(library_id);
                }
                _ => {
                    // Other modes not implemented yet
                    log::info!("Display mode {:?} not implemented yet", display_mode);
                }
            }

            // Refresh views
            state.all_view_model.refresh_from_store();

            // NEW ARCHITECTURE: Also refresh the active tab
            state.tab_manager.refresh_active_tab();

            DomainUpdateResult::task(Task::none())
        }
        ui::Message::SelectLibraryAndMode(library_id) => {
            // NEW ARCHITECTURE: Activate the library tab in TabManager with scroll restoration
            state
                .tab_manager
                .set_active_tab_with_scroll(
                    crate::domains::ui::tabs::TabId::Library(library_id),
                    &mut state.domains.ui.state.scroll_manager
                );
            log::info!("Tab activated: Library({})", library_id);

            // Create scroll restoration task for the newly active tab
            // Note: We ignore the scroll_to result since it returns () and we just need to trigger the scroll
            let scroll_task = {
                let tab_id = crate::domains::ui::tabs::TabId::Library(library_id);
                if let Some(tab) = state.tab_manager.get_tab(tab_id) {
                    if let crate::domains::ui::tabs::TabState::Library(lib_state) = tab {
                        // Restore scroll position (or snap to 0 if no position stored)
                        let scroll_position = lib_state.grid_state.scroll_position;
                        let scrollable_id = lib_state.grid_state.scrollable_id.clone();
                        log::debug!(
                            "Restoring scroll position {} for library {}",
                            scroll_position,
                            library_id
                        );
                        iced::widget::scrollable::scroll_to::<DomainMessage>(
                            scrollable_id,
                            iced::widget::scrollable::AbsoluteOffset {
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
        ui::Message::ViewDetails(media) => {
            let task =
                super::update_handlers::navigation_updates::handle_view_details(state, media);
            DomainUpdateResult::task(task.map(DomainMessage::Ui))
        }
        ui::Message::ViewMovieDetails(movie_ref) => {
            let task = super::update_handlers::navigation_updates::handle_view_movie_details(
                state, movie_ref,
            );
            DomainUpdateResult::task(task.map(DomainMessage::Ui))
        }
        ui::Message::ViewTvShow(series_id) => {
            let task =
                super::update_handlers::navigation_updates::handle_view_tv_show(state, series_id);
            DomainUpdateResult::task(task.map(DomainMessage::Ui))
        }
        ui::Message::ViewSeason(series_id, season_id) => {
            let task = super::update_handlers::navigation_updates::handle_view_season(
                state, series_id, season_id,
            );
            DomainUpdateResult::task(task.map(DomainMessage::Ui))
        }
        ui::Message::ViewEpisode(episode_id) => {
            let task =
                super::update_handlers::navigation_updates::handle_view_episode(state, episode_id);
            DomainUpdateResult::task(task.map(DomainMessage::Ui))
        }
        ui::Message::SetSortBy(sort_by) => {
            state.domains.ui.state.sort_by = sort_by;

            // Use SortingService for optimized parallel sorting on background threads
            let media_store = Arc::clone(&state.domains.media.state.media_store);
            let sort_order = state.domains.ui.state.sort_order;

            DomainUpdateResult::task(
                Task::perform(
                    async move {
                        let sorting_service =
                            crate::domains::media::store::SortingService::new(media_store);

                        // Sort movies and series in parallel on background threads
                        if let Err(e) = sorting_service.sort_all_async(sort_by, sort_order).await {
                            log::error!("Failed to sort media: {}", e);
                        }
                    },
                    |_| ui::Message::RefreshViewModels,
                )
                .map(DomainMessage::Ui),
            )
        }
        ui::Message::ToggleSortOrder => {
            state.domains.ui.state.sort_order = match state.domains.ui.state.sort_order {
                crate::domains::ui::types::SortOrder::Ascending => {
                    crate::domains::ui::types::SortOrder::Descending
                }
                crate::domains::ui::types::SortOrder::Descending => {
                    crate::domains::ui::types::SortOrder::Ascending
                }
            };

            // Use SortingService for optimized parallel sorting on background threads
            let media_store = Arc::clone(&state.domains.media.state.media_store);
            let sort_order = state.domains.ui.state.sort_order;
            let sort_by = state.domains.ui.state.sort_by;

            DomainUpdateResult::task(
                Task::perform(
                    async move {
                        let sorting_service =
                            crate::domains::media::store::SortingService::new(media_store);

                        // Sort movies and series in parallel on background threads
                        if let Err(e) = sorting_service.sort_all_async(sort_by, sort_order).await {
                            log::error!("Failed to sort media: {}", e);
                        }
                    },
                    |_| ui::Message::RefreshViewModels,
                )
                .map(DomainMessage::Ui),
            )
        }
        ui::Message::ShowAdminDashboard => {
            state.domains.ui.state.view = ViewState::AdminDashboard;
            DomainUpdateResult::task(Task::none())
        }
        ui::Message::HideAdminDashboard => {
            state.domains.ui.state.view = ViewState::Library;
            DomainUpdateResult::task(Task::none())
        }
        ui::Message::ShowLibraryManagement => {
            // Save current view to navigation history
            state
                .domains
                .ui
                .state
                .navigation_history
                .push(state.domains.ui.state.view.clone());

            state.domains.ui.state.view = ViewState::LibraryManagement;
            state.domains.library.state.show_library_management = true;

            // Request library refresh if needed
            if state.domains.library.state.libraries.is_empty() {
                DomainUpdateResult::with_events(
                    Task::none(),
                    vec![CrossDomainEvent::RequestLibraryRefresh],
                )
            } else {
                DomainUpdateResult::task(Task::none())
            }
        }
        ui::Message::HideLibraryManagement => {
            state.domains.ui.state.view = ViewState::Library;
            state.domains.library.state.show_library_management = false;
            state.domains.library.state.library_form_data = None; // Clear form when leaving management view
            DomainUpdateResult::task(Task::none())
        }
        ui::Message::ShowClearDatabaseConfirm => {
            state.domains.ui.state.show_clear_database_confirm = true;
            DomainUpdateResult::task(Task::none())
        }
        ui::Message::HideClearDatabaseConfirm => {
            state.domains.ui.state.show_clear_database_confirm = false;
            DomainUpdateResult::task(Task::none())
        }
        ui::Message::ClearDatabase => {
            let task = crate::common::clear_database::handle_clear_database(state);
            DomainUpdateResult::task(task)
        }
        ui::Message::DatabaseCleared(result) => {
            let task = crate::common::clear_database::handle_database_cleared(state, result);
            DomainUpdateResult::task(task)
        }
        ui::Message::ClearError => {
            state.domains.ui.state.error_message = None;
            DomainUpdateResult::task(Task::none())
        }
        ui::Message::MoviesGridScrolled(viewport) => {
            let task = super::update_handlers::scroll_updates::handle_movies_grid_scrolled(
                state, viewport,
            );
            DomainUpdateResult::task(task.map(DomainMessage::Ui))
        }
        ui::Message::TvShowsGridScrolled(viewport) => {
            let task = super::update_handlers::scroll_updates::handle_tv_shows_grid_scrolled(
                state, viewport,
            );
            DomainUpdateResult::task(task.map(DomainMessage::Ui))
        }
        ui::Message::TabGridScrolled(viewport) => {
            let task =
                super::update_handlers::scroll_updates::handle_tab_grid_scrolled(state, viewport);
            DomainUpdateResult::task(task.map(DomainMessage::Ui))
        }
        ui::Message::CheckScrollStopped => {
            // Check if scrolling has actually stopped
            if let Some(last_time) = state.domains.ui.state.last_scroll_time {
                let elapsed = last_time.elapsed();

                // Only process if enough time has passed since last scroll event
                if elapsed >= std::time::Duration::from_millis(
                    crate::infrastructure::performance_config::scrolling::SCROLL_STOP_DEBOUNCE_MS
                ) {
                    // Double-check that we're not still scrolling
                    if state.domains.ui.state.scroll_velocity > 0.0 {
                        // Still have velocity, might be mid-scroll
                        log::debug!("CheckScrollStopped called but velocity still {:.0} px/s, ignoring",
                                  state.domains.ui.state.scroll_velocity);
                        DomainUpdateResult::task(Task::none())
                    } else {
                        // Scrolling has truly stopped
                        log::info!("Scrolling confirmed stopped after {:.0}ms, upgrading visible poster priorities",
                                  elapsed.as_millis());

                        // Re-request visible items with Priority::Visible
                        if let Some(image_service) = crate::infrastructure::service_registry::get_image_service() {
                            use crate::domains::metadata::image_types::{ImageRequest, ImageSize, Priority};
                            use ferrex_core::api_types::MediaId;

                            // When in Library view, we need to check what content is currently shown
                            // based on the current library type or selected tab
                            let visible_items = match state.domains.ui.state.view {
                                ViewState::Library => {
                                    // Get visible items from all view models since we don't know which is active
                                    // The view models will only return items if they have visible content
                                    // Get visible items from active tab
                                    let mut items = state.all_view_model.get_visible_items();
                                    let tv_items = crate::domains::ui::view_models::VisibleItems {
                                        movies: vec![],
                                        series: vec![],
                                    };
                                    let all_items = state.all_view_model.get_visible_items();

                                    // Combine all visible items
                                    items.movies.extend(all_items.movies);
                                    items.series.extend(tv_items.series);
                                    items.series.extend(all_items.series);

                                    items
                                }
                                _ => {
                                    // For other views, just get from all view model
                                    ViewModel::get_visible_items(&state.all_view_model)
                                }
                            };

                            log::info!("Re-requesting {} movies and {} series with VISIBLE priority",
                                      visible_items.movies.len(), visible_items.series.len());

                            // Re-request movie posters with Visible priority
                            for movie in visible_items.movies {
                                let request = ImageRequest {
                                    media_id: MediaId::Movie(movie.id.clone()),
                                    size: ImageSize::Poster,
                                    priority: Priority::Visible,
                                };
                                image_service.get().request_image(request);
                            }

                            // Re-request series posters with Visible priority
                            for series in visible_items.series {
                                let request = ImageRequest {
                                    media_id: MediaId::Series(series.id.clone()),
                                    size: ImageSize::Poster,
                                    priority: Priority::Visible,
                                };
                                image_service.get().request_image(request);
                            }
                        }

                        // Clear scroll velocity and fast scrolling flag
                        state.domains.ui.state.scroll_velocity = 0.0;
                        state.domains.ui.state.fast_scrolling = false;
                        state.domains.ui.state.scroll_stopped_time = Some(std::time::Instant::now());

                        DomainUpdateResult::task(Task::none())
                    }
                } else {
                    // Not enough time has passed, ignore this check
                    DomainUpdateResult::task(Task::none())
                }
            } else {
                // No last scroll time recorded
                DomainUpdateResult::task(Task::none())
            }
        }
        ui::Message::RecalculateGridsAfterResize => {
            // Force recalculation of visible items for both grids
            // Force recalculation of visible items
            // Note: visible_range fields don't exist in current state

            // Queue recalculation on next scroll event or view update
            log::debug!("Grid recalculation queued after resize");

            DomainUpdateResult::task(Task::none())
        }
        ui::Message::DetailViewScrolled(viewport) => DomainUpdateResult::task(
            super::update_handlers::scroll_updates::handle_detail_view_scrolled(state, viewport)
                .map(DomainMessage::Ui),
        ),
        ui::Message::WindowResized(size) => DomainUpdateResult::task(
            super::update_handlers::window_update::handle_window_resized(state, size)
                .map(DomainMessage::Ui),
        ),
        ui::Message::MediaHovered(media_id) => {
            state.domains.ui.state.hovered_media_id = Some(media_id);
            DomainUpdateResult::task(Task::none())
        }
        ui::Message::MediaUnhovered(media_id) => {
            // Only clear hover state if it matches the media being unhovered
            // This prevents race conditions when quickly moving between posters
            if state.domains.ui.state.hovered_media_id.as_ref() == Some(&media_id) {
                state.domains.ui.state.hovered_media_id = None;
            }
            DomainUpdateResult::task(Task::none())
        }
        ui::Message::NavigateHome => {
            state.domains.ui.state.view = ViewState::Library;
            state.domains.ui.state.display_mode = DisplayMode::Curated;

            // REMOVED: No longer clearing duplicate state fields
            // MediaStore is the single source of truth

            // For curated mode (All libraries), activate the All tab with scroll restoration
            state
                .tab_manager
                .set_active_tab_with_scroll(
                    crate::domains::ui::tabs::TabId::All,
                    &mut state.domains.ui.state.scroll_manager
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
                    state.domains.library.state.current_library_id,
                );

            DomainUpdateResult::task(Task::none())
        }
        ui::Message::NavigateBack => {
            // Navigate to the previous view in history
            if let Some(previous_view) = state.domains.ui.state.navigation_history.pop() {
                state.domains.ui.state.view = previous_view.clone();

                // Restore scroll state when returning to views
                match &previous_view {
                    ViewState::Library => {
                        // Determine library context based on display mode
                        let library_id = match state.domains.ui.state.display_mode {
                            DisplayMode::Library => state.domains.library.state.current_library_id,
                            DisplayMode::Curated => None, // All libraries
                            _ => None,
                        };

                        // Restore scroll state through TabManager with ScrollPositionManager
                        let tab_id = if let Some(lib_id) = library_id {
                            crate::domains::ui::tabs::TabId::Library(lib_id)
                        } else {
                            crate::domains::ui::tabs::TabId::All
                        };
                        
                        // Use the scroll-aware tab switching which automatically restores position
                        state.tab_manager.set_active_tab_with_scroll(
                            tab_id,
                            &mut state.domains.ui.state.scroll_manager
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
                                iced::widget::scrollable::scroll_to::<DomainMessage>(
                                    scrollable_id,
                                    iced::widget::scrollable::AbsoluteOffset {
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
                        
                        state
                            .domains
                            .ui
                            .state
                            .background_shader_state
                            .update_depth_lines(
                                &state.domains.ui.state.view,
                                state.window_size.width,
                                state.window_size.height,
                                state.domains.library.state.current_library_id,
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
                        state.domains.library.state.current_library_id,
                    );

                DomainUpdateResult::task(Task::none())
            } else {
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
        ui::Message::UpdateSearchQuery(query) => {
            // Update local UI state so the text input shows the current value
            state.domains.ui.state.search_query = query.clone();

            // Preserve scroll position when search query changes
            // The UI rebuild causes scrollables to reset, so we need to restore position
            let scroll_restore_task =
                if let Some(tab) = state.tab_manager.get_tab(state.tab_manager.active_tab_id()) {
                    if let crate::domains::ui::tabs::TabState::Library(lib_state) = tab {
                        let scroll_position = lib_state.grid_state.scroll_position;
                        let scrollable_id = lib_state.grid_state.scrollable_id.clone();
                        iced::widget::scrollable::scroll_to::<DomainMessage>(
                            scrollable_id,
                            iced::widget::scrollable::AbsoluteOffset {
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

            // Forward to search domain and restore scroll position
            DomainUpdateResult::task(Task::batch([
                Task::done(DomainMessage::Search(
                    crate::domains::search::messages::Message::UpdateQuery(query),
                )),
                scroll_restore_task,
            ]))
        }
        ui::Message::ExecuteSearch => {
            // Forward directly to search domain
            DomainUpdateResult::task(Task::done(DomainMessage::Search(
                crate::domains::search::messages::Message::ExecuteSearch,
            )))
        }
        ui::Message::ShowLibraryMenu => {
            state.domains.ui.state.show_library_menu = !state.domains.ui.state.show_library_menu;
            DomainUpdateResult::task(Task::none())
        }
        ui::Message::ShowAllLibrariesMenu => {
            state.domains.ui.state.show_library_menu = !state.domains.ui.state.show_library_menu;
            state.domains.ui.state.library_menu_target = None;
            DomainUpdateResult::task(Task::none())
        }
        ui::Message::ShowProfile => {
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
                    |enabled| ui::Message::AutoLoginToggled(Ok(enabled)),
                )
                .map(DomainMessage::Ui),
            )
        }

        ui::Message::ShowUserProfile => {
            state.domains.settings.current_view =
                crate::domains::settings::state::SettingsView::Profile;
            DomainUpdateResult::task(Task::none())
        }

        ui::Message::ShowUserPreferences => {
            state.domains.settings.current_view =
                crate::domains::settings::state::SettingsView::Preferences;
            DomainUpdateResult::task(Task::none())
        }

        ui::Message::ShowUserSecurity => {
            state.domains.settings.current_view =
                crate::domains::settings::state::SettingsView::Security;
            // Check if user has PIN when entering security settings
            check_user_has_pin()
        }

        ui::Message::ShowDeviceManagement => {
            state.domains.settings.current_view =
                crate::domains::settings::state::SettingsView::DeviceManagement;
            // Load devices when the view is shown - send direct message to Settings domain
            DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                crate::domains::settings::messages::Message::LoadDevices,
            )))
        }

        ui::Message::BackToSettings => {
            state.domains.ui.state.view = ViewState::UserSettings;
            state.domains.settings.current_view =
                crate::domains::settings::state::SettingsView::Main;
            // Clear any security settings state
            state.domains.settings.security = Default::default();
            DomainUpdateResult::task(Task::none())
        }

        // Security settings handlers - emit cross-domain events to Settings domain
        ui::Message::ShowChangePassword => {
            DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                crate::domains::settings::messages::Message::ShowChangePassword,
            )))
        }

        ui::Message::UpdatePasswordCurrent(value) => {
            DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                crate::domains::settings::messages::Message::UpdatePasswordCurrent(value),
            )))
        }

        ui::Message::UpdatePasswordNew(value) => {
            DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                crate::domains::settings::messages::Message::UpdatePasswordNew(value),
            )))
        }

        ui::Message::UpdatePasswordConfirm(value) => {
            DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                crate::domains::settings::messages::Message::UpdatePasswordConfirm(value),
            )))
        }

        ui::Message::TogglePasswordVisibility => {
            DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                crate::domains::settings::messages::Message::TogglePasswordVisibility,
            )))
        }

        ui::Message::SubmitPasswordChange => {
            DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                crate::domains::settings::messages::Message::SubmitPasswordChange,
            )))
        }

        ui::Message::PasswordChangeResult(result) => {
            // UI handles displaying the result
            DomainUpdateResult::task(Task::none())
        }

        ui::Message::CancelPasswordChange => {
            DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                crate::domains::settings::messages::Message::CancelPasswordChange,
            )))
        }

        ui::Message::ShowSetPin => DomainUpdateResult::task(Task::done(DomainMessage::Settings(
            crate::domains::settings::messages::Message::ShowSetPin,
        ))),

        ui::Message::ShowChangePin => DomainUpdateResult::task(Task::done(
            DomainMessage::Settings(crate::domains::settings::messages::Message::ShowChangePin),
        )),

        ui::Message::UpdatePinCurrent(value) => {
            DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                crate::domains::settings::messages::Message::UpdatePinCurrent(value),
            )))
        }

        ui::Message::UpdatePinNew(value) => {
            DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                crate::domains::settings::messages::Message::UpdatePinNew(value),
            )))
        }

        ui::Message::UpdatePinConfirm(value) => {
            DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                crate::domains::settings::messages::Message::UpdatePinConfirm(value),
            )))
        }

        ui::Message::SubmitPinChange => DomainUpdateResult::task(Task::done(
            DomainMessage::Settings(crate::domains::settings::messages::Message::SubmitPinChange),
        )),

        ui::Message::PinChangeResult(result) => {
            // UI handles displaying the result
            DomainUpdateResult::task(Task::none())
        }

        ui::Message::CancelPinChange => DomainUpdateResult::task(Task::done(
            DomainMessage::Settings(crate::domains::settings::messages::Message::CancelPinChange),
        )),

        // User preferences - emit cross-domain events to Settings domain
        ui::Message::ToggleAutoLogin(enabled) => {
            DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                crate::domains::settings::messages::Message::ToggleAutoLogin(enabled),
            )))
        }

        ui::Message::AutoLoginToggled(result) => {
            // UI handles displaying the result
            DomainUpdateResult::task(Task::none())
        }

        // Device management - send direct messages to Settings domain
        ui::Message::LoadDevices => {
            // Send direct message to Settings domain to load devices
            DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                crate::domains::settings::messages::Message::LoadDevices,
            )))
        }

        ui::Message::DevicesLoaded(result) => {
            // This message should now come from settings domain, but kept for compatibility
            log::warn!("DevicesLoaded should now come from settings domain via cross-domain event");
            DomainUpdateResult::task(Task::none())
        }

        ui::Message::RefreshDevices => {
            // Send direct message to Settings domain to refresh devices
            DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                crate::domains::settings::messages::Message::RefreshDevices,
            )))
        }

        ui::Message::RevokeDevice(device_id) => {
            // Send direct message to Settings domain to revoke device
            DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                crate::domains::settings::messages::Message::RevokeDevice(device_id),
            )))
        }

        ui::Message::DeviceRevoked(result) => {
            // This message should now come from settings domain, but kept for compatibility
            log::warn!("DeviceRevoked should now come from settings domain via cross-domain event");
            DomainUpdateResult::task(Task::none())
        }

        ui::Message::Logout => {
            // Proxy to auth domain for logout via cross-domain event
            DomainUpdateResult::with_events(Task::none(), vec![CrossDomainEvent::UserLoggedOut])
        }
        ui::Message::CarouselNavigation(carousel_msg) => DomainUpdateResult::task(
            handle_carousel_navigation(state, carousel_msg).map(DomainMessage::Ui),
        ),
        ui::Message::UpdateTransitions => {
            // Update all active transitions
            state
                .domains
                .ui
                .state
                .background_shader_state
                .color_transitions
                .update();
            state
                .domains
                .ui
                .state
                .background_shader_state
                .backdrop_transitions
                .update();
            state
                .domains
                .ui
                .state
                .background_shader_state
                .gradient_transitions
                .update();

            // Update the actual colors based on transition progress
            let (primary, secondary) = state
                .domains
                .ui
                .state
                .background_shader_state
                .color_transitions
                .get_interpolated_colors();
            state.domains.ui.state.background_shader_state.primary_color = primary;
            state
                .domains
                .ui
                .state
                .background_shader_state
                .secondary_color = secondary;

            // Update the gradient center based on transition progress
            state
                .domains
                .ui
                .state
                .background_shader_state
                .gradient_center = state
                .domains
                .ui
                .state
                .background_shader_state
                .gradient_transitions
                .get_interpolated_center();

            DomainUpdateResult::task(Task::none())
        }
        ui::Message::ToggleBackdropAspectMode => {
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
                crate::domains::ui::background_state::BackdropAspectMode::Auto => {
                    crate::domains::ui::background_state::BackdropAspectMode::Force21x9
                }
                crate::domains::ui::background_state::BackdropAspectMode::Force21x9 => {
                    crate::domains::ui::background_state::BackdropAspectMode::Auto
                }
            };
            DomainUpdateResult::task(Task::none())
        }
        ui::Message::UpdateBackdropHandle(_handle) => {
            // Deprecated - backdrops are now pulled reactively from image service
            // This message handler kept for compatibility but does nothing
            DomainUpdateResult::task(Task::none())
        }
        ui::Message::CheckMediaStoreRefresh => {
            // Check if MediaStore notifier indicates a refresh is needed
            if state.media_store_notifier.should_refresh() {
                log::debug!(
                    "[MediaStoreNotifier] ViewModels refresh needed - triggering RefreshViewModels"
                );
                DomainUpdateResult::task(
                    Task::done(ui::Message::RefreshViewModels).map(DomainMessage::Ui),
                )
            } else {
                DomainUpdateResult::task(Task::none())
            }
        }
        ui::Message::RefreshViewModels => {
            // Refresh view models - pull latest data from MediaStore
            log::info!("[MediaStoreNotifier] RefreshViewModels triggered - updating view models with latest MediaStore data");

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
            log::info!(
                "UI: View models refreshed with {} movies, {} series in AllViewModel",
                state.all_view_model.all_movies().len(),
                state.all_view_model.all_series().len()
            );

            // NEW ARCHITECTURE: Refresh TabManager tabs with sorted data
            // This ensures the tab-based views show the newly sorted content
            // Refresh only the active tab for better performance
            state.tab_manager.mark_tab_needs_refresh(state.tab_manager.active_tab_id());
            state.tab_manager.refresh_active_tab();
            log::info!("TabManager: All tabs refreshed with sorted data from MediaStore");

            DomainUpdateResult::task(Task::none())
        }
        ui::Message::UpdateViewModelFilters => {
            // Lightweight update - just change filters without re-reading from MediaStore
            let library_filter = match state.domains.ui.state.display_mode {
                DisplayMode::Library => state.domains.ui.state.current_library_id,
                DisplayMode::Curated => None, // Always show all in curated mode
                _ => None,
            };

            log::info!("UI: UpdateViewModelFilters called - library_filter = {:?}, display_mode = {:?}, ui.current_library_id = {:?}, library.current_library_id = {:?}",
                library_filter, state.domains.ui.state.display_mode, state.domains.ui.state.current_library_id, state.domains.library.state.current_library_id);

            // Always update AllViewModel as it handles both types
            state.all_view_model.set_library_filter(library_filter);

            log::info!(
                "UI: Filter updated - All: {} movies + {} series",
                state.all_view_model.all_movies().len(),
                state.all_view_model.all_series().len()
            );

            DomainUpdateResult::task(Task::none()) // View will update on next frame
        }

        ui::Message::QueueVisibleDetailsForFetch => {
            // TODO: Implement queue visible details for fetch
            log::debug!("Queue visible details for fetch requested");
            DomainUpdateResult::task(Task::none())
        }

        // Cross-domain proxy messages
        ui::Message::ToggleFullscreen => {
            // Forward to media domain
            DomainUpdateResult::with_events(
                Task::none(),
                vec![CrossDomainEvent::MediaToggleFullscreen],
            )
        }
        ui::Message::ToggleScanProgress => {
            // Send direct message to library domain
            DomainUpdateResult::task(Task::done(DomainMessage::Library(
                library::messages::Message::ToggleScanProgress,
            )))
        }
        ui::Message::SelectLibrary(library_id) => {
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
        ui::Message::PlayMediaWithId(media_file, media_id) => {
            // Forward to media domain
            DomainUpdateResult::with_events(
                Task::none(),
                vec![CrossDomainEvent::MediaPlayWithId(media_file, media_id)],
            )
        }

        // Library management proxies
        ui::Message::ShowLibraryForm(library) => {
            // Send direct message to library domain
            DomainUpdateResult::task(Task::done(DomainMessage::Library(
                library::messages::Message::ShowLibraryForm(library),
            )))
        }
        ui::Message::HideLibraryForm => {
            // Send direct message to library domain
            DomainUpdateResult::task(Task::done(DomainMessage::Library(
                library::messages::Message::HideLibraryForm,
            )))
        }
        ui::Message::ScanLibrary_(library_id) => {
            // Send direct message to library domain
            DomainUpdateResult::task(Task::done(DomainMessage::Library(
                library::messages::Message::ScanLibrary(library_id),
            )))
        }
        ui::Message::DeleteLibrary(library_id) => {
            // Send direct message to library domain
            DomainUpdateResult::task(Task::done(DomainMessage::Library(
                library::messages::Message::DeleteLibrary(library_id),
            )))
        }
        ui::Message::UpdateLibraryFormName(name) => {
            // Send direct message to library domain
            DomainUpdateResult::task(Task::done(DomainMessage::Library(
                library::messages::Message::UpdateLibraryFormName(name),
            )))
        }
        ui::Message::UpdateLibraryFormType(library_type) => {
            // Send direct message to library domain
            DomainUpdateResult::task(Task::done(DomainMessage::Library(
                library::messages::Message::UpdateLibraryFormType(library_type),
            )))
        }
        ui::Message::UpdateLibraryFormPaths(paths) => {
            // Send direct message to library domain
            DomainUpdateResult::task(Task::done(DomainMessage::Library(
                library::messages::Message::UpdateLibraryFormPaths(paths),
            )))
        }
        ui::Message::UpdateLibraryFormScanInterval(interval) => {
            // Send direct message to library domain
            DomainUpdateResult::task(Task::done(DomainMessage::Library(
                library::messages::Message::UpdateLibraryFormScanInterval(interval),
            )))
        }
        ui::Message::ToggleLibraryFormEnabled => {
            // Send direct message to library domain
            DomainUpdateResult::task(Task::done(DomainMessage::Library(
                library::messages::Message::ToggleLibraryFormEnabled,
            )))
        }
        ui::Message::SubmitLibraryForm => {
            // Send direct message to library domain
            DomainUpdateResult::task(Task::done(DomainMessage::Library(
                library::messages::Message::SubmitLibraryForm,
            )))
        }

        // TV show loaded
        ui::Message::TvShowLoaded(series_id, result) => {
            match result {
                Ok(details) => {
                    log::info!("TV show details loaded for series: {}", series_id);
                    // TV show details are already stored in state by the library domain
                    DomainUpdateResult::task(Task::none())
                }
                Err(e) => {
                    log::error!("Failed to load TV show details for {}: {}", series_id, e);
                    state.domains.ui.state.error_message =
                        Some(format!("Failed to load TV show: {}", e));
                    DomainUpdateResult::task(Task::none())
                }
            }
        }

        // Aggregate all libraries
        ui::Message::AggregateAllLibraries => {
            // Emit cross-domain event to trigger library aggregation
            DomainUpdateResult::with_events(
                Task::none(),
                vec![CrossDomainEvent::RequestLibraryRefresh],
            )
        }

        // No-op
        ui::Message::NoOp => DomainUpdateResult::task(Task::none()),
    }
}

/// Handle carousel navigation messages
fn handle_carousel_navigation(state: &mut State, message: CarouselMessage) -> Task<ui::Message> {
    match message {
        CarouselMessage::Next(carousel_id) => {
            // Carousel state not tracked in current state
            log::debug!("Carousel {} scrolled right", carousel_id);
            Task::none()
        }
        CarouselMessage::Previous(carousel_id) => {
            // Carousel state not tracked in current state
            log::debug!("Carousel {} scrolled left", carousel_id);
            Task::none()
        }
        CarouselMessage::Scrolled(section_id, viewport) => {
            // Handle carousel scroll events
            log::debug!(
                "Carousel {} scrolled to viewport: {:?}",
                section_id,
                viewport
            );
            Task::none()
        }
    }
}
