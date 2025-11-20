//! Authentication service trait and implementations
//!
//! Provides abstraction over authentication operations,
//! replacing direct AuthManager access per RUS-136.

use crate::domains::auth::storage::StoredAuth;
use crate::infrastructure::repository::{RepositoryError, RepositoryResult};
use async_trait::async_trait;
use ferrex_core::rbac::UserPermissions;
use ferrex_core::user::{AuthToken, User};
use uuid::Uuid;

/// Authentication service trait for managing user authentication
#[async_trait]
pub trait AuthService: Send + Sync {
    /// Authenticate a user (UI provides server_url and either password or PIN)
    async fn login(
        &self,
        username: String,
        pin: String,
        server_url: String,
    ) -> RepositoryResult<(User, UserPermissions)>;

    /// Logout the current user
    async fn logout(&self) -> RepositoryResult<()>;

    /// Refresh access token if needed
    async fn refresh_access_token(&self) -> RepositoryResult<()>;

    /// Get the current authenticated user, if any
    async fn get_current_user(&self) -> RepositoryResult<Option<User>>;

    /// Get current user permissions, if any
    async fn get_current_permissions(&self) -> RepositoryResult<Option<UserPermissions>>;

    /// Check if this is first run (no users exist)
    async fn is_first_run(&self) -> RepositoryResult<bool>;

    /// Get all users for selection screen
    async fn get_all_users(
        &self,
    ) -> RepositoryResult<Vec<crate::domains::auth::dto::UserListItemDto>>;

    /// Check device authentication status for a user
    async fn check_device_auth(
        &self,
        user_id: Uuid,
    ) -> RepositoryResult<crate::domains::auth::manager::DeviceAuthStatus>;

    /// Set PIN for current device
    async fn set_device_pin(&self, pin: String) -> RepositoryResult<()>;

    /// Check if setup is needed (no admin exists)
    async fn check_setup_status(&self) -> RepositoryResult<bool>;

    /// Enable admin PIN unlock
    async fn enable_admin_pin_unlock(&self) -> RepositoryResult<()>;

    /// Disable admin PIN unlock
    async fn disable_admin_pin_unlock(&self) -> RepositoryResult<()>;

    /// Authenticate with username/password and device info
    async fn authenticate_device(
        &self,
        username: String,
        password: String,
        remember_device: bool,
    ) -> RepositoryResult<crate::domains::auth::manager::PlayerAuthResult>;

    /// Authenticate with PIN
    async fn authenticate_pin(
        &self,
        user_id: Uuid,
        pin: String,
    ) -> RepositoryResult<crate::domains::auth::manager::PlayerAuthResult>;

    /// Load stored authentication from keychain (for auto-login)
    async fn load_from_keychain(&self) -> RepositoryResult<Option<StoredAuth>>;

    /// Apply stored authentication (restore session from keychain)
    async fn apply_stored_auth(&self, stored_auth: StoredAuth) -> RepositoryResult<()>;

    /// Check if auto-login is enabled for a specific user on this device
    async fn is_auto_login_enabled(&self, user_id: &Uuid) -> RepositoryResult<bool>;

    /// Validate the current session against the server returning fresh identity data
    async fn validate_session(&self) -> RepositoryResult<(User, UserPermissions)>;

    /// Check if auto-login is enabled for the current user (both server and device preferences)
    async fn is_current_user_auto_login_enabled(&self) -> RepositoryResult<bool>;

    /// Authenticate and set current auth state (used after successful login)
    async fn authenticate(
        &self,
        user: User,
        token: AuthToken,
        permissions: UserPermissions,
        server_url: String,
    ) -> RepositoryResult<()>;

    /// Save current authentication to keychain for persistence
    async fn save_current_auth(&self) -> RepositoryResult<()>;

    /// Set auto-login preference for the current user
    async fn set_auto_login(&self, enabled: bool) -> RepositoryResult<()>;
}

/// Mock implementation for testing
#[cfg(test)]
pub mod mock {
    use super::*;
    use crate::domains::auth::storage::StoredAuth;
    use chrono::{DateTime, Utc};
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    pub struct MockAuthService {
        users: Arc<RwLock<HashMap<Uuid, User>>>,
        tokens: Arc<RwLock<HashMap<String, AuthToken>>>,
        trusted_devices: Arc<RwLock<HashMap<(Uuid, String), bool>>>,
        pub login_called: Arc<RwLock<Vec<(String, String)>>>,
    }

    impl MockAuthService {
        pub fn new() -> Self {
            Self {
                users: Arc::new(RwLock::new(HashMap::new())),
                tokens: Arc::new(RwLock::new(HashMap::new())),
                trusted_devices: Arc::new(RwLock::new(HashMap::new())),
                login_called: Arc::new(RwLock::new(Vec::new())),
            }
        }

        pub async fn add_test_user(&self, user: User) {
            self.users.write().await.insert(user.id, user);
        }
    }

    #[async_trait]
    impl AuthService for MockAuthService {
        async fn login(
            &self,
            username: String,
            pin: String,
            _server_url: String,
        ) -> RepositoryResult<(User, UserPermissions)> {
            self.login_called.write().await.push((username, pin));
            // Return first user (or create a dummy) with empty permissions
            if let Some(user) = self.users.read().await.values().next().cloned() {
                Ok((
                    user.clone(),
                    UserPermissions {
                        user_id: user.id,
                        roles: vec![],
                        permissions: HashMap::new(),
                        permission_details: None,
                    },
                ))
            } else {
                let now = chrono::Utc::now();
                let user = User {
                    id: Uuid::new_v4(),
                    username: "test".into(),
                    display_name: "Test".into(),
                    avatar_url: None,
                    created_at: now,
                    updated_at: now,
                    last_login: Some(now),
                    is_active: true,
                    email: None,
                    preferences: Default::default(),
                };
                Ok((
                    user.clone(),
                    UserPermissions {
                        user_id: user.id,
                        roles: vec![],
                        permissions: HashMap::new(),
                        permission_details: None,
                    },
                ))
            }
        }

        async fn logout(&self) -> RepositoryResult<()> {
            self.tokens.write().await.clear();
            Ok(())
        }

        async fn refresh_access_token(&self) -> RepositoryResult<()> {
            Ok(())
        }

        async fn get_current_user(&self) -> RepositoryResult<Option<User>> {
            Ok(self.users.read().await.values().next().cloned())
        }

        async fn get_current_permissions(&self) -> RepositoryResult<Option<UserPermissions>> {
            Ok(None)
        }

        async fn is_first_run(&self) -> RepositoryResult<bool> {
            Ok(self.users.read().await.is_empty())
        }

        async fn get_all_users(
            &self,
        ) -> RepositoryResult<Vec<crate::domains::auth::dto::UserListItemDto>> {
            Ok(vec![])
        }

        async fn check_device_auth(
            &self,
            _user_id: Uuid,
        ) -> RepositoryResult<crate::domains::auth::manager::DeviceAuthStatus> {
            Ok(crate::domains::auth::manager::DeviceAuthStatus {
                device_registered: true,
                has_pin: true,
                remaining_attempts: Some(5),
            })
        }

        async fn set_device_pin(&self, _pin: String) -> RepositoryResult<()> {
            Ok(())
        }

        async fn check_setup_status(&self) -> RepositoryResult<bool> {
            Ok(false)
        }

        async fn enable_admin_pin_unlock(&self) -> RepositoryResult<()> {
            Ok(())
        }

        async fn disable_admin_pin_unlock(&self) -> RepositoryResult<()> {
            Ok(())
        }

        async fn authenticate_device(
            &self,
            _username: String,
            _password: String,
            _remember_device: bool,
        ) -> RepositoryResult<crate::domains::auth::manager::PlayerAuthResult> {
            let user = User {
                id: Uuid::new_v4(),
                username: "test".into(),
                display_name: "Test".into(),
                avatar_url: None,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
                last_login: Some(chrono::Utc::now()),
                is_active: true,
                email: None,
                preferences: Default::default(),
            };
            let permissions = UserPermissions {
                user_id: user.id,
                roles: vec![],
                permissions: HashMap::new(),
                permission_details: None,
            };
            Ok(crate::domains::auth::manager::PlayerAuthResult {
                user: user.clone(),
                permissions,
                device_has_pin: false,
            })
        }

        async fn authenticate_pin(
            &self,
            _user_id: Uuid,
            _pin: String,
        ) -> RepositoryResult<crate::domains::auth::manager::PlayerAuthResult> {
            let user = User {
                id: Uuid::new_v4(),
                username: "test".into(),
                display_name: "Test".into(),
                avatar_url: None,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
                last_login: Some(chrono::Utc::now()),
                is_active: true,
                email: None,
                preferences: Default::default(),
            };
            let permissions = UserPermissions {
                user_id: user.id,
                roles: vec![],
                permissions: HashMap::new(),
                permission_details: None,
            };
            Ok(crate::domains::auth::manager::PlayerAuthResult {
                user: user.clone(),
                permissions,
                device_has_pin: true,
            })
        }

        async fn load_from_keychain(&self) -> RepositoryResult<Option<StoredAuth>> {
            Ok(None)
        }

        async fn apply_stored_auth(&self, _stored_auth: StoredAuth) -> RepositoryResult<()> {
            Ok(())
        }

        async fn is_auto_login_enabled(&self, _user_id: &Uuid) -> RepositoryResult<bool> {
            Ok(false)
        }

        async fn validate_session(&self) -> RepositoryResult<(User, UserPermissions)> {
            let user = self
                .users
                .read()
                .await
                .values()
                .next()
                .cloned()
                .ok_or_else(|| RepositoryError::QueryFailed("No user available".into()))?;

            Ok((
                user.clone(),
                UserPermissions {
                    user_id: user.id,
                    roles: vec![],
                    permissions: HashMap::new(),
                    permission_details: None,
                },
            ))
        }

        async fn is_current_user_auto_login_enabled(&self) -> RepositoryResult<bool> {
            Ok(false)
        }

        async fn authenticate(
            &self,
            user: User,
            _token: AuthToken,
            _permissions: UserPermissions,
            _server_url: String,
        ) -> RepositoryResult<()> {
            self.users.write().await.insert(user.id, user);
            Ok(())
        }

        async fn save_current_auth(&self) -> RepositoryResult<()> {
            Ok(())
        }

        async fn set_auto_login(&self, _enabled: bool) -> RepositoryResult<()> {
            Ok(())
        }
    }
}
