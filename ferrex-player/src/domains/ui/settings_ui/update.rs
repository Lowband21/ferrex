use iced::Task;

use crate::infra::cache::PlayerDiskImageCacheLimits;
use crate::{
    common::{
        ViewState,
        messages::{CrossDomainEvent, DomainMessage, DomainUpdateResult},
    },
    domains::{
        library::messages::LibraryMessage,
        settings::{
            messages::SettingsMessage,
            sections::{
                display::DisplayMessage, performance::PerformanceMessage,
                playback::PlaybackMessage,
            },
        },
        ui::{
            feedback_ui::{FeedbackMessage, ToastNotification},
            settings_ui::{RuntimeConfigMessage, SettingsUiMessage},
        },
    },
    state::State,
};

#[cfg(feature = "demo")]
use crate::domains::ui::update_handlers::demo_controls;

pub fn update_settings_ui(
    state: &mut State,
    message: SettingsUiMessage,
) -> DomainUpdateResult {
    match message {
        // Unified settings navigation (new sidebar)
        SettingsUiMessage::NavigateToSection(section) => {
            // Check if any in-use setting was modified and show toast
            let was_dirty = state.runtime_config.take_dirty();
            let nav_task = Task::done(DomainMessage::Settings(
                SettingsMessage::NavigateToSection(section),
            ));

            if was_dirty {
                let toast_task = Task::done(DomainMessage::Ui(
                    FeedbackMessage::ShowToast(ToastNotification::success("Settings applied"))
                        .into(),
                ));
                DomainUpdateResult::task(Task::batch([nav_task, toast_task]))
            } else {
                DomainUpdateResult::task(nav_task)
            }
        }

        SettingsUiMessage::ShowAdminDashboard => {
            state.domains.ui.state.view = ViewState::AdminDashboard;
            DomainUpdateResult::task(Task::none())
        }
        SettingsUiMessage::HideAdminDashboard => {
            state.domains.ui.state.view = ViewState::Library;
            DomainUpdateResult::task(Task::none())
        }

        SettingsUiMessage::ShowLibraryManagement => {
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
                LibraryMessage::FetchActiveScans,
            ));

            let tasks = {
                let tasks = vec![fetch_scans_task];

                #[cfg(feature = "demo")]
                let tasks = {
                    use crate::domains::ui::update_handlers::demo_controls;

                    demo_controls::augment_show_library_management_tasks(
                        state, tasks,
                    )
                };

                tasks
            };

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
        SettingsUiMessage::HideLibraryManagement => {
            // Return to Admin Dashboard
            state.domains.ui.state.view = ViewState::AdminDashboard;
            DomainUpdateResult::task(Task::none())
        }
        SettingsUiMessage::ShowSettings => {
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
                        |enabled| SettingsUiMessage::AutoLoginToggled(Ok(enabled)).into(),
                    )
                    .map(DomainMessage::Ui),
                )
            }
        SettingsUiMessage::ShowUserManagement => {
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
                crate::domains::user_management::messages::UserManagementMessage::LoadUsers,
            ));
            DomainUpdateResult::task(task)
        }
        SettingsUiMessage::HideUserManagement => {
            state.domains.ui.state.view = ViewState::Library;
            state.domains.library.state.show_library_management = false;
            state.domains.library.state.library_form_data = None; // Clear form when leaving management view
            state.domains.library.state.library_form_success = None;
            DomainUpdateResult::task(Task::none())
        }
         SettingsUiMessage::UserAdminDelete(user_id) => {
                // Proxy to user_management domain delete confirm action
                let task = Task::done(DomainMessage::UserManagement(
                    crate::domains::user_management::messages::UserManagementMessage::DeleteUserConfirm(user_id),
                ));
                DomainUpdateResult::task(task)
            }
        #[cfg(feature = "demo")]
            SettingsUiMessage::DemoMoviesTargetChanged(value) => {

                demo_controls::handle_movies_input(state, value)
            }
        #[cfg(feature = "demo")]
            SettingsUiMessage::DemoSeriesTargetChanged(value) => {
                demo_controls::handle_series_input(state, value)
            }
        #[cfg(feature = "demo")]
            SettingsUiMessage::DemoApplySizing => demo_controls::handle_apply_sizing(state),
        #[cfg(feature = "demo")]
            SettingsUiMessage::DemoRefreshStatus => demo_controls::handle_refresh_status(state),
        SettingsUiMessage::ShowClearDatabaseConfirm => {
                state.domains.ui.state.show_clear_database_confirm = true;
                DomainUpdateResult::task(Task::none())
            }
        SettingsUiMessage::HideClearDatabaseConfirm => {
                state.domains.ui.state.show_clear_database_confirm = false;
                DomainUpdateResult::task(Task::none())
            }
        SettingsUiMessage::ClearDatabase => {
                let task = crate::common::clear_database::handle_clear_database(state);
                DomainUpdateResult::task(task)
            }
        SettingsUiMessage::DatabaseCleared(result) => {
                let task = crate::common::clear_database::handle_database_cleared(state, result);
                DomainUpdateResult::task(task)
            }
        SettingsUiMessage::ShowChangePassword => {
                DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                    crate::domains::settings::messages::SettingsMessage::ShowChangePassword,
                )))
            }
        SettingsUiMessage::UpdatePasswordCurrent(value) => {
                DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                    crate::domains::settings::messages::SettingsMessage::UpdatePasswordCurrent(value),
                )))
            }
        SettingsUiMessage::UpdatePasswordNew(value) => {
                DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                    crate::domains::settings::messages::SettingsMessage::UpdatePasswordNew(value),
                )))
            }
        SettingsUiMessage::UpdatePasswordConfirm(value) => {
                DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                    crate::domains::settings::messages::SettingsMessage::UpdatePasswordConfirm(value),
                )))
            }
        SettingsUiMessage::TogglePasswordVisibility => {
                DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                    crate::domains::settings::messages::SettingsMessage::TogglePasswordVisibility,
                )))
            }
        SettingsUiMessage::SubmitPasswordChange => {
                DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                    crate::domains::settings::messages::SettingsMessage::SubmitPasswordChange,
                )))
            }
        SettingsUiMessage::PasswordChangeResult(_result) => {
                // UI handles displaying the result
                DomainUpdateResult::task(Task::none())
            }
        SettingsUiMessage::CancelPasswordChange => {
                DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                    crate::domains::settings::messages::SettingsMessage::CancelPasswordChange,
                )))
            }
        SettingsUiMessage::ShowSetPin => DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                crate::domains::settings::messages::SettingsMessage::ShowSetPin,
            ))),
        SettingsUiMessage::ShowChangePin => DomainUpdateResult::task(Task::done(
                DomainMessage::Settings(crate::domains::settings::messages::SettingsMessage::ShowChangePin),
            )),
        SettingsUiMessage::UpdatePinCurrent(value) => {
                DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                    crate::domains::settings::messages::SettingsMessage::UpdatePinCurrent(value),
                )))
            }
        SettingsUiMessage::UpdatePinNew(value) => {
                DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                    crate::domains::settings::messages::SettingsMessage::UpdatePinNew(value),
                )))
            }
        SettingsUiMessage::UpdatePinConfirm(value) => {
                DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                    crate::domains::settings::messages::SettingsMessage::UpdatePinConfirm(value),
                )))
            }
        SettingsUiMessage::SubmitPinChange => DomainUpdateResult::task(Task::done(
                DomainMessage::Settings(crate::domains::settings::messages::SettingsMessage::SubmitPinChange),
            )),
        SettingsUiMessage::PinChangeResult(_result) => {
                DomainUpdateResult::task(Task::none())
            }
        SettingsUiMessage::CancelPinChange => DomainUpdateResult::task(Task::done(
                DomainMessage::Settings(SettingsMessage::CancelPinChange),
            )),
        SettingsUiMessage::EnableAdminPinUnlock => {
                DomainUpdateResult::task(Task::done(DomainMessage::Auth(
                    crate::domains::auth::messages::AuthMessage::EnableAdminPinUnlock,
                )))
            }
        SettingsUiMessage::DisableAdminPinUnlock => {
                DomainUpdateResult::task(Task::done(DomainMessage::Auth(
                    crate::domains::auth::messages::AuthMessage::DisableAdminPinUnlock,
                )))
            }
        SettingsUiMessage::ToggleAutoLogin(enabled) => {
                DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                    SettingsMessage::ToggleAutoLogin(enabled),
                )))
            }
        SettingsUiMessage::AutoLoginToggled(_result) => {
                DomainUpdateResult::task(Task::none())
            }
        SettingsUiMessage::SetUserScale(user_scale) => {
            // Clear preview when scale is applied
            state.domains.ui.state.scale_slider_preview = None;
            // Update text input to match applied value
            if let ferrex_core::player_prelude::UserScale::Custom(v) = user_scale {
                state.domains.ui.state.scale_text_input = format!("{:.2}", v);
            }
            DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                SettingsMessage::SetUserScale(user_scale),
            )))
        }
        SettingsUiMessage::SetScalePreset(preset) => {
            // Clear preview and update text input when preset is selected
            state.domains.ui.state.scale_slider_preview = None;
            state.domains.ui.state.scale_text_input =
                format!("{:.2}", preset.scale_factor());
            DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                SettingsMessage::SetScalePreset(preset),
            )))
        }
        SettingsUiMessage::ScaleSliderPreview(value) => {
            // Update preview value during slider drag (UI-only, no domain update)
            state.domains.ui.state.scale_slider_preview = Some(value);
            state.domains.ui.state.scale_text_input = format!("{:.2}", value);
            DomainUpdateResult::task(Task::none())
        }
        SettingsUiMessage::ScaleTextInput(text) => {
            // Update text input field (UI-only, no domain update until submit)
            state.domains.ui.state.scale_text_input = text;
            DomainUpdateResult::task(Task::none())
        }
        SettingsUiMessage::LoadDevices => {
                DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                    SettingsMessage::LoadDevices,
                )))
            }
        SettingsUiMessage::DevicesLoaded(_result) => {
                // This message should now come from settings domain, but kept for compatibility
                log::warn!("DevicesLoaded should now come from settings domain via cross-domain event");
                DomainUpdateResult::task(Task::none())
            }
        SettingsUiMessage::RefreshDevices => {
                DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                    crate::domains::settings::messages::SettingsMessage::RefreshDevices,
                )))
            }
        SettingsUiMessage::RevokeDevice(device_id) => {
                DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                    crate::domains::settings::messages::SettingsMessage::RevokeDevice(device_id),
                )))
            }
        SettingsUiMessage::DeviceRevoked(_result) => {
                log::warn!("DeviceRevoked should now come from settings domain via cross-domain event");
                DomainUpdateResult::task(Task::none())
            }
        SettingsUiMessage::Logout => {
                use crate::domains::auth::messages as auth;
                DomainUpdateResult::task(Task::done(DomainMessage::Auth(
                    auth::AuthMessage::Logout,
                )))
            }

        // Library management proxies (admin UI)
        SettingsUiMessage::ShowLibraryForm(library) => DomainUpdateResult::task(
            Task::done(DomainMessage::Library(LibraryMessage::ShowLibraryForm(
                library,
            ))),
        ),
        SettingsUiMessage::HideLibraryForm => DomainUpdateResult::task(
            Task::done(DomainMessage::Library(LibraryMessage::HideLibraryForm)),
        ),
        SettingsUiMessage::ScanLibrary(library_id) => DomainUpdateResult::task(
            Task::done(DomainMessage::Library(LibraryMessage::ScanLibrary(
                library_id,
            ))),
        ),
        SettingsUiMessage::DeleteLibrary(library_id) => {
            DomainUpdateResult::task(Task::done(DomainMessage::Library(
                LibraryMessage::DeleteLibrary(library_id),
            )))
        }
        SettingsUiMessage::UpdateLibraryFormName(name) => {
            DomainUpdateResult::task(Task::done(DomainMessage::Library(
                LibraryMessage::UpdateLibraryFormName(name),
            )))
        }
        SettingsUiMessage::UpdateLibraryFormType(library_type) => {
            DomainUpdateResult::task(Task::done(DomainMessage::Library(
                LibraryMessage::UpdateLibraryFormType(library_type),
            )))
        }
        SettingsUiMessage::UpdateLibraryFormPaths(paths) => {
            DomainUpdateResult::task(Task::done(DomainMessage::Library(
                LibraryMessage::UpdateLibraryFormPaths(paths),
            )))
        }
        SettingsUiMessage::UpdateLibraryFormScanInterval(interval) => {
            DomainUpdateResult::task(Task::done(DomainMessage::Library(
                LibraryMessage::UpdateLibraryFormScanInterval(interval),
            )))
        }
        SettingsUiMessage::ToggleLibraryFormEnabled => {
            DomainUpdateResult::task(Task::done(DomainMessage::Library(
                LibraryMessage::ToggleLibraryFormEnabled,
            )))
        }
        SettingsUiMessage::ToggleLibraryFormStartScan => {
            DomainUpdateResult::task(Task::done(DomainMessage::Library(
                LibraryMessage::ToggleLibraryFormStartScan,
            )))
        }
        SettingsUiMessage::SubmitLibraryForm => DomainUpdateResult::task(
            Task::done(DomainMessage::Library(LibraryMessage::SubmitLibraryForm)),
        ),
        SettingsUiMessage::LibraryMediaRoot(message) => {
            DomainUpdateResult::task(Task::done(DomainMessage::Library(
                LibraryMessage::MediaRootBrowser(message),
            )))
        }
        SettingsUiMessage::PauseLibraryScan(library_id, scan_id) => {
            DomainUpdateResult::task(Task::done(DomainMessage::Library(
                LibraryMessage::PauseScan {
                    library_id,
                    scan_id,
                },
            )))
        }
        SettingsUiMessage::ResumeLibraryScan(library_id, scan_id) => {
            DomainUpdateResult::task(Task::done(DomainMessage::Library(
                LibraryMessage::ResumeScan {
                    library_id,
                    scan_id,
                },
            )))
        }
        SettingsUiMessage::CancelLibraryScan(library_id, scan_id) => {
            DomainUpdateResult::task(Task::done(DomainMessage::Library(
                LibraryMessage::CancelScan {
                    library_id,
                    scan_id,
                },
            )))
        }
        SettingsUiMessage::FetchScanMetrics => DomainUpdateResult::task(
            Task::done(DomainMessage::Library(
                LibraryMessage::FetchScanMetrics,
            )),
        ),
        SettingsUiMessage::ResetLibrary(library_id) => {
            DomainUpdateResult::task(Task::done(DomainMessage::Library(
                LibraryMessage::ResetLibrary(library_id),
            )))
        }

        // Playback settings - route to settings domain
        SettingsUiMessage::SetSeekForwardCoarse(value) => {
            DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                SettingsMessage::Playback(PlaybackMessage::SetSeekForwardCoarse(value)),
            )))
        }
        SettingsUiMessage::SetSeekBackwardCoarse(value) => {
            DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                SettingsMessage::Playback(PlaybackMessage::SetSeekBackwardCoarse(value)),
            )))
        }
        SettingsUiMessage::SetSeekForwardFine(value) => {
            DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                SettingsMessage::Playback(PlaybackMessage::SetSeekForwardFine(value)),
            )))
        }
        SettingsUiMessage::SetSeekBackwardFine(value) => {
            DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                SettingsMessage::Playback(PlaybackMessage::SetSeekBackwardFine(value)),
            )))
        }

        // Display settings - route to settings domain
        SettingsUiMessage::SetPosterWidth(value) => {
            DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                SettingsMessage::Display(DisplayMessage::SetPosterBaseWidth(value)),
            )))
        }
        SettingsUiMessage::SetPosterHeight(value) => {
            DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                SettingsMessage::Display(DisplayMessage::SetPosterBaseHeight(value)),
            )))
        }
        SettingsUiMessage::SetCornerRadius(value) => {
            DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                SettingsMessage::Display(DisplayMessage::SetPosterCornerRadius(value)),
            )))
        }
        SettingsUiMessage::SetGridSpacing(value) => {
            DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                SettingsMessage::Display(DisplayMessage::SetGridPosterGap(value)),
            )))
        }
        SettingsUiMessage::SetRowSpacing(value) => {
            DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                SettingsMessage::Display(DisplayMessage::SetGridRowSpacing(value)),
            )))
        }
        SettingsUiMessage::SetHoverScale(value) => {
            DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                SettingsMessage::Display(DisplayMessage::SetAnimationHoverScale(value)),
            )))
        }
        SettingsUiMessage::SetAnimationDuration(value) => {
            DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                SettingsMessage::Display(DisplayMessage::SetAnimationDefaultDuration(value)),
            )))
        }

        // Performance settings - route to settings domain
        SettingsUiMessage::SetScrollDebounce(value) => {
            DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                SettingsMessage::Performance(PerformanceMessage::SetScrollDebounceMs(value)),
            )))
        }
        SettingsUiMessage::SetScrollMaxVelocity(value) => {
            DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                SettingsMessage::Performance(PerformanceMessage::SetScrollMaxVelocity(value)),
            )))
        }
        SettingsUiMessage::SetScrollDecay(value) => {
            DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                SettingsMessage::Performance(PerformanceMessage::SetScrollDecayTauMs(value)),
            )))
        }

        // RuntimeConfig sub-router - updates runtime config directly and shows toast
        SettingsUiMessage::RuntimeConfig(msg) => {
            update_runtime_config(state, msg)
        }

        // Display settings sub-router - routes to settings domain
        SettingsUiMessage::Display(msg) => {
            DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                SettingsMessage::Display(msg),
            )))
        }

        // Theme settings sub-router - routes to settings domain
        SettingsUiMessage::Theme(msg) => {
            DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                SettingsMessage::Theme(msg),
            )))
        }
    }
}

/// Handle RuntimeConfig updates - modifies state directly (values take effect immediately)
/// Marks dirty only for settings that are actively consumed by the application.
fn update_runtime_config(
    state: &mut State,
    msg: RuntimeConfigMessage,
) -> DomainUpdateResult {
    let config = &mut state.runtime_config;

    // Track whether this is an in-use setting (wired to actual consumers)
    let is_in_use = matches!(
        msg,
        // Grid Scrolling - used in motion_controller/update.rs
        RuntimeConfigMessage::ScrollDebounce(_)
            | RuntimeConfigMessage::ScrollBaseVelocity(_)
            | RuntimeConfigMessage::ScrollMaxVelocity(_)
            | RuntimeConfigMessage::ScrollDecayTau(_)
            | RuntimeConfigMessage::ScrollRamp(_)
            | RuntimeConfigMessage::ScrollBoost(_)
            // Carousel motion - used in ensure_scroller_for_key
            | RuntimeConfigMessage::CarouselBaseVelocity(_)
            | RuntimeConfigMessage::CarouselMaxVelocity(_)
            | RuntimeConfigMessage::CarouselDecayTau(_)
            | RuntimeConfigMessage::CarouselRamp(_)
            | RuntimeConfigMessage::CarouselBoost(_)
            // Snap animations - used in virtual_carousel_updates.rs and home_focus.rs
            | RuntimeConfigMessage::SnapItemDuration(_)
            | RuntimeConfigMessage::SnapPageDuration(_)
            | RuntimeConfigMessage::SnapHoldThreshold(_)
            | RuntimeConfigMessage::SnapEpsilon(_)
            // Carousel prefetch - used in planner
            | RuntimeConfigMessage::CarouselPrefetch(_)
            | RuntimeConfigMessage::CarouselBackground(_)
            // Grid prefetch - used in scroll_updates.rs, window_update.rs, library_ui/update.rs, shell_ui/update.rs
            | RuntimeConfigMessage::PrefetchRowsAbove(_)
            | RuntimeConfigMessage::PrefetchRowsBelow(_)
            // Keep-alive - used in utils.rs bump_keep_alive
            | RuntimeConfigMessage::KeepAlive(_)
            // Image cache - used in metadata loader + UnifiedImageService
            | RuntimeConfigMessage::ImageCacheRamMax(_)
            | RuntimeConfigMessage::ImageCacheDiskMax(_)
            | RuntimeConfigMessage::ImageCacheDiskTtlDays(_)
            // Animation effects - used in AnimationConfig via runtime_config.animation_config()
            | RuntimeConfigMessage::HoverScale(_)
            | RuntimeConfigMessage::HoverTransition(_)
            | RuntimeConfigMessage::HoverScaleDownDelay(_)
            | RuntimeConfigMessage::AnimationDuration(_)
            | RuntimeConfigMessage::TextureFadeInitial(_)
            | RuntimeConfigMessage::TextureFade(_)
    );

    match msg {
        // Grid Scrolling (IN USE)
        RuntimeConfigMessage::ScrollDebounce(v) => {
            config.scroll_debounce_ms = Some(v)
        }
        RuntimeConfigMessage::ScrollBaseVelocity(v) => {
            config.scroll_base_velocity = Some(v)
        }
        RuntimeConfigMessage::ScrollMaxVelocity(v) => {
            config.scroll_max_velocity = Some(v)
        }
        RuntimeConfigMessage::ScrollDecayTau(v) => {
            config.scroll_decay_tau_ms = Some(v)
        }
        RuntimeConfigMessage::ScrollRamp(v) => config.scroll_ramp_ms = Some(v),
        RuntimeConfigMessage::ScrollBoost(v) => {
            config.scroll_boost_multiplier = Some(v)
        }

        // Carousel Motion (IN USE)
        RuntimeConfigMessage::CarouselBaseVelocity(v) => {
            config.carousel_base_velocity = Some(v)
        }
        RuntimeConfigMessage::CarouselMaxVelocity(v) => {
            config.carousel_max_velocity = Some(v)
        }
        RuntimeConfigMessage::CarouselDecayTau(v) => {
            config.carousel_decay_tau_ms = Some(v)
        }
        RuntimeConfigMessage::CarouselRamp(v) => {
            config.carousel_ramp_ms = Some(v)
        }
        RuntimeConfigMessage::CarouselBoost(v) => {
            config.carousel_boost_multiplier = Some(v)
        }

        // Snap Animations (IN USE)
        RuntimeConfigMessage::SnapItemDuration(v) => {
            config.snap_item_duration_ms = Some(v)
        }
        RuntimeConfigMessage::SnapPageDuration(v) => {
            config.snap_page_duration_ms = Some(v)
        }
        RuntimeConfigMessage::SnapHoldThreshold(v) => {
            config.snap_hold_threshold_ms = Some(v)
        }
        RuntimeConfigMessage::SnapEpsilon(v) => {
            config.snap_epsilon_fraction = Some(v)
        }

        // Animation Effects (used in AnimationConfig via runtime_config.animation_config())
        RuntimeConfigMessage::HoverScale(v) => {
            config.animation_hover_scale = Some(v);
            // Update global for immediate effect on all posters
            crate::infra::shader_widgets::poster::set_hover_scale(v);
        }
        RuntimeConfigMessage::HoverTransition(v) => {
            config.animation_hover_transition_ms = Some(v);
            // Update global for immediate effect on all posters
            crate::infra::shader_widgets::poster::set_hover_transition_ms(v);
        }
        RuntimeConfigMessage::HoverScaleDownDelay(v) => {
            config.animation_hover_scale_down_delay_ms = Some(v);
            // Update global for immediate effect on all posters
            crate::infra::shader_widgets::poster::set_hover_scale_down_delay_ms(
                v,
            );
        }
        RuntimeConfigMessage::AnimationDuration(v) => {
            config.animation_default_duration_ms = Some(v)
        }
        RuntimeConfigMessage::TextureFadeInitial(v) => {
            config.animation_texture_fade_initial_ms = Some(v)
        }
        RuntimeConfigMessage::TextureFade(v) => {
            config.animation_texture_fade_ms = Some(v)
        }

        // GPU/Memory (CarouselPrefetch/Background IN USE)
        RuntimeConfigMessage::TextureUploads(v) => {
            config.texture_max_uploads = Some(v)
        }
        RuntimeConfigMessage::PrefetchRowsAbove(v) => {
            config.prefetch_rows_above = Some(v)
        }
        RuntimeConfigMessage::PrefetchRowsBelow(v) => {
            config.prefetch_rows_below = Some(v)
        }
        RuntimeConfigMessage::CarouselPrefetch(v) => {
            config.carousel_prefetch_items = Some(v)
        }
        RuntimeConfigMessage::CarouselBackground(v) => {
            config.carousel_background_items = Some(v)
        }
        RuntimeConfigMessage::KeepAlive(v) => config.keep_alive_ms = Some(v),

        // Image Cache (IN USE)
        RuntimeConfigMessage::ImageCacheRamMax(byte_size) => {
            config.image_cache_ram_max_bytes = Some(byte_size);
            state.image_service.set_ram_max_bytes(byte_size);
        }
        RuntimeConfigMessage::ImageCacheDiskMax(byte_size) => {
            config.image_cache_disk_max_bytes = Some(byte_size);
            if let Some(cache) = state.disk_image_cache.as_ref() {
                cache.set_limits_and_enforce(PlayerDiskImageCacheLimits {
                    max_bytes: config.image_cache_disk_max_bytes(),
                    ttl: config.image_cache_disk_ttl(),
                    touch_interval: PlayerDiskImageCacheLimits::defaults()
                        .touch_interval,
                    access_index_flush_interval:
                        PlayerDiskImageCacheLimits::defaults()
                            .access_index_flush_interval,
                });
            }
        }
        RuntimeConfigMessage::ImageCacheDiskTtlDays(days) => {
            config.image_cache_disk_ttl_days = Some(days.max(1));
            if let Some(cache) = state.disk_image_cache.as_ref() {
                cache.set_limits_and_enforce(PlayerDiskImageCacheLimits {
                    max_bytes: config.image_cache_disk_max_bytes(),
                    ttl: config.image_cache_disk_ttl(),
                    touch_interval: PlayerDiskImageCacheLimits::defaults()
                        .touch_interval,
                    access_index_flush_interval:
                        PlayerDiskImageCacheLimits::defaults()
                            .access_index_flush_interval,
                });
            }
        }

        // Player Seeking (not yet wired to consumers)
        RuntimeConfigMessage::SeekForwardCoarse(v) => {
            config.seek_forward_coarse = Some(v)
        }
        RuntimeConfigMessage::SeekBackwardCoarse(v) => {
            config.seek_backward_coarse = Some(v)
        }
        RuntimeConfigMessage::SeekForwardFine(v) => {
            config.seek_forward_fine = Some(v)
        }
        RuntimeConfigMessage::SeekBackwardFine(v) => {
            config.seek_backward_fine = Some(v)
        }
    }

    // Only mark dirty if this setting is actually being consumed
    if is_in_use {
        config.mark_dirty();
    }

    DomainUpdateResult::task(Task::none())
}
