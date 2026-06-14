use anyhow::{Result, anyhow};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::PgPool;
use std::fmt;
use uuid::Uuid;

use crate::domain::users::auth::domain::repositories::{
    AuthSessionRecord, AuthSessionRepository,
};
use crate::domain::users::auth::domain::value_objects::{
    RevocationReason, SessionScope,
};

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

    fn scope_from_str(&self, scope: &str) -> Result<SessionScope> {
        SessionScope::try_from(scope)
            .map_err(|_| anyhow!("invalid session scope in database"))
    }

    #[allow(clippy::too_many_arguments)]
    fn map_row(
        &self,
        id: Uuid,
        user_id: Uuid,
        device_session_id: Option<Uuid>,
        scope: String,
        created_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
        last_activity: DateTime<Utc>,
        ip_address: Option<String>,
        user_agent: Option<String>,
        metadata: Value,
        revoked: bool,
    ) -> Result<AuthSessionRecord> {
        Ok(AuthSessionRecord {
            id,
            user_id,
            device_session_id,
            scope: self.scope_from_str(scope.as_str())?,
            created_at,
            expires_at,
            last_activity,
            ip_address,
            user_agent,
            metadata,
            revoked,
        })
    }
}

const AUTH_SESSION_TOUCH_MIN_INTERVAL_SECS: i64 = 30;

#[async_trait]
impl AuthSessionRepository for PostgresAuthSessionRepository {
    async fn find_by_id(
        &self,
        session_id: Uuid,
    ) -> Result<Option<AuthSessionRecord>> {
        let record = sqlx::query!(
            r#"
            SELECT
                id,
                user_id,
                device_session_id,
                scope,
                created_at,
                expires_at,
                last_activity,
                ip_address::text AS ip_address,
                user_agent,
                metadata,
                revoked
            FROM auth_sessions
            WHERE id = $1
            "#,
            session_id
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(record
            .map(|row| {
                self.map_row(
                    row.id,
                    row.user_id,
                    row.device_session_id,
                    row.scope,
                    row.created_at,
                    row.expires_at,
                    row.last_activity,
                    row.ip_address,
                    row.user_agent,
                    row.metadata,
                    row.revoked,
                )
            })
            .transpose()?)
    }

    async fn insert_session(
        &self,
        user_id: Uuid,
        device_session_id: Option<Uuid>,
        scope: SessionScope,
        token_hash: &str,
        created_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
    ) -> Result<Uuid> {
        let record = sqlx::query!(
            r#"
            INSERT INTO auth_sessions (
                user_id,
                device_session_id,
                scope,
                session_token_hash,
                created_at,
                expires_at,
                last_activity,
                metadata
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, '{}'::jsonb)
            RETURNING id
            "#,
            user_id,
            device_session_id,
            scope.as_str(),
            token_hash,
            created_at,
            expires_at,
            created_at
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(record.id)
    }

    async fn revoke_by_hash(
        &self,
        token_hash: &str,
        reason: RevocationReason,
    ) -> Result<()> {
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
            reason.as_str()
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn find_by_hash(
        &self,
        token_hash: &str,
    ) -> Result<Option<AuthSessionRecord>> {
        let record = sqlx::query!(
            r#"
            SELECT
                id,
                user_id,
                device_session_id,
                scope,
                created_at,
                expires_at,
                last_activity,
                ip_address::text AS ip_address,
                user_agent,
                metadata,
                revoked
            FROM auth_sessions
            WHERE session_token_hash = $1
            LIMIT 1
            "#,
            token_hash
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(record
            .map(|row| {
                self.map_row(
                    row.id,
                    row.user_id,
                    row.device_session_id,
                    row.scope,
                    row.created_at,
                    row.expires_at,
                    row.last_activity,
                    row.ip_address,
                    row.user_agent,
                    row.metadata,
                    row.revoked,
                )
            })
            .transpose()?)
    }

    async fn touch(&self, session_id: Uuid) -> Result<()> {
        sqlx::query!(
            r#"
            UPDATE auth_sessions
            SET last_activity = NOW()
            WHERE id = $1
              AND last_activity < (NOW() - make_interval(secs => $2))
            "#,
            session_id,
            AUTH_SESSION_TOUCH_MIN_INTERVAL_SECS as f64
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn list_by_user(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<AuthSessionRecord>> {
        let rows = sqlx::query!(
            r#"
            SELECT
                id,
                user_id,
                device_session_id,
                scope,
                created_at,
                expires_at,
                last_activity,
                ip_address::text AS ip_address,
                user_agent,
                metadata,
                revoked
            FROM auth_sessions
            WHERE user_id = $1
            ORDER BY created_at DESC
            "#,
            user_id
        )
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter()
            .map(|row| {
                self.map_row(
                    row.id,
                    row.user_id,
                    row.device_session_id,
                    row.scope,
                    row.created_at,
                    row.expires_at,
                    row.last_activity,
                    row.ip_address,
                    row.user_agent,
                    row.metadata,
                    row.revoked,
                )
            })
            .collect()
    }

    async fn revoke_by_user(
        &self,
        user_id: Uuid,
        reason: RevocationReason,
    ) -> Result<()> {
        sqlx::query!(
            r#"
            UPDATE auth_sessions
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

    async fn revoke_by_device(
        &self,
        device_session_id: Uuid,
        reason: RevocationReason,
    ) -> Result<()> {
        sqlx::query!(
            r#"
            UPDATE auth_sessions
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

    async fn revoke_by_id(
        &self,
        session_id: Uuid,
        reason: RevocationReason,
    ) -> Result<()> {
        sqlx::query!(
            r#"
            UPDATE auth_sessions
            SET revoked = TRUE,
                revoked_at = NOW(),
                revoked_reason = COALESCE(revoked_reason, $2)
            WHERE id = $1
              AND revoked = FALSE
            "#,
            session_id,
            reason.as_str()
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}
