// Refresh Token Tests
//
// Requirements from security review:
// - Access tokens expire after 15 minutes (server-side)
// - Refresh tokens should automatically renew access tokens
// - Refresh should work within device trust period (30 days)
// - Failed refresh should clear authentication

use ferrex_player::domains::auth::manager::AuthManager;
use ferrex_player::domains::auth::errors::{AuthError, TokenError};
use ferrex_core::user::{AuthToken, User};
use ferrex_core::rbac::UserPermissions;
use chrono::{Duration, Utc};
use uuid::Uuid;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Mock API client for testing refresh token flow
struct MockApiClient {
    refresh_count: Arc<RwLock<usize>>,
    should_fail_refresh: Arc<RwLock<bool>>,
}

impl MockApiClient {
    fn new() -> Self {
        Self {
            refresh_count: Arc::new(RwLock::new(0)),
            should_fail_refresh: Arc::new(RwLock::new(false)),
        }
    }

    async fn set_should_fail(&self, should_fail: bool) {
        *self.should_fail_refresh.write().await = should_fail;
    }

    async fn get_refresh_count(&self) -> usize {
        *self.refresh_count.read().await
    }

    async fn refresh_token(&self, _refresh_token: &str) -> Result<AuthToken, String> {
        let mut count = self.refresh_count.write().await;
        *count += 1;

        if *self.should_fail_refresh.read().await {
            Err("Refresh token invalid or expired".to_string())
        } else {
            Ok(AuthToken {
                access_token: format!("refreshed_access_token_{}", *count),
                refresh_token: format!("refreshed_refresh_token_{}", *count),
                expires_in: 900, // 15 minutes
            })
        }
    }
}

#[tokio::test]
async fn test_automatic_token_refresh_on_expiry() {
    // GIVEN: A user with an expired access token but valid refresh token
    let auth_manager = create_test_auth_manager();
    let user = create_test_user();
    let expired_token = AuthToken {
        access_token: "<REDACTED>".to_string(),
        refresh_token: "valid_refresh_token".to_string(),
        expires_in: -60, // Already expired
    };
    
    // Store auth with expired token
    auth_manager.store_auth_with_token(user.clone(), expired_token).await.unwrap();
    
    // WHEN: Loading authentication from storage
    let result = auth_manager.load_from_keychain().await;
    
    // THEN: The token should be automatically refreshed
    assert!(result.is_ok(), "Should successfully load auth with refreshed token");
    let (token, loaded_user) = result.unwrap().unwrap();
    assert_ne!(token.access_token, "expired_token", "Access token should be refreshed");
    assert!(token.access_token.starts_with("refreshed_"), "Should have new refreshed token");
    assert_eq!(loaded_user.id, user.id, "User should remain the same");
}

#[tokio::test]
async fn test_refresh_token_failure_clears_auth() {
    // GIVEN: A user with expired tokens and a refresh that will fail
    let auth_manager = create_test_auth_manager();
    let mock_api = MockApiClient::new();
    mock_api.set_should_fail(true).await;
    
    let user = create_test_user();
    let expired_token = AuthToken {
        access_token: "<REDACTED>".to_string(),
        refresh_token: "invalid_refresh_token".to_string(),
        expires_in: -60,
    };
    
    auth_manager.store_auth_with_token(user.clone(), expired_token).await.unwrap();
    
    // WHEN: Loading auth triggers refresh which fails
    let result = auth_manager.load_from_keychain().await;
    
    // THEN: Auth should be cleared
    assert!(result.is_ok(), "Should not error even if refresh fails");
    assert!(result.unwrap().is_none(), "Should return None when refresh fails");
    
    // Verify auth was cleared from storage
    let second_load = auth_manager.load_from_keychain().await;
    assert!(second_load.unwrap().is_none(), "Auth should be cleared from storage");
}

#[tokio::test]
async fn test_refresh_token_within_device_trust_period() {
    // GIVEN: Device trust valid for 30 days, token expired after 15 minutes
    let auth_manager = create_test_auth_manager();
    let user = create_test_user();
    
    // Create auth with device trust
    let token = AuthToken {
        access_token: "<REDACTED>".to_string(),
        refresh_token: "valid_refresh".to_string(),
        expires_in: 900, // 15 minutes
    };
    
    auth_manager.store_auth_with_device_trust(
        user.clone(),
        token,
        Utc::now() + Duration::days(30),
    ).await.unwrap();
    
    // Simulate time passing (20 minutes)
    auth_manager.advance_time(Duration::minutes(20)).await;
    
    // WHEN: Loading auth after token expiry but within device trust
    let result = auth_manager.load_from_keychain().await;
    
    // THEN: Should successfully refresh
    assert!(result.is_ok(), "Should load auth within device trust period");
    let (refreshed_token, _) = result.unwrap().unwrap();
    assert_ne!(refreshed_token.access_token, "token_will_expire", "Token should be refreshed");
}

#[tokio::test]
async fn test_no_refresh_without_refresh_token() {
    // GIVEN: An expired access token with no refresh token
    let auth_manager = create_test_auth_manager();
    let user = create_test_user();
    let token_no_refresh = AuthToken {
        access_token: "<REDACTED>".to_string(),
        refresh_token: "".to_string(), // Empty refresh token
        expires_in: -60,
    };
    
    auth_manager.store_auth_with_token(user, token_no_refresh).await.unwrap();
    
    // WHEN: Loading auth with expired token and no refresh token
    let result = auth_manager.load_from_keychain().await;
    
    // THEN: Auth should be cleared
    assert!(result.is_ok());
    assert!(result.unwrap().is_none(), "Should clear auth when no refresh token available");
}

#[tokio::test]
async fn test_refresh_updates_both_tokens() {
    // GIVEN: An auth manager with a valid refresh token
    let auth_manager = create_test_auth_manager();
    let user = create_test_user();
    let initial_token = AuthToken {
        access_token: "<REDACTED>".to_string(),
        refresh_token: "refresh_1".to_string(),
        expires_in: 60, // About to expire
    };
    
    auth_manager.authenticate_with_token(user.clone(), initial_token).await.unwrap();
    
    // WHEN: Explicitly calling refresh
    let refresh_result = auth_manager.refresh_access_token().await;
    
    // THEN: Both tokens should be updated
    assert!(refresh_result.is_ok(), "Refresh should succeed");
    
    let current_token = auth_manager.get_current_token().await;
    assert!(current_token.is_some());
    let token = current_token.unwrap();
    assert_ne!(token.access_token, "access_1", "Access token should be updated");
    assert_ne!(token.refresh_token, "refresh_1", "Refresh token should be updated");
}

#[tokio::test]
async fn test_multiple_refresh_attempts_tracked() {
    // GIVEN: A mock API that tracks refresh attempts
    let mock_api = MockApiClient::new();
    let auth_manager = create_test_auth_manager_with_mock(mock_api.clone());
    
    // Perform multiple refreshes
    for i in 1..=3 {
        let token = AuthToken {
            access_token: format!("access_{}", i),
            refresh_token: format!("refresh_{}", i),
            expires_in: -1, // Expired
        };
        
        auth_manager.store_auth_with_token(create_test_user(), token).await.unwrap();
        let _ = auth_manager.load_from_keychain().await;
    }
    
    // THEN: Should track all refresh attempts
    assert_eq!(mock_api.get_refresh_count().await, 3, "Should have attempted 3 refreshes");
}

// Helper functions for test setup

fn create_test_user() -> User {
    User {
        id: Uuid::new_v4(),
        username: "test_user".to_string(),
        display_name: "Test User".to_string(),
        avatar_url: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        last_login: Some(Utc::now()),
        is_active: true,
        email: None,
        preferences: Default::default(),
    }
}

#[cfg(test)]
fn create_test_auth_manager() -> AuthManager {
    // This would need to be implemented with test doubles/mocks
    // For now, marking as todo
    todo!("Implement test auth manager with mocked dependencies")
}

#[cfg(test)]
fn create_test_auth_manager_with_mock(api: MockApiClient) -> AuthManager {
    // This would need to be implemented to inject the mock API
    todo!("Implement test auth manager with custom mock API")
}

// Test-only extension methods
#[cfg(test)]
impl AuthManager {
    async fn store_auth_with_token(&self, user: User, token: AuthToken) -> Result<(), AuthError> {
        todo!("Implement test helper for storing auth")
    }
    
    async fn store_auth_with_device_trust(
        &self,
        user: User,
        token: AuthToken,
        trust_expires: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), AuthError> {
        todo!("Implement test helper for storing auth with device trust")
    }
    
    async fn authenticate_with_token(&self, user: User, token: AuthToken) -> Result<(), AuthError> {
        todo!("Implement test helper for authentication")
    }
    
    async fn get_current_token(&self) -> Option<AuthToken> {
        todo!("Implement test helper for getting current token")
    }
    
    async fn advance_time(&self, duration: Duration) {
        todo!("Implement virtual time advancement for testing")
    }
}