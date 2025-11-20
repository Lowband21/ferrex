// Device trust management tests
//
// Requirements:
// - Device trust expires after 30 days
// - Admin always requires password on untrusted devices
// - Device-specific trust management

use ferrex_player::domains::auth::MockAuthService;

#[tokio::test]
async fn device_trust_expires_after_30_days() {
    // Requirement: Device trust expires after 30 days
    let auth = MockAuthService::new();

    // Create a user and authenticate
    let user_id = auth
        .create_user("user".to_string(), "password".to_string())
        .await
        .expect("User creation should succeed");

    let device_id = "device_123".to_string();

    // Trust the device
    auth.trust_device(user_id, device_id.clone())
        .await
        .expect("Device trust should succeed");

    // Verify device is trusted initially
    assert!(
        auth.is_device_trusted(user_id, &device_id).await,
        "Device should be trusted immediately after trusting"
    );

    // Advance time by 29 days - should still be trusted
    auth.advance_time(chrono::Duration::days(29)).await;
    assert!(
        auth.is_device_trusted(user_id, &device_id).await,
        "Device should still be trusted after 29 days"
    );

    // Advance time by 2 more days (total 31 days) - should expire
    auth.advance_time(chrono::Duration::days(2)).await;
    assert!(
        !auth.is_device_trusted(user_id, &device_id).await,
        "Device trust should expire after 30 days"
    );
}

#[tokio::test]
async fn admin_requires_password_on_untrusted_device() {
    // Critical security requirement: Admin cannot use PIN on unrecognized devices
    let auth = MockAuthService::new();

    // Create admin (first user)
    let admin_id = auth
        .create_user("admin".to_string(), "admin_pass".to_string())
        .await
        .expect("Admin creation should succeed");

    // Setup PIN for admin
    let admin_session = auth
        .authenticate(admin_id, "admin_pass".to_string())
        .await
        .expect("Admin authentication should succeed");

    auth.setup_pin(admin_id, "1234".to_string(), Some(admin_session.clone()))
        .await
        .expect("Admin PIN setup should succeed");

    // Trust device 1
    let device1 = "trusted_device".to_string();
    auth.trust_device(admin_id, device1.clone())
        .await
        .expect("Device trust should succeed");

    // On trusted device, admin can use PIN
    let result = auth
        .authenticate_with_pin(admin_id, "1234".to_string(), device1.clone())
        .await;
    assert!(
        result.is_ok(),
        "Admin should be able to use PIN on trusted device"
    );

    // On NEW device, admin CANNOT use PIN
    let device2 = "untrusted_device".to_string();
    let result = auth
        .authenticate_with_pin(admin_id, "1234".to_string(), device2.clone())
        .await;

    assert!(
        result.is_err(),
        "Admin should NOT be able to use PIN on untrusted device"
    );

    match result.unwrap_err() {
        ferrex_player::domains::auth::AuthError::AdminRequiresPassword => {}
        other => {
            panic!("Expected AdminRequiresPassword error, got: {:?}", other)
        }
    }

    // But password authentication should work
    let result = auth
        .authenticate_with_password(admin_id, "admin_pass".to_string(), device2)
        .await;
    assert!(
        result.is_ok(),
        "Admin should be able to use password on any device"
    );
}

#[tokio::test]
async fn device_can_be_revoked() {
    let auth = MockAuthService::new();

    // Create admin and user
    let admin_id = auth
        .create_user("admin".to_string(), "admin_pass".to_string())
        .await
        .expect("Admin creation should succeed");

    let user_id = auth
        .create_user("user".to_string(), "user_pass".to_string())
        .await
        .expect("User creation should succeed");

    let device_id = "device_to_revoke".to_string();

    // Trust the device for user
    auth.trust_device(user_id, device_id.clone())
        .await
        .expect("Device trust should succeed");

    // Verify device is trusted
    assert!(auth.is_device_trusted(user_id, &device_id).await);

    // Admin revokes the device
    let admin_session = auth
        .authenticate(admin_id, "admin_pass".to_string())
        .await
        .expect("Admin auth should succeed");

    auth.revoke_device(device_id.clone(), admin_session)
        .await
        .expect("Device revocation should succeed");

    // Device should no longer be trusted
    assert!(
        !auth.is_device_trusted(user_id, &device_id).await,
        "Device should not be trusted after revocation"
    );
}
