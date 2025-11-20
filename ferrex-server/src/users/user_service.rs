//! Thin wrapper around core user administration services for server use.

use ferrex_core::{
    application::rbac_bootstrap::RbacBootstrapService,
    user::User,
    user_management::{
        CreateUserCommand, DeleteUserCommand, ListUsersOptions, UpdateUserCommand, UserAdminError,
        UserAdministrationService,
    },
};
use std::fmt;
use tracing::info;
use uuid::Uuid;

use crate::infra::{
    app_state::AppState,
    errors::{AppError, AppResult},
};

/// Parameters for creating a new user via the server façade.
#[derive(Debug, Clone, Default)]
pub struct CreateUserParams {
    pub username: String,
    pub display_name: String,
    pub password: String,
    pub email: Option<String>,
    pub avatar_url: Option<String>,
    pub role_ids: Vec<Uuid>,
    pub is_active: bool,
    pub created_by: Option<Uuid>,
}

/// Parameters for updating an existing user.
#[derive(Debug, Clone, Default)]
pub struct UpdateUserParams {
    pub display_name: Option<String>,
    pub email: Option<String>,
    pub avatar_url: Option<String>,
    pub is_active: Option<bool>,
    pub role_ids: Option<Vec<Uuid>>,
    pub updated_by: Uuid,
}

/// High-level user management façade used by HTTP handlers.
pub struct UserService<'a> {
    state: &'a AppState,
    admin: UserAdministrationService,
}

impl<'a> fmt::Debug for UserService<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UserService")
            .field("state_ptr", &(self.state as *const AppState))
            .finish()
    }
}

impl<'a> UserService<'a> {
    pub fn new(state: &'a AppState) -> Self {
        let admin = UserAdministrationService::new(
            state.unit_of_work.users.clone(),
            state.unit_of_work.rbac.clone(),
            state.unit_of_work.security_settings.clone(),
            state.auth_crypto.clone(),
        );

        Self { state, admin }
    }

    fn admin(&self) -> &UserAdministrationService {
        &self.admin
    }

    pub async fn ensure_admin_role_exists(&self) -> AppResult<()> {
        let bootstrap = RbacBootstrapService::new(self.state.unit_of_work.rbac.clone());
        bootstrap.ensure_defaults().await.map_err(AppError::from)
    }

    pub async fn list_users(
        &self,
        options: ListUsersOptions,
    ) -> AppResult<ferrex_core::user_management::PaginatedUsers> {
        self.admin()
            .list_users(options)
            .await
            .map_err(map_admin_error)
    }

    pub async fn get_user_by_username(&self, username: &str) -> AppResult<Option<User>> {
        let lookup = username.to_lowercase();
        self.state
            .unit_of_work
            .users
            .get_user_by_username(&lookup)
            .await
            .map_err(AppError::from)
    }

    pub async fn create_user(&self, params: CreateUserParams) -> AppResult<User> {
        Self::validate_username(&params.username).map_err(AppError::bad_request)?;

        let command = CreateUserCommand {
            username: params.username,
            display_name: params.display_name,
            password: params.password,
            email: params.email,
            avatar_url: params.avatar_url,
            role_ids: params.role_ids,
            is_active: params.is_active,
            created_by: params.created_by,
        };

        let record = self
            .admin()
            .create_user(command)
            .await
            .map_err(map_admin_error)?;

        info!(
            target: "user.service",
            user_id = %record.user.id,
            username = %record.user.username,
            action = "create"
        );
        Ok(record.user)
    }

    pub async fn update_user(&self, user_id: Uuid, params: UpdateUserParams) -> AppResult<User> {
        let command = UpdateUserCommand {
            display_name: params.display_name,
            email: params.email,
            avatar_url: params.avatar_url,
            is_active: params.is_active,
            role_ids: params.role_ids,
            updated_by: params.updated_by,
        };

        let record = self
            .admin()
            .update_user(user_id, command)
            .await
            .map_err(map_admin_error)?;

        info!(
            target: "user.service",
            user_id = %record.user.id,
            username = %record.user.username,
            action = "update"
        );
        Ok(record.user)
    }

    pub async fn delete_user(&self, user_id: Uuid, deleted_by: Uuid) -> AppResult<()> {
        self.admin()
            .delete_user(DeleteUserCommand {
                user_id,
                deleted_by,
                check_admin: true,
            })
            .await
            .map_err(map_admin_error)
    }

    pub async fn needs_setup(&self) -> AppResult<bool> {
        self.admin()
            .needs_initial_setup()
            .await
            .map_err(map_admin_error)
    }

    pub async fn assign_role(
        &self,
        user_id: Uuid,
        role_id: Uuid,
        assigned_by: Uuid,
    ) -> AppResult<()> {
        self.state
            .unit_of_work
            .rbac
            .assign_user_role(user_id, role_id, assigned_by)
            .await
            .map_err(AppError::from)
    }

    pub async fn remove_role(
        &self,
        user_id: Uuid,
        role_id: Uuid,
        removed_by: Uuid,
    ) -> AppResult<()> {
        let admin_role = self
            .state
            .unit_of_work
            .rbac
            .get_all_roles()
            .await
            .map_err(AppError::from)?
            .into_iter()
            .find(|role| role.name == "admin");

        let check_last_admin = admin_role.as_ref().map(|role| role.id) == Some(role_id);

        self.state
            .unit_of_work
            .rbac
            .remove_user_role_atomic(user_id, role_id, check_last_admin)
            .await
            .map_err(AppError::from)?;

        info!(
            target: "user.service",
            role_id = %role_id,
            user_id = %user_id,
            actor = %removed_by,
            action = "remove-role"
        );
        Ok(())
    }

    pub fn validate_username(username: &str) -> Result<(), String> {
        UserAdministrationService::validate_username(username).map_err(|err| match err {
            UserAdminError::Validation(message) => message,
            other => other.to_string(),
        })
    }
}

fn map_admin_error(err: UserAdminError) -> AppError {
    match err {
        UserAdminError::UserNotFound => AppError::not_found("User not found"),
        UserAdminError::UsernameExists => AppError::conflict("Username already exists"),
        UserAdminError::EmailExists => AppError::conflict("Email already exists"),
        UserAdminError::Validation(message) => AppError::bad_request(message),
        UserAdminError::PermissionDenied(message) => AppError::forbidden(message),
        UserAdminError::Internal(message) => AppError::internal(message),
    }
}
