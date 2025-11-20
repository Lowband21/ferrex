//! Auth Integration Tests - Main test runner for all auth tests
//!
//! This file includes all auth tests and provides a single entry point
//! for running the complete auth test suite.

// Basic auth functionality tests
mod auth_basic {
    use ferrex_player::domains::auth::service::AuthService;

    #[tokio::test]
    async fn test_device_trust_and_pin_auth() {
        let auth = AuthService::new();

        // Create admin user (first user)
        let admin_id = auth
            .create_user("admin".to_string(), "password".to_string())
            .await
            .expect("Admin creation should succeed");

        // Setup PIN for admin
        auth.setup_pin(admin_id, "1234".to_string(), None)
            .await
            .expect("Admin should be able to setup own PIN");

        // Trust a device
        let device1 = "trusted_device".to_string();
        auth.trust_device(admin_id, device1.clone())
            .await
            .expect("Device trust should succeed");

        // Admin can use PIN on trusted device
        let session = auth
            .authenticate_with_pin(admin_id, "1234".to_string(), device1.clone())
            .await
            .expect("Admin should authenticate with PIN on trusted device");

        assert!(session.is_admin, "Session should be admin");

        // Admin CANNOT use PIN on untrusted device
        let device2 = "untrusted_device".to_string();
        let result = auth
            .authenticate_with_pin(admin_id, "1234".to_string(), device2.clone())
            .await;

        assert!(
            result.is_err(),
            "Admin should NOT authenticate with PIN on untrusted device"
        );
        match result.unwrap_err() {
            ferrex_player::domains::auth::AuthError::AdminRequiresPassword => {}
            other => panic!("Expected AdminRequiresPassword, got: {:?}", other),
        }
    }
}

// PIN authentication requirements
mod pin_auth_requirements {
    include!("auth/pin_auth_requirements.rs");
}

// Rate limiting and brute force protection
mod rate_limiting {
    include!("auth/rate_limiting_test.rs");
}

// Session management tests
mod session_management {
    include!("auth/session_management_tests.rs");
}

// Device trust tests
mod device_trust {
    include!("auth/device_trust_tests.rs");
}

// First run tests
mod first_run {
    include!("auth/first_run_tests.rs");
}

// Token expiry extraction tests
mod token_expiry_extraction {
    include!("auth/token_expiry_extraction_test.rs");
}

// Token persistence behaviour
mod auth_storage_persistence {
    include!("auth/auth_storage_tests.rs");
}

// Token expiry handling
mod token_expiry_behaviour {
    include!("auth/token_expiry_tests.rs");
}

// Refresh token integration tests
mod refresh_token_integration {
    include!("auth/refresh_token_integration_tests.rs");
}

// Previously pending tests - now implemented
mod completed_tests {
    use ferrex_player::domains::auth::service::AuthService;
    

    #[tokio::test]
    async fn auto_login_device_specific() {
        // Auto-login functionality is device-specific
        let auth = AuthService::new();

        // Create a user
        let user_id = auth
            .create_user("testuser".to_string(), "password123".to_string())
            .await
            .expect("User creation should succeed");

        let device1 = "device1".to_string();
        let device2 = "device2".to_string();

        // Authenticate with device1 and enable auto-login
        let _session = auth
            .authenticate_with_device(user_id, "password123".to_string(), device1.clone())
            .await
            .expect("Authentication should succeed");

        auth.enable_auto_login(user_id, device1.clone())
            .await
            .expect("Enable auto-login should succeed");

        // Auto-login should work on device1
        let result = auth.attempt_auto_login(device1.clone()).await;
        assert!(result.is_ok(), "Auto-login should work on device1");

        // Auto-login should NOT work on device2
        let result = auth.attempt_auto_login(device2.clone()).await;
        assert!(result.is_err(), "Auto-login should not work on device2");
    }

    #[tokio::test]
    async fn user_deletion_cascades_sessions() {
        // User deletion should cascade to sessions and other data
        let auth = AuthService::new();

        // Create admin (first user)
        let admin_id = auth
            .create_user("admin".to_string(), "admin_pass".to_string())
            .await
            .expect("Admin creation should succeed");

        // Create regular user
        let user_id = auth
            .create_user("user".to_string(), "user_pass".to_string())
            .await
            .expect("User creation should succeed");

        // Create sessions for the user
        let session1 = auth
            .authenticate_with_device(user_id, "user_pass".to_string(), "device1".to_string())
            .await
            .expect("Auth should succeed");

        let session2 = auth
            .authenticate_with_device(user_id, "user_pass".to_string(), "device2".to_string())
            .await
            .expect("Auth should succeed");

        // Trust a device
        auth.trust_device(user_id, "device1".to_string())
            .await
            .expect("Trust device should succeed");

        // Enable auto-login
        auth.enable_auto_login(user_id, "device1".to_string())
            .await
            .expect("Enable auto-login should succeed");

        // Verify everything is set up
        assert!(auth.is_session_valid(&session1).await);
        assert!(auth.is_session_valid(&session2).await);
        assert!(
            auth.is_device_trusted(user_id, "device1")
                .await
        );
        assert!(
            auth.is_auto_login_enabled(user_id, "device1".to_string())
                .await
        );

        // Admin deletes the user
        let admin_session = auth
            .authenticate(admin_id, "admin_pass".to_string())
            .await
            .expect("Admin auth should succeed");

        auth.delete_user(user_id, admin_session)
            .await
            .expect("User deletion should succeed");

        // Verify cascading deletion
        assert!(
            !auth.is_session_valid(&session1).await,
            "Session1 should be invalid"
        );
        assert!(
            !auth.is_session_valid(&session2).await,
            "Session2 should be invalid"
        );
        assert!(
            !auth
                .is_device_trusted(user_id, "device1")
                .await,
            "Device should not be trusted"
        );
        assert!(
            !auth
                .is_auto_login_enabled(user_id, "device1".to_string())
                .await,
            "Auto-login should be disabled"
        );
        assert!(
            auth.get_user(user_id).await.is_none(),
            "User should not exist"
        );
    }
}
