//! AuthManager adapter that implements AuthService trait
//!
//! Wraps the existing AuthManager to provide a trait-based interface

use async_trait::async_trait;
use std::sync::Arc;
use uuid::Uuid;

use crate::domains::auth::manager::AuthManager;
use crate::domains::auth::storage::StoredAuth;
use crate::infrastructure::repository::{RepositoryError, RepositoryResult};
use crate::infrastructure::services::auth::AuthService;
use ferrex_core::rbac::UserPermissions;
use ferrex_core::user::{AuthToken, User};

/// Adapter that implements AuthService using the existing AuthManager
pub struct AuthManagerAdapter {
    manager: Arc<AuthManager>,
}

impl AuthManagerAdapter {
    pub fn new(manager: Arc<AuthManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl AuthService for AuthManagerAdapter {
    async fn login(
        &self,
        username: String,
        pin: String,
        server_url: String,
    ) -> RepositoryResult<(User, UserPermissions)> {
        self.manager
            .login(username, pin, server_url)
            .await
            .map_err(|e| RepositoryError::QueryFailed(format!("Login failed: {}", e)))
    }

    async fn logout(&self) -> RepositoryResult<()> {
        self.manager
            .logout()
            .await
            .map_err(|e| RepositoryError::QueryFailed(format!("Logout failed: {}", e)))
    }

    async fn refresh_access_token(&self) -> RepositoryResult<()> {
        self.manager
            .refresh_access_token()
            .await
            .map_err(|e| RepositoryError::QueryFailed(format!("Token refresh failed: {}", e)))
    }

    async fn get_current_user(&self) -> RepositoryResult<Option<User>> {
        Ok(self.manager.get_current_user().await)
    }

    async fn get_current_permissions(&self) -> RepositoryResult<Option<UserPermissions>> {
        Ok(self.manager.get_current_permissions().await)
    }

    async fn is_first_run(&self) -> RepositoryResult<bool> {
        // Consider first run if no users are returned by the public/users list
        let is_empty = self
            .manager
            .get_all_users()
            .await
            .map(|v| v.is_empty())
            .map_err(|e| RepositoryError::QueryFailed(format!("First run check failed: {}", e)))?;
        Ok(is_empty)
    }

    async fn get_all_users(
        &self,
    ) -> RepositoryResult<Vec<crate::domains::auth::dto::UserListItemDto>> {
        self.manager
            .get_all_users()
            .await
            .map_err(|e| RepositoryError::QueryFailed(format!("Get all users failed: {}", e)))
    }

    async fn check_device_auth(
        &self,
        user_id: Uuid,
    ) -> RepositoryResult<crate::domains::auth::manager::DeviceAuthStatus> {
        self.manager
            .check_device_auth(user_id)
            .await
            .map_err(|e| RepositoryError::QueryFailed(format!("Check device auth failed: {}", e)))
    }

    async fn set_device_pin(&self, pin: String) -> RepositoryResult<()> {
        self.manager
            .set_device_pin(pin)
            .await
            .map_err(|e| RepositoryError::QueryFailed(format!("Set device PIN failed: {}", e)))
    }

    async fn check_setup_status(&self) -> RepositoryResult<bool> {
        self.manager
            .check_setup_status()
            .await
            .map_err(|e| RepositoryError::QueryFailed(format!("Check setup status failed: {}", e)))
    }

    async fn enable_admin_pin_unlock(&self) -> RepositoryResult<()> {
        self.manager.enable_admin_pin_unlock().await.map_err(|e| {
            RepositoryError::QueryFailed(format!("Enable admin PIN unlock failed: {}", e))
        })
    }

    async fn disable_admin_pin_unlock(&self) -> RepositoryResult<()> {
        self.manager.disable_admin_pin_unlock().await.map_err(|e| {
            RepositoryError::QueryFailed(format!("Disable admin PIN unlock failed: {}", e))
        })
    }

    async fn authenticate_device(
        &self,
        username: String,
        password: String,
        remember_device: bool,
    ) -> RepositoryResult<crate::domains::auth::manager::PlayerAuthResult> {
        self.manager
            .authenticate_device(username, password, remember_device)
            .await
            .map_err(|e| {
                RepositoryError::QueryFailed(format!("Device authentication failed: {}", e))
            })
    }

    async fn authenticate_pin(
        &self,
        user_id: Uuid,
        pin: String,
    ) -> RepositoryResult<crate::domains::auth::manager::PlayerAuthResult> {
        self.manager
            .authenticate_pin(user_id, pin)
            .await
            .map_err(|e| RepositoryError::QueryFailed(format!("PIN authentication failed: {}", e)))
    }

    async fn load_from_keychain(&self) -> RepositoryResult<Option<StoredAuth>> {
        self.manager
            .load_from_keychain()
            .await
            .map_err(|e| RepositoryError::QueryFailed(format!("Load from keychain failed: {}", e)))
    }

    async fn apply_stored_auth(&self, stored_auth: StoredAuth) -> RepositoryResult<()> {
        self.manager
            .apply_stored_auth(stored_auth)
            .await
            .map_err(|e| RepositoryError::QueryFailed(format!("Apply stored auth failed: {}", e)))
    }

    async fn is_auto_login_enabled(&self, user_id: &Uuid) -> RepositoryResult<bool> {
        Ok(self
            .manager
            .auth_storage()
            .is_auto_login_enabled(user_id)
            .await
            .unwrap_or(false))
    }

    async fn is_current_user_auto_login_enabled(&self) -> RepositoryResult<bool> {
        Ok(self.manager.is_auto_login_enabled().await)
    }

    async fn current_device_id(&self) -> RepositoryResult<Uuid> {
        self.manager.current_device_id().await.map_err(|e| {
            RepositoryError::QueryFailed(format!("Get current device id failed: {}", e))
        })
    }

    async fn validate_session(&self) -> RepositoryResult<(User, UserPermissions)> {
        self.manager
            .validate_session()
            .await
            .map_err(|e| RepositoryError::QueryFailed(format!("Validate session failed: {}", e)))
    }

    async fn authenticate(
        &self,
        user: User,
        token: AuthToken,
        permissions: UserPermissions,
        server_url: String,
    ) -> RepositoryResult<()> {
        self.manager
            .auth_state()
            .authenticate(user, token, permissions, server_url);
        Ok(())
    }

    async fn save_current_auth(&self) -> RepositoryResult<()> {
        self.manager
            .save_current_auth()
            .await
            .map_err(|e| RepositoryError::QueryFailed(format!("Save current auth failed: {}", e)))
    }

    async fn set_auto_login(&self, enabled: bool) -> RepositoryResult<()> {
        self.manager
            .set_auto_login(enabled)
            .await
            .map_err(|e| RepositoryError::QueryFailed(format!("Set auto login failed: {}", e)))
    }
}
