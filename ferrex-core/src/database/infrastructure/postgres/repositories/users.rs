use async_trait::async_trait;
use sqlx::PgPool;
use tracing::info;
use uuid::Uuid;

use crate::database::ports::users::UsersRepository;
use crate::{
    error::{MediaError, Result},
    user::User,
};

/// PostgreSQL-backed implementation of the `UsersRepository` port.
#[derive(Clone, Debug)]
pub struct PostgresUsersRepository {
    pool: PgPool,
}

impl PostgresUsersRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    fn pool(&self) -> &PgPool {
        &self.pool
    }
}

#[async_trait]
impl UsersRepository for PostgresUsersRepository {
    async fn create_user_with_password(&self, user: &User, password_hash: &str) -> Result<()> {
        let mut tx = self
            .pool()
            .begin()
            .await
            .map_err(|e| MediaError::Internal(format!("Failed to start transaction: {}", e)))?;

        // Insert user
        sqlx::query!(
            r#"
            INSERT INTO users (
                id, username, display_name, avatar_url, 
                created_at, updated_at, last_login, is_active, 
                email, preferences
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            "#,
            user.id,
            user.username,
            user.display_name,
            user.avatar_url,
            user.created_at,
            user.updated_at,
            user.last_login,
            user.is_active,
            user.email,
            serde_json::to_value(&user.preferences).unwrap()
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            if let Some(db_err) = e.as_database_error() {
                if db_err.constraint() == Some("users_username_key") {
                    return MediaError::Conflict("Username already exists".to_string());
                }
                if db_err.constraint() == Some("idx_users_email_unique") {
                    return MediaError::Conflict("Email already exists".to_string());
                }
            }
            MediaError::Internal(format!("Failed to create user: {}", e))
        })?;

        // Insert password hash
        sqlx::query!(
            r#"
            INSERT INTO user_credentials (user_id, password_hash)
            VALUES ($1, $2)
            "#,
            user.id,
            password_hash
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to store password: {}", e)))?;

        tx.commit()
            .await
            .map_err(|e| MediaError::Internal(format!("Failed to commit transaction: {}", e)))?;

        info!("Created user: {} ({})", user.username, user.id);
        Ok(())
    }

    async fn get_user_by_id(&self, id: Uuid) -> Result<Option<User>> {
        let row = sqlx::query!(
            r#"
            SELECT 
                id, username, display_name, avatar_url,
                created_at, updated_at, last_login, is_active,
                email, preferences
            FROM users
            WHERE id = $1
            "#,
            id
        )
        .fetch_optional(self.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to get user by id: {}", e)))?;

        Ok(row.map(|r| User {
            id: r.id,
            username: r.username,
            display_name: r.display_name,
            avatar_url: r.avatar_url,
            created_at: r.created_at,
            updated_at: r.updated_at,
            last_login: r.last_login,
            is_active: r.is_active,
            email: r.email,
            preferences: serde_json::from_value(r.preferences).unwrap_or_default(),
        }))
    }

    async fn get_user_by_username(&self, username: &str) -> Result<Option<User>> {
        let row = sqlx::query!(
            r#"
            SELECT 
                id, username, display_name, avatar_url,
                created_at, updated_at, last_login, is_active,
                email, preferences
            FROM users
            WHERE username = $1
            "#,
            username
        )
        .fetch_optional(self.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to get user by username: {}", e)))?;

        Ok(row.map(|r| User {
            id: r.id,
            username: r.username,
            display_name: r.display_name,
            avatar_url: r.avatar_url,
            created_at: r.created_at,
            updated_at: r.updated_at,
            last_login: r.last_login,
            is_active: r.is_active,
            email: r.email,
            preferences: serde_json::from_value(r.preferences).unwrap_or_default(),
        }))
    }

    async fn get_all_users(&self) -> Result<Vec<User>> {
        let rows = sqlx::query!(
            r#"
            SELECT 
                id, username, display_name, avatar_url,
                created_at, updated_at, last_login, is_active,
                email, preferences
            FROM users
            ORDER BY display_name, username
            "#
        )
        .fetch_all(self.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to get all users: {}", e)))?;

        let users: Vec<User> = rows
            .into_iter()
            .map(|r| User {
                id: r.id,
                username: r.username,
                display_name: r.display_name,
                avatar_url: r.avatar_url,
                created_at: r.created_at,
                updated_at: r.updated_at,
                last_login: r.last_login,
                is_active: r.is_active,
                email: r.email,
                preferences: serde_json::from_value(r.preferences).unwrap_or_default(),
            })
            .collect();

        info!("Retrieved {} users", users.len());
        Ok(users)
    }

    async fn update_user(&self, user: &User) -> Result<()> {
        let result = sqlx::query!(
            r#"
            UPDATE users 
            SET display_name = $2, avatar_url = $3, email = $4, 
                is_active = $5, preferences = $6, updated_at = NOW()
            WHERE id = $1
            "#,
            user.id,
            user.display_name,
            user.avatar_url,
            user.email,
            user.is_active,
            serde_json::to_value(&user.preferences).unwrap()
        )
        .execute(self.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to update user: {}", e)))?;

        if result.rows_affected() == 0 {
            return Err(MediaError::NotFound("User not found".to_string()));
        }

        info!("Updated user: {} ({})", user.username, user.id);
        Ok(())
    }

    /// Get password hash for a user
    async fn get_user_password_hash(&self, user_id: Uuid) -> Result<Option<String>> {
        let row = sqlx::query!(
            r#"
            SELECT password_hash
            FROM user_credentials
            WHERE user_id = $1
            "#,
            user_id
        )
        .fetch_optional(self.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to get password hash: {}", e)))?;

        Ok(row.map(|r| r.password_hash))
    }

    /// Update user password
    async fn update_user_password(&self, user_id: Uuid, password_hash: &str) -> Result<()> {
        let result = sqlx::query!(
            r#"
            INSERT INTO user_credentials (user_id, password_hash, updated_at)
            VALUES ($1, $2, NOW())
            ON CONFLICT (user_id) DO UPDATE
            SET password_hash = EXCLUDED.password_hash,
                updated_at = EXCLUDED.updated_at
            "#,
            user_id,
            password_hash
        )
        .execute(self.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to update password: {}", e)))?;

        if result.rows_affected() == 0 {
            return Err(MediaError::NotFound("User not found".to_string()));
        }

        info!("Updated password for user: {}", user_id);
        Ok(())
    }

    async fn delete_user(&self, id: Uuid) -> Result<()> {
        // Start a transaction to ensure all deletions happen atomically
        let mut tx = self
            .pool()
            .begin()
            .await
            .map_err(|e| MediaError::Internal(format!("Failed to start transaction: {}", e)))?;

        // First, deactivate any sync sessions where the user is the host
        sqlx::query!(
            "UPDATE sync_sessions SET is_active = false WHERE host_id = $1",
            id
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to deactivate sync sessions: {}", e)))?;

        // Remove user from any sync sessions they're participating in
        sqlx::query!("DELETE FROM sync_participants WHERE user_id = $1", id)
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                MediaError::Internal(format!("Failed to remove from sync sessions: {}", e))
            })?;

        // Delete user watch progress
        sqlx::query!("DELETE FROM user_watch_progress WHERE user_id = $1", id)
            .execute(&mut *tx)
            .await
            .map_err(|e| MediaError::Internal(format!("Failed to delete watch progress: {}", e)))?;

        // Delete completed media records
        sqlx::query!("DELETE FROM user_completed_media WHERE user_id = $1", id)
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                MediaError::Internal(format!("Failed to delete completed media: {}", e))
            })?;

        // Delete user sessions
        sqlx::query!("DELETE FROM auth_sessions WHERE user_id = $1", id)
            .execute(&mut *tx)
            .await
            .map_err(|e| MediaError::Internal(format!("Failed to delete user sessions: {}", e)))?;

        // Delete refresh tokens
        sqlx::query!("DELETE FROM auth_refresh_tokens WHERE user_id = $1", id)
            .execute(&mut *tx)
            .await
            .map_err(|e| MediaError::Internal(format!("Failed to delete refresh tokens: {}", e)))?;

        // Finally, delete the user
        let result = sqlx::query!("DELETE FROM users WHERE id = $1", id)
            .execute(&mut *tx)
            .await
            .map_err(|e| MediaError::Internal(format!("Failed to delete user: {}", e)))?;

        if result.rows_affected() == 0 {
            return Err(MediaError::NotFound("User not found".to_string()));
        }

        // Commit the transaction
        tx.commit()
            .await
            .map_err(|e| MediaError::Internal(format!("Failed to commit transaction: {}", e)))?;

        info!("Deleted user: {} and all associated data", id);
        Ok(())
    }

    /// Delete user with atomic check for last admin
    async fn delete_user_atomic(&self, user_id: Uuid, check_last_admin: bool) -> Result<()> {
        let mut tx = self
            .pool()
            .begin()
            .await
            .map_err(|e| MediaError::Internal(format!("Failed to start transaction: {}", e)))?;

        if check_last_admin {
            // First, lock all admin users except the one being deleted
            let admin_users: Vec<Uuid> = sqlx::query_scalar!(
                r#"
                SELECT u.id
                FROM users u
                INNER JOIN user_roles ur ON u.id = ur.user_id
                INNER JOIN roles r ON ur.role_id = r.id
                WHERE r.name = 'admin'
                AND u.id != $1
                FOR UPDATE OF u
                "#,
                user_id
            )
            .fetch_all(&mut *tx)
            .await
            .map_err(|e| MediaError::Internal(format!("Failed to lock admin users: {}", e)))?;

            if admin_users.is_empty() {
                tx.rollback().await.map_err(|e| {
                    MediaError::Internal(format!("Failed to rollback transaction: {}", e))
                })?;
                return Err(MediaError::Conflict(
                    "Cannot delete the last admin user".to_string(),
                ));
            }
        }

        // Deactivate sync sessions where user is host
        sqlx::query!(
            "UPDATE sync_sessions SET is_active = false WHERE host_id = $1",
            user_id
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to deactivate sync sessions: {}", e)))?;

        // Remove user from sync sessions
        sqlx::query!("DELETE FROM sync_participants WHERE user_id = $1", user_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                MediaError::Internal(format!("Failed to remove from sync sessions: {}", e))
            })?;

        // Delete user watch progress
        sqlx::query!(
            "DELETE FROM user_watch_progress WHERE user_id = $1",
            user_id
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to delete watch progress: {}", e)))?;

        // Delete completed media records
        sqlx::query!(
            "DELETE FROM user_completed_media WHERE user_id = $1",
            user_id
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to delete completed media: {}", e)))?;

        // Delete user sessions
        sqlx::query!("DELETE FROM auth_sessions WHERE user_id = $1", user_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| MediaError::Internal(format!("Failed to delete user sessions: {}", e)))?;

        // Delete refresh tokens
        sqlx::query!(
            "DELETE FROM auth_refresh_tokens WHERE user_id = $1",
            user_id
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to delete refresh tokens: {}", e)))?;

        // Delete the user
        let result = sqlx::query!("DELETE FROM users WHERE id = $1", user_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| MediaError::Internal(format!("Failed to delete user: {}", e)))?;

        if result.rows_affected() == 0 {
            tx.rollback().await.map_err(|e| {
                MediaError::Internal(format!("Failed to rollback transaction: {}", e))
            })?;
            return Err(MediaError::NotFound("User not found".to_string()));
        }

        tx.commit()
            .await
            .map_err(|e| MediaError::Internal(format!("Failed to commit transaction: {}", e)))?;

        info!(
            "Atomically deleted user: {} with last admin check: {}",
            user_id, check_last_admin
        );
        Ok(())
    }
}
