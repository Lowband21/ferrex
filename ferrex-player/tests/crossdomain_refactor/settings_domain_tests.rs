use ferrex_player::common::messages::CrossDomainEvent;
use ferrex_player::domains::auth::messages::commands::AuthCommand;
use ferrex_player::domains::settings::messages::Message as SettingsMessage;
use ferrex_player::domains::settings::state::SettingsView;
use ferrex_player::domains::settings::update;
use ferrex_player::state::State;

fn create_test_state() -> State {
    State::default()
}

#[test]
fn password_change_validates_and_emits_event() {
    let mut state = create_test_state();

    // Valid inputs
    state.domains.settings.security.password_current =
        ferrex_player::domains::auth::security::secure_credential::SecureCredential::from("OldPass1");
    state.domains.settings.security.password_new =
        ferrex_player::domains::auth::security::secure_credential::SecureCredential::from("NewPass1");
    state.domains.settings.security.password_confirm =
        ferrex_player::domains::auth::security::secure_credential::SecureCredential::from("NewPass1");

    let result = update::update_settings(&mut state, SettingsMessage::SubmitPasswordChange);

    // Emits AuthCommandRequested(ChangePassword) and sets loading, no error
    assert!(state.domains.settings.security.password_loading);
    assert!(state.domains.settings.security.password_error.is_none());
    let change_cmd = result.events.iter().any(|e| match e {
        CrossDomainEvent::AuthCommandRequested(AuthCommand::ChangePassword { .. }) => true,
        _ => false,
    });
    assert!(change_cmd, "should request ChangePassword auth command");
}

#[test]
fn password_change_validation_errors_no_event() {
    let mut state = create_test_state();

    // Missing complexity and too short
    state.domains.settings.security.password_current =
        ferrex_player::domains::auth::security::secure_credential::SecureCredential::from("old");
    state.domains.settings.security.password_new =
        ferrex_player::domains::auth::security::secure_credential::SecureCredential::from("short");
    state.domains.settings.security.password_confirm =
        ferrex_player::domains::auth::security::secure_credential::SecureCredential::from("short");

    let result = update::update_settings(&mut state, SettingsMessage::SubmitPasswordChange);

    assert!(result.events.is_empty());
    assert!(state.domains.settings.security.password_error.is_some());
    assert!(!state.domains.settings.security.password_loading);
}

#[test]
fn pin_set_and_change_emit_correct_commands() {
    // Case 1: setting a new PIN (has_pin = false)
    let mut state = create_test_state();
    state.domains.settings.security.has_pin = false;
    state.domains.settings.security.pin_new =
        ferrex_player::domains::auth::security::secure_credential::SecureCredential::from("1234");
    state.domains.settings.security.pin_confirm =
        ferrex_player::domains::auth::security::secure_credential::SecureCredential::from("1234");

    let result = update::update_settings(&mut state, SettingsMessage::SubmitPinChange);
    assert!(state.domains.settings.security.pin_loading);
    let set_cmd = result.events.iter().any(|e| match e {
        CrossDomainEvent::AuthCommandRequested(AuthCommand::SetUserPin { .. }) => true,
        _ => false,
    });
    assert!(set_cmd, "should request SetUserPin when user has no PIN");

    // Case 2: changing existing PIN (has_pin = true)
    let mut state = create_test_state();
    state.domains.settings.security.has_pin = true;
    state.domains.settings.security.pin_current =
        ferrex_player::domains::auth::security::secure_credential::SecureCredential::from("1111");
    state.domains.settings.security.pin_new =
        ferrex_player::domains::auth::security::secure_credential::SecureCredential::from("2222");
    state.domains.settings.security.pin_confirm =
        ferrex_player::domains::auth::security::secure_credential::SecureCredential::from("2222");

    let result = update::update_settings(&mut state, SettingsMessage::SubmitPinChange);
    assert!(state.domains.settings.security.pin_loading);
    let change_cmd = result.events.iter().any(|e| match e {
        CrossDomainEvent::AuthCommandRequested(AuthCommand::ChangeUserPin { .. }) => true,
        _ => false,
    });
    assert!(change_cmd, "should request ChangeUserPin when user has a PIN");
}

#[test]
fn auto_login_toggle_updates_state_on_result() {
    let mut state = create_test_state();
    // Simulate successful toggle completion message
    let _ = update::update_settings(&mut state, SettingsMessage::AutoLoginToggled(Ok(true)));

    assert!(state.domains.settings.preferences.auto_login_enabled);
    assert!(state.domains.auth.state.auto_login_enabled);
}

#[test]
fn navigation_sets_current_view() {
    let mut state = create_test_state();
    assert_eq!(state.domains.settings.current_view, SettingsView::Main);

    let _ = update::update_settings(&mut state, SettingsMessage::ShowSecurity);
    assert_eq!(state.domains.settings.current_view, SettingsView::Security);

    let _ = update::update_settings(&mut state, SettingsMessage::BackToMain);
    assert_eq!(state.domains.settings.current_view, SettingsView::Main);
}
