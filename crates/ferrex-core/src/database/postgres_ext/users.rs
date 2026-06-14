use crate::database::PostgresDatabase;
use crate::database::repository_ports::rbac::RbacRepository;
use crate::database::repository_ports::users::UsersRepository;
use crate::domain::users::user::User;
use crate::error::Result;
use uuid::Uuid;

/// User management and authentication extensions for PostgresDatabase
impl PostgresDatabase {
    pub async fn create_user(
        &self,
        user: &User,
        password_hash: &str,
    ) -> Result<()> {
        self.users_repository()
            .create_user_with_password(user, password_hash)
            .await
    }

    pub async fn get_user_by_id(&self, id: Uuid) -> Result<Option<User>> {
        self.users_repository().get_user_by_id(id).await
    }

    pub async fn get_user_by_username(
        &self,
        username: &str,
    ) -> Result<Option<User>> {
        self.users_repository().get_user_by_username(username).await
    }

    pub async fn get_all_users(&self) -> Result<Vec<User>> {
        self.users_repository().get_all_users().await
    }

    pub async fn update_user(&self, user: &User) -> Result<()> {
        self.users_repository().update_user(user).await
    }

    /// Get password hash for a user
    pub async fn get_user_password_hash(
        &self,
        user_id: Uuid,
    ) -> Result<Option<String>> {
        self.users_repository()
            .get_user_password_hash(user_id)
            .await
    }

    /// Update user password
    pub async fn update_user_password(
        &self,
        user_id: Uuid,
        password_hash: &str,
    ) -> Result<()> {
        self.users_repository()
            .update_user_password(user_id, password_hash)
            .await
    }

    pub async fn delete_user(&self, id: Uuid) -> Result<()> {
        self.users_repository().delete_user(id).await
    }

    /// Delete user with atomic check for last admin
    pub async fn delete_user_atomic(
        &self,
        user_id: Uuid,
        check_last_admin: bool,
    ) -> Result<()> {
        self.users_repository()
            .delete_user_atomic(user_id, check_last_admin)
            .await
    }

    /// Get count of admin users with optional exclusion
    pub async fn get_admin_count(
        &self,
        exclude_user_id: Option<Uuid>,
    ) -> Result<usize> {
        self.rbac_repository()
            .get_admin_count(exclude_user_id)
            .await
    }

    /// Check if a user has a specific role efficiently
    pub async fn user_has_role(
        &self,
        user_id: Uuid,
        role_name: &str,
    ) -> Result<bool> {
        self.rbac_repository()
            .user_has_role(user_id, role_name)
            .await
    }

    /// Get all users with a specific role
    pub async fn get_users_with_role(
        &self,
        role_name: &str,
    ) -> Result<Vec<Uuid>> {
        self.rbac_repository().get_users_with_role(role_name).await
    }
}
