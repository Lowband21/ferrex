//! Centralized user management service
//!
//! Provides a modular service for all user-related operations to avoid
//! code duplication and maintain consistency across the application.

use argon2::{
    Argon2,
    password_hash::{PasswordHasher, SaltString, rand_core::OsRng},
};
use chrono::Utc;
use ferrex_core::{
    MediaError,
    database::ports::{rbac::RbacRepository, users::UsersRepository},
    rbac::{PermissionCategory, permissions, roles},
    user::{AuthToken, User},
};
use std::fmt;
use tracing::info;
use uuid::Uuid;

use crate::infra::{
    app_state::AppState,
    errors::{AppError, AppResult},
};

/// User creation parameters
#[derive(Debug, Clone)]
pub struct CreateUserParams {
    pub username: String,
    pub display_name: String,
    pub password: String,
    pub created_by: Option<Uuid>,
}

/// User update parameters
#[derive(Debug, Clone)]
pub struct UpdateUserParams {
    pub display_name: Option<String>,
    pub password: Option<String>,
}

/// Password validation requirements
#[derive(Debug, Clone)]
pub struct PasswordRequirements {
    pub min_length: usize,
    pub require_uppercase: bool,
    pub require_lowercase: bool,
    pub require_numbers: bool,
    pub require_special: bool,
    pub is_admin: bool,
}

impl Default for PasswordRequirements {
    fn default() -> Self {
        Self {
            min_length: 4, // Default for PIN
            require_uppercase: false,
            require_lowercase: false,
            require_numbers: true,
            require_special: false,
            is_admin: false,
        }
    }
}

impl PasswordRequirements {
    /// Requirements for admin users
    pub fn admin() -> Self {
        Self {
            min_length: 8,
            require_uppercase: true,
            require_lowercase: true,
            require_numbers: true,
            require_special: false,
            is_admin: true,
        }
    }

    /// Requirements for standard users (PIN)
    pub fn standard() -> Self {
        Self::default()
    }
}

/// Centralized service for user operations
pub struct UserService<'a> {
    state: &'a AppState,
}

impl<'a> fmt::Debug for UserService<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state_ptr = self.state as *const AppState;
        f.debug_struct("UserService")
            .field("state_ptr", &state_ptr)
            .finish()
    }
}

impl<'a> UserService<'a> {
    /// Create a new user service instance
    pub fn new(state: &'a AppState) -> Self {
        Self { state }
    }

    fn users_repo(&self) -> &dyn UsersRepository {
        &*self.state.unit_of_work.users
    }

    fn rbac_repo(&self) -> &dyn RbacRepository {
        &*self.state.unit_of_work.rbac
    }

    fn pool(&self) -> &sqlx::PgPool {
        self.state.postgres.pool()
    }

    /// Ensure the built-in roles and their default permissions exist.
    ///
    /// Consolidated schema migrations removed the original RBAC seed data.
    /// This guard re-creates the default roles (admin, user, guest), ensures
    /// the well-known permission set exists, and assigns the expected
    /// permissions to each role so the UI behaves correctly for admins.
    pub async fn ensure_admin_role_exists(&self) -> AppResult<()> {
        // Access the underlying Postgres pool
        let mut tx =
            self.pool().begin().await.map_err(|e| {
                AppError::internal(format!("Failed to start RBAC bootstrap: {}", e))
            })?;

        // Keep UUIDs stable to preserve compatibility with existing references
        let admin_role_id = Uuid::parse_str("00000000-0000-0000-0000-000000000001")
            .expect("Invalid admin role UUID");
        let user_role_id = Uuid::parse_str("00000000-0000-0000-0000-000000000002")
            .expect("Invalid user role UUID");
        let guest_role_id = Uuid::parse_str("00000000-0000-0000-0000-000000000003")
            .expect("Invalid guest role UUID");

        let system_roles = [
            (
                admin_role_id,
                roles::ADMIN,
                "Full system administrator with all permissions",
                true,
            ),
            (
                user_role_id,
                roles::USER,
                "Standard user with media access",
                true,
            ),
            (
                guest_role_id,
                roles::GUEST,
                "Limited guest access (no persistent data)",
                true,
            ),
        ];

        for (id, name, description, is_system) in system_roles {
            sqlx::query!(
                r#"
                INSERT INTO roles (id, name, description, is_system)
                VALUES ($1, $2, $3, $4)
                ON CONFLICT (name) DO UPDATE
                SET description = EXCLUDED.description,
                    is_system = EXCLUDED.is_system
                "#,
                id,
                name,
                description,
                is_system
            )
            .execute(&mut *tx)
            .await
            .map_err(|e| AppError::internal(format!("Failed to upsert role '{}': {}", name, e)))?;
        }

        struct PermissionSeed {
            name: &'static str,
            category: PermissionCategory,
            description: &'static str,
        }

        let default_permissions = [
            // User management
            PermissionSeed {
                name: permissions::USERS_READ,
                category: PermissionCategory::Users,
                description: "View user profiles and list users",
            },
            PermissionSeed {
                name: permissions::USERS_CREATE,
                category: PermissionCategory::Users,
                description: "Create new user accounts",
            },
            PermissionSeed {
                name: permissions::USERS_UPDATE,
                category: PermissionCategory::Users,
                description: "Modify user profiles and settings",
            },
            PermissionSeed {
                name: permissions::USERS_DELETE,
                category: PermissionCategory::Users,
                description: "Delete user accounts",
            },
            PermissionSeed {
                name: permissions::USERS_MANAGE_ROLES,
                category: PermissionCategory::Users,
                description: "Assign and remove user roles",
            },
            // Library management
            PermissionSeed {
                name: permissions::LIBRARIES_READ,
                category: PermissionCategory::Libraries,
                description: "View library information",
            },
            PermissionSeed {
                name: permissions::LIBRARIES_CREATE,
                category: PermissionCategory::Libraries,
                description: "Create new libraries",
            },
            PermissionSeed {
                name: permissions::LIBRARIES_UPDATE,
                category: PermissionCategory::Libraries,
                description: "Modify library settings",
            },
            PermissionSeed {
                name: permissions::LIBRARIES_DELETE,
                category: PermissionCategory::Libraries,
                description: "Delete libraries",
            },
            PermissionSeed {
                name: permissions::LIBRARIES_SCAN,
                category: PermissionCategory::Libraries,
                description: "Trigger library scans",
            },
            // Media access
            PermissionSeed {
                name: permissions::MEDIA_READ,
                category: PermissionCategory::Media,
                description: "View media information and browse",
            },
            PermissionSeed {
                name: permissions::MEDIA_STREAM,
                category: PermissionCategory::Media,
                description: "Stream and playback media",
            },
            PermissionSeed {
                name: permissions::MEDIA_DOWNLOAD,
                category: PermissionCategory::Media,
                description: "Download media files",
            },
            PermissionSeed {
                name: permissions::MEDIA_UPDATE,
                category: PermissionCategory::Media,
                description: "Edit media metadata",
            },
            PermissionSeed {
                name: permissions::MEDIA_DELETE,
                category: PermissionCategory::Media,
                description: "Delete media files",
            },
            // Server management
            PermissionSeed {
                name: permissions::SERVER_READ_SETTINGS,
                category: PermissionCategory::Server,
                description: "View server configuration",
            },
            PermissionSeed {
                name: permissions::SERVER_UPDATE_SETTINGS,
                category: PermissionCategory::Server,
                description: "Modify server configuration",
            },
            PermissionSeed {
                name: permissions::SERVER_READ_LOGS,
                category: PermissionCategory::Server,
                description: "View server logs",
            },
            PermissionSeed {
                name: permissions::SERVER_MANAGE_TASKS,
                category: PermissionCategory::Server,
                description: "Run maintenance tasks",
            },
            // Sync sessions
            PermissionSeed {
                name: permissions::SYNC_CREATE,
                category: PermissionCategory::Sync,
                description: "Create synchronized playback sessions",
            },
            PermissionSeed {
                name: permissions::SYNC_JOIN,
                category: PermissionCategory::Sync,
                description: "Join synchronized playback sessions",
            },
            PermissionSeed {
                name: permissions::SYNC_MANAGE,
                category: PermissionCategory::Sync,
                description: "Force-end any sync session",
            },
            // Development utilities (admin-only)
            PermissionSeed {
                name: "server:reset_database",
                category: PermissionCategory::Server,
                description: "Reset the database during development",
            },
            PermissionSeed {
                name: "server:seed_database",
                category: PermissionCategory::Server,
                description: "Seed the database with development fixtures",
            },
        ];

        use std::collections::HashMap;
        let mut permission_ids = HashMap::new();

        for perm in default_permissions.iter() {
            let record = sqlx::query!(
                r#"
                INSERT INTO permissions (name, category, description)
                VALUES ($1, $2, $3)
                ON CONFLICT (name) DO UPDATE
                SET category = EXCLUDED.category,
                    description = EXCLUDED.description
                RETURNING id
                "#,
                perm.name,
                perm.category.as_str(),
                perm.description
            )
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| {
                AppError::internal(format!(
                    "Failed to upsert permission '{}': {}",
                    perm.name, e
                ))
            })?;

            permission_ids.insert(perm.name, record.id);
        }

        // Admin receives every seeded permission
        let admin_permission_entries: Vec<(&'static str, Uuid)> = permission_ids
            .iter()
            .map(|(&name, &id)| (name, id))
            .collect();

        for (perm_name, permission_id) in admin_permission_entries {
            sqlx::query!(
                r#"
                INSERT INTO role_permissions (role_id, permission_id)
                VALUES ($1, $2)
                ON CONFLICT (role_id, permission_id) DO NOTHING
                "#,
                admin_role_id,
                permission_id
            )
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                AppError::internal(format!(
                    "Failed to assign permission '{}' to admin role: {}",
                    perm_name, e
                ))
            })?;
        }

        // Standard user defaults
        let user_defaults = [
            permissions::USERS_READ,
            permissions::LIBRARIES_READ,
            permissions::MEDIA_READ,
            permissions::MEDIA_STREAM,
            permissions::SYNC_CREATE,
            permissions::SYNC_JOIN,
        ];
        for perm_name in user_defaults.iter() {
            if let Some(permission_id) = permission_ids.get(perm_name).copied() {
                sqlx::query!(
                    r#"
                    INSERT INTO role_permissions (role_id, permission_id)
                    VALUES ($1, $2)
                    ON CONFLICT (role_id, permission_id) DO NOTHING
                    "#,
                    user_role_id,
                    permission_id
                )
                .execute(&mut *tx)
                .await
                .map_err(|e| {
                    AppError::internal(format!(
                        "Failed to assign permission '{}' to user role: {}",
                        perm_name, e
                    ))
                })?;
            }
        }

        // Guest defaults
        let guest_defaults = [
            permissions::LIBRARIES_READ,
            permissions::MEDIA_READ,
            permissions::MEDIA_STREAM,
        ];
        for perm_name in guest_defaults.iter() {
            if let Some(permission_id) = permission_ids.get(perm_name).copied() {
                sqlx::query!(
                    r#"
                    INSERT INTO role_permissions (role_id, permission_id)
                    VALUES ($1, $2)
                    ON CONFLICT (role_id, permission_id) DO NOTHING
                    "#,
                    guest_role_id,
                    permission_id
                )
                .execute(&mut *tx)
                .await
                .map_err(|e| {
                    AppError::internal(format!(
                        "Failed to assign permission '{}' to guest role: {}",
                        perm_name, e
                    ))
                })?;
            }
        }

        tx.commit()
            .await
            .map_err(|e| AppError::internal(format!("Failed to finalize RBAC bootstrap: {}", e)))?;

        Ok(())
    }

    /// Validate a password against requirements
    pub fn validate_password(
        password: &str,
        requirements: &PasswordRequirements,
    ) -> Result<(), String> {
        // Length check
        if password.len() < requirements.min_length {
            return Err(format!(
                "Password must be at least {} characters",
                requirements.min_length
            ));
        }

        // Maximum length check
        if password.len() > 128 {
            return Err("Password cannot exceed 128 characters".to_string());
        }

        // Character requirements
        if requirements.require_uppercase && !password.chars().any(|c| c.is_uppercase()) {
            return Err("Password must contain at least one uppercase letter".to_string());
        }

        if requirements.require_lowercase && !password.chars().any(|c| c.is_lowercase()) {
            return Err("Password must contain at least one lowercase letter".to_string());
        }

        if requirements.require_numbers && !password.chars().any(|c| c.is_numeric()) {
            return Err("Password must contain at least one number".to_string());
        }

        if requirements.require_special && !password.chars().any(|c| !c.is_alphanumeric()) {
            return Err("Password must contain at least one special character".to_string());
        }

        // PIN-specific validation
        if !requirements.is_admin && password.len() <= 6 {
            // Ensure it's all numeric for PINs
            if !password.chars().all(|c| c.is_numeric()) {
                return Err("PIN must contain only numbers".to_string());
            }
        }

        Ok(())
    }

    /// Validate username format
    pub fn validate_username(username: &str) -> Result<(), String> {
        let username = username.trim();

        if username.is_empty() {
            return Err("Username cannot be empty".to_string());
        }

        if username.len() < 3 {
            return Err("Username must be at least 3 characters".to_string());
        }

        if username.len() > 32 {
            return Err("Username cannot exceed 32 characters".to_string());
        }

        if !username
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
        {
            return Err(
                "Username can only contain letters, numbers, underscores, and hyphens".to_string(),
            );
        }

        // Check for reserved usernames
        let reserved = ["admin", "root", "system", "api", "setup"];
        if reserved.contains(&username.to_lowercase().as_str()) {
            return Err("This username is reserved".to_string());
        }

        Ok(())
    }

    /// Hash a password using Argon2
    pub fn hash_password(password: &str) -> AppResult<String> {
        let salt = SaltString::generate(&mut OsRng);
        let argon2 = Argon2::default();

        let hash = argon2
            .hash_password(password.as_bytes(), &salt)
            .map_err(|_| AppError::internal("Failed to hash password"))?
            .to_string();

        Ok(hash)
    }

    /// Create a new user
    pub async fn create_user(&self, params: CreateUserParams) -> AppResult<User> {
        // Validate username
        Self::validate_username(&params.username).map_err(AppError::bad_request)?;

        // Check if username exists
        if let Ok(Some(_)) = self
            .users_repo()
            .get_user_by_username(&params.username)
            .await
        {
            return Err(AppError::conflict("Username already exists"));
        }

        // Hash password
        let password_hash = Self::hash_password(&params.password)?;

        // Create user
        let user_id = Uuid::now_v7();
        let user = User {
            id: user_id,
            username: params.username.to_lowercase(),
            display_name: params.display_name,
            avatar_url: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            last_login: None,
            is_active: true,
            email: None,
            preferences: Default::default(),
        };

        self.users_repo()
            .create_user_with_password(&user, &password_hash)
            .await
            .map_err(|e| match e {
                MediaError::Conflict(msg) => AppError::conflict(msg),
                _ => AppError::internal(format!("Failed to create user: {}", e)),
            })?;

        info!(
            "User created: {} ({}) by {:?}",
            user.username, user.id, params.created_by
        );

        Ok(user)
    }

    /// Update a user
    pub async fn update_user(&self, user_id: Uuid, params: UpdateUserParams) -> AppResult<User> {
        // Get existing user
        let mut user = self
            .users_repo()
            .get_user_by_id(user_id)
            .await
            .map_err(|_| AppError::not_found("User not found"))?
            .ok_or_else(|| AppError::not_found("User not found"))?;

        // Update fields
        if let Some(display_name) = params.display_name {
            user.display_name = display_name;
        }

        if let Some(password) = params.password {
            let password_hash = Self::hash_password(&password)?;
            // Update password in credentials table
            self.users_repo()
                .update_user_password(user_id, &password_hash)
                .await
                .map_err(|_| AppError::internal("Failed to update password"))?;
        }

        // Update in database
        self.users_repo()
            .update_user(&user)
            .await
            .map_err(|e| AppError::internal(format!("Failed to update user: {}", e)))?;

        info!("User updated: {} ({})", user.username, user.id);

        Ok(user)
    }

    /// Delete a user and all associated data
    pub async fn delete_user(&self, user_id: Uuid, deleted_by: Uuid) -> AppResult<()> {
        // Check if user is admin to determine if we need to check for last admin
        let is_admin = self
            .rbac_repo()
            .user_has_role(user_id, "admin")
            .await
            .map_err(|e| AppError::internal(format!("Failed to check user role: {}", e)))?;

        // Use atomic delete operation that handles race conditions
        self.users_repo()
            .delete_user_atomic(user_id, is_admin)
            .await
            .map_err(|e| match e {
                MediaError::Conflict(msg) => AppError::conflict(msg),
                MediaError::NotFound(msg) => AppError::not_found(msg),
                _ => AppError::internal(format!("Failed to delete user: {}", e)),
            })?;

        info!("User {} deleted by {}", user_id, deleted_by);

        Ok(())
    }

    /// Assign a role to a user
    pub async fn assign_role(
        &self,
        user_id: Uuid,
        role_id: Uuid,
        assigned_by: Uuid,
    ) -> AppResult<()> {
        // Verify user exists
        let user = self
            .users_repo()
            .get_user_by_id(user_id)
            .await
            .map_err(|_| AppError::not_found("User not found"))?
            .ok_or_else(|| AppError::not_found("User not found"))?;

        // Verify role exists
        let roles = self
            .rbac_repo()
            .get_all_roles()
            .await
            .map_err(|e| AppError::internal(format!("Failed to get roles: {}", e)))?;

        let role = roles
            .iter()
            .find(|r| r.id == role_id)
            .ok_or_else(|| AppError::not_found("Role not found"))?;

        // Assign role
        self.rbac_repo()
            .assign_user_role(user_id, role_id, assigned_by)
            .await
            .map_err(|e| match e {
                MediaError::Conflict(msg) => AppError::conflict(msg),
                _ => AppError::internal(format!("Failed to assign role: {}", e)),
            })?;

        info!(
            "Role '{}' assigned to user {} by {}",
            role.name, user.username, assigned_by
        );

        Ok(())
    }

    /// Remove a role from a user
    pub async fn remove_role(
        &self,
        user_id: Uuid,
        role_id: Uuid,
        removed_by: Uuid,
    ) -> AppResult<()> {
        // Check if we're removing the admin role to determine if we need to check for last admin
        let admin_role_id = Uuid::parse_str("00000000-0000-0000-0000-000000000001")
            .expect("Invalid admin role UUID");
        let check_last_admin = role_id == admin_role_id;

        // Use atomic remove operation that handles race conditions
        self.rbac_repo()
            .remove_user_role_atomic(user_id, role_id, check_last_admin)
            .await
            .map_err(|e| match e {
                MediaError::Conflict(msg) => AppError::conflict(msg),
                MediaError::NotFound(msg) => AppError::not_found(msg),
                _ => AppError::internal(format!("Failed to remove role: {}", e)),
            })?;

        info!(
            "Role {} removed from user {} by {}",
            role_id, user_id, removed_by
        );

        Ok(())
    }

    /// Check if a server needs initial setup (no admin exists)
    pub async fn needs_setup(&self) -> AppResult<bool> {
        let users = self
            .users_repo()
            .get_all_users()
            .await
            .map_err(|e| AppError::internal(format!("Failed to get users: {}", e)))?;

        for user in users {
            if let Ok(perms) = self.rbac_repo().get_user_permissions(user.id).await
                && perms.has_role("admin")
            {
                return Ok(false);
            }
        }

        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_username_validation() {
        // Valid usernames
        assert!(UserService::validate_username("john_doe").is_ok());
        assert!(UserService::validate_username("user123").is_ok());
        assert!(UserService::validate_username("test-user").is_ok());

        // Invalid usernames
        assert!(UserService::validate_username("").is_err());
        assert!(UserService::validate_username("ab").is_err()); // Too short
        assert!(UserService::validate_username("a".repeat(33).as_str()).is_err()); // Too long
        assert!(UserService::validate_username("user@name").is_err()); // Invalid character
        assert!(UserService::validate_username("admin").is_err()); // Reserved
    }

    #[test]
    fn test_password_validation() {
        // Admin requirements
        let admin_req = PasswordRequirements::admin();
        assert!(UserService::validate_password("SecurePass123", &admin_req).is_ok());
        assert!(UserService::validate_password("weak", &admin_req).is_err());
        assert!(UserService::validate_password("nouppercase123", &admin_req).is_err());
        assert!(UserService::validate_password("NOLOWERCASE123", &admin_req).is_err());
        assert!(UserService::validate_password("NoNumbers", &admin_req).is_err());

        // Standard requirements (PIN)
        let std_req = PasswordRequirements::standard();
        assert!(UserService::validate_password("1234", &std_req).is_ok());
        assert!(UserService::validate_password("123", &std_req).is_err()); // Too short
        assert!(UserService::validate_password("abcd", &std_req).is_err()); // Not numeric
    }
}
