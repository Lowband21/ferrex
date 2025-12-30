use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use crate::database::repository_ports::security_settings::{
    SecuritySettingsRepository, SecuritySettingsUpdate,
};
use crate::domain::users::auth::policy::{
    AuthSecuritySettings, PasswordPolicy,
};
use crate::error::{MediaError, Result};

#[derive(Debug, Clone)]
pub struct PostgresSecuritySettingsRepository {
    pool: PgPool,
}

impl PostgresSecuritySettingsRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    fn pool(&self) -> &PgPool {
        &self.pool
    }

    async fn ensure_singleton_id(&self) -> Result<Uuid> {
        if let Some(record) = sqlx::query!(
            r#"
            SELECT id
            FROM auth_security_settings
            ORDER BY updated_at DESC
            LIMIT 1
            "#,
        )
        .fetch_optional(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to load auth security settings id: {e}"
            ))
        })? {
            return Ok(record.id);
        }

        let default = AuthSecuritySettings::default();
        let admin_json = serde_json::to_value(&default.admin_password_policy)
            .map_err(|e| {
            MediaError::Internal(format!("Failed to encode admin policy: {e}"))
        })?;
        let user_json = serde_json::to_value(&default.user_password_policy)
            .map_err(|e| {
                MediaError::Internal(format!(
                    "Failed to encode user policy: {e}"
                ))
            })?;

        let row = sqlx::query!(
            r#"
            INSERT INTO auth_security_settings (admin_password_policy, user_password_policy, updated_at)
            VALUES ($1, $2, $3)
            RETURNING id
            "#,
            admin_json,
            user_json,
            default.updated_at
        )
            .fetch_one(self.pool())
            .await
            .map_err(|e| MediaError::Internal(format!("Failed to insert default auth security settings: {e}")))?;

        Ok(row.id)
    }

    fn map_policy(value: Value) -> Result<PasswordPolicy> {
        serde_json::from_value(value).map_err(|e| {
            MediaError::Internal(format!(
                "Invalid password policy payload: {e}"
            ))
        })
    }

    fn map_row(
        admin_policy: Value,
        user_policy: Value,
        updated_at: DateTime<Utc>,
        updated_by: Option<Uuid>,
    ) -> Result<AuthSecuritySettings> {
        Ok(AuthSecuritySettings {
            admin_password_policy: Self::map_policy(admin_policy)?,
            user_password_policy: Self::map_policy(user_policy)?,
            updated_at,
            updated_by,
        })
    }
}

#[async_trait]
impl SecuritySettingsRepository for PostgresSecuritySettingsRepository {
    async fn get_settings(&self) -> Result<AuthSecuritySettings> {
        let id = self.ensure_singleton_id().await?;

        let row = sqlx::query!(
            r#"
            SELECT
                admin_password_policy,
                user_password_policy,
                updated_at,
                updated_by
            FROM auth_security_settings
            WHERE id = $1
            "#,
            id
        )
        .fetch_one(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to load auth security settings: {e}"
            ))
        })?;

        Self::map_row(
            row.admin_password_policy,
            row.user_password_policy,
            row.updated_at,
            row.updated_by,
        )
    }

    async fn update_settings(
        &self,
        update: SecuritySettingsUpdate,
    ) -> Result<AuthSecuritySettings> {
        let id = self.ensure_singleton_id().await?;

        let admin_json = serde_json::to_value(&update.admin_password_policy)
            .map_err(|e| {
                MediaError::Internal(format!(
                    "Failed to encode admin password policy: {e}"
                ))
            })?;
        let user_json = serde_json::to_value(&update.user_password_policy)
            .map_err(|e| {
                MediaError::Internal(format!(
                    "Failed to encode user password policy: {e}"
                ))
            })?;

        let row = sqlx::query!(
            r#"
            UPDATE auth_security_settings
            SET admin_password_policy = $1,
                user_password_policy = $2,
                updated_at = NOW(),
                updated_by = $3
            WHERE id = $4
            RETURNING
                admin_password_policy,
                user_password_policy,
                updated_at,
                updated_by
            "#,
            admin_json,
            user_json,
            update.updated_by,
            id
        )
        .fetch_one(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to update auth security settings: {e}"
            ))
        })?;

        Self::map_row(
            row.admin_password_policy,
            row.user_password_policy,
            row.updated_at,
            row.updated_by,
        )
    }
}
