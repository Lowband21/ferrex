use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use sqlx::types::ipnetwork::IpNetwork;
use uuid::Uuid;

use crate::database::repository_ports::setup_claims::{
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

    fn map_row(row: SetupClaimRow) -> Result<SetupClaimRecord> {
        let client_ip = row.client_ip.map(|network| network.ip());
        Ok(SetupClaimRecord {
            id: row.id,
            code_hash: row.code_hash,
            claim_token_hash: row.claim_token_hash,
            created_at: row.created_at,
            expires_at: row.expires_at,
            confirmed_at: row.confirmed_at,
            client_name: row.client_name,
            client_ip,
            attempts: row.attempts,
            last_attempt_at: row.last_attempt_at,
            revoked_at: row.revoked_at,
            revoked_reason: row.revoked_reason,
        })
    }
}

#[derive(Debug)]
struct SetupClaimRow {
    id: Uuid,
    code_hash: String,
    claim_token_hash: Option<String>,
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    confirmed_at: Option<DateTime<Utc>>,
    client_name: Option<String>,
    client_ip: Option<IpNetwork>,
    attempts: i32,
    last_attempt_at: Option<DateTime<Utc>>,
    revoked_at: Option<DateTime<Utc>>,
    revoked_reason: Option<String>,
}

#[async_trait]
impl SetupClaimsRepository for PostgresSetupClaimsRepository {
    async fn create(&self, claim: NewSetupClaim) -> Result<SetupClaimRecord> {
        let row = sqlx::query_as!(
            SetupClaimRow,
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
            claim.code_hash,
            claim.expires_at,
            claim.client_name,
            claim.client_ip.map(IpNetwork::from)
        )
            .fetch_one(self.pool())
            .await
            .map_err(|e| MediaError::Internal(format!("Failed to create setup claim: {e}")))?;

        Self::map_row(row)
    }

    async fn get_active(
        &self,
        now: DateTime<Utc>,
    ) -> Result<Option<SetupClaimRecord>> {
        let row = sqlx::query_as!(
            SetupClaimRow,
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
            now
        )
        .fetch_optional(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to load active setup claim: {e}"
            ))
        })?;

        Ok(row.map(Self::map_row).transpose()?)
    }

    async fn find_active_by_code_hash(
        &self,
        code_hash: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<SetupClaimRecord>> {
        let row = sqlx::query_as!(
            SetupClaimRow,
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
            code_hash,
            now
        )
        .fetch_optional(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Failed to lookup setup claim: {e}"))
        })?;

        Ok(row.map(Self::map_row).transpose()?)
    }

    async fn mark_confirmed(
        &self,
        id: Uuid,
        token_hash: String,
        now: DateTime<Utc>,
    ) -> Result<SetupClaimRecord> {
        let row = sqlx::query_as!(
            SetupClaimRow,
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
            id,
            token_hash,
            now
        )
        .fetch_one(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Failed to confirm setup claim: {e}"))
        })?;

        Self::map_row(row)
    }

    async fn increment_attempt(
        &self,
        id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<()> {
        sqlx::query!(
            r#"
            UPDATE setup_claims
            SET attempts = attempts + 1,
                last_attempt_at = $2
            WHERE id = $1
            "#,
            id,
            now
        )
        .execute(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Failed to record claim attempt: {e}"))
        })?;

        Ok(())
    }

    async fn find_confirmed_by_token_hash(
        &self,
        token_hash: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<SetupClaimRecord>> {
        let row = sqlx::query_as!(
            SetupClaimRow,
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
            token_hash,
            now
        )
        .fetch_optional(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to lookup setup claim by token: {e}"
            ))
        })?;

        Ok(row.map(Self::map_row).transpose()?)
    }

    async fn revoke_by_id(
        &self,
        id: Uuid,
        reason: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<SetupClaimRecord> {
        let row = sqlx::query_as!(
            SetupClaimRow,
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
            id,
            now,
            reason
        )
        .fetch_one(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Failed to revoke setup claim: {e}"))
        })?;

        Self::map_row(row)
    }

    async fn revoke_all(
        &self,
        reason: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<u64> {
        let result = sqlx::query!(
            r#"
            UPDATE setup_claims
            SET revoked_at = COALESCE(revoked_at, $1),
                revoked_reason = COALESCE($2, revoked_reason)
            WHERE revoked_at IS NULL
            "#,
            now,
            reason
        )
        .execute(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Failed to revoke setup claims: {e}"))
        })?;

        Ok(result.rows_affected())
    }

    async fn purge_stale(&self, before: DateTime<Utc>) -> Result<u64> {
        let result = sqlx::query!(
            r#"
            DELETE FROM setup_claims
            WHERE (revoked_at IS NOT NULL AND revoked_at < $1)
               OR (confirmed_at IS NULL AND expires_at < $1)
            "#,
            before
        )
        .execute(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to purge stale setup claims: {e}"
            ))
        })?;

        Ok(result.rows_affected())
    }
}
