//! UI flow tests for first-run transition when setup status fails

use ferrex_player::app::bootstrap::{AppConfig, base_state};
use ferrex_player::domains::auth::messages as auth_msgs;
use ferrex_player::domains::auth::types::AuthenticationFlow;

#[tokio::test]
async fn empty_users_plus_setup_failure_transitions_to_first_run() {
    // Build a minimal state; we won't execute network tasks in this test.
    let config = AppConfig::new("http://localhost:3000");
    let mut state = base_state(&config);

    // Simulate entering the load-users path
    let _ = ferrex_player::domains::auth::update::update_auth(
        &mut state,
        auth_msgs::AuthMessage::LoadUsers,
    );

    // Sanity: now in loading state
    assert!(matches!(
        state.domains.auth.state.auth_flow,
        AuthenticationFlow::LoadingUsers
    ));

    // Simulate the setup-status failure mapping to `needs_setup = true`
    let setup_status = ferrex_player::infra::api_client::SetupStatus {
        needs_setup: true,
        has_admin: false,
        requires_setup_token: false,
        user_count: 0,
        library_count: 0,
    };

    let _ = ferrex_player::domains::auth::update::update_auth(
        &mut state,
        auth_msgs::AuthMessage::SetupStatusChecked(setup_status),
    );

    // Expect to transition to FirstRunSetup UI
    assert!(matches!(
        state.domains.auth.state.auth_flow,
        AuthenticationFlow::FirstRunSetup { .. }
    ));
}
