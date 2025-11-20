// Test helper for refresh token tests
//
// This provides mock implementations and utilities for testing
// the refresh token functionality

use ferrex_player::domains::auth::manager::AuthManager;
use ferrex_player::domains::auth::errors::{AuthError, TokenError};
use ferrex_player::infrastructure::api_client::ApiClient;
use ferrex_core::user::{AuthToken, User};
use ferrex_core::rbac::UserPermissions;
use chrono::{Duration, Utc};
use uuid::Uuid;
use std::sync::Arc;
use tokio::sync::RwLock;
use std::collections::HashMap;

/// Mock server state for testing
pub struct MockAuthServer {
    pub refresh_tokens: Arc<RwLock<HashMap<String, RefreshTokenData>>>,
    pub should_fail_refresh: Arc<RwLock<bool>>,
    pub refresh_count: Arc<RwLock<usize>>,
}

#[derive(Clone)]
pub struct RefreshTokenData {
    pub user_id: Uuid,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub device_id: String,
}

impl MockAuthServer {
    pub fn new() -> Self {
        Self {
            refresh_tokens: Arc::new(RwLock::new(HashMap::new())),
            should_fail_refresh: Arc::new(RwLock::new(false)),
            refresh_count: Arc::new(RwLock::new(0)),
        }
    }
    
    pub async fn add_refresh_token(&self, token: String, user_id: Uuid, device_id: String) {
        let data = RefreshTokenData {
            user_id,
            expires_at: Utc::now() + Duration::days(30),
            device_id,
        };
        self.refresh_tokens.write().await.insert(token, data);
    }
    
    pub async fn set_should_fail(&self, should_fail: bool) {
        *self.should_fail_refresh.write().await = should_fail;
    }
    
    pub async fn get_refresh_count(&self) -> usize {
        *self.refresh_count.read().await
    }
    
    pub async fn validate_refresh_token(&self, token: &str) -> Result<AuthToken, String> {
        // Increment refresh count
        let mut count = self.refresh_count.write().await;
        *count += 1;
        
        // Check if we should fail
        if *self.should_fail_refresh.read().await {
            return Err("Refresh token invalid or expired".to_string());
        }
        
        // Check if token exists
        let tokens = self.refresh_tokens.read().await;
        if let Some(data) = tokens.get(token) {
            // Check if token is expired
            if data.expires_at < Utc::now() {
                return Err("Refresh token expired".to_string());
            }
            
            // Generate new tokens
            Ok(AuthToken {
                access_token: format!("new_access_token_{}", *count),
                refresh_token: format!("new_refresh_token_{}", *count),
                expires_in: 900, // 15 minutes
            })
        } else {
            Err("Refresh token not found".to_string())
        }
    }
}

/// Create a test user
pub fn create_test_user() -> User {
    User {
        id: Uuid::now_v7(),
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

/// Create test permissions
pub fn create_test_permissions(user_id: Uuid) -> UserPermissions {
    UserPermissions {
        user_id,
        roles: vec!["user".to_string()],
        permissions: HashMap::new(),
        permission_details: None,
    }
}

/// Create an expired token
pub fn create_expired_token() -> AuthToken {
    AuthToken {
        access_token: "<REDACTED>".to_string(),
        refresh_token: "valid_refresh_token".to_string(),
        expires_in: -60, // Already expired
    }
}

/// Create a valid token
pub fn create_valid_token() -> AuthToken {
    AuthToken {
        access_token: "<REDACTED>".to_string(),
        refresh_token: "valid_refresh_token".to_string(),
        expires_in: 900, // 15 minutes
    }
}

/// Create a token that's about to expire
pub fn create_expiring_token() -> AuthToken {
    AuthToken {
        access_token: "<REDACTED>".to_string(),
        refresh_token: "valid_refresh_token".to_string(),
        expires_in: 30, // 30 seconds
    }
}