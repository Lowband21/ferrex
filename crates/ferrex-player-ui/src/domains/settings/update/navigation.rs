//! Desktop adapter for settings navigation reducers.

use crate::{
    common::messages::{DomainMessage, DomainUpdateResult},
    domains::{
        settings::{messages::SettingsMessage, state::SettingsSection},
        ui::shell_ui::UiShellMessage,
    },
    state::State,
};
use ferrex_player_settings::update::navigation as reducer;
use iced::Task;

use super::apply_settings_update;

pub fn handle_navigate_to_section(
    state: &mut State,
    section: SettingsSection,
) -> DomainUpdateResult {
    let update = reducer::navigate(
        &mut state.domains.settings.current_section,
        &mut state.domains.settings.security,
        section,
    );
    apply_settings_update(state, update)
}

pub fn handle_show_profile(state: &mut State) -> DomainUpdateResult {
    let auth_service = state.domains.auth.state.auth_service.clone();
    let task = Task::perform(
        async move {
            auth_service
                .get_current_user()
                .await
                .map_err(|error| error.to_string())?
                .ok_or_else(|| "No current user".to_string())
        },
        |result| match result {
            Ok(user) => DomainMessage::Settings(
                SettingsMessage::UpdateDisplayName(user.display_name),
            ),
            Err(error) => DomainMessage::Settings(
                SettingsMessage::ProfileChangeResult(Err(error)),
            ),
        },
    );
    DomainUpdateResult::task(task)
}

pub fn handle_back_to_main(state: &mut State) -> DomainUpdateResult {
    let update =
        reducer::clear_sensitive_security(&mut state.domains.settings.security);
    apply_settings_update(state, update)
}

pub fn handle_back_to_home(state: &mut State) -> DomainUpdateResult {
    let update =
        reducer::clear_sensitive_security(&mut state.domains.settings.security);
    let mut result = apply_settings_update(state, update);
    result.task = Task::batch(vec![
        result.task,
        Task::done(DomainMessage::Ui(UiShellMessage::NavigateHome.into())),
    ]);
    result
}
