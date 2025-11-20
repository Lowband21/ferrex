// Device-Based Authentication Persistence Tests
//
// Tests that authentication persistence is based on device trust (30 days)
// not short-lived JWT tokens

use ferrex_player::domains::auth::service::AuthService;
use uuid::Uuid;

#[tokio::test]
async fn device_trust_enables_30_day_persistence() {
    let auth = AuthService::new();
    
    // Create a user
    let user_id = auth.create_user("user".to_string(), "password".to_string())
        .await
        .expect("User creation should succeed");
    
    let device_id = "my-device".to_string();
    
    // Authenticate with device
    let session = auth.authenticate_with_device(user_id, "password".to_string(), device_id.clone())
        .await
        .expect("Authentication should succeed");
    
    // Trust the device (enables 30-day persistence)
    auth.trust_device(user_id, device_id.clone())
        .await
        .expect("Device trust should succeed");
    
    // Enable auto-login for this device
    auth.enable_auto_login(user_id, device_id.clone())
        .await
        .expect("Auto-login enable should succeed");
    
    // Simulate app closure - logout the session
    auth.logout(session).await.expect("Logout should succeed");
    
    // Device should still be trusted after logout
    assert!(auth.is_device_trusted(user_id, &device_id).await,
            "Device should remain trusted after normal logout");
    
    // Auto-login should still work (device-based, not token-based)
    let new_session = auth.attempt_auto_login(device_id.clone())
        .await
        .expect("Auto-login should work on trusted device");
    
    assert_eq!(new_session.user_id, user_id, "Should login as same user");
}

#[tokio::test]
async fn device_trust_persists_for_30_days() {
    let auth = AuthService::new();
    
    // Create a user
    let user_id = auth.create_user("user".to_string(), "password".to_string())
        .await
        .expect("User creation should succeed");
    
    let device_id = "long-term-device".to_string();
    
    // Authenticate and trust device
    let _session = auth.authenticate_with_device(user_id, "password".to_string(), device_id.clone())
        .await
        .expect("Authentication should succeed");
    
    auth.trust_device(user_id, device_id.clone())
        .await
        .expect("Device trust should succeed");
    
    auth.enable_auto_login(user_id, device_id.clone())
        .await
        .expect("Auto-login enable should succeed");
    
    // Advance time by 29 days - should still be trusted
    auth.advance_time(chrono::Duration::days(29)).await;
    
    assert!(auth.is_device_trusted(user_id, &device_id).await,
            "Device should still be trusted after 29 days");
    
    let session = auth.attempt_auto_login(device_id.clone())
        .await
        .expect("Auto-login should work after 29 days");
    
    assert_eq!(session.user_id, user_id, "Should login as same user");
    
    // Advance time by 2 more days (total 31 days) - should expire
    auth.advance_time(chrono::Duration::days(2)).await;
    
    assert!(!auth.is_device_trusted(user_id, &device_id).await,
            "Device trust should expire after 30 days");
    
    let result = auth.attempt_auto_login(device_id.clone()).await;
    assert!(result.is_err(), "Auto-login should fail after device trust expires");
}

#[tokio::test]
async fn jwt_token_expiry_should_not_affect_device_trust() {
    // This test demonstrates that JWT token expiry (1 hour) should not
    // affect device-based authentication persistence
    
    let auth = AuthService::new();
    
    // Create a user
    let user_id = auth.create_user("user".to_string(), "password".to_string())
        .await
        .expect("User creation should succeed");
    
    let device_id = "persistent-device".to_string();
    
    // Authenticate with device
    let initial_session = auth.authenticate_with_device(
        user_id, 
        "password".to_string(), 
        device_id.clone()
    ).await.expect("Authentication should succeed");
    
    // Trust device and enable auto-login
    auth.trust_device(user_id, device_id.clone())
        .await
        .expect("Device trust should succeed");
    
    auth.enable_auto_login(user_id, device_id.clone())
        .await
        .expect("Auto-login enable should succeed");
    
    // Advance time by 2 hours (JWT token would be expired)
    auth.advance_time(chrono::Duration::hours(2)).await;
    
    // The JWT token in initial_session is now "expired" but device is still trusted
    assert!(auth.is_device_trusted(user_id, &device_id).await,
            "Device should still be trusted after 2 hours");
    
    // Auto-login should create a NEW session with a fresh token
    let new_session = auth.attempt_auto_login(device_id.clone())
        .await
        .expect("Auto-login should work even after JWT expiry");
    
    assert_eq!(new_session.user_id, user_id, "Should login as same user");
    assert_ne!(new_session.token, initial_session.token, "Should have new session token");
}

#[tokio::test]
async fn typical_user_experience_with_device_trust() {
    // Simulates a typical user experience:
    // 1. User logs in on their device
    // 2. Trusts device and enables auto-login
    // 3. Uses app throughout the day (closing/reopening)
    // 4. Still auto-logs in days later
    
    let auth = AuthService::new();
    
    // User sets up their account
    let user_id = auth.create_user("john".to_string(), "secure_password".to_string())
        .await
        .expect("User creation should succeed");
    
    let device_id = "johns-phone".to_string();
    
    // Initial login
    println!("Day 1: Initial login");
    let session1 = auth.authenticate_with_device(
        user_id,
        "secure_password".to_string(),
        device_id.clone()
    ).await.expect("Initial login should succeed");
    
    // User decides to trust this device and enable auto-login
    auth.trust_device(user_id, device_id.clone())
        .await
        .expect("Device trust should succeed");
    
    auth.enable_auto_login(user_id, device_id.clone())
        .await
        .expect("Auto-login enable should succeed");
    
    // User closes app after watching a movie
    auth.logout(session1).await.expect("Logout should succeed");
    
    // Later that day - app reopens
    auth.advance_time(chrono::Duration::hours(4)).await;
    println!("Day 1 (4 hours later): App reopened");
    
    let session2 = auth.attempt_auto_login(device_id.clone())
        .await
        .expect("Auto-login should work same day");
    
    // User closes app again
    auth.logout(session2).await.expect("Logout should succeed");
    
    // Next day
    auth.advance_time(chrono::Duration::days(1)).await;
    println!("Day 2: App opened");
    
    let session3 = auth.attempt_auto_login(device_id.clone())
        .await
        .expect("Auto-login should work next day");
    
    auth.logout(session3).await.expect("Logout should succeed");
    
    // A week later
    auth.advance_time(chrono::Duration::days(7)).await;
    println!("Day 9: App opened after a week");
    
    let session4 = auth.attempt_auto_login(device_id.clone())
        .await
        .expect("Auto-login should work after a week");
    
    auth.logout(session4).await.expect("Logout should succeed");
    
    // Three weeks later (total ~29 days)
    auth.advance_time(chrono::Duration::days(20)).await;
    println!("Day 29: App opened after three weeks");
    
    let session5 = auth.attempt_auto_login(device_id.clone())
        .await
        .expect("Auto-login should still work at day 29");
    
    assert_eq!(session5.user_id, user_id, "Should still be same user");
    
    println!("✓ User had seamless auto-login experience for 29 days");
}

#[tokio::test]
async fn manual_logout_vs_app_closure() {
    // Tests the difference between manual logout (disables auto-login)
    // and app closure (keeps auto-login enabled)
    
    let auth = AuthService::new();
    
    let user_id = auth.create_user("user".to_string(), "password".to_string())
        .await
        .expect("User creation should succeed");
    
    let device_id = "test-device".to_string();
    
    // Setup auto-login
    let session = auth.authenticate_with_device(user_id, "password".to_string(), device_id.clone())
        .await
        .expect("Auth should succeed");
    
    auth.trust_device(user_id, device_id.clone())
        .await
        .expect("Device trust should succeed");
    
    auth.enable_auto_login(user_id, device_id.clone())
        .await
        .expect("Auto-login enable should succeed");
    
    // Test 1: Normal logout (app closure) - auto-login remains enabled
    auth.logout(session.clone()).await.expect("Logout should succeed");
    
    assert!(auth.is_auto_login_enabled(user_id, device_id.clone()).await,
            "Auto-login should remain enabled after normal logout");
    
    let new_session = auth.attempt_auto_login(device_id.clone())
        .await
        .expect("Auto-login should work after normal logout");
    
    // Test 2: Manual logout - auto-login is disabled
    auth.logout_manual(new_session).await
        .expect("Manual logout should succeed");
    
    assert!(!auth.is_auto_login_enabled(user_id, device_id.clone()).await,
            "Auto-login should be disabled after manual logout");
    
    let result = auth.attempt_auto_login(device_id.clone()).await;
    assert!(result.is_err(), "Auto-login should fail after manual logout");
    
    // But device is still trusted
    assert!(auth.is_device_trusted(user_id, &device_id).await,
            "Device should still be trusted even after manual logout");
}