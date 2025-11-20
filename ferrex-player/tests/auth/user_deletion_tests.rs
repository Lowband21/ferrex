//! User deletion cascade tests
//!
//! Requirements from USER_MANAGEMENT_REQUIREMENTS.md:
//! - User deletion invalidates all sessions
//! - Trusted devices are removed
//! - Auto-login is disabled

use crate::auth::TestHelper;

#[tokio::test]
async fn user_deletion_invalidates_all_sessions() {
    // Requirement: Deleting a user should invalidate all their sessions
    let helper = TestHelper::new();

    // Create admin and regular user
    let admin_id = helper
        .auth_service
        .create_user("admin".to_string(), "admin_pass".to_string())
        .await
        .expect("Admin creation should succeed");

    let user_id = helper
        .auth_service
        .create_user("user".to_string(), "user_pass".to_string())
        .await
        .expect("User creation should succeed");

    // Create multiple sessions for the user
    let session1 = helper
        .auth_service
        .authenticate_with_device(user_id, "user_pass".to_string(), "device1".to_string())
        .await
        .expect("Authentication should succeed");

    let session2 = helper
        .auth_service
        .authenticate_with_device(user_id, "user_pass".to_string(), "device2".to_string())
        .await
        .expect("Authentication should succeed");

    // Verify sessions are valid
    assert!(helper.auth_service.is_session_valid(&session1).await);
    assert!(helper.auth_service.is_session_valid(&session2).await);

    // Admin deletes the user
    let admin_session = helper
        .auth_service
        .authenticate(admin_id, "admin_pass".to_string())
        .await
        .expect("Admin authentication should succeed");

    helper
        .auth_service
        .delete_user(user_id, admin_session)
        .await
        .expect("User deletion should succeed");

    // All user sessions should be invalid
    assert!(
        !helper.auth_service.is_session_valid(&session1).await,
        "Session1 should be invalid after user deletion"
    );
    assert!(
        !helper.auth_service.is_session_valid(&session2).await,
        "Session2 should be invalid after user deletion"
    );

    // User should not exist
    let user = helper.auth_service.get_user(user_id).await;
    assert!(user.is_none(), "User should not exist after deletion");
}

#[tokio::test]
async fn user_deletion_removes_trusted_devices() {
    let helper = TestHelper::new();

    // Create admin and user
    let admin_id = helper
        .auth_service
        .create_user("admin".to_string(), "admin_pass".to_string())
        .await
        .expect("Admin creation should succeed");

    let user_id = helper
        .auth_service
        .create_user("user".to_string(), "user_pass".to_string())
        .await
        .expect("User creation should succeed");

    // Trust some devices for the user
    let device1 = "device1".to_string();
    let device2 = "device2".to_string();

    helper
        .auth_service
        .trust_device(user_id, device1.clone())
        .await
        .unwrap();
    helper
        .auth_service
        .trust_device(user_id, device2.clone())
        .await
        .unwrap();

    // Verify devices are trusted
    assert!(
        helper
            .auth_service
            .is_device_trusted(user_id, &device1)
            .await
    );
    assert!(
        helper
            .auth_service
            .is_device_trusted(user_id, &device2)
            .await
    );

    // Admin deletes the user
    let admin_session = helper
        .auth_service
        .authenticate(admin_id, "admin_pass".to_string())
        .await
        .expect("Admin auth should succeed");

    helper
        .auth_service
        .delete_user(user_id, admin_session)
        .await
        .expect("User deletion should succeed");

    // Devices should no longer be trusted for the deleted user
    assert!(
        !helper
            .auth_service
            .is_device_trusted(user_id, &device1)
            .await
    );
    assert!(
        !helper
            .auth_service
            .is_device_trusted(user_id, &device2)
            .await
    );
}

#[tokio::test]
async fn only_admin_can_delete_users() {
    let helper = TestHelper::new();

    // Create admin and two regular users
    let _admin_id = helper
        .auth_service
        .create_user("admin".to_string(), "admin_pass".to_string())
        .await
        .expect("Admin creation should succeed");

    let user1_id = helper
        .auth_service
        .create_user("user1".to_string(), "pass1".to_string())
        .await
        .expect("User1 creation should succeed");

    let user2_id = helper
        .auth_service
        .create_user("user2".to_string(), "pass2".to_string())
        .await
        .expect("User2 creation should succeed");

    // User1 tries to delete user2 - should fail
    let user1_session = helper
        .auth_service
        .authenticate(user1_id, "pass1".to_string())
        .await
        .expect("User1 auth should succeed");

    let result = helper
        .auth_service
        .delete_user(user2_id, user1_session)
        .await;

    assert!(
        result.is_err(),
        "Non-admin should not be able to delete users"
    );

    match result.unwrap_err() {
        ferrex_player::domains::auth::AuthError::InsufficientPermissions => {}
        other => panic!("Expected InsufficientPermissions error, got: {:?}", other),
    }

    // User2 should still exist
    assert!(helper.auth_service.get_user(user2_id).await.is_some());
}
