// Rate Limiting Tests - Following TDD Red-Green-Refactor
//
// From USER_MANAGEMENT_REQUIREMENTS.md:
// "### PIN Security
// - Rate limiting on failed attempts
// - Temporary lockout after repeated failures
// - Falls back to password requirement"

use ferrex_player::domains::auth::service::AuthService;

#[tokio::test]
async fn rate_limiting_on_failed_pin_attempts() {
    // Requirement: Rate limiting and lockout on failed PIN attempts
    let auth = AuthService::new();
    
    // Create user
    let user_id = auth
        .create_user("user".to_string(), "password".to_string())
        .await
        .expect("User creation should succeed");
    
    // Admin sets up PIN (first user is admin)
    let admin_id = auth
        .create_user("admin".to_string(), "admin_pass".to_string())
        .await
        .expect("Admin creation should succeed");
    
    // Actually, the first user created is admin, so let's correct this
    // User is the first, so they are admin. Create another user for testing
    let regular_user_id = auth
        .create_user("regular".to_string(), "regular_pass".to_string())
        .await
        .expect("Regular user creation should succeed");
    
    // Setup PIN for regular user (admin session required)
    let admin_session = auth
        .authenticate(user_id, "password".to_string())
        .await
        .expect("Admin auth should succeed");
    
    auth.setup_pin(regular_user_id, "1234".to_string(), Some(admin_session.clone()))
        .await
        .expect("PIN setup should succeed");
    
    // Make admin session active for PIN auth
    auth.set_admin_session_active(user_id).await;
    
    let device = "test_device".to_string();
    
    // Try wrong PIN 4 times (threshold before lockout)
    for i in 1..=4 {
        let result = auth
            .authenticate_with_pin(regular_user_id, format!("000{}", i), device.clone())
            .await;
        
        assert!(result.is_err(), "Wrong PIN should fail");
        
        // Should not be locked yet
        assert!(
            !auth.is_account_locked(regular_user_id).await,
            "Account should not be locked after {} attempts", i
        );
    }
    
    // 5th wrong attempt should trigger temporary lockout
    let result = auth
        .authenticate_with_pin(regular_user_id, "0005".to_string(), device.clone())
        .await;
    
    assert!(result.is_err(), "Wrong PIN should fail");
    
    // Account should be temporarily locked
    assert!(
        auth.is_account_locked(regular_user_id).await,
        "Account should be locked after 5 failed PIN attempts"
    );
    
    // Even correct PIN should fail during lockout
    let result = auth
        .authenticate_with_pin(regular_user_id, "1234".to_string(), device.clone())
        .await;
    
    assert!(result.is_err(), "PIN auth should fail during lockout");
    match result.unwrap_err() {
        ferrex_player::domains::auth::AuthError::AccountLocked => {},
        other => panic!("Expected AccountLocked error, got: {:?}", other),
    }
    
    // But password should still work (fallback requirement)
    let session = auth
        .authenticate_with_password(regular_user_id, "regular_pass".to_string(), device)
        .await
        .expect("Password auth should work even during PIN lockout");
    
    assert!(!session.is_admin, "Regular user session should not be admin");
}