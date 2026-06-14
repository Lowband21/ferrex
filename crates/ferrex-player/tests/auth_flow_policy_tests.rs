use ferrex_player::domains::auth::update::update_auth;
use ferrex_player::domains::auth::update_handlers as auth_updates;
use ferrex_player::infra::api_client::SetupStatus;
use ferrex_player::state::State;
use ferrex_player_auth::manager::{
    DeviceAuthStatus, DeviceTrustPolicyResponse, PinPolicyResponse,
};
use ferrex_player_auth::messages::AuthMessage;
use ferrex_player_auth::security::secure_credential::SecureCredential;
use ferrex_player_auth::types::{
    AuthenticationFlow, CredentialType, SetupStep,
};

fn make_user(
    id: uuid::Uuid,
    username: &str,
) -> ferrex_core::domain::users::user::User {
    ferrex_core::domain::users::user::User {
        id,
        username: username.to_string(),
        display_name: format!("{} display", username),
        avatar_url: None,
        email: None,
        is_active: true,
        last_login: None,
        preferences: ferrex_core::domain::users::user::UserPreferences::default(
        ),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }
}

fn make_permissions(
    user_id: uuid::Uuid,
) -> ferrex_core::player_prelude::UserPermissions {
    ferrex_core::player_prelude::UserPermissions {
        user_id,
        roles: Vec::new(),
        permissions: std::collections::HashMap::new(),
        permission_details: None,
    }
}

#[tokio::test]
async fn device_status_threads_configured_pin_policy_into_pin_entry() {
    let mut state = State::default();
    let user = make_user(uuid::Uuid::now_v7(), "dana");

    let status = DeviceAuthStatus {
        device_registered: true,
        has_pin: true,
        remaining_attempts: Some(2),
        pin_policy: PinPolicyResponse {
            min_length: 5,
            max_length: 6,
            ..Default::default()
        },
        ..Default::default()
    };

    let _ = auth_updates::handle_device_status_checked(
        &mut state,
        user.clone(),
        Ok(status),
    );

    assert_eq!(state.domains.auth.state.pin_policy.min_length, 5);
    assert_eq!(state.domains.auth.state.pin_policy.max_length, 6);
    match &state.domains.auth.state.auth_flow {
        AuthenticationFlow::EnteringCredentials {
            user: u,
            input_type,
            ..
        } => {
            assert_eq!(u.username, "dana");
            match input_type {
                CredentialType::Pin {
                    min_length,
                    max_length,
                } => {
                    assert_eq!(*min_length, 5);
                    assert_eq!(*max_length, 6);
                }
                _ => panic!("expected PIN input type"),
            }
        }
        other => panic!("unexpected flow: {:?}", other),
    }
}

#[tokio::test]
async fn remember_device_sync_preserves_server_default_after_device_status() {
    let mut state = State::default();
    state.domains.auth.state.auto_login_enabled = false;
    let user = make_user(uuid::Uuid::now_v7(), "fran");

    let status = DeviceAuthStatus {
        device_registered: false,
        has_pin: false,
        device_trust_policy: DeviceTrustPolicyResponse {
            remember_device_default: true,
            ..Default::default()
        },
        ..Default::default()
    };

    let _ = update_auth(
        &mut state,
        AuthMessage::DeviceStatusChecked(user, Ok(status)),
    );

    match &state.domains.auth.state.auth_flow {
        AuthenticationFlow::EnteringCredentials {
            input_type,
            remember_device,
            ..
        } => {
            assert!(matches!(input_type, CredentialType::Password));
            assert!(*remember_device);
        }
        other => panic!("unexpected flow: {:?}", other),
    }

    let _ = update_auth(&mut state, AuthMessage::RememberDeviceSynced(false));

    assert!(state.domains.auth.state.auto_login_enabled);
    match &state.domains.auth.state.auth_flow {
        AuthenticationFlow::EnteringCredentials {
            input_type,
            remember_device,
            ..
        } => {
            assert!(matches!(input_type, CredentialType::Password));
            assert!(*remember_device);
        }
        other => panic!("unexpected flow: {:?}", other),
    }
}

#[tokio::test]
async fn setup_status_threads_configured_policy_into_first_run_pin_entry() {
    let mut state = State::default();

    let status = SetupStatus {
        needs_setup: true,
        has_admin: false,
        requires_setup_token: true,
        user_count: 0,
        library_count: 0,
        pin_policy: PinPolicyResponse {
            min_length: 5,
            max_length: 6,
            ..Default::default()
        },
        device_trust_policy: DeviceTrustPolicyResponse {
            remember_device_default: true,
            pin_max_attempts: 7,
            ..Default::default()
        },
        ..Default::default()
    };

    let _ = auth_updates::handle_setup_status_checked(&mut state, status);

    assert_eq!(state.domains.auth.state.pin_policy.min_length, 5);
    assert_eq!(state.domains.auth.state.pin_policy.max_length, 6);
    assert!(
        state
            .domains
            .auth
            .state
            .device_trust_policy
            .remember_device_default
    );
    assert_eq!(
        state
            .domains
            .auth
            .state
            .device_trust_policy
            .pin_max_attempts,
        7
    );

    match &mut state.domains.auth.state.auth_flow {
        AuthenticationFlow::FirstRunSetup {
            current_step,
            setup_token_required,
            ..
        } => {
            assert!(*setup_token_required);
            *current_step = SetupStep::Pin;
        }
        other => panic!("unexpected flow: {:?}", other),
    }

    let _ = auth_updates::handle_auth_flow_update_pin(
        &mut state,
        "2580987".to_string(),
    );
    match &state.domains.auth.state.auth_flow {
        AuthenticationFlow::FirstRunSetup { pin, .. } => {
            assert_eq!(pin.as_str(), "258098");
        }
        other => panic!("unexpected flow: {:?}", other),
    }

    let _ = auth_updates::handle_auth_flow_update_pin(
        &mut state,
        "2580".to_string(),
    );
    let _ = auth_updates::handle_auth_flow_update_confirm_pin(
        &mut state,
        "2580".to_string(),
    );
    let _ = auth_updates::handle_setup_next_step(&mut state);

    match &state.domains.auth.state.auth_flow {
        AuthenticationFlow::FirstRunSetup {
            current_step,
            error,
            ..
        } => {
            assert!(matches!(current_step, SetupStep::Pin));
            assert_eq!(error.as_deref(), Some("PIN must be at least 5 digits"));
        }
        other => panic!("unexpected flow: {:?}", other),
    }
}

#[tokio::test]
async fn password_login_with_remember_prompts_for_pin_setup() {
    let mut state = State::default();
    let user = make_user(uuid::Uuid::now_v7(), "paula");
    state.domains.auth.state.auth_flow = AuthenticationFlow::PreAuthLogin {
        username: user.username.clone(),
        password: SecureCredential::new("OldPass123".to_string()),
        show_password: false,
        remember_device: true,
        error: None,
        loading: true,
    };

    let _ = auth_updates::handle_auth_flow_auth_result(
        &mut state,
        Ok(ferrex_player_auth::PlayerAuthResult {
            user: user.clone(),
            permissions: make_permissions(user.id),
            device_has_pin: false,
        }),
    );

    match &state.domains.auth.state.auth_flow {
        AuthenticationFlow::SettingUpPin {
            user: setup_user,
            pin,
            confirm_pin,
            error,
            ..
        } => {
            assert_eq!(setup_user.id, user.id);
            assert!(pin.is_empty());
            assert!(confirm_pin.is_empty());
            assert!(error.is_none());
        }
        other => panic!("unexpected flow: {:?}", other),
    }
}

#[tokio::test]
async fn pin_login_success_completes_without_reprompting_for_pin_setup() {
    let mut state = State::default();
    let user = make_user(uuid::Uuid::now_v7(), "quinn");
    state.domains.auth.state.auth_flow =
        AuthenticationFlow::EnteringCredentials {
            user: user.clone(),
            input_type: CredentialType::Pin {
                min_length: 4,
                max_length: 8,
            },
            input: SecureCredential::new("2580".to_string()),
            show_password: false,
            remember_device: false,
            error: None,
            attempts_remaining: Some(5),
            loading: true,
        };

    let _ = auth_updates::handle_auth_flow_auth_result(
        &mut state,
        Ok(ferrex_player_auth::PlayerAuthResult {
            user: user.clone(),
            permissions: make_permissions(user.id),
            device_has_pin: true,
        }),
    );

    match &state.domains.auth.state.auth_flow {
        AuthenticationFlow::Authenticated { user: authed, .. } => {
            assert_eq!(authed.id, user.id);
        }
        other => panic!("unexpected flow: {:?}", other),
    }
    assert!(state.domains.auth.state.user_permissions.is_some());
}

#[tokio::test]
async fn pin_lockout_error_stops_loading_and_decrements_attempts() {
    let mut state = State::default();
    let user = make_user(uuid::Uuid::now_v7(), "riley");
    state.domains.auth.state.auth_flow =
        AuthenticationFlow::EnteringCredentials {
            user,
            input_type: CredentialType::Pin {
                min_length: 4,
                max_length: 8,
            },
            input: SecureCredential::new("9999".to_string()),
            show_password: false,
            remember_device: false,
            error: None,
            attempts_remaining: Some(1),
            loading: true,
        };

    let _ = auth_updates::handle_auth_flow_auth_result(
        &mut state,
        Err("PIN locked after too many attempts".to_string()),
    );

    match &state.domains.auth.state.auth_flow {
        AuthenticationFlow::EnteringCredentials {
            error,
            loading,
            attempts_remaining,
            ..
        } => {
            assert!(!loading);
            assert_eq!(*attempts_remaining, Some(0));
            assert!(error.as_deref().unwrap_or_default().contains("locked"));
        }
        other => panic!("unexpected flow: {:?}", other),
    }
}

#[tokio::test]
async fn first_run_skip_pin_clears_partial_pin_and_completes() {
    let mut state = State::default();
    let _ = auth_updates::handle_setup_status_checked(
        &mut state,
        SetupStatus {
            needs_setup: true,
            ..Default::default()
        },
    );

    match &mut state.domains.auth.state.auth_flow {
        AuthenticationFlow::FirstRunSetup { current_step, .. } => {
            *current_step = SetupStep::Pin;
        }
        other => panic!("unexpected flow: {:?}", other),
    }

    let _ = auth_updates::handle_auth_flow_update_pin(
        &mut state,
        "2580".to_string(),
    );
    let _ = auth_updates::handle_skip_pin_setup(&mut state);

    match &state.domains.auth.state.auth_flow {
        AuthenticationFlow::FirstRunSetup {
            current_step,
            pin,
            confirm_pin,
            error,
            ..
        } => {
            assert!(matches!(current_step, SetupStep::Complete));
            assert!(pin.is_empty());
            assert!(confirm_pin.is_empty());
            assert!(error.is_none());
        }
        other => panic!("unexpected flow: {:?}", other),
    }
}

#[tokio::test]
async fn pin_entry_can_fall_back_to_password_for_recovery() {
    let mut state = State::default();
    let user = make_user(uuid::Uuid::now_v7(), "gina");
    state.domains.auth.state.auth_flow =
        AuthenticationFlow::EnteringCredentials {
            user: user.clone(),
            input_type: CredentialType::Pin {
                min_length: 4,
                max_length: 8,
            },
            input: SecureCredential::new("2580".to_string()),
            show_password: false,
            remember_device: false,
            error: Some("device key not registered".to_string()),
            attempts_remaining: Some(2),
            loading: false,
        };

    let _ = auth_updates::handle_auth_flow_use_password_login(&mut state);

    match &state.domains.auth.state.auth_flow {
        AuthenticationFlow::EnteringCredentials {
            user: recovered_user,
            input_type,
            input,
            remember_device,
            error,
            attempts_remaining,
            ..
        } => {
            assert_eq!(recovered_user.id, user.id);
            assert!(matches!(input_type, CredentialType::Password));
            assert!(input.is_empty());
            assert!(*remember_device);
            assert!(attempts_remaining.is_none());
            assert!(
                error
                    .as_deref()
                    .unwrap_or_default()
                    .contains("repair this device")
            );
        }
        other => panic!("unexpected flow: {:?}", other),
    }
}

#[tokio::test]
async fn reset_local_auth_completion_shows_password_recovery() {
    let mut state = State::default();
    state.domains.auth.state.auto_login_enabled = true;
    state.domains.settings.preferences.auto_login_enabled = true;

    let _ = auth_updates::handle_local_auth_state_reset(&mut state, Ok(()));

    match &state.domains.auth.state.auth_flow {
        AuthenticationFlow::PreAuthLogin {
            username,
            remember_device,
            error,
            ..
        } => {
            assert!(username.is_empty());
            assert!(!remember_device);
            assert!(
                error.as_deref().unwrap_or_default().contains("Local auth")
            );
        }
        other => panic!("unexpected flow: {:?}", other),
    }
}

#[tokio::test]
async fn pin_credential_update_and_submit_use_configured_policy_lengths() {
    let mut state = State::default();
    state.domains.auth.state.pin_policy = PinPolicyResponse {
        min_length: 5,
        max_length: 6,
        ..Default::default()
    };
    state.domains.auth.state.auth_flow =
        AuthenticationFlow::EnteringCredentials {
            user: make_user(uuid::Uuid::now_v7(), "erin"),
            input_type: CredentialType::Pin {
                min_length: 5,
                max_length: 6,
            },
            input: SecureCredential::new(String::new()),
            show_password: false,
            remember_device: false,
            error: None,
            attempts_remaining: None,
            loading: false,
        };

    let _ = auth_updates::handle_auth_flow_update_credential(
        &mut state,
        "2580987".to_string(),
    );
    match &state.domains.auth.state.auth_flow {
        AuthenticationFlow::EnteringCredentials { input, .. } => {
            assert_eq!(input.as_str(), "258098");
        }
        other => panic!("unexpected flow: {:?}", other),
    }

    let _ = auth_updates::handle_auth_flow_update_credential(
        &mut state,
        "2580".to_string(),
    );
    let _ = auth_updates::handle_auth_flow_submit_credential(&mut state);
    match &state.domains.auth.state.auth_flow {
        AuthenticationFlow::EnteringCredentials { error, loading, .. } => {
            assert_eq!(error.as_deref(), Some("PIN must be at least 5 digits"));
            assert!(!loading);
        }
        other => panic!("unexpected flow: {:?}", other),
    }
}
