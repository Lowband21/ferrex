use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use std::fmt;
use uuid::Uuid;

use crate::auth::domain::repositories::AuthSessionRepository;

pub struct PostgresAuthSessionRepository {
    pool: PgPool,
}

impl fmt::Debug for PostgresAuthSessionRepository {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PostgresAuthSessionRepository").finish()
    }
}

impl PostgresAuthSessionRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl AuthSessionRepository for PostgresAuthSessionRepository {
    async fn insert_session(
        &self,
        user_id: Uuid,
        device_session_id: Option<Uuid>,
        token_hash: &str,
        created_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
    ) -> Result<Uuid> {
        let record = sqlx::query!(
            r#"
            INSERT INTO auth_sessions (
                user_id,
                device_session_id,
                session_token_hash,
                created_at,
                expires_at,
                last_activity,
                metadata
            )
            VALUES ($1, $2, $3, $4, $5, $6, '{}'::jsonb)
            RETURNING id
            "#,
            user_id,
            device_session_id,
            token_hash,
            created_at,
            expires_at,
            created_at
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(record.id)
    }

    async fn revoke_by_hash(&self, token_hash: &str, reason: &str) -> Result<()> {
        sqlx::query!(
            r#"
            UPDATE auth_sessions
            SET revoked = TRUE,
                revoked_at = NOW(),
                revoked_reason = COALESCE(revoked_reason, $2)
            WHERE session_token_hash = $1
              AND revoked = FALSE
            "#,
            token_hash,
            reason
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}
