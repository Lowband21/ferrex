#![cfg(feature = "database")]

use std::cmp;
use std::sync::Arc;

use chrono::Utc;
use thiserror::Error;
use uuid::Uuid;

use crate::auth::AuthCrypto;
use crate::auth::policy::{PasswordPolicy, PasswordPolicyCheck, PasswordPolicyRule};
use crate::database::ports::{
    rbac::RbacRepository, security_settings::SecuritySettingsRepository, users::UsersRepository,
};
use crate::error::{MediaError, Result as CoreResult};
use crate::rbac::Role;
use crate::user::User;

#[derive(Clone)]
pub struct UserAdministrationService {
    users: Arc<dyn UsersRepository>,
    rbac: Arc<dyn RbacRepository>,
    security: Arc<dyn SecuritySettingsRepository>,
    crypto: Arc<AuthCrypto>,
}

impl std::fmt::Debug for UserAdministrationService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UserAdministrationService")
            .field("users_repo", &Arc::strong_count(&self.users))
            .field("rbac_repo", &Arc::strong_count(&self.rbac))
            .field("security_repo", &Arc::strong_count(&self.security))
            .finish()
    }
}

impl UserAdministrationService {
    pub fn new(
        users: Arc<dyn UsersRepository>,
        rbac: Arc<dyn RbacRepository>,
        security: Arc<dyn SecuritySettingsRepository>,
        crypto: Arc<AuthCrypto>,
    ) -> Self {
        Self {
            users,
            rbac,
            security,
            crypto,
        }
    }

    pub async fn list_users(
        &self,
        options: ListUsersOptions,
    ) -> Result<PaginatedUsers, UserAdminError> {
        let mut users = self
            .users
            .get_all_users()
            .await
            .map_err(UserAdminError::from)?;

        if !options.include_inactive {
            users.retain(|user| user.is_active);
        }

        if let Some(search) = &options.search {
            let term = search.to_lowercase();
            users.retain(|user| {
                user.username.to_lowercase().contains(&term)
                    || user.display_name.to_lowercase().contains(&term)
            });
        }

        let total = users.len();
        users.sort_by(|a, b| {
            a.display_name
                .to_lowercase()
                .cmp(&b.display_name.to_lowercase())
                .then_with(|| a.username.to_lowercase().cmp(&b.username.to_lowercase()))
        });

        let limit = options.limit.unwrap_or(50).clamp(1, 1_000) as usize;
        let offset = cmp::min(options.offset.unwrap_or(0).max(0) as usize, total);

        let paged = users
            .into_iter()
            .skip(offset)
            .take(limit)
            .collect::<Vec<_>>();

        let records = self.decorate_users(paged, options.role.clone()).await?;

        Ok(PaginatedUsers {
            total,
            limit,
            offset,
            users: records,
        })
    }

    pub async fn create_user(
        &self,
        command: CreateUserCommand,
    ) -> Result<UserAdminRecord, UserAdminError> {
        Self::validate_username(&command.username)?;

        if self
            .users
            .get_user_by_username(&command.username)
            .await
            .map_err(UserAdminError::from)?
            .is_some()
        {
            return Err(UserAdminError::UsernameExists);
        }

        let security_settings = self
            .security
            .get_settings()
            .await
            .map_err(UserAdminError::from)?;

        let role_map = self
            .map_role_ids(&command.role_ids)
            .await
            .map_err(UserAdminError::from)?;

        let is_admin = role_map.values().any(|role| role.name == "admin");
        let policy = if is_admin {
            security_settings.admin_password_policy
        } else {
            security_settings.user_password_policy
        };

        Self::validate_password(&command.password, &policy)?;

        let password_hash = self
            .crypto
            .hash_password(&command.password)
            .map_err(|err| UserAdminError::Internal(err.to_string()))?;

        let now = Utc::now();
        let user = User {
            id: Uuid::now_v7(),
            username: command.username.to_lowercase(),
            display_name: command.display_name,
            avatar_url: command.avatar_url,
            created_at: now,
            updated_at: now,
            last_login: None,
            is_active: command.is_active,
            email: command.email,
            preferences: Default::default(),
        };

        self.users
            .create_user_with_password(&user, &password_hash)
            .await
            .map_err(UserAdminError::from)?;

        let mut assigned_roles = Vec::new();
        if command.role_ids.is_empty() {
            if let Some(default_role) = self
                .find_role_by_name("user")
                .await
                .map_err(UserAdminError::from)?
            {
                self.rbac
                    .assign_user_role(
                        user.id,
                        default_role.id,
                        command.created_by.unwrap_or(user.id),
                    )
                    .await
                    .map_err(UserAdminError::from)?;
                assigned_roles.push(default_role);
            }
        } else {
            for role in role_map.values() {
                self.rbac
                    .assign_user_role(user.id, role.id, command.created_by.unwrap_or(user.id))
                    .await
                    .map_err(UserAdminError::from)?;
                assigned_roles.push(role.clone());
            }
        }

        Ok(UserAdminRecord {
            user,
            roles: assigned_roles,
            session_count: 0,
        })
    }

    pub async fn update_user(
        &self,
        user_id: Uuid,
        command: UpdateUserCommand,
    ) -> Result<UserAdminRecord, UserAdminError> {
        let mut user = self
            .users
            .get_user_by_id(user_id)
            .await
            .map_err(UserAdminError::from)?
            .ok_or(UserAdminError::UserNotFound)?;

        if let Some(display_name) = command.display_name {
            user.display_name = display_name;
        }
        if let Some(email) = command.email {
            user.email = Some(email);
        }
        if let Some(avatar) = command.avatar_url {
            user.avatar_url = Some(avatar);
        }
        if let Some(active) = command.is_active {
            user.is_active = active;
        }
        user.updated_at = Utc::now();

        self.users
            .update_user(&user)
            .await
            .map_err(UserAdminError::from)?;

        let mut current_permissions = self
            .rbac
            .get_user_permissions(user_id)
            .await
            .map_err(UserAdminError::from)?;

        if let Some(role_ids) = command.role_ids {
            let desired_roles = self
                .map_role_ids(&role_ids)
                .await
                .map_err(UserAdminError::from)?;

            let desired_ids: Vec<Uuid> = desired_roles.keys().copied().collect();

            for role in &current_permissions.roles {
                if !desired_ids.contains(&role.id) {
                    let check_last = role.name == "admin";
                    self.rbac
                        .remove_user_role_atomic(user_id, role.id, check_last)
                        .await
                        .map_err(UserAdminError::from)?;
                }
            }

            for (role_id, _) in desired_roles {
                if !current_permissions.roles.iter().any(|r| r.id == role_id) {
                    self.rbac
                        .assign_user_role(user_id, role_id, command.updated_by)
                        .await
                        .map_err(UserAdminError::from)?;
                }
            }

            current_permissions = self
                .rbac
                .get_user_permissions(user_id)
                .await
                .map_err(UserAdminError::from)?;
        }

        let sessions = self
            .users
            .get_user_sessions(user_id)
            .await
            .map_err(UserAdminError::from)?;

        Ok(UserAdminRecord {
            user,
            roles: current_permissions.roles,
            session_count: sessions.len(),
        })
    }

    pub async fn delete_user(&self, command: DeleteUserCommand) -> Result<(), UserAdminError> {
        let is_admin = self
            .rbac
            .user_has_role(command.user_id, "admin")
            .await
            .map_err(UserAdminError::from)?;

        self.users
            .delete_user_atomic(command.user_id, is_admin && command.check_admin)
            .await
            .map_err(UserAdminError::from)?;

        Ok(())
    }

    pub async fn needs_initial_setup(&self) -> Result<bool, UserAdminError> {
        let users = self
            .users
            .get_all_users()
            .await
            .map_err(UserAdminError::from)?;

        if users.is_empty() {
            return Ok(true);
        }

        for user in users {
            let permissions = self
                .rbac
                .get_user_permissions(user.id)
                .await
                .map_err(UserAdminError::from)?;
            if permissions.has_role("admin") {
                return Ok(false);
            }
        }

        Ok(true)
    }

    fn describe_policy_failures(failures: &[PasswordPolicyRule]) -> String {
        failures
            .iter()
            .map(|rule| rule.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    }

    pub fn validate_username(username: &str) -> Result<(), UserAdminError> {
        let username = username.trim();

        if username.is_empty() {
            return Err(UserAdminError::Validation(
                "Username cannot be empty".to_string(),
            ));
        }

        if username.len() < 3 {
            return Err(UserAdminError::Validation(
                "Username must be at least 3 characters".to_string(),
            ));
        }

        if username.len() > 32 {
            return Err(UserAdminError::Validation(
                "Username cannot exceed 32 characters".to_string(),
            ));
        }

        if !username
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
        {
            return Err(UserAdminError::Validation(
                "Username can only contain letters, numbers, underscores, and hyphens".to_string(),
            ));
        }

        let reserved = ["admin", "root", "system", "api", "setup"];
        if reserved.contains(&username.to_lowercase().as_str()) {
            return Err(UserAdminError::Validation(
                "This username is reserved".to_string(),
            ));
        }

        Ok(())
    }

    pub fn validate_password(
        password: &str,
        policy: &PasswordPolicy,
    ) -> Result<(), UserAdminError> {
        if password.len() > 128 {
            return Err(UserAdminError::Validation(
                "Password cannot exceed 128 characters".to_string(),
            ));
        }

        let PasswordPolicyCheck { failures } = policy.check(password);
        if policy.enforce && !failures.is_empty() {
            return Err(UserAdminError::Validation(format!(
                "Password does not meet the required policy: {}",
                Self::describe_policy_failures(&failures)
            )));
        }

        if policy.enforce && policy.min_length > 0 && password.len() < policy.min_length as usize {
            return Err(UserAdminError::Validation(format!(
                "Password must be at least {} characters",
                policy.min_length
            )));
        }

        Ok(())
    }

    async fn decorate_users(
        &self,
        users: Vec<User>,
        role_filter: Option<String>,
    ) -> Result<Vec<UserAdminRecord>, UserAdminError> {
        let mut records = Vec::with_capacity(users.len());

        for user in users {
            let permissions = self
                .rbac
                .get_user_permissions(user.id)
                .await
                .map_err(UserAdminError::from)?;

            if let Some(role_name) = &role_filter {
                if !permissions.has_role(role_name) {
                    continue;
                }
            }

            let sessions = self
                .users
                .get_user_sessions(user.id)
                .await
                .map_err(UserAdminError::from)?;

            records.push(UserAdminRecord {
                user,
                roles: permissions.roles,
                session_count: sessions.len(),
            });
        }

        Ok(records)
    }

    async fn map_role_ids(
        &self,
        role_ids: &[Uuid],
    ) -> CoreResult<std::collections::HashMap<Uuid, Role>> {
        if role_ids.is_empty() {
            return Ok(Default::default());
        }

        let all_roles = self.rbac.get_all_roles().await?;
        let mut map = std::collections::HashMap::new();
        for role_id in role_ids {
            let role = all_roles
                .iter()
                .find(|role| &role.id == role_id)
                .cloned()
                .ok_or_else(|| MediaError::NotFound(format!("Role {role_id} does not exist")))?;
            map.insert(*role_id, role);
        }
        Ok(map)
    }

    async fn find_role_by_name(&self, name: &str) -> CoreResult<Option<Role>> {
        let roles = self.rbac.get_all_roles().await?;
        Ok(roles.into_iter().find(|role| role.name == name))
    }
}

#[derive(Debug, Clone, Default)]
pub struct ListUsersOptions {
    pub search: Option<String>,
    pub role: Option<String>,
    pub include_inactive: bool,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct PaginatedUsers {
    pub total: usize,
    pub limit: usize,
    pub offset: usize,
    pub users: Vec<UserAdminRecord>,
}

#[derive(Debug, Clone)]
pub struct UserAdminRecord {
    pub user: User,
    pub roles: Vec<Role>,
    pub session_count: usize,
}

#[derive(Debug, Clone)]
pub struct CreateUserCommand {
    pub username: String,
    pub display_name: String,
    pub password: String,
    pub email: Option<String>,
    pub avatar_url: Option<String>,
    pub role_ids: Vec<Uuid>,
    pub is_active: bool,
    pub created_by: Option<Uuid>,
}

impl Default for CreateUserCommand {
    fn default() -> Self {
        Self {
            username: String::new(),
            display_name: String::new(),
            password: String::new(),
            email: None,
            avatar_url: None,
            role_ids: Vec::new(),
            is_active: true,
            created_by: None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct UpdateUserCommand {
    pub display_name: Option<String>,
    pub email: Option<String>,
    pub avatar_url: Option<String>,
    pub is_active: Option<bool>,
    pub role_ids: Option<Vec<Uuid>>,
    pub updated_by: Uuid,
}

#[derive(Debug, Clone)]
pub struct DeleteUserCommand {
    pub user_id: Uuid,
    pub deleted_by: Uuid,
    pub check_admin: bool,
}

#[derive(Debug, Error)]
pub enum UserAdminError {
    #[error("user not found")]
    UserNotFound,
    #[error("username already exists")]
    UsernameExists,
    #[error("email already in use")]
    EmailExists,
    #[error("validation error: {0}")]
    Validation(String),
    #[error("permission denied: {0}")]
    PermissionDenied(String),
    #[error("internal error: {0}")]
    Internal(String),
}

impl From<MediaError> for UserAdminError {
    fn from(err: MediaError) -> Self {
        match err {
            MediaError::NotFound(_) => UserAdminError::UserNotFound,
            MediaError::Conflict(message) => {
                if message.to_lowercase().contains("username") {
                    UserAdminError::UsernameExists
                } else if message.to_lowercase().contains("email") {
                    UserAdminError::EmailExists
                } else {
                    UserAdminError::Validation(message)
                }
            }
            other => UserAdminError::Internal(other.to_string()),
        }
    }
}
