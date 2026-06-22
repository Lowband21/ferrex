//! User administration domain surfaces for Ferrex player clients.
//!
//! This crate owns admin user-management state and messages while concrete API
//! adapters remain in `ferrex-player-api` and UI rendering remains in the app.

#![forbid(unsafe_code)]

/// User-administration message types.
pub mod messages;
/// UI-agnostic user-administration reducers and effects.
pub mod update;

use ferrex_core::player_prelude::UserPermissions;
use ferrex_player_api::{
    api_types::AdminUserInfo,
    services::{api::ApiService, user_management::UserAdminService},
};
use std::sync::Arc;

pub use messages::UserManagementMessage;
pub use update::{
    UserManagementEffect, UserManagementUpdate, update_user_management,
};

/// User management domain state.
#[derive(Default)]
pub struct UserManagementDomainState {
    /// API service reference retained for admin integrations.
    pub api_service: Option<Arc<dyn ApiService>>,
    /// Authenticated user's permissions, when available.
    pub user_permissions: Option<UserPermissions>,
    /// User-administration service port implemented by API adapters.
    pub user_admin_service: Option<Arc<dyn UserAdminService>>,
    /// Cached admin user list for the admin users page.
    pub users: Vec<AdminUserInfo>,
}

impl UserManagementDomainState {
    /// Replace the cached user list after a successful load.
    pub fn set_users(&mut self, users: Vec<AdminUserInfo>) {
        self.users = users;
    }

    /// Remove a deleted user from the cached user list for immediate feedback.
    pub fn remove_user(&mut self, user_id: uuid::Uuid) {
        self.users.retain(|user| user.id != user_id);
    }

    /// Record authenticated permissions for admin gating.
    pub fn set_user_permissions(&mut self, permissions: UserPermissions) {
        self.user_permissions = Some(permissions);
    }
}

impl std::fmt::Debug for UserManagementDomainState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UserManagementDomainState")
            .field("has_api_service", &self.api_service.as_ref().map(|_| true))
            .field("user_permissions", &self.user_permissions)
            .field(
                "has_user_admin_service",
                &self.user_admin_service.as_ref().map(|_| true),
            )
            .field("users_len", &self.users.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::messages::UserManagementMessage;

    #[test]
    fn password_messages_sanitize_display_values() {
        let message = UserManagementMessage::CreateUserFormUpdatePassword(
            "super-secret".into(),
        );
        assert_eq!(
            message.sanitized_display(),
            "CreateUserFormUpdatePassword(***)"
        );
    }
}
