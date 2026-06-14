use ferrex_player::common::messages::CrossDomainEvent;
use ferrex_player::domains::auth::messages::commands::AuthCommand;
use ferrex_player::domains::auth::security::secure_credential::SecureCredential;
use ferrex_player::domains::settings::messages::SettingsMessage;
use ferrex_player::domains::settings::update;
use ferrex_player::state::State;

#[tokio::test]
async fn password_change_emits_auth_command() {
    let mut state = State::default();
    state.domains.settings.security.password_current =
        SecureCredential::from("OldPass123");
    state.domains.settings.security.password_new =
        SecureCredential::from("NewPass123");
    state.domains.settings.security.password_confirm =
        SecureCredential::from("NewPass123");

    let result = update::update_settings(
        &mut state,
        SettingsMessage::SubmitPasswordChange,
    );

    assert!(state.domains.settings.security.password_loading);
    assert!(result.events.iter().any(|event| matches!(
        event,
        CrossDomainEvent::AuthCommandRequested(
            AuthCommand::ChangePassword { .. }
        )
    )));
}

#[tokio::test]
async fn pin_set_and_change_emit_distinct_auth_commands() {
    let mut state = State::default();
    state.domains.settings.security.has_pin = false;
    state.domains.settings.security.pin_new = SecureCredential::from("2580");
    state.domains.settings.security.pin_confirm =
        SecureCredential::from("2580");

    let set_result =
        update::update_settings(&mut state, SettingsMessage::SubmitPinChange);
    assert!(set_result.events.iter().any(|event| matches!(
        event,
        CrossDomainEvent::AuthCommandRequested(AuthCommand::SetUserPin { .. })
    )));

    state.domains.settings.security.pin_loading = false;
    state.domains.settings.security.has_pin = true;
    state.domains.settings.security.pin_current =
        SecureCredential::from("2580");
    state.domains.settings.security.pin_new = SecureCredential::from("3690");
    state.domains.settings.security.pin_confirm =
        SecureCredential::from("3690");

    let change_result =
        update::update_settings(&mut state, SettingsMessage::SubmitPinChange);
    assert!(change_result.events.iter().any(|event| matches!(
        event,
        CrossDomainEvent::AuthCommandRequested(
            AuthCommand::ChangeUserPin { .. }
        )
    )));
}

#[tokio::test]
async fn pin_removal_emits_command_and_clears_state_on_result() {
    let mut state = State::default();
    state.domains.settings.security.has_pin = true;
    state.domains.settings.security.pin_current =
        SecureCredential::from("2580");

    let result =
        update::update_settings(&mut state, SettingsMessage::SubmitPinRemoval);

    assert!(state.domains.settings.security.pin_loading);
    let remove_cmd = result.events.iter().any(|e| {
        matches!(
            e,
            CrossDomainEvent::AuthCommandRequested(
                AuthCommand::RemoveUserPin { .. }
            )
        )
    });
    assert!(remove_cmd, "should request RemoveUserPin");

    let _ = update::update_settings(
        &mut state,
        SettingsMessage::PinRemovalResult(Ok(())),
    );
    assert!(!state.domains.settings.security.has_pin);
    assert!(!state.domains.settings.security.pin_loading);
}
