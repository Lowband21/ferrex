use crate::{
    messages::{ui, CrossDomainEvent},
    state::{State, ViewState},
    views::carousel::CarouselMessage,
};
use iced::Task;

/// Check if user has PIN
fn check_user_has_pin(state: &mut State) -> Task<ui::Message> {
    if let Some(auth_manager) = &state.auth_manager {
        let auth_manager = auth_manager.clone();
        
        return Task::perform(
            async move {
                if let Some(user) = auth_manager.get_current_user().await {
                    auth_manager.check_device_auth(user.id).await
                        .map(|status| status.has_pin)
                        .unwrap_or(false)
                } else {
                    false
                }
            },
            |has_pin| {
                // Update state with PIN status
                ui::Message::NoOp // TODO: Add UserHasPinResult message
            },
        );
    }
    
    Task::none()
}

/// Handle UI domain messages
pub fn update_ui(state: &mut State, message: ui::Message) -> Task<ui::Message> {
    match message {
        ui::Message::SetViewMode(view_mode) => {
            super::set_view_mode::handle_set_view_mode(state, view_mode)
        }
        ui::Message::ViewDetails(media) => {
            super::navigation_updates::handle_view_details(state, media)
        }
        ui::Message::ViewMovieDetails(movie_ref) => {
            super::navigation_updates::handle_view_movie_details(state, movie_ref)
        }
        ui::Message::ViewTvShow(series_id) => {
            super::navigation_updates::handle_view_tv_show(state, series_id)
        }
        ui::Message::ViewSeason(series_id, season_id) => {
            super::navigation_updates::handle_view_season(state, series_id, season_id)
        }
        ui::Message::ViewEpisode(episode_id) => {
            super::navigation_updates::handle_view_episode(state, episode_id)
        }
        ui::Message::SetSortBy(sort_by) => {
            state.sort_by = sort_by;
            Task::none()
        }
        ui::Message::ToggleSortOrder => {
            state.sort_order = match state.sort_order {
                crate::state::SortOrder::Ascending => crate::state::SortOrder::Descending,
                crate::state::SortOrder::Descending => crate::state::SortOrder::Ascending,
            };
            Task::none()
        }
        ui::Message::ShowAdminDashboard => {
            state.view = ViewState::AdminDashboard;
            Task::none()
        }
        ui::Message::HideAdminDashboard => {
            state.view = ViewState::Library;
            Task::none()
        }
        ui::Message::ShowLibraryManagement => {
            state.view = ViewState::LibraryManagement;
            state.show_library_management = true;

            // Request library refresh if needed
            if state.libraries.is_empty() {
                Task::done(ui::Message::_EmitCrossDomainEvent(
                    CrossDomainEvent::RequestLibraryRefresh,
                ))
            } else {
                Task::none()
            }
        }
        ui::Message::HideLibraryManagement => {
            state.view = ViewState::Library;
            state.show_library_management = false;
            state.library_form_data = None; // Clear form when leaving management view
            Task::none()
        }
        ui::Message::ShowClearDatabaseConfirm => {
            state.show_clear_database_confirm = true;
            Task::none()
        }
        ui::Message::HideClearDatabaseConfirm => {
            state.show_clear_database_confirm = false;
            Task::none()
        }
        ui::Message::ClearDatabase => super::clear_database::handle_clear_database(state),
        ui::Message::DatabaseCleared(result) => {
            super::clear_database::handle_database_cleared(state, result)
        }
        ui::Message::ClearError => {
            state.error_message = None;
            Task::none()
        }
        ui::Message::MoviesGridScrolled(viewport) => {
            super::scroll_updates::handle_movies_grid_scrolled(state, viewport)
        }
        ui::Message::TvShowsGridScrolled(viewport) => {
            super::scroll_updates::handle_tv_shows_grid_scrolled(state, viewport)
        }
        ui::Message::CheckScrollStopped => {
            // Check if scrolling has stopped for movies grid
            // Note: Scroll state tracking not implemented in current state
            // This would need to be added to state if scroll timing is needed

            // For now, just log that check was requested
            log::debug!("Check scroll stopped requested");

            Task::none()
        }
        ui::Message::RecalculateGridsAfterResize => {
            // Force recalculation of visible items for both grids
            // Force recalculation of visible items
            // Note: visible_range fields don't exist in current state

            // Queue recalculation on next scroll event or view update
            log::debug!("Grid recalculation queued after resize");

            Task::none()
        }
        ui::Message::DetailViewScrolled(viewport) => {
            // Update background shader scroll offset for parallax effect
            state.background_shader_state.scroll_offset = viewport.absolute_offset().y;
            Task::none()
        }
        ui::Message::WindowResized(size) => {
            super::window_update::handle_window_resized(state, size)
        }
        ui::Message::MediaHovered(media_id) => {
            state.hovered_media_id = Some(media_id);
            Task::none()
        }
        ui::Message::MediaUnhovered(media_id) => {
            // Only clear hover state if it matches the media being unhovered
            // This prevents race conditions when quickly moving between posters
            if state.hovered_media_id.as_ref() == Some(&media_id) {
                state.hovered_media_id = None;
            }
            Task::none()
        }
        ui::Message::NavigateHome => {
            state.view = ViewState::Library;
            state.view_mode = crate::state::ViewMode::All;

            // Clear detail view data
            state.current_show_seasons.clear();
            state.current_season_episodes.clear();

            Task::none()
        }
        ui::Message::BackToLibrary => {
            // Emit cross-domain event to navigate home
            Task::done(ui::Message::_EmitCrossDomainEvent(
                CrossDomainEvent::NavigateHome,
            ))
        }
        ui::Message::UpdateSearchQuery(query) => {
            state.search_query = query;
            Task::none()
        }
        ui::Message::ExecuteSearch => {
            if !state.search_query.is_empty() {
                super::search_updates::handle_execute_search(state)
            } else {
                Task::none()
            }
        }
        ui::Message::ShowLibraryMenu => {
            state.show_library_menu = !state.show_library_menu;
            Task::none()
        }
        ui::Message::ShowAllLibrariesMenu => {
            state.show_library_menu = !state.show_library_menu;
            state.library_menu_target = None;
            Task::none()
        }
        ui::Message::ShowProfile => {
            state.view = ViewState::UserSettings;
            
            // Load auto-login preference when showing settings
            if let Some(auth_manager) = &state.auth_manager {
                let auth_manager = auth_manager.clone();
                
                Task::perform(
                    async move {
                        auth_manager.is_auto_login_enabled().await
                    },
                    |enabled| {
                        ui::Message::AutoLoginToggled(Ok(enabled))
                    },
                )
            } else {
                Task::none()
            }
        }
        
        ui::Message::ShowUserProfile => {
            state.settings_subview = crate::state::SettingsSubview::Profile;
            Task::none()
        }
        
        ui::Message::ShowUserPreferences => {
            state.settings_subview = crate::state::SettingsSubview::Preferences;
            Task::none()
        }
        
        ui::Message::ShowUserSecurity => {
            state.settings_subview = crate::state::SettingsSubview::Security;
            // Check if user has PIN when entering security settings
            check_user_has_pin(state)
        }
        
        ui::Message::ShowDeviceManagement => {
            state.settings_subview = crate::state::SettingsSubview::DeviceManagement;
            // Load devices when the view is shown
            use crate::updates::settings::device_management::handle_load_devices;
            handle_load_devices(state)
        }
        
        ui::Message::BackToSettings => {
            state.view = ViewState::UserSettings;
            state.settings_subview = crate::state::SettingsSubview::Main;
            // Clear any security settings state
            state.security_settings_state = Default::default();
            Task::none()
        }
        
        // Security settings handlers
        ui::Message::ShowChangePassword => {
            use crate::updates::settings::security::handle_show_change_password;
            handle_show_change_password(state).map(|_| ui::Message::NoOp)
        }
        
        ui::Message::UpdatePasswordCurrent(value) => {
            use crate::updates::settings::security::handle_update_password_current;
            handle_update_password_current(state, value).map(|_| ui::Message::NoOp)
        }
        
        ui::Message::UpdatePasswordNew(value) => {
            use crate::updates::settings::security::handle_update_password_new;
            handle_update_password_new(state, value).map(|_| ui::Message::NoOp)
        }
        
        ui::Message::UpdatePasswordConfirm(value) => {
            use crate::updates::settings::security::handle_update_password_confirm;
            handle_update_password_confirm(state, value).map(|_| ui::Message::NoOp)
        }
        
        ui::Message::TogglePasswordVisibility => {
            use crate::updates::settings::security::handle_toggle_password_visibility;
            handle_toggle_password_visibility(state).map(|_| ui::Message::NoOp)
        }
        
        ui::Message::SubmitPasswordChange => {
            use crate::updates::settings::security::handle_submit_password_change;
            handle_submit_password_change(state)
                .map(|msg| match msg {
                    crate::messages::settings::Message::PasswordChangeResult(result) => {
                        ui::Message::PasswordChangeResult(result)
                    }
                    _ => ui::Message::NoOp,
                })
        }
        
        ui::Message::PasswordChangeResult(result) => {
            use crate::updates::settings::security::handle_password_change_result;
            handle_password_change_result(state, result).map(|_| ui::Message::NoOp)
        }
        
        ui::Message::CancelPasswordChange => {
            use crate::updates::settings::security::handle_cancel_password_change;
            handle_cancel_password_change(state).map(|_| ui::Message::NoOp)
        }
        
        ui::Message::ShowSetPin => {
            use crate::updates::settings::security::handle_show_set_pin;
            handle_show_set_pin(state).map(|_| ui::Message::NoOp)
        }
        
        ui::Message::ShowChangePin => {
            use crate::updates::settings::security::handle_show_change_pin;
            handle_show_change_pin(state).map(|_| ui::Message::NoOp)
        }
        
        ui::Message::UpdatePinCurrent(value) => {
            use crate::updates::settings::security::handle_update_pin_current;
            handle_update_pin_current(state, value).map(|_| ui::Message::NoOp)
        }
        
        ui::Message::UpdatePinNew(value) => {
            use crate::updates::settings::security::handle_update_pin_new;
            handle_update_pin_new(state, value).map(|_| ui::Message::NoOp)
        }
        
        ui::Message::UpdatePinConfirm(value) => {
            use crate::updates::settings::security::handle_update_pin_confirm;
            handle_update_pin_confirm(state, value).map(|_| ui::Message::NoOp)
        }
        
        ui::Message::SubmitPinChange => {
            use crate::updates::settings::security::handle_submit_pin_change;
            handle_submit_pin_change(state)
                .map(|msg| match msg {
                    crate::messages::settings::Message::PinChangeResult(result) => {
                        ui::Message::PinChangeResult(result)
                    }
                    _ => ui::Message::NoOp,
                })
        }
        
        ui::Message::PinChangeResult(result) => {
            use crate::updates::settings::security::handle_pin_change_result;
            handle_pin_change_result(state, result).map(|_| ui::Message::NoOp)
        }
        
        ui::Message::CancelPinChange => {
            use crate::updates::settings::security::handle_cancel_pin_change;
            handle_cancel_pin_change(state).map(|_| ui::Message::NoOp)
        }
        
        // User preferences
        ui::Message::ToggleAutoLogin(enabled) => {
            use crate::updates::settings::preferences::handle_toggle_auto_login;
            handle_toggle_auto_login(state, enabled)
                .map(|msg| match msg {
                    crate::messages::settings::Message::AutoLoginToggled(result) => {
                        ui::Message::AutoLoginToggled(result)
                    }
                    _ => ui::Message::NoOp,
                })
        }
        
        ui::Message::AutoLoginToggled(result) => {
            use crate::updates::settings::preferences::handle_auto_login_toggled;
            handle_auto_login_toggled(state, result).map(|_| ui::Message::NoOp)
        }
        
        // Device management
        ui::Message::LoadDevices => {
            use crate::updates::settings::device_management::handle_load_devices;
            handle_load_devices(state)
        }
        
        ui::Message::DevicesLoaded(result) => {
            use crate::updates::settings::device_management::handle_devices_loaded;
            handle_devices_loaded(state, result)
        }
        
        ui::Message::RefreshDevices => {
            use crate::updates::settings::device_management::handle_refresh_devices;
            handle_refresh_devices(state)
        }
        
        ui::Message::RevokeDevice(device_id) => {
            use crate::updates::settings::device_management::handle_revoke_device;
            handle_revoke_device(state, device_id)
        }
        
        ui::Message::DeviceRevoked(result) => {
            use crate::updates::settings::device_management::handle_device_revoked;
            handle_device_revoked(state, result)
        }
        
        ui::Message::Logout => {
            // Proxy to auth domain for logout via cross-domain event
            Task::done(ui::Message::_EmitCrossDomainEvent(
                CrossDomainEvent::UserLoggedOut,
            ))
        }
        ui::Message::CarouselNavigation(carousel_msg) => {
            handle_carousel_navigation(state, carousel_msg)
        }
        ui::Message::UpdateTransitions => {
            // Update all active transitions
            state.background_shader_state.color_transitions.update();
            state.background_shader_state.backdrop_transitions.update();
            state.background_shader_state.gradient_transitions.update();

            // Update the actual colors based on transition progress
            let (primary, secondary) = state
                .background_shader_state
                .color_transitions
                .get_interpolated_colors();
            state.background_shader_state.primary_color = primary;
            state.background_shader_state.secondary_color = secondary;

            // Update the gradient center based on transition progress
            state.background_shader_state.gradient_center = state
                .background_shader_state
                .gradient_transitions
                .get_interpolated_center();

            Task::none()
        }
        ui::Message::ToggleBackdropAspectMode => {
            state.background_shader_state.backdrop_aspect_mode =
                match state.background_shader_state.backdrop_aspect_mode {
                    crate::state::BackdropAspectMode::Auto => {
                        crate::state::BackdropAspectMode::Force21x9
                    }
                    crate::state::BackdropAspectMode::Force21x9 => {
                        crate::state::BackdropAspectMode::Auto
                    }
                };
            Task::none()
        }
        ui::Message::UpdateBackdropHandle(_handle) => {
            // Deprecated - backdrops are now pulled reactively from image service
            // This message handler kept for compatibility but does nothing
            Task::none()
        }
        ui::Message::RefreshViewModels => {
            // Refresh view models - implementation needed
            log::debug!("Refresh view models requested");
            Task::none()
        }
        ui::Message::QueueVisibleDetailsForFetch => {
            // TODO: Implement queue visible details for fetch
            log::debug!("Queue visible details for fetch requested");
            Task::none()
        }

        // Cross-domain proxy messages
        ui::Message::ToggleFullscreen => {
            // Forward to media domain
            Task::done(ui::Message::_EmitCrossDomainEvent(
                CrossDomainEvent::MediaToggleFullscreen,
            ))
        }
        ui::Message::ToggleScanProgress => {
            // Forward to library domain
            Task::done(ui::Message::_EmitCrossDomainEvent(
                CrossDomainEvent::LibraryToggleScanProgress,
            ))
        }
        ui::Message::SelectLibrary(library_id) => {
            // Forward to library domain
            Task::done(ui::Message::_EmitCrossDomainEvent(
                if let Some(id) = library_id {
                    CrossDomainEvent::LibrarySelected(id)
                } else {
                    CrossDomainEvent::RequestLibraryRefresh
                },
            ))
        }
        ui::Message::PlayMediaWithId(media_file, media_id) => {
            // Forward to media domain
            Task::done(ui::Message::_EmitCrossDomainEvent(
                CrossDomainEvent::MediaPlayWithId(media_file, media_id),
            ))
        }

        // Library management proxies
        ui::Message::ShowLibraryForm(library) => {
            // Forward to library domain
            Task::done(ui::Message::_EmitCrossDomainEvent(
                CrossDomainEvent::LibraryShowForm(library),
            ))
        }
        ui::Message::HideLibraryForm => {
            // Forward to library domain
            Task::done(ui::Message::_EmitCrossDomainEvent(
                CrossDomainEvent::LibraryHideForm,
            ))
        }
        ui::Message::ScanLibrary_(library_id) => {
            // Forward to library domain
            Task::done(ui::Message::_EmitCrossDomainEvent(
                CrossDomainEvent::LibraryScan(library_id),
            ))
        }
        ui::Message::DeleteLibrary(library_id) => {
            // Forward to library domain
            Task::done(ui::Message::_EmitCrossDomainEvent(
                CrossDomainEvent::LibraryDelete(library_id),
            ))
        }
        ui::Message::UpdateLibraryFormName(name) => {
            // Forward to library domain
            Task::done(ui::Message::_EmitCrossDomainEvent(
                CrossDomainEvent::LibraryFormUpdateName(name),
            ))
        }
        ui::Message::UpdateLibraryFormType(library_type) => {
            // Forward to library domain
            Task::done(ui::Message::_EmitCrossDomainEvent(
                CrossDomainEvent::LibraryFormUpdateType(library_type),
            ))
        }
        ui::Message::UpdateLibraryFormPaths(paths) => {
            // Forward to library domain
            Task::done(ui::Message::_EmitCrossDomainEvent(
                CrossDomainEvent::LibraryFormUpdatePaths(paths),
            ))
        }
        ui::Message::UpdateLibraryFormScanInterval(interval) => {
            // Forward to library domain
            Task::done(ui::Message::_EmitCrossDomainEvent(
                CrossDomainEvent::LibraryFormUpdateScanInterval(interval),
            ))
        }
        ui::Message::ToggleLibraryFormEnabled => {
            // Forward to library domain
            Task::done(ui::Message::_EmitCrossDomainEvent(
                CrossDomainEvent::LibraryFormToggleEnabled,
            ))
        }
        ui::Message::SubmitLibraryForm => {
            // Forward to library domain
            Task::done(ui::Message::_EmitCrossDomainEvent(
                CrossDomainEvent::LibraryFormSubmit,
            ))
        }

        // Internal cross-domain coordination
        ui::Message::_EmitCrossDomainEvent(_) => {
            // This should be handled by the main update loop, not here
            log::warn!("_EmitCrossDomainEvent should be handled by main update loop");
            Task::none()
        }

        // TV show loaded
        ui::Message::TvShowLoaded(series_id, result) => {
            match result {
                Ok(details) => {
                    log::info!("TV show details loaded for series: {}", series_id);
                    // TV show details are already stored in state by the library domain
                    Task::none()
                }
                Err(e) => {
                    log::error!("Failed to load TV show details for {}: {}", series_id, e);
                    state.error_message = Some(format!("Failed to load TV show: {}", e));
                    Task::none()
                }
            }
        }

        // Aggregate all libraries
        ui::Message::AggregateAllLibraries => {
            // Emit cross-domain event to trigger library aggregation
            Task::done(ui::Message::_EmitCrossDomainEvent(
                CrossDomainEvent::RequestLibraryRefresh,
            ))
        }

        // No-op
        ui::Message::NoOp => Task::none(),
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
