//! Library load state machine tests
//!
//! These tests validate idempotent loading, auth gating, retry on failure,
//! session-scoped success handling, and proper resets on clear/logout events.

use ferrex_player::domains::library::LibrariesLoadState;
use ferrex_player::domains::library::messages::LibraryMessage;
use ferrex_player::state::State;

// Import auth types to fabricate an authenticated session
use chrono::Utc;
use ferrex_core::domain::users::user::{User, UserPreferences};
use ferrex_player::domains::auth::types::{
    AuthenticationFlow, AuthenticationMode,
};
use uuid::Uuid;

// Handler we want to exercise for load-start transitions
use ferrex_player::domains::library::update::update_library;
// Success/failure completion handler (we'll use the failure path)
use ferrex_player::domains::library::update_handlers::library_loaded::handle_libraries_loaded;

fn make_user(id: Uuid) -> User {
    User {
        id,
        username: format!("user-{id}"),
        display_name: "Test User".to_string(),
        avatar_url: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        last_login: Some(Utc::now()),
        is_active: true,
        email: None,
        preferences: UserPreferences::default(),
    }
}

#[tokio::test]
async fn load_is_gated_when_not_authenticated() {
    let mut state = State::default();
    // Ensure unauthenticated
    state.is_authenticated = false;
    match &state.domains.library.state.load_state {
        LibrariesLoadState::NotStarted => {}
        _ => panic!("expected NotStarted"),
    }

    // Attempt to load libraries while unauthenticated
    let _ = update_library(&mut state, LibraryMessage::LoadLibraries);

    // State must remain NotStarted
    match &state.domains.library.state.load_state {
        LibrariesLoadState::NotStarted => {}
        other => panic!("unexpected load_state after gated load: {:?}", other),
    }
}

#[tokio::test]
async fn start_load_transitions_to_in_progress_when_authenticated() {
    let mut state = State::default();
    state.is_authenticated = true;

    // Trigger loading
    let _ = update_library(&mut state, LibraryMessage::LoadLibraries);

    match &state.domains.library.state.load_state {
        LibrariesLoadState::InProgress => {}
        other => panic!("expected InProgress, got {:?}", other),
    }
}

#[tokio::test]
async fn duplicate_load_during_in_progress_is_idempotent() {
    let mut state = State::default();
    state.is_authenticated = true;

    // First load → InProgress
    let _ = update_library(&mut state, LibraryMessage::LoadLibraries);
    // Duplicate load while in progress
    let _ = update_library(&mut state, LibraryMessage::LoadLibraries);

    match &state.domains.library.state.load_state {
        LibrariesLoadState::InProgress => {}
        other => panic!("duplicate should be ignored; got {:?}", other),
    }
}

#[tokio::test]
async fn failure_transitions_to_failed_and_allows_retry() {
    let mut state = State::default();
    state.is_authenticated = true;

    // Begin loading
    let _ = update_library(&mut state, LibraryMessage::LoadLibraries);
    // Simulate error result
    let _ =
        handle_libraries_loaded(&mut state, Err("network error".to_string()));

    match &state.domains.library.state.load_state {
        LibrariesLoadState::Failed { last_error } => {
            assert!(
                last_error.contains("network error"),
                "error should be captured"
            );
        }
        other => panic!("expected Failed, got {:?}", other),
    }

    // Retry after failure → should go to InProgress again
    let _ = update_library(&mut state, LibraryMessage::LoadLibraries);
    match &state.domains.library.state.load_state {
        LibrariesLoadState::InProgress => {}
        other => panic!("retry should set InProgress; got {:?}", other),
    }
}

#[tokio::test]
async fn succeeded_same_session_ignores_duplicate_load() {
    let mut state = State::default();
    state.is_authenticated = true;

    let user_id = Uuid::now_v7();
    let user = make_user(user_id);
    state.domains.auth.state.auth_flow = AuthenticationFlow::Authenticated {
        user: user.clone(),
        mode: AuthenticationMode::Online,
    };

    // Simulate a previous successful load for this session
    state.domains.library.state.load_state = LibrariesLoadState::Succeeded {
        user_id: Some(user_id),
        server_url: state.server_url.clone(),
    };

    // Duplicate load should be ignored
    let _ = update_library(&mut state, LibraryMessage::LoadLibraries);

    match &state.domains.library.state.load_state {
        LibrariesLoadState::Succeeded {
            user_id: uid,
            server_url,
        } => {
            assert_eq!(uid, &Some(user_id));
            assert_eq!(server_url, &state.server_url);
        }
        other => panic!("state should remain Succeeded; got {:?}", other),
    }
}

#[tokio::test]
async fn succeeded_different_server_triggers_reload() {
    let mut state = State::default();
    state.is_authenticated = true;

    let user_id = Uuid::now_v7();
    let user = make_user(user_id);
    state.domains.auth.state.auth_flow = AuthenticationFlow::Authenticated {
        user: user.clone(),
        mode: AuthenticationMode::Online,
    };

    // Mark success on original server
    state.domains.library.state.load_state = LibrariesLoadState::Succeeded {
        user_id: Some(user_id),
        server_url: state.server_url.clone(),
    };

    // Change server URL
    state.server_url = "http://localhost:3999".to_string();

    // Now a load should trigger reload → InProgress
    let _ = update_library(&mut state, LibraryMessage::LoadLibraries);

    match &state.domains.library.state.load_state {
        LibrariesLoadState::InProgress => {}
        other => panic!("server change should trigger reload; got {:?}", other),
    }
}

#[tokio::test]
async fn succeeded_different_user_triggers_reload() {
    let mut state = State::default();
    state.is_authenticated = true;

    // Original user/session
    let user_a = Uuid::now_v7();
    let user = make_user(user_a);
    state.domains.auth.state.auth_flow = AuthenticationFlow::Authenticated {
        user: user.clone(),
        mode: AuthenticationMode::Online,
    };
    state.domains.library.state.load_state = LibrariesLoadState::Succeeded {
        user_id: Some(user_a),
        server_url: state.server_url.clone(),
    };

    // Switch to a different authenticated user
    let user_b = Uuid::now_v7();
    let user2 = make_user(user_b);
    state.domains.auth.state.auth_flow = AuthenticationFlow::Authenticated {
        user: user2,
        mode: AuthenticationMode::Online,
    };

    let _ = update_library(&mut state, LibraryMessage::LoadLibraries);
    match &state.domains.library.state.load_state {
        LibrariesLoadState::InProgress => {}
        other => panic!("user change should trigger reload; got {:?}", other),
    }
}

#[tokio::test]
async fn clear_events_reset_to_not_started() {
    use ferrex_player::common::messages::CrossDomainEvent;

    let mut state = State::default();
    state.domains.library.state.load_state = LibrariesLoadState::Failed {
        last_error: "x".to_string(),
    };

    // ClearLibraries event
    let _ = state
        .domains
        .library
        .handle_event(&CrossDomainEvent::ClearLibraries);
    match &state.domains.library.state.load_state {
        LibrariesLoadState::NotStarted => {}
        other => panic!("ClearLibraries should reset; got {:?}", other),
    }

    // Move to Succeeded and then DatabaseCleared
    state.domains.library.state.load_state = LibrariesLoadState::Succeeded {
        user_id: None,
        server_url: state.server_url.clone(),
    };
    let _ = state
        .domains
        .library
        .handle_event(&CrossDomainEvent::DatabaseCleared);
    match &state.domains.library.state.load_state {
        LibrariesLoadState::NotStarted => {}
        other => panic!("DatabaseCleared should reset; got {:?}", other),
    }
}
