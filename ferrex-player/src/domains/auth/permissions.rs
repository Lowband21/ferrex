//! Permission helper module for RBAC checks
//!
//! This module provides helper functions to check user permissions
//! throughout the player application. It keeps permission logic
//! centralized and out of the already-large State struct.

use ferrex_core::rbac::{UserPermissions, permissions};

/// Permission checker for the current user
pub struct PermissionChecker<'a> {
    permissions: Option<&'a UserPermissions>,
}

impl<'a> PermissionChecker<'a> {
    /// Create a new permission checker with the given permissions
    pub fn new(permissions: Option<&'a UserPermissions>) -> Self {
        Self { permissions }
    }

    /// Check if the user has a specific permission
    pub fn has_permission(&self, permission: &str) -> bool {
        self.permissions
            .map(|p| p.has_permission(permission))
            .unwrap_or(false)
    }

    /// Check if the user has admin role
    pub fn is_admin(&self) -> bool {
        self.permissions
            .map(|p| p.has_role("admin"))
            .unwrap_or(false)
    }

    /// Check if the user can manage libraries
    pub fn can_manage_libraries(&self) -> bool {
        self.has_permission(permissions::LIBRARIES_CREATE)
            || self.has_permission(permissions::LIBRARIES_UPDATE)
            || self.has_permission(permissions::LIBRARIES_DELETE)
    }

    /// Check if the user can view library settings
    pub fn can_view_library_settings(&self) -> bool {
        self.has_permission(permissions::LIBRARIES_READ) || self.can_manage_libraries()
    }

    /// Check if the user can scan libraries
    pub fn can_scan_libraries(&self) -> bool {
        self.has_permission(permissions::LIBRARIES_SCAN)
    }

    /// Check if the user can manage users
    pub fn can_manage_users(&self) -> bool {
        self.has_permission(permissions::USERS_CREATE)
            || self.has_permission(permissions::USERS_UPDATE)
            || self.has_permission(permissions::USERS_DELETE)
            || self.has_permission(permissions::USERS_MANAGE_ROLES)
    }

    /// Check if the user can view user list
    pub fn can_view_users(&self) -> bool {
        self.has_permission(permissions::USERS_READ) || self.can_manage_users()
    }

    /// Check if the user can access server settings
    pub fn can_access_server_settings(&self) -> bool {
        self.has_permission(permissions::SERVER_READ_SETTINGS)
            || self.has_permission(permissions::SERVER_UPDATE_SETTINGS)
    }

    /// Check if the user can reset the database (dev tools)
    pub fn can_reset_database(&self) -> bool {
        self.has_permission("server:reset_database") || self.is_admin()
    }

    /// Check if the user can view admin dashboard
    pub fn can_view_admin_dashboard(&self) -> bool {
        self.can_view_library_settings()
            || self.can_view_users()
            || self.can_access_server_settings()
            || self.is_admin()
    }

    /// Check if the user can stream media
    pub fn can_stream_media(&self) -> bool {
        self.has_permission(permissions::MEDIA_STREAM)
    }

    /// Check if the user can download media
    pub fn can_download_media(&self) -> bool {
        self.has_permission(permissions::MEDIA_DOWNLOAD)
    }
}

/// Extension trait for State to easily check permissions
pub trait StatePermissionExt {
    /// Get a permission checker for the current user
    fn permission_checker(&self) -> PermissionChecker<'_>;

    /// Quick check if user has a specific permission
    fn has_permission(&self, permission: &str) -> bool;

    /// Quick check if user is admin
    fn is_admin(&self) -> bool;
}

impl StatePermissionExt for crate::state_refactored::State {
    fn permission_checker(&self) -> PermissionChecker<'_> {
        PermissionChecker::new(self.domains.auth.state.user_permissions.as_ref())
    }

    fn has_permission(&self, permission: &str) -> bool {
        self.permission_checker().has_permission(permission)
    }

    fn is_admin(&self) -> bool {
        self.permission_checker().is_admin()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use uuid::Uuid;

    #[test]
    fn test_permission_checker_no_permissions() {
        let checker = PermissionChecker::new(None);
        assert!(!checker.has_permission("any:permission"));
        assert!(!checker.is_admin());
        assert!(!checker.can_manage_libraries());
    }

    #[test]
    fn test_permission_checker_with_permissions() {
        let mut permissions_map = HashMap::new();
        permissions_map.insert(permissions::LIBRARIES_CREATE.to_string(), true);
        permissions_map.insert(permissions::USERS_READ.to_string(), true);

        let user_permissions = UserPermissions {
            user_id: Uuid::new_v4(),
            roles: vec![],
            permissions: permissions_map,
            permission_details: None,
        };

        let checker = PermissionChecker::new(Some(&user_permissions));
        assert!(checker.has_permission(permissions::LIBRARIES_CREATE));
        assert!(checker.can_manage_libraries());
        assert!(checker.can_view_users());
        assert!(!checker.can_manage_users());
    }
}
