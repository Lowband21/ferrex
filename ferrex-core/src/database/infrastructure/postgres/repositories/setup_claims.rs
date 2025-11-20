use std::net::IpAddr;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::types::ipnetwork::IpNetwork;
use sqlx::{PgPool, Row, postgres::PgRow};
use uuid::Uuid;

use crate::database::ports::setup_claims::{
    NewSetupClaim, SetupClaimRecord, SetupClaimsRepository,
};
use crate::error::{MediaError, Result};

#[derive(Debug, Clone)]
pub struct PostgresSetupClaimsRepository {
    pool: PgPool,
}

impl PostgresSetupClaimsRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    fn pool(&self) -> &PgPool {
        &self.pool
    }

    fn map_row(row: &PgRow) -> Result<SetupClaimRecord> {
        let id: Uuid = row
            .try_get("id")
            .map_err(|e| MediaError::Internal(format!("Failed to read claim id: {e}")))?;
        let code_hash: String = row
            .try_get("code_hash")
            .map_err(|e| MediaError::Internal(format!("Failed to read claim code hash: {e}")))?;
        let claim_token_hash: Option<String> = row
            .try_get("claim_token_hash")
            .map_err(|e| MediaError::Internal(format!("Failed to read claim token hash: {e}")))?;
        let created_at: DateTime<Utc> = row
            .try_get("created_at")
            .map_err(|e| MediaError::Internal(format!("Failed to read created_at: {e}")))?;
        let expires_at: DateTime<Utc> = row
            .try_get("expires_at")
            .map_err(|e| MediaError::Internal(format!("Failed to read expires_at: {e}")))?;
        let confirmed_at: Option<DateTime<Utc>> = row
            .try_get("confirmed_at")
            .map_err(|e| MediaError::Internal(format!("Failed to read confirmed_at: {e}")))?;
        let client_name: Option<String> = row
            .try_get("client_name")
            .map_err(|e| MediaError::Internal(format!("Failed to read client_name: {e}")))?;
        let client_ip: Option<IpAddr> = row
            .try_get::<Option<IpNetwork>, _>("client_ip")
            .map_err(|e| MediaError::Internal(format!("Failed to read client_ip: {e}")))?
            .map(|network| network.ip());
        let attempts: i32 = row
            .try_get("attempts")
            .map_err(|e| MediaError::Internal(format!("Failed to read attempts: {e}")))?;
        let last_attempt_at: Option<DateTime<Utc>> = row
            .try_get("last_attempt_at")
            .map_err(|e| MediaError::Internal(format!("Failed to read last_attempt_at: {e}")))?;
        let revoked_at: Option<DateTime<Utc>> = row
            .try_get("revoked_at")
            .map_err(|e| MediaError::Internal(format!("Failed to read revoked_at: {e}")))?;
        let revoked_reason: Option<String> = row
            .try_get("revoked_reason")
            .map_err(|e| MediaError::Internal(format!("Failed to read revoked_reason: {e}")))?;

        Ok(SetupClaimRecord {
            id,
            code_hash,
            claim_token_hash,
            created_at,
            expires_at,
            confirmed_at,
            client_name,
            client_ip,
            attempts,
            last_attempt_at,
            revoked_at,
            revoked_reason,
        })
    }
}

#[async_trait]
impl SetupClaimsRepository for PostgresSetupClaimsRepository {
    async fn create(&self, claim: NewSetupClaim) -> Result<SetupClaimRecord> {
        let row = sqlx::query(
            r#"
            INSERT INTO setup_claims (code_hash, expires_at, client_name, client_ip)
            VALUES ($1, $2, $3, $4)
            RETURNING
                id,
                code_hash,
                claim_token_hash,
                created_at,
                expires_at,
                confirmed_at,
                client_name,
                client_ip,
                attempts,
                last_attempt_at,
                revoked_at,
                revoked_reason
            "#,
        )
        .bind(claim.code_hash)
        .bind(claim.expires_at)
        .bind(claim.client_name)
        .bind(claim.client_ip)
        .fetch_one(self.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to create setup claim: {e}")))?;

        Self::map_row(&row)
    }

    async fn get_active(&self, now: DateTime<Utc>) -> Result<Option<SetupClaimRecord>> {
        let row = sqlx::query(
            r#"
            SELECT
                id,
                code_hash,
                claim_token_hash,
                created_at,
                expires_at,
                confirmed_at,
                client_name,
                client_ip,
                attempts,
                last_attempt_at,
                revoked_at,
                revoked_reason
            FROM setup_claims
            WHERE confirmed_at IS NULL
              AND revoked_at IS NULL
              AND expires_at > $1
            ORDER BY created_at DESC
            LIMIT 1
            "#,
        )
        .bind(now)
        .fetch_optional(self.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to load active setup claim: {e}")))?;

        Ok(row.map(|row| Self::map_row(&row)).transpose()?)
    }

    async fn find_active_by_code_hash(
        &self,
        code_hash: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<SetupClaimRecord>> {
        let row = sqlx::query(
            r#"
            SELECT
                id,
                code_hash,
                claim_token_hash,
                created_at,
                expires_at,
                confirmed_at,
                client_name,
                client_ip,
                attempts,
                last_attempt_at,
                revoked_at,
                revoked_reason
            FROM setup_claims
            WHERE code_hash = $1
              AND confirmed_at IS NULL
              AND revoked_at IS NULL
              AND expires_at > $2
            ORDER BY created_at DESC
            LIMIT 1
            "#,
        )
        .bind(code_hash)
        .bind(now)
        .fetch_optional(self.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to lookup setup claim: {e}")))?;

        Ok(row.map(|row| Self::map_row(&row)).transpose()?)
    }

    async fn mark_confirmed(
        &self,
        id: Uuid,
        token_hash: String,
        now: DateTime<Utc>,
    ) -> Result<SetupClaimRecord> {
        let row = sqlx::query(
            r#"
            UPDATE setup_claims
            SET
                claim_token_hash = $2,
                confirmed_at = $3,
                last_attempt_at = $3
            WHERE id = $1
            RETURNING
                id,
                code_hash,
                claim_token_hash,
                created_at,
                expires_at,
                confirmed_at,
                client_name,
                client_ip,
                attempts,
                last_attempt_at,
                revoked_at,
                revoked_reason
            "#,
        )
        .bind(id)
        .bind(token_hash)
        .bind(now)
        .fetch_one(self.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to confirm setup claim: {e}")))?;

        Self::map_row(&row)
    }

    async fn increment_attempt(&self, id: Uuid, now: DateTime<Utc>) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE setup_claims
            SET attempts = attempts + 1,
                last_attempt_at = $2
            WHERE id = $1
            "#,
        )
        .bind(id)
        .bind(now)
        .execute(self.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to record claim attempt: {e}")))?;

        Ok(())
    }

    async fn find_confirmed_by_token_hash(
        &self,
        token_hash: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<SetupClaimRecord>> {
        let row = sqlx::query(
            r#"
            SELECT
                id,
                code_hash,
                claim_token_hash,
                created_at,
                expires_at,
                confirmed_at,
                client_name,
                client_ip,
                attempts,
                last_attempt_at,
                revoked_at,
                revoked_reason
            FROM setup_claims
            WHERE claim_token_hash = $1
              AND confirmed_at IS NOT NULL
              AND revoked_at IS NULL
              AND expires_at > $2
            ORDER BY confirmed_at DESC
            LIMIT 1
            "#,
        )
        .bind(token_hash)
        .bind(now)
        .fetch_optional(self.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to lookup setup claim by token: {e}")))?;

        Ok(row.map(|row| Self::map_row(&row)).transpose()?)
    }

    async fn revoke_by_id(
        &self,
        id: Uuid,
        reason: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<SetupClaimRecord> {
        let row = sqlx::query(
            r#"
            UPDATE setup_claims
            SET revoked_at = $2,
                revoked_reason = COALESCE($3, revoked_reason)
            WHERE id = $1
            RETURNING
                id,
                code_hash,
                claim_token_hash,
                created_at,
                expires_at,
                confirmed_at,
                client_name,
                client_ip,
                attempts,
                last_attempt_at,
                revoked_at,
                revoked_reason
            "#,
        )
        .bind(id)
        .bind(now)
        .bind(reason)
        .fetch_one(self.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to revoke setup claim: {e}")))?;

        Self::map_row(&row)
    }

    async fn revoke_all(&self, reason: Option<&str>, now: DateTime<Utc>) -> Result<u64> {
        let result = sqlx::query(
            r#"
            UPDATE setup_claims
            SET revoked_at = COALESCE(revoked_at, $1),
                revoked_reason = COALESCE($2, revoked_reason)
            WHERE revoked_at IS NULL
            "#,
        )
        .bind(now)
        .bind(reason)
        .execute(self.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to revoke setup claims: {e}")))?;

        Ok(result.rows_affected())
    }

    async fn purge_stale(&self, before: DateTime<Utc>) -> Result<u64> {
        let result = sqlx::query(
            r#"
            DELETE FROM setup_claims
            WHERE (revoked_at IS NOT NULL AND revoked_at < $1)
               OR (confirmed_at IS NULL AND expires_at < $1)
            "#,
        )
        .bind(before)
        .execute(self.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to purge stale setup claims: {e}")))?;

        Ok(result.rows_affected())
    }
}
