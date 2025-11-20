use crate::common::messages::{DomainMessage, DomainUpdateResult};
use crate::domains::auth::security::SecureCredential;
use crate::domains::settings::messages::SettingsMessage;
use crate::domains::settings::state::SettingsView as SettingsSubview;
use crate::domains::ui;
use crate::state::State;
use iced::Task;

/// Handle showing profile view
pub fn handle_show_profile(state: &mut State) -> DomainUpdateResult {
    state.domains.settings.current_view = SettingsSubview::Profile;

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

/// Handle showing preferences view
pub fn handle_show_preferences(state: &mut State) -> DomainUpdateResult {
    state.domains.settings.current_view = SettingsSubview::Preferences;

    // Load current preferences
    let svc = state.domains.auth.state.auth_service.clone();

    let task = Task::perform(
        async move {
            // Get auto-login preference from auth storage
            svc.is_current_user_auto_login_enabled()
                .await
                .unwrap_or(false)
        },
        |enabled| {
            // Set the initial state then return message
            SettingsMessage::AutoLoginToggled(Ok(enabled))
        },
    );
    DomainUpdateResult::task(task.map(DomainMessage::Settings))
}

/// Handle showing security view
pub fn handle_show_security(state: &mut State) -> DomainUpdateResult {
    state.domains.settings.current_view = SettingsSubview::Security;

    // Check if user has PIN when entering security view
    DomainUpdateResult::task(
        Task::done(SettingsMessage::CheckUserHasPin).map(DomainMessage::Settings),
    )
}

/// Handle back to main settings view
pub fn handle_back_to_main(state: &mut State) -> DomainUpdateResult {
    state.domains.settings.current_view = SettingsSubview::Main;

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
        ui::messages::UiMessage::NavigateHome,
    )))
}
