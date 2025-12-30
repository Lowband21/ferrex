use crate::{
    common::messages::{DomainMessage, DomainUpdateResult},
    domains::{
        auth::security::SecureCredential,
        settings::{messages::SettingsMessage, state::SettingsSection},
        ui::shell_ui::UiShellMessage,
    },
    state::State,
};

use iced::Task;

/// Handle navigation to a settings section (new unified sidebar)
pub fn handle_navigate_to_section(
    state: &mut State,
    section: SettingsSection,
) -> DomainUpdateResult {
    state.domains.settings.current_section = section;

    // Clear sensitive data when navigating away from security
    if section != SettingsSection::Security {
        state.domains.settings.security.password_current =
            SecureCredential::new(String::new());
        state.domains.settings.security.password_new =
            SecureCredential::new(String::new());
        state.domains.settings.security.password_confirm =
            SecureCredential::new(String::new());
        state.domains.settings.security.pin_current =
            SecureCredential::new(String::new());
        state.domains.settings.security.pin_new =
            SecureCredential::new(String::new());
        state.domains.settings.security.pin_confirm =
            SecureCredential::new(String::new());
        state.domains.settings.security.password_error = None;
        state.domains.settings.security.pin_error = None;
    }

    // Trigger section-specific initialization if needed
    match section {
        SettingsSection::Security => {
            // Check if user has PIN when entering security section
            DomainUpdateResult::task(
                Task::done(SettingsMessage::CheckUserHasPin)
                    .map(DomainMessage::Settings),
            )
        }
        SettingsSection::Devices => {
            // Load devices when entering devices section
            DomainUpdateResult::task(
                Task::done(SettingsMessage::LoadDevices)
                    .map(DomainMessage::Settings),
            )
        }
        _ => DomainUpdateResult::task(Task::none()),
    }
}

/// Handle showing profile view
pub fn handle_show_profile(state: &mut State) -> DomainUpdateResult {
    // Load current user profile data
    let svc = state.domains.auth.state.auth_service.clone();
    let task = Task::perform(
        async move {
            svc.get_current_user()
                .await
                .map_err(|e| format!("{}", e))?
                .ok_or_else(|| "No current user".to_string())
        },
        |result| match result {
            Ok(user) => SettingsMessage::UpdateDisplayName(user.display_name),
            Err(e) => {
                log::error!("Failed to load user profile: {}", e);
                SettingsMessage::ProfileChangeResult(Err(e))
            }
        },
    );
    DomainUpdateResult::task(task.map(DomainMessage::Settings))
}

/// Handle back to main settings view
pub fn handle_back_to_main(state: &mut State) -> DomainUpdateResult {
    // Clear any sensitive data from security state
    state.domains.settings.security.password_current =
        SecureCredential::new(String::new());
    state.domains.settings.security.password_new =
        SecureCredential::new(String::new());
    state.domains.settings.security.password_confirm =
        SecureCredential::new(String::new());
    state.domains.settings.security.pin_current =
        SecureCredential::new(String::new());
    state.domains.settings.security.pin_new =
        SecureCredential::new(String::new());
    state.domains.settings.security.pin_confirm =
        SecureCredential::new(String::new());
    state.domains.settings.security.password_error = None;
    state.domains.settings.security.pin_error = None;

    DomainUpdateResult::task(Task::none())
}

/// Handle back to home (exit settings)
pub fn handle_back_to_home(state: &mut State) -> DomainUpdateResult {
    // Clear sensitive data from security state
    state.domains.settings.security.password_current =
        SecureCredential::new(String::new());
    state.domains.settings.security.password_new =
        SecureCredential::new(String::new());
    state.domains.settings.security.password_confirm =
        SecureCredential::new(String::new());
    state.domains.settings.security.pin_current =
        SecureCredential::new(String::new());
    state.domains.settings.security.pin_new =
        SecureCredential::new(String::new());
    state.domains.settings.security.pin_confirm =
        SecureCredential::new(String::new());
    state.domains.settings.security.password_error = None;
    state.domains.settings.security.pin_error = None;

    // Send direct UI domain message to navigate home
    DomainUpdateResult::task(Task::done(DomainMessage::Ui(
        UiShellMessage::NavigateHome.into(),
    )))
}
