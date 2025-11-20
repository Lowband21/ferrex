// PIN Authentication Requirements Tests
//
// Tests for PIN authentication rules from USER_MANAGEMENT_REQUIREMENTS.md:
// - Standard users can only use PIN if admin has authenticated in session
// - PIN setup requires admin session for standard users

use ferrex_player::domains::auth::service::AuthService;

#[tokio::test]
async fn standard_user_pin_requires_active_admin_session() {
    // Requirement: Standard users can only use PIN after admin has authenticated
    let auth = AuthService::new();

    // Create admin (first user)
    let admin_id = auth
        .create_user("admin".to_string(), "admin_pass".to_string())
        .await
        .expect("Admin creation should succeed");

    // Create standard user
    let user_id = auth
        .create_user("user".to_string(), "user_pass".to_string())
        .await
        .expect("User creation should succeed");

    // Admin authenticates to enable PIN setup
    let admin_session = auth
        .authenticate(admin_id, "admin_pass".to_string())
        .await
        .expect("Admin authentication should succeed");

    // Setup PIN for standard user (requires admin session)
    auth.setup_pin(user_id, "1234".to_string(), Some(admin_session.clone()))
        .await
        .expect("PIN setup should succeed with admin session");

    let device = "test_device".to_string();

    // TEST: Without admin session active, standard user CANNOT use PIN
    auth.clear_admin_session().await; // Clear the admin session

    let result = auth
        .authenticate_with_pin(user_id, "1234".to_string(), device.clone())
        .await;

    assert!(result.is_err(), "Standard user should NOT authenticate with PIN without admin session");
    match result.unwrap_err() {
        ferrex_player::domains::auth::AuthError::AdminSessionRequired => {},
        other => panic!("Expected AdminSessionRequired error, got: {:?}", other),
    }

    // TEST: With admin session active, standard user CAN use PIN
    let _admin_session = auth
        .authenticate(admin_id, "admin_pass".to_string())
        .await
        .expect("Admin authentication should succeed");

    // Mark admin session as active for PIN authentication
    auth.set_admin_session_active(admin_id).await;

    let user_session = auth
        .authenticate_with_pin(user_id, "1234".to_string(), device)
        .await
        .expect("Standard user should authenticate with PIN when admin session is active");

    assert!(!user_session.is_admin, "User session should not be admin");
}
