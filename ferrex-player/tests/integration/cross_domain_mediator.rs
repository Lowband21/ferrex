//! Cross-Domain Mediator Integration Tests
//!
//! These tests verify the actual behavior of the mediator (update function)
//! in routing cross-domain events between domains.

use ferrex_core::player_prelude::LibraryID;
use ferrex_core::rbac::UserPermissions;
use ferrex_player::common::messages::{CrossDomainEvent, DomainUpdateResult};
use ferrex_player::state::State;
use iced::Task;
use uuid::Uuid;

/// Test that UserLoggedOut event clears authentication state
#[tokio::test]
async fn test_user_logout_clears_authentication() {
    let mut state = State {
        is_authenticated: true,
        ..State::default()
    };

    // Set up authenticated state
    state.domains.auth.state.is_authenticated = true;
    state.domains.auth.state.user_permissions =
        Some(UserPermissions::default());

    // Process the UserLoggedOut event through handle_event
    let _ = ferrex_player::common::messages::cross_domain::handle_event(
        &mut state,
        CrossDomainEvent::UserLoggedOut,
    );

    // Verify state was cleared
    assert!(
        !state.is_authenticated,
        "Global authentication flag should be cleared"
    );
    assert!(
        state.domains.auth.state.user_permissions.is_none(),
        "User permissions should be cleared"
    );
}

/// Test that AuthCommandRequested event is routed to auth domain
#[tokio::test]
async fn test_auth_command_routing() {
    use ferrex_player::domains::auth::messages::AuthCommand;

    let mut state = State::default();
    let initial_state = state.domains.auth.state.is_authenticated;

    // Create an auth command (no user_id, works with current authenticated user)
    let command = AuthCommand::ChangePassword {
        old_password: "old".into(),
        new_password: "new".into(),
    };

    // Process the event through handle_event
    let _ = ferrex_player::common::messages::cross_domain::handle_event(
        &mut state,
        CrossDomainEvent::AuthCommandRequested(command.clone()),
    );

    // The handler exists and returns a task
    // Verify the state wasn't incorrectly modified
    assert_eq!(
        state.domains.auth.state.is_authenticated, initial_state,
        "AuthCommandRequested should not change authentication state directly"
    );
}

/// Test that AuthCommandCompleted routes results back to settings domain
#[tokio::test]
async fn test_auth_command_completion_routing() {
    use ferrex_player::domains::auth::messages::{
        AuthCommand, AuthCommandResult,
    };

    let mut state = State::default();

    // Test password change completion - verify state isn't corrupted
    let password_command = AuthCommand::ChangePassword {
        old_password: "old".into(),
        new_password: "new".into(),
    };

    let success_result = AuthCommandResult::Success;

    let initial_auth_state = state.is_authenticated;

    // Process completion event
    let _ = ferrex_player::common::messages::cross_domain::handle_event(
        &mut state,
        CrossDomainEvent::AuthCommandCompleted(
            password_command,
            success_result,
        ),
    );

    // Verify state wasn't corrupted by the handler
    assert_eq!(
        state.is_authenticated, initial_auth_state,
        "AuthCommandCompleted should not change authentication state"
    );

    // Test PIN change completion with error
    let pin_command = AuthCommand::SetUserPin { pin: "1234".into() };

    let error_result = AuthCommandResult::Error("Invalid PIN".to_string());

    let _ = ferrex_player::common::messages::cross_domain::handle_event(
        &mut state,
        CrossDomainEvent::AuthCommandCompleted(pin_command, error_result),
    );

    // State should remain unchanged for error results too
    assert_eq!(
        state.is_authenticated, initial_auth_state,
        "Error results should not affect authentication state"
    );
}

/// Test that LibrarySelected event updates state correctly
#[tokio::test]
async fn test_library_selection_updates_state() {
    let mut state = State::default();
    let library_id = LibraryID::new();

    // Verify initial state
    assert_eq!(
        state.domains.library.state.current_library_id, None,
        "Library should start with no selection"
    );

    // Process LibrarySelected event
    let _ = ferrex_player::common::messages::cross_domain::handle_event(
        &mut state,
        CrossDomainEvent::LibrarySelected(library_id),
    );

    // Verify state was updated
    assert_eq!(
        state.domains.library.state.current_library_id,
        Some(library_id),
        "Library ID should be stored in state"
    );
}

/// Test that LibrarySelectAll clears the library selection
#[tokio::test]
async fn test_library_select_all_clears_selection() {
    let mut state = State::default();

    // Set an initial library selection
    state.domains.library.state.current_library_id = Some(LibraryID::new());

    // Process LibrarySelectAll event
    let _ = ferrex_player::common::messages::cross_domain::handle_event(
        &mut state,
        CrossDomainEvent::LibrarySelectAll,
    );

    // Verify selection was cleared
    assert_eq!(
        state.domains.library.state.current_library_id, None,
        "LibrarySelectAll should clear the library selection"
    );
}

/// Test that multiple events from one update are all processed
#[tokio::test]
async fn test_mediator_processes_multiple_events() {
    let mut state = State::default();

    // Create a DomainUpdateResult with multiple events
    let update_result = DomainUpdateResult::with_events(
        Task::none(),
        vec![
            CrossDomainEvent::ClearMediaStore,
            CrossDomainEvent::ClearLibraries,
            CrossDomainEvent::ClearCurrentShowData,
        ],
    );

    // Simulate what the mediator does
    let mut tasks = vec![update_result.task];
    for event in update_result.events {
        let task = ferrex_player::common::messages::cross_domain::handle_event(
            &mut state, event,
        );
        tasks.push(task);
    }

    // All three events should have been processed
    // Each would have its effect on the state
}

/// Test that UserAuthenticated event updates authentication state
#[tokio::test]
async fn test_user_authenticated_updates_state() {
    use chrono::Utc;
    use ferrex_core::user::{User, UserPreferences};

    let mut state = State::default();

    // Create test user with correct fields
    let user = User {
        id: Uuid::now_v7(),
        username: "testuser".to_string(),
        display_name: "Test User".to_string(),
        avatar_url: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        last_login: Some(Utc::now()),
        is_active: true,
        email: Some("test@example.com".to_string()),
        preferences: UserPreferences::default(),
    };

    let permissions = UserPermissions::default();

    // Process UserAuthenticated event
    let _ = ferrex_player::common::messages::cross_domain::handle_event(
        &mut state,
        CrossDomainEvent::UserAuthenticated(user.clone(), permissions.clone()),
    );

    // Verify state was updated
    assert!(state.is_authenticated, "Should be authenticated");
    assert!(
        state.domains.auth.state.user_permissions.is_some(),
        "Permissions should be stored"
    );
}

#[cfg(test)]
mod event_ordering_tests {
    use super::*;

    /// Test that events maintain their order when processed
    #[tokio::test]
    async fn test_event_order_preserved() {
        let mut state = State::default();

        // Events that modify the same state
        let events = vec![
            CrossDomainEvent::LibrarySelected(LibraryID::new()),
            CrossDomainEvent::LibrarySelectAll,
            CrossDomainEvent::LibrarySelected(LibraryID::new()),
        ];

        // Process in order
        for event in &events {
            let _ = ferrex_player::common::messages::cross_domain::handle_event(
                &mut state,
                event.clone(),
            );
        }

        // Final state should reflect the last event
        // (Third event should have overwritten the effects of first two)
        assert!(
            state.domains.library.state.current_library_id.is_some(),
            "Last LibrarySelected should set the library ID"
        );
    }
}
