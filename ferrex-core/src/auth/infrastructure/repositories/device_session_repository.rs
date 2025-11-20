use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Row};
use std::{fmt, sync::Arc};
use uuid::Uuid;

use crate::auth::AuthCrypto;
use crate::auth::domain::aggregates::{DeviceSession, DeviceStatus};
use crate::auth::domain::repositories::{DevicePinStatus, DeviceSessionRepository};
use crate::auth::domain::value_objects::{
    DeviceFingerprint, RevocationReason, SessionScope, SessionToken,
};

#[derive(sqlx::FromRow, Debug)]
struct DeviceSessionRecord {
    id: Uuid,
    user_id: Uuid,
    device_fingerprint: String,
    device_name: String,
    device_public_key: Option<String>,
    device_key_alg: Option<String>,
    status: String,
    pin_configured: bool,
    failed_attempts: i16,
    created_at: DateTime<Utc>,
    last_activity: DateTime<Utc>,
    #[allow(dead_code)]
    updated_at: DateTime<Utc>,
    session_token_hash: Option<String>,
    session_created_at: Option<DateTime<Utc>>,
    session_expires_at: Option<DateTime<Utc>>,
}

pub struct PostgresDeviceSessionRepository {
    pool: PgPool,
    crypto: Arc<AuthCrypto>,
}

impl fmt::Debug for PostgresDeviceSessionRepository {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PostgresDeviceSessionRepository").finish()
    }
}

impl PostgresDeviceSessionRepository {
    pub fn new(pool: PgPool, crypto: Arc<AuthCrypto>) -> Self {
        Self { pool, crypto }
    }
}

#[async_trait]
impl DeviceSessionRepository for PostgresDeviceSessionRepository {
    async fn find_by_id(&self, session_id: Uuid) -> Result<Option<DeviceSession>> {
        let record = sqlx::query_as!(
            DeviceSessionRecord,
            r#"
            SELECT
                ds.id,
                ds.user_id,
                ds.device_fingerprint,
                ds.device_name,
                ds.device_public_key,
                ds.device_key_alg::text AS device_key_alg,
                ds.status::text AS "status!",
                (uc.pin_hash IS NOT NULL) AS "pin_configured!",
                ds.failed_attempts,
                ds.created_at,
                ds.last_activity,
                ds.updated_at,
                sess.session_token_hash,
                sess.created_at AS session_created_at,
                sess.expires_at AS session_expires_at
            FROM auth_device_sessions ds
            INNER JOIN user_credentials uc ON uc.user_id = ds.user_id
            LEFT JOIN LATERAL (
                SELECT
                    s.session_token_hash,
                    s.created_at,
                    s.expires_at
                FROM auth_sessions s
                WHERE s.device_session_id = ds.id
                  AND s.revoked = FALSE
                  AND s.expires_at > NOW()
                ORDER BY s.created_at DESC
                LIMIT 1
            ) sess ON TRUE
            WHERE ds.id = $1
            "#,
            session_id
        )
        .fetch_optional(&self.pool)
        .await?;

        record.map(|row| self.hydrate_session(row)).transpose()
    }

    async fn find_by_user_and_fingerprint(
        &self,
        user_id: Uuid,
        fingerprint: &DeviceFingerprint,
    ) -> Result<Option<DeviceSession>> {
        let record = sqlx::query_as!(
            DeviceSessionRecord,
            r#"
            SELECT
                ds.id,
                ds.user_id,
                ds.device_fingerprint,
                ds.device_name,
                ds.device_public_key,
                ds.device_key_alg::text AS device_key_alg,
                ds.status::text AS "status!",
                (uc.pin_hash IS NOT NULL) AS "pin_configured!",
                ds.failed_attempts,
                ds.created_at,
                ds.last_activity,
                ds.updated_at,
                sess.session_token_hash,
                sess.created_at AS session_created_at,
                sess.expires_at AS session_expires_at
            FROM auth_device_sessions ds
            INNER JOIN user_credentials uc ON uc.user_id = ds.user_id
            LEFT JOIN LATERAL (
                SELECT
                    s.session_token_hash,
                    s.created_at,
                    s.expires_at
                FROM auth_sessions s
                WHERE s.device_session_id = ds.id
                  AND s.revoked = FALSE
                  AND s.expires_at > NOW()
                ORDER BY s.created_at DESC
                LIMIT 1
            ) sess ON TRUE
            WHERE ds.user_id = $1
              AND ds.device_fingerprint = $2
            "#,
            user_id,
            fingerprint.as_str()
        )
        .fetch_optional(&self.pool)
        .await?;

        record.map(|row| self.hydrate_session(row)).transpose()
    }

    async fn find_by_user_id(&self, user_id: Uuid) -> Result<Vec<DeviceSession>> {
        let rows = sqlx::query_as!(
            DeviceSessionRecord,
            r#"
            SELECT
                ds.id,
                ds.user_id,
                ds.device_fingerprint,
                ds.device_name,
                ds.device_public_key,
                ds.device_key_alg::text AS device_key_alg,
                ds.status::text AS "status!",
                (uc.pin_hash IS NOT NULL) AS "pin_configured!",
                ds.failed_attempts,
                ds.created_at,
                ds.last_activity,
                ds.updated_at,
                sess.session_token_hash,
                sess.created_at AS session_created_at,
                sess.expires_at AS session_expires_at
            FROM auth_device_sessions ds
            INNER JOIN user_credentials uc ON uc.user_id = ds.user_id
            LEFT JOIN LATERAL (
                SELECT
                    s.session_token_hash,
                    s.created_at,
                    s.expires_at
                FROM auth_sessions s
                WHERE s.device_session_id = ds.id
                  AND s.revoked = FALSE
                  AND s.expires_at > NOW()
                ORDER BY s.created_at DESC
                LIMIT 1
            ) sess ON TRUE
            WHERE ds.user_id = $1
            ORDER BY ds.created_at DESC
            "#,
            user_id
        )
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter()
            .map(|row| self.hydrate_session(row))
            .collect()
    }

    async fn save(&self, session: &DeviceSession) -> Result<Option<Uuid>> {
        let status = status_to_db(session.status());
        let failed_attempts: i16 = session
            .failed_attempts()
            .try_into()
            .context("failed attempts exceeds database representation")?;

            sqlx::query!(
                r#"
                INSERT INTO auth_device_sessions (
                id,
                user_id,
                device_fingerprint,
                device_name,
                device_public_key,
                device_key_alg,
                status,
                failed_attempts,
                first_authenticated_by,
                first_authenticated_at,
                last_seen_at,
                last_activity,
                created_at
            ) VALUES (
                $1,
                $2,
                $3,
                $4,
                $5,
                ($6)::text::auth_device_key_alg,
                ($7)::text::auth_device_status,
                $8,
                $2,
                $9,
                $10,
                $11,
                $12
                )
            ON CONFLICT (id) DO UPDATE SET
                device_name = EXCLUDED.device_name,
                device_public_key = COALESCE(EXCLUDED.device_public_key, auth_device_sessions.device_public_key),
                device_key_alg = COALESCE(EXCLUDED.device_key_alg, auth_device_sessions.device_key_alg),
                status = EXCLUDED.status,
                failed_attempts = EXCLUDED.failed_attempts,
                last_seen_at = EXCLUDED.last_seen_at,
                last_activity = EXCLUDED.last_activity,
                updated_at = NOW(),
                revoked_at = CASE
                    WHEN EXCLUDED.status = 'revoked'::auth_device_status THEN
                        COALESCE(auth_device_sessions.revoked_at, NOW())
                    ELSE NULL
                END,
                revoked_by = CASE
                    WHEN EXCLUDED.status = 'revoked'::auth_device_status THEN auth_device_sessions.revoked_by
                    ELSE NULL
                END,
                revoked_reason = CASE
                    WHEN EXCLUDED.status = 'revoked'::auth_device_status THEN auth_device_sessions.revoked_reason
                    ELSE NULL
                END
            "#,
            session.id(),
            session.user_id(),
            session.device_fingerprint().as_str(),
            session.device_name(),
            session.device_public_key(),
            session.device_key_alg(),
            status,
            failed_attempts,
            session.created_at(),
            session.last_activity(),
            session.created_at(),
            session.created_at()
        )
        .execute(&self.pool)
        .await?;

        let mut persisted_session_id = None;

        if matches!(session.status(), DeviceStatus::Revoked) {
            let reason = RevocationReason::DeviceRevoked.as_str();
            sqlx::query!(
                r#"
                UPDATE auth_sessions
                SET revoked = TRUE,
                    revoked_at = NOW(),
                    revoked_reason = COALESCE(revoked_reason, $2)
                WHERE device_session_id = $1
                  AND revoked = FALSE
                "#,
                session.id(),
                reason
            )
            .execute(&self.pool)
            .await?;
        } else if let Some(token) = session.session_token() {
            let token_hash = if is_hex_digest(token.as_str()) {
                token.as_str().to_string()
            } else {
                self.crypto.hash_token(token.as_str())
            };

            sqlx::query!(
                r#"
                UPDATE auth_sessions
                SET revoked = TRUE,
                    revoked_at = NOW(),
                    revoked_reason = COALESCE(revoked_reason, $2)
                WHERE device_session_id = $1
                  AND revoked = FALSE
                "#,
                session.id(),
                RevocationReason::SessionReplaced.as_str()
            )
            .execute(&self.pool)
            .await?;

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
                ON CONFLICT (session_token_hash) DO UPDATE
                SET expires_at = EXCLUDED.expires_at,
                    last_activity = EXCLUDED.last_activity,
                    scope = EXCLUDED.scope,
                    revoked = FALSE,
                    revoked_at = NULL,
                    revoked_reason = NULL,
                    metadata = EXCLUDED.metadata
                RETURNING id
                "#,
                session.user_id(),
                session.id(),
                SessionScope::Playback.as_str(),
                token_hash,
                token.created_at(),
                token.expires_at(),
                session.last_activity()
            )
            .fetch_one(&self.pool)
            .await?;

            persisted_session_id = Some(record.id);
        }

        Ok(persisted_session_id)
    }

    async fn exists_by_fingerprint(&self, fingerprint: &DeviceFingerprint) -> Result<bool> {
        let exists = sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS(
                SELECT 1
                FROM auth_device_sessions
                WHERE device_fingerprint = $1
            )
            "#,
        )
        .bind(fingerprint.as_str())
        .fetch_one(&self.pool)
        .await?;

        Ok(exists)
    }

    async fn pin_status_by_fingerprint(
        &self,
        fingerprint: &DeviceFingerprint,
    ) -> Result<Vec<DevicePinStatus>> {
        let rows = sqlx::query(
            r#"
            SELECT
                ds.user_id,
                (uc.pin_hash IS NOT NULL) AS has_pin
            FROM auth_device_sessions ds
            INNER JOIN user_credentials uc ON uc.user_id = ds.user_id
            WHERE ds.device_fingerprint = $1
            "#,
        )
        .bind(fingerprint.as_str())
        .fetch_all(&self.pool)
        .await?;

        let mut statuses = Vec::with_capacity(rows.len());
        for row in rows {
            let user_id: Uuid = row.try_get("user_id")?;
            let has_pin: bool = row.try_get("has_pin")?;
            statuses.push(DevicePinStatus { user_id, has_pin });
        }

        Ok(statuses)
    }
}

impl PostgresDeviceSessionRepository {
    fn hydrate_session(&self, row: DeviceSessionRecord) -> Result<DeviceSession> {
        let fingerprint = DeviceFingerprint::from_hash(row.device_fingerprint)
            .context("invalid device fingerprint stored in database")?;

        let session_token = match (
            row.session_token_hash,
            row.session_created_at,
            row.session_expires_at,
        ) {
            (Some(token), Some(created_at), Some(expires_at)) => Some(
                SessionToken::from_value(token, created_at, expires_at)
                    .context("failed to hydrate session token from database row")?,
            ),
            _ => None,
        };

        let status =
            status_from_db(&row.status).context("invalid auth_device_sessions.status value")?;

        let failed_attempts: u8 = row
            .failed_attempts
            .try_into()
            .context("failed_attempts exceeds u8 range")?;

        Ok(DeviceSession::hydrate(
            row.id,
            row.user_id,
            fingerprint,
            row.device_name,
            status,
            row.pin_configured,
            session_token,
            failed_attempts,
            row.created_at,
            row.last_activity,
        ))
        .map(|mut s| {
            if let Some(alg) = row.device_key_alg.as_ref() {
                if let Some(pk) = row.device_public_key.as_ref() {
                    s.set_device_public_key(alg.clone(), pk.clone());
                }
            }
            s
        })
    }
}

fn is_hex_digest(candidate: &str) -> bool {
    candidate.len() == 64 && candidate.chars().all(|c| c.is_ascii_hexdigit())
}

fn status_from_db(value: &str) -> Result<crate::auth::domain::aggregates::DeviceStatus> {
    use crate::auth::domain::aggregates::DeviceStatus;

    match value {
        "pending" => Ok(DeviceStatus::Pending),
        "trusted" => Ok(DeviceStatus::Trusted),
        "revoked" => Ok(DeviceStatus::Revoked),
        other => anyhow::bail!("unknown device session status: {other}"),
    }
}

fn status_to_db(status: crate::auth::domain::aggregates::DeviceStatus) -> &'static str {
    use crate::auth::domain::aggregates::DeviceStatus;

    match status {
        DeviceStatus::Pending => "pending",
        DeviceStatus::Trusted => "trusted",
        DeviceStatus::Revoked => "revoked",
    }
}
