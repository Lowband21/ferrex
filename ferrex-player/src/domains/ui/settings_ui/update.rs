use iced::Task;

use crate::{
    common::{
        ViewState,
        messages::{CrossDomainEvent, DomainMessage, DomainUpdateResult},
    },
    domains::{
        library::messages::LibraryMessage,
        settings::messages::SettingsMessage,
        ui::{messages::UiMessage, settings_ui::SettingsUiMessage},
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

            let mut tasks = vec![fetch_scans_task];

            #[cfg(feature = "demo")]
            {
                use crate::domains::ui::update_handlers::demo_controls;

                tasks = demo_controls::augment_show_library_management_tasks(
                    state, tasks,
                );
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
        SettingsUiMessage::HideLibraryManagement => {
            // Return to Admin Dashboard
            state.domains.ui.state.view = ViewState::AdminDashboard;
            DomainUpdateResult::task(Task::none())
        }
        SettingsUiMessage::ShowProfile => {
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
        SettingsUiMessage::ShowUserProfile => {
                state.domains.settings.current_view =
                    crate::domains::settings::state::SettingsView::Profile;
                DomainUpdateResult::task(Task::none())
            }
        SettingsUiMessage::ShowUserPreferences => {
                state.domains.settings.current_view =
                    crate::domains::settings::state::SettingsView::Preferences;
                DomainUpdateResult::task(Task::none())
            }
        SettingsUiMessage::ShowUserSecurity => {
                state.domains.settings.current_view =
                    crate::domains::settings::state::SettingsView::Security;

                DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                    SettingsMessage::CheckUserHasPin,
                )))
            }
        SettingsUiMessage::ShowDeviceManagement => {
                state.domains.settings.current_view =
                    crate::domains::settings::state::SettingsView::DeviceManagement;
                // Load devices when the view is shown - send direct message to Settings domain
                DomainUpdateResult::task(Task::done(DomainMessage::Settings(
                    crate::domains::settings::messages::SettingsMessage::LoadDevices,
                )))
            }
        SettingsUiMessage::BackToSettings => {
                state.domains.ui.state.view = ViewState::UserSettings;
                state.domains.settings.current_view =
                    crate::domains::settings::state::SettingsView::Main;
                // Clear any security settings state
                state.domains.settings.security = Default::default();
                DomainUpdateResult::task(Task::none())
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
    }
}
