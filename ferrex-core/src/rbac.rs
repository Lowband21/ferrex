//! Role-Based Access Control (RBAC) system
//!
//! This module provides a flexible permission system for Ferrex Media Server.
//! Users are assigned roles, and roles have permissions. Permissions can also
//! be overridden on a per-user basis for fine-grained control.
//!
//! ## Overview
//!
//! - **Roles**: Collections of permissions (e.g., Admin, User, Guest)
//! - **Permissions**: Granular actions that can be performed (e.g., media:stream)
//! - **User Roles**: Users can have multiple roles
//! - **Permission Overrides**: Individual permissions can be granted/denied per user
//!
//! ## Example
//!
//! ```no_run
//! use ferrex_core::rbac::{Role, Permission, UserPermissions};
//!
//! // Check if a user has a permission
//! let user_permissions = UserPermissions::default();
//! if user_permissions.has_permission("media:stream") {
//!     // Allow streaming
//! }
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// A role that can be assigned to users
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Role {
    /// Unique identifier
    pub id: Uuid,
    /// Unique role name (e.g., "admin", "user")
    pub name: String,
    /// Human-readable description
    pub description: Option<String>,
    /// Whether this is a system role (cannot be deleted)
    pub is_system: bool,
    /// When the role was created
    pub created_at: i64,
}

/// A granular permission that can be checked
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Permission {
    /// Unique identifier
    pub id: Uuid,
    /// Permission name (e.g., "media:stream", "users:create")
    pub name: String,
    /// Category for grouping (e.g., "media", "users")
    pub category: String,
    /// Human-readable description
    pub description: Option<String>,
}

/// User's role assignment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserRole {
    /// User ID
    pub user_id: Uuid,
    /// Role ID
    pub role_id: Uuid,
    /// Who granted this role
    pub granted_by: Option<Uuid>,
    /// When the role was granted
    pub granted_at: i64,
}

/// Per-user permission override
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPermissionOverride {
    /// User ID
    pub user_id: Uuid,
    /// Permission ID
    pub permission_id: Uuid,
    /// Whether the permission is granted (true) or denied (false)
    pub granted: bool,
    /// Who set this override
    pub granted_by: Option<Uuid>,
    /// When the override was set
    pub granted_at: i64,
    /// Optional reason for the override
    pub reason: Option<String>,
}

/// Complete set of user permissions (computed from roles + overrides)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPermissions {
    /// User ID
    pub user_id: Uuid,
    /// All roles assigned to the user
    pub roles: Vec<Role>,
    /// All effective permissions (name -> granted)
    pub permissions: HashMap<String, bool>,
    /// Permission details for UI display
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_details: Option<Vec<Permission>>,
}

impl Default for UserPermissions {
    fn default() -> Self {
        Self {
            user_id: Uuid::nil(),
            roles: Vec::new(),
            permissions: HashMap::new(),
            permission_details: None,
        }
    }
}

impl UserPermissions {
    /// Check if the user has a specific permission
    pub fn has_permission(&self, permission: &str) -> bool {
        self.permissions.get(permission).copied().unwrap_or(false)
    }

    /// Check if the user has any of the specified permissions
    pub fn has_any_permission(&self, permissions: &[&str]) -> bool {
        permissions.iter().any(|p| self.has_permission(p))
    }

    /// Check if the user has all of the specified permissions
    pub fn has_all_permissions(&self, permissions: &[&str]) -> bool {
        permissions.iter().all(|p| self.has_permission(p))
    }

    /// Check if the user has a specific role
    pub fn has_role(&self, role_name: &str) -> bool {
        self.roles.iter().any(|r| r.name == role_name)
    }

    /// Get all permission names the user has
    pub fn granted_permissions(&self) -> Vec<&str> {
        self.permissions
            .iter()
            .filter_map(
                |(name, &granted)| {
                    if granted { Some(name.as_str()) } else { None }
                },
            )
            .collect()
    }
}

/// Permission categories for organization
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionCategory {
    /// User management permissions
    Users,
    /// Library management permissions
    Libraries,
    /// Media access permissions
    Media,
    /// Server configuration permissions
    Server,
    /// Sync session permissions
    Sync,
}

impl PermissionCategory {
    /// Get all categories
    pub fn all() -> &'static [PermissionCategory] {
        &[
            PermissionCategory::Users,
            PermissionCategory::Libraries,
            PermissionCategory::Media,
            PermissionCategory::Server,
            PermissionCategory::Sync,
        ]
    }

    /// Convert to string representation
    pub fn as_str(&self) -> &'static str {
        match self {
            PermissionCategory::Users => "users",
            PermissionCategory::Libraries => "libraries",
            PermissionCategory::Media => "media",
            PermissionCategory::Server => "server",
            PermissionCategory::Sync => "sync",
        }
    }
}

/// Well-known permission constants
pub mod permissions {
    // User Management
    pub const USERS_READ: &str = "users:read";
    pub const USERS_CREATE: &str = "users:create";
    pub const USERS_UPDATE: &str = "users:update";
    pub const USERS_DELETE: &str = "users:delete";
    pub const USERS_MANAGE_ROLES: &str = "users:manage_roles";

    // Library Management
    pub const LIBRARIES_READ: &str = "libraries:read";
    pub const LIBRARIES_CREATE: &str = "libraries:create";
    pub const LIBRARIES_UPDATE: &str = "libraries:update";
    pub const LIBRARIES_DELETE: &str = "libraries:delete";
    pub const LIBRARIES_SCAN: &str = "libraries:scan";

    // Media Access
    pub const MEDIA_READ: &str = "media:read";
    pub const MEDIA_STREAM: &str = "media:stream";
    pub const MEDIA_DOWNLOAD: &str = "media:download";
    pub const MEDIA_UPDATE: &str = "media:update";
    pub const MEDIA_DELETE: &str = "media:delete";

    // Server Management
    pub const SERVER_READ_SETTINGS: &str = "server:read_settings";
    pub const SERVER_UPDATE_SETTINGS: &str = "server:update_settings";
    pub const SERVER_READ_LOGS: &str = "server:read_logs";
    pub const SERVER_MANAGE_TASKS: &str = "server:manage_tasks";

    // Sync Sessions
    pub const SYNC_CREATE: &str = "sync:create";
    pub const SYNC_JOIN: &str = "sync:join";
    pub const SYNC_MANAGE: &str = "sync:manage";
}

/// Well-known role names
pub mod roles {
    pub const ADMIN: &str = "admin";
    pub const USER: &str = "user";
    pub const GUEST: &str = "guest";
}

/// Request to assign roles to a user
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssignRolesRequest {
    pub user_id: Uuid,
    pub role_ids: Vec<Uuid>,
}

/// Request to override a user's permission
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverridePermissionRequest {
    pub user_id: Uuid,
    pub permission: String,
    pub granted: bool,
    pub reason: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_permissions() {
        let mut permissions = HashMap::new();
        permissions.insert("media:stream".to_string(), true);
        permissions.insert("media:delete".to_string(), false);
        permissions.insert("users:read".to_string(), true);

        let user_perms = UserPermissions {
            user_id: Uuid::now_v7(),
            roles: vec![],
            permissions,
            permission_details: None,
        };

        assert!(user_perms.has_permission("media:stream"));
        assert!(!user_perms.has_permission("media:delete"));
        assert!(user_perms.has_permission("users:read"));
        assert!(!user_perms.has_permission("unknown:permission"));

        assert!(user_perms.has_any_permission(&["media:stream", "media:delete"]));
        assert!(!user_perms.has_all_permissions(&["media:stream", "media:delete"]));

        let granted = user_perms.granted_permissions();
        assert_eq!(granted.len(), 2);
        assert!(granted.contains(&"media:stream"));
        assert!(granted.contains(&"users:read"));
    }
}
