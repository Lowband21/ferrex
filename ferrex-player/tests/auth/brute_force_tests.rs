//! Brute force protection tests
//! 
//! Requirements from USER_MANAGEMENT_REQUIREMENTS.md:
//! - 5 failed attempts trigger lockout
//! - Lockout expires after timeout
//! - Rate limiting on failed attempts

use crate::auth::TestHelper;

#[tokio::test]
async fn account_locks_after_five_failed_attempts() {
    // Requirement: 5 failed attempts trigger lockout
    let helper = TestHelper::new();
    
    // Create a user
    let user_id = helper.auth_service
        .create_user("user".to_string(), "correct_password".to_string())
        .await
        .expect("User creation should succeed");
    
    // Try 4 wrong passwords - should not lock yet
    for i in 1..=4 {
        let result = helper.auth_service
            .authenticate(user_id, format!("wrong_{}", i))
            .await;
        assert!(result.is_err(), "Wrong password should fail");
        
        let is_locked = helper.auth_service.is_account_locked(user_id).await;
        assert!(!is_locked, "Account should not be locked after {} attempts", i);
    }
    
    // 5th wrong attempt should trigger lockout
    let result = helper.auth_service
        .authenticate(user_id, "wrong_5".to_string())
        .await;
    assert!(result.is_err(), "Wrong password should fail");
    
    let is_locked = helper.auth_service.is_account_locked(user_id).await;
    assert!(is_locked, "Account should be locked after 5 failed attempts");
    
    // Even correct password should fail when locked
    let result = helper.auth_service
        .authenticate(user_id, "correct_password".to_string())
        .await;
    
    assert!(result.is_err(), "Correct password should fail when account is locked");
    
    match result.unwrap_err() {
        ferrex_player::domains::auth::AuthError::AccountLocked { .. } => {},
        other => panic!("Expected AccountLocked error, got: {:?}", other),
    }
}

#[tokio::test]
async fn lockout_expires_after_timeout() {
    // Requirement: Lockout should expire after timeout period
    let helper = TestHelper::new();
    
    // Create and lock a user account
    let user_id = helper.auth_service
        .create_user("user".to_string(), "password".to_string())
        .await
        .expect("User creation should succeed");
    
    // Trigger lockout with 5 failed attempts
    for i in 1..=5 {
        let _ = helper.auth_service
            .authenticate(user_id, format!("wrong_{}", i))
            .await;
    }
    
    assert!(helper.auth_service.is_account_locked(user_id).await, "Account should be locked");
    
    // Advance time by lockout duration (15 minutes)
    helper.auth_service.advance_time(chrono::Duration::minutes(15)).await;
    
    // Account should be unlocked
    assert!(!helper.auth_service.is_account_locked(user_id).await, "Account should be unlocked after timeout");
    
    // Authentication should succeed now
    let result = helper.auth_service
        .authenticate(user_id, "password".to_string())
        .await;
    
    assert!(result.is_ok(), "Authentication should succeed after lockout expires");
}

#[tokio::test]
async fn successful_login_resets_failed_attempts() {
    let helper = TestHelper::new();
    
    let user_id = helper.auth_service
        .create_user("user".to_string(), "password".to_string())
        .await
        .expect("User creation should succeed");
    
    // Make 3 failed attempts
    for i in 1..=3 {
        let _ = helper.auth_service
            .authenticate(user_id, format!("wrong_{}", i))
            .await;
    }
    
    // Successful login
    let _ = helper.auth_service
        .authenticate(user_id, "password".to_string())
        .await
        .expect("Correct password should succeed");
    
    // Failed attempts should be reset, so 4 more attempts should not lock
    for i in 1..=4 {
        let _ = helper.auth_service
            .authenticate(user_id, format!("wrong_again_{}", i))
            .await;
        
        let is_locked = helper.auth_service.is_account_locked(user_id).await;
        assert!(!is_locked, "Account should not be locked after {} attempts after reset", i);
    }
    
    // 5th attempt should lock
    let _ = helper.auth_service
        .authenticate(user_id, "wrong_again_5".to_string())
        .await;
    
    assert!(helper.auth_service.is_account_locked(user_id).await, "Account should lock after 5 new attempts");
}