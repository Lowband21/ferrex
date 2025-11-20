//! Authentication service trait and implementations
//!
//! This is the runtime-facing contract the app uses to authenticate against
//! the server. The server is the authority for identity, device trust, PIN and
//! token issuance. For UI/testing stubs used by presets and demos, see
//! `infra::testing::stubs::StubAuthService`.

use crate::{
    domains::auth::{manager::AutoLoginScope, storage::StoredAuth},
    infra::repository::RepositoryResult,
};

use ferrex_core::player_prelude::{AuthToken, User, UserPermissions};

use async_trait::async_trait;
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
    async fn get_current_permissions(
        &self,
    ) -> RepositoryResult<Option<UserPermissions>>;

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
    async fn apply_stored_auth(
        &self,
        stored_auth: StoredAuth,
    ) -> RepositoryResult<()>;

    /// Check if auto-login is enabled for a specific user on this device
    async fn is_auto_login_enabled(
        &self,
        user_id: &Uuid,
    ) -> RepositoryResult<bool>;

    /// Validate the current session against the server returning fresh identity data
    async fn validate_session(
        &self,
    ) -> RepositoryResult<(User, UserPermissions)>;

    /// Check if auto-login is enabled for the current user (both server and device preferences)
    async fn is_current_user_auto_login_enabled(
        &self,
    ) -> RepositoryResult<bool>;

    /// Get the identifier for the current device
    async fn current_device_id(&self) -> RepositoryResult<Uuid>;

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

    /// Set auto-login preference for the current user with a specific scope.
    async fn set_auto_login_scope(
        &self,
        enabled: bool,
        scope: AutoLoginScope,
    ) -> RepositoryResult<()>;

    /// Backwards compatibility helper - calls into `set_auto_login_scope` with user scope.
    async fn set_auto_login(&self, enabled: bool) -> RepositoryResult<()> {
        self.set_auto_login_scope(enabled, AutoLoginScope::UserDefault)
            .await
    }
}
