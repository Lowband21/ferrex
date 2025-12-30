//! Tests for centralized auth completion signaling

use chrono::Utc;
use ferrex_core::player_prelude::{User, UserPermissions};
use ferrex_player::common::messages::DomainUpdateResult;
use ferrex_player::domains::auth::messages as auth_msgs;
use ferrex_player::state::State;
use uuid::Uuid;

fn make_user() -> User {
    User {
        id: Uuid::now_v7(),
        username: "testuser".to_string(),
        display_name: "Test User".to_string(),
        avatar_url: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        last_login: Some(Utc::now()),
        is_active: true,
        email: None,
        preferences: Default::default(),
    }
}

fn make_permissions(user_id: Uuid) -> UserPermissions {
    UserPermissions {
        user_id,
        roles: vec![],
        permissions: std::collections::HashMap::new(),
        permission_details: None,
    }
}

#[tokio::test]
async fn login_success_emits_authentication_complete() {
    let mut state = State::default();
    let user = make_user();
    let perms = make_permissions(user.id);

    let result: DomainUpdateResult =
        ferrex_player::domains::auth::update::update_auth(
            &mut state,
            auth_msgs::AuthMessage::LoginSuccess(user.clone(), perms.clone()),
        );

    // Expect both UserAuthenticated and AuthenticationComplete events
    let mut has_user_auth = false;
    let mut has_auth_complete = false;
    for ev in &result.events {
        match ev {
            ferrex_player::common::messages::CrossDomainEvent::UserAuthenticated(u, p) => {
                has_user_auth = true;
                assert_eq!(u.id, user.id);
                assert_eq!(p.user_id, perms.user_id);
            }
            ferrex_player::common::messages::CrossDomainEvent::AuthenticationComplete => {
                has_auth_complete = true;
            }
            _ => {}
        }
    }
    assert!(has_user_auth, "LoginSuccess should emit UserAuthenticated");
    assert!(
        has_auth_complete,
        "LoginSuccess should emit AuthenticationComplete"
    );
}

#[tokio::test]
async fn watch_state_loaded_does_not_emit_authentication_complete() {
    let mut state = State {
        is_authenticated: true,
        ..Default::default()
    };

    // Simulate already authenticated state so watch-state handler still runs
    state.domains.auth.state.is_authenticated = true;

    let result_ok: DomainUpdateResult =
        ferrex_player::domains::auth::update::update_auth(
            &mut state,
            auth_msgs::AuthMessage::WatchStatusLoaded(Ok(
                ferrex_core::player_prelude::UserWatchState::default(),
            )),
        );
    assert!(
        result_ok.events.is_empty(),
        "WatchStatusLoaded(Ok) should not emit events"
    );

    let result_err: DomainUpdateResult =
        ferrex_player::domains::auth::update::update_auth(
            &mut state,
            auth_msgs::AuthMessage::WatchStatusLoaded(
                Err("neterr".to_string()),
            ),
        );
    assert!(
        result_err.events.is_empty(),
        "WatchStatusLoaded(Err) should not emit events"
    );
}
