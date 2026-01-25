use ferrex_player::domains::auth::types::{AuthenticationFlow, CredentialType};
use ferrex_player::domains::auth::update_handlers::auth_updates;
use ferrex_player::state::State;

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

#[test]
fn users_loaded_success_sets_selecting_user() {
    let mut state = State::default();
    let user_dto = ferrex_player::domains::auth::dto::UserListItemDto {
        id: uuid::Uuid::now_v7(),
        username: "alice".into(),
        display_name: "Alice".into(),
        avatar_url: None,
        last_login: None,
    };

    let _ = auth_updates::handle_users_loaded(
        &mut state,
        Ok(vec![user_dto.clone()]),
    );

    match &state.domains.auth.state.auth_flow {
        AuthenticationFlow::SelectingUser { users, error } => {
            assert_eq!(users.len(), 1);
            assert!(error.is_none());
            assert_eq!(users[0].username, "alice");
        }
        other => panic!("unexpected flow: {:?}", other),
    }
}

#[test]
fn device_status_with_pin_shows_pin_entry() {
    let mut state = State::default();
    let user = make_user(uuid::Uuid::now_v7(), "bob");

    let status = ferrex_player::domains::auth::manager::DeviceAuthStatus {
        device_registered: true,
        has_pin: true,
        remaining_attempts: Some(3),
    };

    let _ = auth_updates::handle_device_status_checked(
        &mut state,
        user.clone(),
        Ok(status),
    );

    match &state.domains.auth.state.auth_flow {
        AuthenticationFlow::EnteringCredentials {
            user: u,
            input_type,
            ..
        } => {
            assert_eq!(u.username, "bob");
            match input_type {
                CredentialType::Pin { max_length } => {
                    assert_eq!(*max_length, 4)
                }
                _ => panic!("expected PIN input type"),
            }
        }
        other => panic!("unexpected flow: {:?}", other),
    }
}

#[test]
fn device_status_no_pin_shows_password_entry() {
    let mut state = State::default();
    let user = make_user(uuid::Uuid::now_v7(), "carol");

    let status = ferrex_player::domains::auth::manager::DeviceAuthStatus {
        device_registered: false,
        has_pin: false,
        remaining_attempts: None,
    };

    let _ = auth_updates::handle_device_status_checked(
        &mut state,
        user.clone(),
        Ok(status),
    );

    match &state.domains.auth.state.auth_flow {
        AuthenticationFlow::EnteringCredentials {
            user: u,
            input_type,
            ..
        } => {
            assert_eq!(u.username, "carol");
            match input_type {
                CredentialType::Password => {}
                _ => panic!("expected Password input type"),
            }
        }
        other => panic!("unexpected flow: {:?}", other),
    }
}
