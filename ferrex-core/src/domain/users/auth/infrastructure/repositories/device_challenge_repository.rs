use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use std::fmt;
use uuid::Uuid;

use crate::domain::users::auth::domain::repositories::{
    DeviceChallengeRecord, DeviceChallengeRepository,
};

pub struct PostgresDeviceChallengeRepository {
    pool: PgPool,
}

impl fmt::Debug for PostgresDeviceChallengeRepository {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PostgresDeviceChallengeRepository").finish()
    }
}

impl PostgresDeviceChallengeRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl DeviceChallengeRepository for PostgresDeviceChallengeRepository {
    async fn insert_challenge(
        &self,
        device_session_id: Uuid,
        nonce: &[u8],
        expires_at: DateTime<Utc>,
    ) -> Result<Uuid> {
        let rec = sqlx::query!(
            r#"
            INSERT INTO auth_device_challenges (device_session_id, nonce, expires_at)
            VALUES ($1, $2, $3)
            RETURNING id
            "#,
            device_session_id,
            nonce,
            expires_at
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(rec.id)
    }

    async fn get(&self, id: Uuid) -> Result<Option<DeviceChallengeRecord>> {
        let row = sqlx::query!(
            r#"
            SELECT id, device_session_id, nonce, issued_at, expires_at, used
            FROM auth_device_challenges
            WHERE id = $1
            "#,
            id
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| DeviceChallengeRecord {
            id: r.id,
            device_session_id: r.device_session_id,
            nonce: r.nonce,
            issued_at: r.issued_at,
            expires_at: r.expires_at,
            used: r.used,
        }))
    }

    async fn mark_used(&self, id: Uuid) -> Result<()> {
        sqlx::query!(
            r#"
            UPDATE auth_device_challenges
            SET used = TRUE
            WHERE id = $1
            "#,
            id
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn consume_if_fresh(
        &self,
        id: Uuid,
    ) -> Result<Option<(Uuid, Vec<u8>)>> {
        let row = sqlx::query!(
            r#"
            WITH got AS (
              UPDATE auth_device_challenges
              SET used = TRUE
              WHERE id = $1 AND used = FALSE AND expires_at > NOW()
              RETURNING device_session_id, nonce
            )
            SELECT device_session_id, nonce FROM got
            "#,
            id
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| (r.device_session_id, r.nonce)))
    }
}
