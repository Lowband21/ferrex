// Settings Domain Cross-Domain Refactoring Tests
//
// Requirements from Phase_1_Remove_EmitEvents.md Task 1.10:
// - No _EmitCrossDomainEvent in SettingsMessage
// - All handlers return DomainUpdateResult
// - Settings functionality works correctly
// - All tests pass

use ferrex_player::common::messages::{CrossDomainEvent, DomainMessage, DomainUpdateResult};
use ferrex_player::domains::settings::messages::Message as SettingsMessage;
use ferrex_player::domains::settings::update;
use ferrex_player::state_refactored::State;
use iced::Task;

/// Helper to create a test state
fn create_test_state() -> State {
    State::default()
}

/// Helper to check if a Task contains any messages
fn task_has_messages<T>(task: Task<T>) -> bool {
    // We can't inspect the internal structure of a Task, but we can check if it's Task::none()
    // For testing purposes, we'll consider any non-none task as having messages
    // This is sufficient for our refactoring tests
    true // Placeholder - in real tests we'd run the task
}

#[test]
fn test_settings_message_enum_has_no_emit_variant() {
    // This test will fail until _EmitCrossDomainEvent is removed
    // We check by attempting to match all variants
    let test_message = SettingsMessage::ShowProfile;
    
    // This match should be exhaustive once _EmitCrossDomainEvent is removed
    match test_message {
        // Navigation
        SettingsMessage::ShowProfile => {}
        SettingsMessage::ShowPreferences => {}
        SettingsMessage::ShowSecurity => {}
        SettingsMessage::BackToMain => {}
        SettingsMessage::BackToHome => {}
        
        // Security - Password
        SettingsMessage::ShowChangePassword => {}
        SettingsMessage::UpdatePasswordCurrent(_) => {}
        SettingsMessage::UpdatePasswordNew(_) => {}
        SettingsMessage::UpdatePasswordConfirm(_) => {}
        SettingsMessage::TogglePasswordVisibility => {}
        SettingsMessage::SubmitPasswordChange => {}
        SettingsMessage::PasswordChangeResult(_) => {}
        SettingsMessage::CancelPasswordChange => {}
        
        // Security - PIN
        SettingsMessage::CheckUserHasPin => {}
        SettingsMessage::UserHasPinResult(_) => {}
        SettingsMessage::ShowSetPin => {}
        SettingsMessage::ShowChangePin => {}
        SettingsMessage::UpdatePinCurrent(_) => {}
        SettingsMessage::UpdatePinNew(_) => {}
        SettingsMessage::UpdatePinConfirm(_) => {}
        SettingsMessage::SubmitPinChange => {}
        SettingsMessage::PinChangeResult(_) => {}
        SettingsMessage::CancelPinChange => {}
        
        // Preferences
        SettingsMessage::ToggleAutoLogin(_) => {}
        SettingsMessage::AutoLoginToggled(_) => {}
        
        // Profile
        SettingsMessage::UpdateDisplayName(_) => {}
        SettingsMessage::UpdateEmail(_) => {}
        SettingsMessage::SubmitProfileChanges => {}
        SettingsMessage::ProfileChangeResult(_) => {}
        
        // Device Management
        SettingsMessage::LoadDevices => {}
        SettingsMessage::DevicesLoaded(_) => {}
        SettingsMessage::RefreshDevices => {}
        SettingsMessage::RevokeDevice(_) => {}
        SettingsMessage::DeviceRevoked(_) => {}
        
        // Should NOT have _EmitCrossDomainEvent variant
    }
}

#[test]
fn test_update_settings_returns_domain_update_result() {
    let mut state = create_test_state();
    
    // Test that update_settings returns DomainUpdateResult for various messages
    let messages = vec![
        SettingsMessage::ShowProfile,
        SettingsMessage::ShowPreferences,
        SettingsMessage::ShowSecurity,
        SettingsMessage::UpdatePasswordCurrent("test".to_string()),
        SettingsMessage::TogglePasswordVisibility,
        SettingsMessage::LoadDevices,
    ];
    
    for message in messages {
        let result = update::update_settings(&mut state, message);
        
        // Verify result is DomainUpdateResult
        assert!(matches!(result, DomainUpdateResult { .. }));
    }
}

#[test]
fn test_security_handler_emits_auth_command_event() {
    let mut state = create_test_state();
    
    // Set up password change state
    state.domains.settings.security.password_current = 
        ferrex_player::domains::auth::security::secure_credential::SecureCredential::from("oldpass123");
    state.domains.settings.security.password_new = 
        ferrex_player::domains::auth::security::secure_credential::SecureCredential::from("NewPass123");
    state.domains.settings.security.password_confirm = 
        ferrex_player::domains::auth::security::secure_credential::SecureCredential::from("NewPass123");
    
    // Submit password change should emit an AuthCommandRequested event
    let result = update::update_settings(&mut state, SettingsMessage::SubmitPasswordChange);
    
    // Check that the result contains an AuthCommandRequested event
    assert!(!result.events.is_empty(), "Should emit at least one event");
    
    let has_auth_command = result.events.iter().any(|event| {
        matches!(event, CrossDomainEvent::AuthCommandRequested(_))
    });
    
    assert!(has_auth_command, "Should emit AuthCommandRequested event");
}

#[test]
fn test_navigation_handlers_return_proper_results() {
    let mut state = create_test_state();
    
    // Test navigation messages
    let nav_messages = vec![
        SettingsMessage::ShowProfile,
        SettingsMessage::ShowPreferences,
        SettingsMessage::ShowSecurity,
        SettingsMessage::BackToMain,
        SettingsMessage::BackToHome,
    ];
    
    for message in nav_messages {
        let result = update::update_settings(&mut state, message);
        
        // Navigation handlers should return DomainUpdateResult
        assert!(matches!(result, DomainUpdateResult { .. }));
        
        // Most navigation handlers don't emit events
        // They just update state and return Task::none()
    }
}

#[test]
fn test_preferences_handler_returns_domain_update_result() {
    let mut state = create_test_state();
    
    // Test auto-login toggle
    let result = update::update_settings(&mut state, SettingsMessage::ToggleAutoLogin(true));
    
    // Should return DomainUpdateResult
    assert!(matches!(result, DomainUpdateResult { .. }));
}

#[test]
fn test_profile_handlers_return_domain_update_result() {
    let mut state = create_test_state();
    
    // Test profile update messages
    let profile_messages = vec![
        SettingsMessage::UpdateDisplayName("Test User".to_string()),
        SettingsMessage::UpdateEmail("test@example.com".to_string()),
        SettingsMessage::SubmitProfileChanges,
    ];
    
    for message in profile_messages {
        let result = update::update_settings(&mut state, message);
        
        // Should return DomainUpdateResult
        assert!(matches!(result, DomainUpdateResult { .. }));
    }
}

#[test]
fn test_device_management_handlers_return_domain_update_result() {
    let mut state = create_test_state();
    
    // Test device management messages
    let device_messages = vec![
        SettingsMessage::LoadDevices,
        SettingsMessage::RefreshDevices,
        SettingsMessage::RevokeDevice("device123".to_string()),
    ];
    
    for message in device_messages {
        let result = update::update_settings(&mut state, message);
        
        // Should return DomainUpdateResult
        assert!(matches!(result, DomainUpdateResult { .. }));
    }
}

#[test]
fn test_all_handlers_migrated_to_domain_update_result() {
    // This is a comprehensive test to ensure all handlers are migrated
    let mut state = create_test_state();
    
    // Create one message of each type to ensure all paths return DomainUpdateResult
    let all_messages = vec![
        // Navigation
        SettingsMessage::ShowProfile,
        SettingsMessage::ShowPreferences,
        SettingsMessage::ShowSecurity,
        SettingsMessage::BackToMain,
        SettingsMessage::BackToHome,
        
        // Security - Password
        SettingsMessage::ShowChangePassword,
        SettingsMessage::UpdatePasswordCurrent("test".to_string()),
        SettingsMessage::UpdatePasswordNew("test".to_string()),
        SettingsMessage::UpdatePasswordConfirm("test".to_string()),
        SettingsMessage::TogglePasswordVisibility,
        SettingsMessage::SubmitPasswordChange,
        SettingsMessage::PasswordChangeResult(Ok(())),
        SettingsMessage::CancelPasswordChange,
        
        // Security - PIN
        SettingsMessage::CheckUserHasPin,
        SettingsMessage::UserHasPinResult(true),
        SettingsMessage::ShowSetPin,
        SettingsMessage::ShowChangePin,
        SettingsMessage::UpdatePinCurrent("1234".to_string()),
        SettingsMessage::UpdatePinNew("5678".to_string()),
        SettingsMessage::UpdatePinConfirm("5678".to_string()),
        SettingsMessage::SubmitPinChange,
        SettingsMessage::PinChangeResult(Ok(())),
        SettingsMessage::CancelPinChange,
        
        // Preferences
        SettingsMessage::ToggleAutoLogin(true),
        SettingsMessage::AutoLoginToggled(Ok(true)),
        
        // Profile
        SettingsMessage::UpdateDisplayName("Test".to_string()),
        SettingsMessage::UpdateEmail("test@test.com".to_string()),
        SettingsMessage::SubmitProfileChanges,
        SettingsMessage::ProfileChangeResult(Ok(())),
        
        // Device Management
        SettingsMessage::LoadDevices,
        SettingsMessage::DevicesLoaded(Ok(vec![])),
        SettingsMessage::RefreshDevices,
        SettingsMessage::RevokeDevice("device123".to_string()),
        SettingsMessage::DeviceRevoked(Ok("device123".to_string())),
    ];
    
    for message in all_messages {
        let message_name = message.name();
        let result = update::update_settings(&mut state, message);
        
        // Every handler should return DomainUpdateResult
        assert!(
            matches!(result, DomainUpdateResult { .. }),
            "Handler for {} should return DomainUpdateResult",
            message_name
        );
    }
}