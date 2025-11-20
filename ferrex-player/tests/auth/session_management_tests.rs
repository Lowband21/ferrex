// Session management tests
//
// Requirements from USER_MANAGEMENT_REQUIREMENTS.md:
// - Users can have multiple active devices (no fixed limit)
// - Session invalidation on logout
// - Device-based sessions with no fixed expiration

use ferrex_player::domains::auth::service::AuthService;

#[tokio::test]
async fn user_can_have_multiple_active_sessions() {
    // Requirement: Users can have multiple active devices
    let auth = AuthService::new();

    // Create a user
    let user_id = auth
        .create_user("user".to_string(), "password".to_string())
        .await
        .expect("User creation should succeed");

    // Create many sessions from different devices
    let mut sessions = Vec::new();
    for i in 0..5 {
        let device = format!("device_{}", i);
        let session = auth
            .authenticate_with_password(user_id, "password".to_string(), device.clone())
            .await
            .expect("Authentication should succeed");
        sessions.push(session);
    }

    // All sessions should be valid (no fixed limit)
    for (i, session) in sessions.iter().enumerate() {
        assert!(
            auth.is_session_valid(&session).await,
            "Session {} should be valid - no fixed session limit",
            i
        );
    }

    // Count active sessions - should be all 5
    let active_count = auth.count_active_sessions(user_id).await;
    assert_eq!(active_count, 5, "Should have all 5 active sessions");
}

#[tokio::test]
async fn session_invalidated_on_logout() {
    let auth = AuthService::new();

    let user_id = auth
        .create_user("user".to_string(), "password".to_string())
        .await
        .expect("User creation should succeed");

    let session = auth
        .authenticate(user_id, "password".to_string())
        .await
        .expect("Authentication should succeed");

    // Session should be valid initially
    assert!(auth.is_session_valid(&session).await);

    // Logout
    auth.logout(session.clone())
        .await
        .expect("Logout should succeed");

    // Session should be invalid after logout
    assert!(!auth.is_session_valid(&session).await);
}

#[tokio::test]
async fn sessions_are_device_specific() {
    let auth = AuthService::new();

    let user_id = auth
        .create_user("user".to_string(), "password".to_string())
        .await
        .expect("User creation should succeed");

    // Create sessions on different devices
    let session1 = auth
        .authenticate_with_password(user_id, "password".to_string(), "device1".to_string())
        .await
        .expect("Auth should succeed");

    let session2 = auth
        .authenticate_with_password(user_id, "password".to_string(), "device2".to_string())
        .await
        .expect("Auth should succeed");

    // Both sessions should be valid
    assert!(auth.is_session_valid(&session1).await);
    assert!(auth.is_session_valid(&session2).await);

    // Sessions should be associated with their devices
    assert_eq!(
        auth.get_session_device(&session1).await,
        Some("device1".to_string())
    );
    assert_eq!(
        auth.get_session_device(&session2).await,
        Some("device2".to_string())
    );
}
