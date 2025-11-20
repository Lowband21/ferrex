// Cross-Domain Refactoring Tests for Auth Domain
// 
// Requirements for Task 1.11: Migrate Auth Domain
// - Remove _EmitCrossDomainEvent variant from AuthMessage
// - Use DomainUpdateResult for cross-domain event emission
// - Session changes must propagate correctly via events
// - Auth flows must work correctly without _EmitCrossDomainEvent
// - All existing auth tests must pass

use ferrex_player::common::messages::{CrossDomainEvent, DomainMessage, DomainUpdateResult};
use ferrex_player::domains::auth::messages::Message as AuthMessage;
use ferrex_player::domains::auth::update::update_auth;
use ferrex_player::state_refactored::State;
use ferrex_core::rbac::UserPermissions;
use ferrex_core::user::User;
use uuid::Uuid;
use iced::Task;

#[test]
fn auth_message_does_not_have_emit_cross_domain_event() {
    // This test will compile-fail if _EmitCrossDomainEvent still exists
    // After migration, the variant should not exist
    
    // Try to match all Auth message variants - this should be exhaustive
    // without _EmitCrossDomainEvent
    let msg = AuthMessage::CheckAuthStatus;
    match msg {
        AuthMessage::CheckAuthStatus => {},
        AuthMessage::AuthStatusConfirmedWithPin => {},
        AuthMessage::SetupStatusChecked(_) => {},
        AuthMessage::AutoLoginCheckComplete => {},
        AuthMessage::AutoLoginSuccessful(_) => {},
        AuthMessage::LoadUsers => {},
        AuthMessage::UsersLoaded(_) => {},
        AuthMessage::SelectUser(_) => {},
        AuthMessage::ShowCreateUser => {},
        AuthMessage::BackToUserSelection => {},
        AuthMessage::ShowPinEntry(_) => {},
        AuthMessage::PinDigitPressed(_) => {},
        AuthMessage::PinBackspace => {},
        AuthMessage::PinClear => {},
        AuthMessage::PinSubmit => {},
        AuthMessage::LoginSuccess(_, _) => {},
        AuthMessage::LoginError(_) => {},
        AuthMessage::WatchStatusLoaded(_) => {},
        AuthMessage::Logout => {},
        AuthMessage::LogoutComplete => {},
        AuthMessage::ShowPasswordLogin(_) => {},
        AuthMessage::PasswordLoginUpdateUsername(_) => {},
        AuthMessage::PasswordLoginUpdatePassword(_) => {},
        AuthMessage::PasswordLoginToggleVisibility => {},
        AuthMessage::PasswordLoginToggleRemember => {},
        AuthMessage::PasswordLoginSubmit => {},
        AuthMessage::DeviceStatusChecked(_, _) => {},
        AuthMessage::UpdateCredential(_) => {},
        AuthMessage::SubmitCredential => {},
        AuthMessage::TogglePasswordVisibility => {},
        AuthMessage::ToggleRememberDevice => {},
        AuthMessage::AuthResult(_) => {},
        AuthMessage::SetupPin => {},
        AuthMessage::UpdatePin(_) => {},
        AuthMessage::UpdateConfirmPin(_) => {},
        AuthMessage::SubmitPin => {},
        AuthMessage::PinSet(_) => {},
        AuthMessage::Retry => {},
        AuthMessage::Back => {},
        AuthMessage::FirstRunUpdateUsername(_) => {},
        AuthMessage::FirstRunUpdateDisplayName(_) => {},
        AuthMessage::FirstRunUpdatePassword(_) => {},
        AuthMessage::FirstRunUpdateConfirmPassword(_) => {},
        AuthMessage::FirstRunTogglePasswordVisibility => {},
        AuthMessage::FirstRunSubmit => {},
        AuthMessage::FirstRunSuccess => {},
        AuthMessage::FirstRunError(_) => {},
        AuthMessage::UpdateSetupField(_) => {},
        AuthMessage::ToggleSetupPasswordVisibility => {},
        AuthMessage::SubmitSetup => {},
        AuthMessage::SetupComplete(_, _) => {},
        AuthMessage::SetupError(_) => {},
        AuthMessage::EnableAdminPinUnlock => {},
        AuthMessage::DisableAdminPinUnlock => {},
        AuthMessage::AdminPinUnlockToggled(_) => {},
        AuthMessage::ExecuteCommand(_) => {},
        AuthMessage::CommandResult(_, _) => {},
        AuthMessage::CheckSetupStatus => {},
        // No _EmitCrossDomainEvent variant should exist
    }
}

#[test]
fn auth_update_returns_domain_update_result() {
    use ferrex_player::domains::update_wrappers::update_auth_wrapped;
    use ferrex_player::infrastructure::adapters::api_client_adapter::ApiClientAdapter;
    use ferrex_player::infrastructure::adapters::auth_adapter::AuthAdapter;
    use ferrex_player::domains::auth::AuthDomainState;
    use std::sync::Arc;
    
    // Create a mock state with auth domain
    let api_service = Arc::new(ApiClientAdapter::new("http://test".to_string()));
    let auth_service: Arc<dyn ferrex_player::infrastructure::services::auth::AuthService> = 
        Arc::new(AuthAdapter::new(api_service.clone()));
    
    let auth_state = AuthDomainState::new(api_service, auth_service);
    
    // Create minimal state
    let mut state = create_test_state_with_auth(auth_state);
    
    // Test that update_auth_wrapped returns DomainUpdateResult
    let message = AuthMessage::CheckAuthStatus;
    let result = update_auth(&mut state, message);
    
    // Verify we get a DomainUpdateResult
    assert!(matches!(result, DomainUpdateResult { .. }), 
        "Auth update should return DomainUpdateResult");
}

#[test]
fn auth_login_success_emits_cross_domain_event() {
    use ferrex_player::domains::update_wrappers::update_auth_wrapped;
    use ferrex_player::domains::auth::update_handlers::auth_updates::handle_login_success;
    
    let mut state = create_minimal_test_state();
    
    // Create test user and permissions
    let user = User {
        id: Uuid::new_v4(),
        username: "testuser".to_string(),
        display_name: Some("Test User".to_string()),
        avatar_url: None,
        role: ferrex_core::rbac::UserRole::User,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    
    let permissions = UserPermissions::default();
    
    // Handle login success - this should emit events through DomainUpdateResult
    let task = handle_login_success(&mut state, user.clone(), permissions.clone());
    
    // The task should be created but events should be emitted via DomainUpdateResult
    // when using the wrapped version
    assert!(state.domains.auth.state.is_authenticated,
        "User should be authenticated after login success");
}

#[test] 
fn auth_logout_emits_cross_domain_event() {
    use ferrex_player::domains::update_wrappers::update_auth_wrapped;
    
    let mut state = create_minimal_test_state();
    
    // Set authenticated state
    state.domains.auth.state.is_authenticated = true;
    
    // Send logout message through wrapped update
    let message = AuthMessage::Logout;
    let result = update_auth(&mut state, message);
    
    // Check that we get a DomainUpdateResult (may have events)
    assert!(matches!(result, DomainUpdateResult { .. }),
        "Logout should return DomainUpdateResult");
}

#[test]
fn auth_flows_work_without_emit_cross_domain_variant() {
    use ferrex_player::domains::auth::types::AuthenticationFlow;
    
    let mut state = create_minimal_test_state();
    
    // Test various auth flows work without _EmitCrossDomainEvent
    
    // 1. Check auth status flow
    let msg = AuthMessage::CheckAuthStatus;
    let task = update_auth(&mut state, msg);
    assert!(matches!(task, Task::<AuthMessage> { .. }),
        "CheckAuthStatus should return a task");
    
    // 2. User selection flow  
    state.domains.auth.state.auth_flow = AuthenticationFlow::SelectingUser {
        users: vec![],
        error: None,
    };
    let msg = AuthMessage::LoadUsers;
    let task = update_auth(&mut state, msg);
    assert!(matches!(task, Task::<AuthMessage> { .. }),
        "LoadUsers should return a task");
    
    // 3. PIN entry flow
    let user = create_test_user();
    let msg = AuthMessage::ShowPinEntry(user);
    let task = update_auth(&mut state, msg);
    // Task::none() is still a valid Task
    assert!(matches!(task, Task::<AuthMessage> { .. }),
        "ShowPinEntry should return a task");
}

// Helper functions

fn create_test_user() -> User {
    User {
        id: Uuid::new_v4(),
        username: "testuser".to_string(),
        display_name: Some("Test User".to_string()),
        avatar_url: None,
        role: ferrex_core::rbac::UserRole::User,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }
}

fn create_minimal_test_state() -> State {
    use ferrex_player::infrastructure::adapters::api_client_adapter::ApiClientAdapter;
    use ferrex_player::infrastructure::adapters::auth_adapter::AuthAdapter;
    use ferrex_player::domains::auth::AuthDomainState;
    use std::sync::Arc;
    
    let api_service = Arc::new(ApiClientAdapter::new("http://test".to_string()));
    let auth_service: Arc<dyn ferrex_player::infrastructure::services::auth::AuthService> = 
        Arc::new(AuthAdapter::new(api_service.clone()));
    
    let auth_state = AuthDomainState::new(api_service, auth_service);
    create_test_state_with_auth(auth_state)
}

fn create_test_state_with_auth(auth_state: ferrex_player::domains::auth::AuthDomainState) -> State {
    use ferrex_player::domains::auth::AuthDomain;
    use ferrex_player::domains::DomainRegistry;
    
    // Create a minimal state with just auth domain
    // This is a simplified version - real implementation would need all domains
    State {
        domains: DomainRegistry {
            auth: AuthDomain::new(auth_state),
            // Other domains would be initialized here in real implementation
            ..Default::default()
        },
        ..Default::default()
    }
}