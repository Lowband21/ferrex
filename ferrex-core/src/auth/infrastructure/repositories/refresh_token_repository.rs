use anyhow::{Context, Result};
use async_trait::async_trait;
use sqlx::PgPool;
use std::fmt;
use uuid::Uuid;

use crate::auth::domain::repositories::{RefreshTokenRecord, RefreshTokenRepository};
use crate::auth::domain::value_objects::RefreshToken;
use crate::auth::domain::value_objects::RevocationReason;

pub struct PostgresRefreshTokenRepository {
    pool: PgPool,
}

impl fmt::Debug for PostgresRefreshTokenRepository {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PostgresRefreshTokenRepository").finish()
    }
}

impl PostgresRefreshTokenRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl RefreshTokenRepository for PostgresRefreshTokenRepository {
    async fn insert_refresh_token(
        &self,
        token_hash: &str,
        user_id: Uuid,
        device_session_id: Option<Uuid>,
        session_id: Option<Uuid>,
        issued_at: chrono::DateTime<chrono::Utc>,
        expires_at: chrono::DateTime<chrono::Utc>,
        family_id: Uuid,
        generation: i32,
    ) -> Result<Uuid> {
        let record = sqlx::query!(
            r#"
            INSERT INTO auth_refresh_tokens (
                user_id,
                device_session_id,
                session_id,
                token_hash,
                issued_at,
                expires_at,
                metadata,
                device_name,
                family_id,
                generation
            )
            VALUES ($1, $2, $3, $4, $5, $6, '{}'::jsonb, NULL, $7, $8)
            RETURNING id
            "#,
            user_id,
            device_session_id,
            session_id,
            token_hash,
            issued_at,
            expires_at,
            family_id,
            generation
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(record.id)
    }

    async fn get_active_refresh_token(
        &self,
        token_hash: &str,
    ) -> Result<Option<RefreshTokenRecord>> {
        let row = sqlx::query!(
            r#"
            SELECT
                id,
                user_id,
                device_session_id,
                session_id,
                token_hash,
                issued_at,
                expires_at,
                revoked,
                revoked_reason,
                family_id,
                generation AS "generation?",
                used_count AS "used_count?"
            FROM auth_refresh_tokens
            WHERE token_hash = $1
            "#,
            token_hash
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row
            .map(|row| -> Result<_> {
                let family_id = row.family_id.context("refresh token missing family_id")?;

                let generation: u32 = row
                    .generation
                    .unwrap_or(1)
                    .try_into()
                    .context("refresh token generation overflow")?;

                let token = RefreshToken::from_value(
                    row.token_hash,
                    row.issued_at,
                    row.expires_at,
                    family_id,
                    generation,
                )
                .context("failed to hydrate refresh token from database")?;

                Ok(RefreshTokenRecord {
                    id: row.id,
                    user_id: row.user_id,
                    device_session_id: row.device_session_id,
                    session_id: row.session_id,
                    token,
                    revoked: row.revoked,
                    revoked_reason: row.revoked_reason,
                    used_count: row.used_count.unwrap_or_default(),
                })
            })
            .transpose()?)
    }

    async fn mark_used(&self, token_id: Uuid, reason: RevocationReason) -> Result<()> {
        sqlx::query!(
            r#"
            UPDATE auth_refresh_tokens
            SET used_at = NOW(),
                used_count = used_count + 1,
                revoked = TRUE,
                revoked_at = NOW(),
                revoked_reason = COALESCE(revoked_reason, $2)
            WHERE id = $1
            "#,
            token_id,
            reason.as_str()
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn revoke_family(&self, family_id: Uuid, reason: RevocationReason) -> Result<()> {
        sqlx::query!(
            r#"
            UPDATE auth_refresh_tokens
            SET revoked = TRUE,
                revoked_at = NOW(),
                revoked_reason = COALESCE(revoked_reason, $2)
            WHERE family_id = $1 AND revoked = FALSE
            "#,
            family_id,
            reason.as_str()
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn revoke_for_user(&self, user_id: Uuid, reason: RevocationReason) -> Result<()> {
        sqlx::query!(
            r#"
            UPDATE auth_refresh_tokens
            SET revoked = TRUE,
                revoked_at = NOW(),
                revoked_reason = COALESCE(revoked_reason, $2)
            WHERE user_id = $1
              AND revoked = FALSE
            "#,
            user_id,
            reason.as_str()
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn revoke_for_device(
        &self,
        device_session_id: Uuid,
        reason: RevocationReason,
    ) -> Result<()> {
        sqlx::query!(
            r#"
            UPDATE auth_refresh_tokens
            SET revoked = TRUE,
                revoked_at = NOW(),
                revoked_reason = COALESCE(revoked_reason, $2)
            WHERE device_session_id = $1
              AND revoked = FALSE
            "#,
            device_session_id,
            reason.as_str()
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}
