use anyhow::Result;
use async_trait::async_trait;
use sqlx::PgPool;
use std::fmt;
use std::sync::Arc;
use uuid::Uuid;

use crate::auth::domain::aggregates::UserAuthentication;
use crate::auth::domain::repositories::UserAuthenticationRepository;

pub struct PostgresUserAuthRepository {
    pool: Arc<PgPool>,
}

impl fmt::Debug for PostgresUserAuthRepository {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PostgresUserAuthRepository")
            .field("pool_refs", &Arc::strong_count(&self.pool))
            .finish()
    }
}

impl PostgresUserAuthRepository {
    pub fn new(pool: Arc<PgPool>) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl UserAuthenticationRepository for PostgresUserAuthRepository {
    async fn find_by_id(&self, user_id: Uuid) -> Result<Option<UserAuthentication>> {
        let user = sqlx::query!(
            r#"
            SELECT u.id, u.username, uc.password_hash
            FROM users u
            INNER JOIN user_credentials uc ON u.id = uc.user_id
            WHERE u.id = $1
            "#,
            user_id
        )
        .fetch_optional(&*self.pool)
        .await?;

        match user {
            Some(row) => {
                let user_auth = UserAuthentication::new(
                    row.id,
                    row.username,
                    row.password_hash,
                    10, // max_devices hardcoded for now
                );
                Ok(Some(user_auth))
            }
            None => Ok(None),
        }
    }

    async fn find_by_username(&self, username: &str) -> Result<Option<UserAuthentication>> {
        let user = sqlx::query!(
            r#"
            SELECT u.id, u.username, uc.password_hash
            FROM users u
            INNER JOIN user_credentials uc ON u.id = uc.user_id
            WHERE u.username = $1
            "#,
            username
        )
        .fetch_optional(&*self.pool)
        .await?;

        match user {
            Some(row) => {
                let user_auth = UserAuthentication::new(
                    row.id,
                    row.username,
                    row.password_hash,
                    10, // max_devices hardcoded for now
                );
                Ok(Some(user_auth))
            }
            None => Ok(None),
        }
    }

    async fn save(&self, user_auth: &UserAuthentication) -> Result<()> {
        // For now, we only update the password hash if it has changed
        // Full implementation would handle all fields and device sessions

        sqlx::query!(
            r#"
            UPDATE user_credentials
            SET password_hash = $2
            WHERE user_id = $1
            "#,
            user_auth.user_id(),
            user_auth.password_hash()
        )
        .execute(&*self.pool)
        .await?;

        Ok(())
    }
}
