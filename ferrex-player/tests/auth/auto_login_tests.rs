//! Auto-login tests
//!
//! Requirements:
//! - Auto-login is device-specific
//! - Manual logout disables auto-login
//! - Another user login disables auto-login

use crate::auth::TestHelper;

#[tokio::test]
async fn auto_login_is_device_specific() {
    // Requirement: Auto-login should be device-specific
    let helper = TestHelper::new();

    // Create a user
    let user_id = helper
        .auth_service
        .create_user("user".to_string(), "password".to_string())
        .await
        .expect("User creation should succeed");

    let device1 = "device1".to_string();
    let device2 = "device2".to_string();

    // Authenticate and enable auto-login on device1
    let session1 = helper
        .auth_service
        .authenticate_with_device(
            user_id,
            "password".to_string(),
            device1.clone(),
        )
        .await
        .expect("Authentication should succeed");

    helper
        .auth_service
        .enable_auto_login(user_id, device1.clone())
        .await
        .expect("Auto-login enable should succeed");

    // Normal logout (app closure) - auto-login should remain enabled
    helper
        .auth_service
        .logout(session1)
        .await
        .expect("Logout should succeed");

    // Auto-login should work on device1
    let result = helper
        .auth_service
        .attempt_auto_login(device1.clone())
        .await;
    assert!(result.is_ok(), "Auto-login should work on device1");

    // Auto-login should NOT work on device2
    let result = helper
        .auth_service
        .attempt_auto_login(device2.clone())
        .await;
    assert!(result.is_err(), "Auto-login should NOT work on device2");

    match result.unwrap_err() {
        ferrex_player::domains::auth::AuthError::AutoLoginNotEnabled => {}
        other => panic!("Expected AutoLoginNotEnabled error, got: {:?}", other),
    }
}

#[tokio::test]
async fn manual_logout_disables_auto_login() {
    // Requirement: Explicit manual logout should disable auto-login
    let helper = TestHelper::new();

    // Create user and enable auto-login
    let user_id = helper
        .auth_service
        .create_user("user".to_string(), "password".to_string())
        .await
        .expect("User creation should succeed");

    let device = "device".to_string();

    let session = helper
        .auth_service
        .authenticate_with_device(
            user_id,
            "password".to_string(),
            device.clone(),
        )
        .await
        .expect("Authentication should succeed");

    helper
        .auth_service
        .enable_auto_login(user_id, device.clone())
        .await
        .expect("Auto-login enable should succeed");

    // Explicit manual logout (user chooses to logout)
    helper
        .auth_service
        .logout_manual(session)
        .await
        .expect("Manual logout should succeed");

    // Auto-login should be disabled after manual logout
    let result = helper.auth_service.attempt_auto_login(device.clone()).await;

    assert!(
        result.is_err(),
        "Auto-login should be disabled after explicit manual logout"
    );
}

#[tokio::test]
async fn another_user_login_disables_auto_login() {
    // Requirement: Another user logging in on same device disables auto-login
    let helper = TestHelper::new();

    // Create two users
    let user1_id = helper
        .auth_service
        .create_user("user1".to_string(), "password1".to_string())
        .await
        .expect("User1 creation should succeed");

    let user2_id = helper
        .auth_service
        .create_user("user2".to_string(), "password2".to_string())
        .await
        .expect("User2 creation should succeed");

    let device = "shared_device".to_string();

    // User1 enables auto-login
    let session1 = helper
        .auth_service
        .authenticate_with_device(
            user1_id,
            "password1".to_string(),
            device.clone(),
        )
        .await
        .expect("User1 auth should succeed");

    helper
        .auth_service
        .enable_auto_login(user1_id, device.clone())
        .await
        .expect("Auto-login enable should succeed");

    // Normal logout (app closure) for user1
    helper.auth_service.logout(session1).await.unwrap();

    // Verify auto-login still works for user1 after normal logout
    assert!(
        helper
            .auth_service
            .attempt_auto_login(device.clone())
            .await
            .is_ok(),
        "Auto-login should still work after normal logout"
    );

    // User2 logs in on same device
    let _session2 = helper
        .auth_service
        .authenticate_with_device(
            user2_id,
            "password2".to_string(),
            device.clone(),
        )
        .await
        .expect("User2 auth should succeed");

    // User1's auto-login should be disabled
    let is_enabled = helper
        .auth_service
        .is_auto_login_enabled(user1_id, device.clone())
        .await;

    assert!(
        !is_enabled,
        "User1's auto-login should be disabled after user2 logs in"
    );
}
