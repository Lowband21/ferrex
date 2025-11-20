use anyhow::Result;
use async_trait::async_trait;
use sqlx::PgPool;
use std::fmt;
use uuid::Uuid;

use crate::auth::domain::aggregates::UserAuthentication;
use crate::auth::domain::repositories::UserAuthenticationRepository;
use crate::auth::domain::value_objects::PinCode;
use std::collections::HashMap;

pub struct PostgresUserAuthRepository {
    pool: PgPool,
}

impl fmt::Debug for PostgresUserAuthRepository {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PostgresUserAuthRepository").finish()
    }
}

impl PostgresUserAuthRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl UserAuthenticationRepository for PostgresUserAuthRepository {
    async fn find_by_id(
        &self,
        user_id: Uuid,
    ) -> Result<Option<UserAuthentication>> {
        let user = sqlx::query!(
            r#"
            SELECT
                u.id,
                u.username,
                u.is_active,
                u.is_locked,
                u.failed_login_attempts,
                u.locked_until,
                u.last_login,
                uc.password_hash,
                uc.pin_hash,
                uc.pin_client_salt,
                uc.pin_updated_at
            FROM users u
            INNER JOIN user_credentials uc ON u.id = uc.user_id
            WHERE u.id = $1
            "#,
            user_id
        )
        .fetch_optional(&self.pool)
        .await?;

        match user {
            Some(row) => {
                let failed_attempts =
                    row.failed_login_attempts.clamp(0, u8::MAX as i16) as u8;

                let user_auth = UserAuthentication::hydrate(
                    row.id,
                    row.username,
                    row.password_hash,
                    row.is_active,
                    row.is_locked,
                    failed_attempts,
                    row.locked_until,
                    row.pin_hash.map(PinCode::from_hash),
                    row.pin_updated_at,
                    row.pin_client_salt,
                    HashMap::new(),
                    10, // max_devices hardcoded for now
                    row.last_login,
                );
                Ok(Some(user_auth))
            }
            None => Ok(None),
        }
    }

    async fn find_by_username(
        &self,
        username: &str,
    ) -> Result<Option<UserAuthentication>> {
        let user = sqlx::query!(
            r#"
            SELECT
                u.id,
                u.username,
                u.is_active,
                u.is_locked,
                u.failed_login_attempts,
                u.locked_until,
                u.last_login,
                uc.password_hash,
                uc.pin_hash,
                uc.pin_client_salt,
                uc.pin_updated_at
            FROM users u
            INNER JOIN user_credentials uc ON u.id = uc.user_id
            WHERE u.username = $1
            "#,
            username
        )
        .fetch_optional(&self.pool)
        .await?;

        match user {
            Some(row) => {
                let failed_attempts =
                    row.failed_login_attempts.clamp(0, u8::MAX as i16) as u8;

                let user_auth = UserAuthentication::hydrate(
                    row.id,
                    row.username,
                    row.password_hash,
                    row.is_active,
                    row.is_locked,
                    failed_attempts,
                    row.locked_until,
                    row.pin_hash.map(PinCode::from_hash),
                    row.pin_updated_at,
                    row.pin_client_salt,
                    HashMap::new(),
                    10, // max_devices hardcoded for now
                    row.last_login,
                );
                Ok(Some(user_auth))
            }
            None => Ok(None),
        }
    }

    async fn save(&self, user_auth: &UserAuthentication) -> Result<()> {
        // For now, we only update the password hash if it has changed
        // Full implementation would handle all fields and device sessions

        let user_id = user_auth.user_id();

        sqlx::query!(
            r#"
            UPDATE user_credentials
            SET password_hash = $2,
                pin_hash = $3,
                pin_client_salt = $4,
                pin_updated_at = $5,
                updated_at = NOW()
            WHERE user_id = $1
            "#,
            user_id,
            user_auth.password_hash(),
            user_auth.pin_hash(),
            user_auth.pin_client_salt(),
            user_auth.pin_updated_at()
        )
        .execute(&self.pool)
        .await?;

        sqlx::query!(
            r#"
            UPDATE users
            SET is_active = $2,
                is_locked = $3,
                failed_login_attempts = $4,
                locked_until = $5,
                last_login = $6,
                updated_at = NOW()
            WHERE id = $1
            "#,
            user_id,
            user_auth.is_active(),
            user_auth.is_locked(),
            i16::from(user_auth.failed_login_attempts()),
            user_auth.locked_until(),
            user_auth.last_login()
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}
