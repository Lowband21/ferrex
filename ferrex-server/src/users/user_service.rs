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
    user::{AuthToken, User},
};
use tracing::info;
use uuid::Uuid;

use crate::{
    infra::{
        app_state::AppState,
        errors::{AppError, AppResult},
    },
    users::auth::{generate_access_token, generate_refresh_token},
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

impl<'a> UserService<'a> {
    /// Create a new user service instance
    pub fn new(state: &'a AppState) -> Self {
        Self { state }
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
            .state
            .db
            .backend()
            .get_user_by_username(&params.username)
            .await
        {
            return Err(AppError::conflict("Username already exists"));
        }

        // Hash password
        let password_hash = Self::hash_password(&params.password)?;

        // Create user
        let user_id = Uuid::new_v4();
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

        // Get PostgresDatabase directly for user creation with password
        // This is necessary because the trait method doesn't accept password parameter
        let postgres_db = self
            .state
            .db
            .backend()
            .as_any()
            .downcast_ref::<ferrex_core::database::PostgresDatabase>()
            .ok_or_else(|| AppError::internal("Database backend is not PostgreSQL"))?;

        // Create user with password in a single transaction
        postgres_db
            .create_user(&user, &password_hash)
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
            .state
            .db
            .backend()
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
            self.state
                .db
                .backend()
                .update_user_password(user_id, &password_hash)
                .await
                .map_err(|_| AppError::internal("Failed to update password"))?;
        }

        // Update in database
        self.state
            .db
            .backend()
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
            .state
            .db
            .backend()
            .user_has_role(user_id, "admin")
            .await
            .map_err(|e| AppError::internal(format!("Failed to check user role: {}", e)))?;

        // Use atomic delete operation that handles race conditions
        self.state
            .db
            .backend()
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
            .state
            .db
            .backend()
            .get_user_by_id(user_id)
            .await
            .map_err(|_| AppError::not_found("User not found"))?
            .ok_or_else(|| AppError::not_found("User not found"))?;

        // Verify role exists
        let roles = self
            .state
            .db
            .backend()
            .get_all_roles()
            .await
            .map_err(|e| AppError::internal(format!("Failed to get roles: {}", e)))?;

        let role = roles
            .iter()
            .find(|r| r.id == role_id)
            .ok_or_else(|| AppError::not_found("Role not found"))?;

        // Assign role
        self.state
            .db
            .backend()
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
        self.state
            .db
            .backend()
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

    /// Generate authentication tokens for a user
    pub async fn generate_auth_tokens(
        &self,
        user_id: Uuid,
        device_name: Option<String>,
    ) -> AppResult<AuthToken> {
        // Generate tokens
        let access_token = generate_access_token(user_id)
            .map_err(|_| AppError::internal("Failed to generate access token"))?;

        let refresh_token = generate_refresh_token();

        // Store refresh token
        let expires_at = Utc::now() + chrono::Duration::days(30);
        self.state
            .db
            .backend()
            .store_refresh_token(&refresh_token, user_id, device_name, expires_at)
            .await
            .map_err(|_| AppError::internal("Failed to store refresh token"))?;

        Ok(AuthToken {
            access_token,
            refresh_token,
            expires_in: 900, // 15 minutes
        })
    }

    /// Check if a server needs initial setup (no admin exists)
    pub async fn needs_setup(&self) -> AppResult<bool> {
        let users = self
            .state
            .db
            .backend()
            .get_all_users()
            .await
            .map_err(|e| AppError::internal(format!("Failed to get users: {}", e)))?;

        for user in users {
            if let Ok(perms) = self.state.db.backend().get_user_permissions(user.id).await {
                if perms.has_role("admin") {
                    return Ok(false);
                }
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
