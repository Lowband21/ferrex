use std::collections::HashMap;

use chrono::Utc;
use ferrex_core::domain::users::user::UserPreferences;
use ferrex_core::player_prelude::{User, UserPermissions};
use ferrex_player::common::messages::{CrossDomainEvent, DomainMessage};
use ferrex_player::domains::library::messages::LibraryMessage;
use ferrex_player::state::State;
use futures::StreamExt;

#[tokio::test]
async fn user_authenticated_triggers_post_auth_library_initialization() {
    let mut state = State::default();

    let user_id = uuid::Uuid::now_v7();
    let user = User {
        id: user_id,
        username: "test-user".to_string(),
        display_name: "Test User".to_string(),
        avatar_url: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        last_login: Some(Utc::now()),
        is_active: true,
        email: None,
        preferences: UserPreferences::default(),
    };

    let permissions = UserPermissions {
        user_id,
        roles: Vec::new(),
        permissions: HashMap::new(),
        permission_details: None,
    };

    let task = ferrex_player::common::messages::cross_domain::handle_event(
        &mut state,
        CrossDomainEvent::UserAuthenticated(user.clone(), permissions.clone()),
    );

    assert!(
        state.is_authenticated,
        "top-level state should reflect authentication"
    );
    assert!(
        state.domains.auth.state.is_authenticated,
        "auth domain state should reflect authentication"
    );
    assert!(
        state.domains.auth.state.user_permissions.is_some(),
        "permissions should be stored during authentication"
    );

    let mut outputs = Vec::new();
    let Some(mut stream) = iced_runtime::task::into_stream(task) else {
        panic!("expected UserAuthenticated to return a non-empty Task");
    };

    while let Some(action) = stream.next().await {
        if let iced_runtime::Action::Output(message) = action {
            outputs.push(message);
        }
    }

    let mut saw_load_libraries = false;
    let mut saw_fetch_active_scans = false;

    for message in outputs {
        match message {
            DomainMessage::Library(LibraryMessage::LoadLibraries) => {
                saw_load_libraries = true;
            }
            DomainMessage::Library(LibraryMessage::FetchActiveScans) => {
                saw_fetch_active_scans = true;
            }
            _ => {}
        }
    }

    assert!(
        saw_load_libraries,
        "post-auth initialization should trigger Library::LoadLibraries"
    );
    assert!(
        saw_fetch_active_scans,
        "post-auth initialization should trigger Library::FetchActiveScans"
    );
}
