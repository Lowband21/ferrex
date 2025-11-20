use crate::database::PostgresDatabase;
use crate::database::ports::users::UsersRepository;
use crate::ports::rbac::RbacRepository;
use crate::{Result, User, UserSession};
use chrono::{DateTime, Utc};
use uuid::Uuid;

/// User management and authentication extensions for PostgresDatabase
impl PostgresDatabase {
    pub async fn create_user(&self, user: &User, password_hash: &str) -> Result<()> {
        self.users_repository()
            .create_user_with_password(user, password_hash)
            .await
    }

    pub async fn get_user_by_id(&self, id: Uuid) -> Result<Option<User>> {
        self.users_repository().get_user_by_id(id).await
    }

    pub async fn get_user_by_username(&self, username: &str) -> Result<Option<User>> {
        self.users_repository().get_user_by_username(username).await
    }

    pub async fn get_all_users(&self) -> Result<Vec<User>> {
        self.users_repository().get_all_users().await
    }

    pub async fn update_user(&self, user: &User) -> Result<()> {
        self.users_repository().update_user(user).await
    }

    /// Get password hash for a user
    pub async fn get_user_password_hash(&self, user_id: Uuid) -> Result<Option<String>> {
        self.users_repository()
            .get_user_password_hash(user_id)
            .await
    }

    /// Update user password
    pub async fn update_user_password(&self, user_id: Uuid, password_hash: &str) -> Result<()> {
        self.users_repository()
            .update_user_password(user_id, password_hash)
            .await
    }

    pub async fn delete_user(&self, id: Uuid) -> Result<()> {
        self.users_repository().delete_user(id).await
    }

    /// Delete user with atomic check for last admin
    pub async fn delete_user_atomic(&self, user_id: Uuid, check_last_admin: bool) -> Result<()> {
        self.users_repository()
            .delete_user_atomic(user_id, check_last_admin)
            .await
    }

    /// Get count of admin users with optional exclusion
    pub async fn get_admin_count(&self, exclude_user_id: Option<Uuid>) -> Result<usize> {
        self.rbac_repository()
            .get_admin_count(exclude_user_id)
            .await
    }

    /// Check if a user has a specific role efficiently
    pub async fn user_has_role(&self, user_id: Uuid, role_name: &str) -> Result<bool> {
        self.rbac_repository()
            .user_has_role(user_id, role_name)
            .await
    }

    /// Get all users with a specific role
    pub async fn get_users_with_role(&self, role_name: &str) -> Result<Vec<Uuid>> {
        self.rbac_repository().get_users_with_role(role_name).await
    }

    // ==================== Authentication Methods ====================

    pub async fn store_refresh_token(
        &self,
        token: &str,
        user_id: Uuid,
        device_name: Option<String>,
        expires_at: DateTime<Utc>,
    ) -> Result<()> {
        self.users_repository()
            .store_refresh_token(token, user_id, device_name, expires_at)
            .await
    }

    pub async fn get_refresh_token(&self, token: &str) -> Result<Option<(Uuid, DateTime<Utc>)>> {
        self.users_repository().get_refresh_token(token).await
    }

    pub async fn delete_refresh_token(&self, token: &str) -> Result<()> {
        self.users_repository().delete_refresh_token(token).await
    }

    pub async fn delete_user_refresh_tokens(&self, user_id: Uuid) -> Result<()> {
        self.users_repository()
            .delete_user_refresh_tokens(user_id)
            .await
    }

    // ==================== Session Management ====================

    pub async fn create_session(&self, session: &UserSession) -> Result<()> {
        self.users_repository().create_session(session).await
    }

    pub async fn get_user_sessions(&self, user_id: Uuid) -> Result<Vec<UserSession>> {
        self.users_repository().get_user_sessions(user_id).await
    }

    pub async fn delete_session(&self, session_id: Uuid) -> Result<()> {
        self.users_repository().delete_session(session_id).await
    }
}
