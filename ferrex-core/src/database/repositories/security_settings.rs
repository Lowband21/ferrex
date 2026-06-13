use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use crate::database::repository_ports::security_settings::{
    SecuritySettingsRepository, SecuritySettingsUpdate,
};
use crate::domain::users::auth::policy::AuthSecuritySettings;
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
        let admin_json = Self::to_json(
            &default.admin_password_policy,
            "admin password policy",
        )?;
        let user_json = Self::to_json(
            &default.user_password_policy,
            "user password policy",
        )?;
        let pin_json = Self::to_json(&default.pin_policy, "PIN policy")?;
        let trust_json =
            Self::to_json(&default.device_trust_policy, "device trust policy")?;

        let row = sqlx::query!(
            r#"
            INSERT INTO auth_security_settings (
                admin_password_policy,
                user_password_policy,
                pin_policy,
                device_trust_policy,
                updated_at
            )
            VALUES ($1, $2, $3, $4, $5)
            RETURNING id
            "#,
            admin_json,
            user_json,
            pin_json,
            trust_json,
            default.updated_at
        )
        .fetch_one(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to insert default auth security settings: {e}"
            ))
        })?;

        Ok(row.id)
    }

    fn to_json<T: serde::Serialize>(value: &T, label: &str) -> Result<Value> {
        serde_json::to_value(value).map_err(|e| {
            MediaError::Internal(format!("Failed to encode {label}: {e}"))
        })
    }

    fn map_json<T: DeserializeOwned>(value: Value, label: &str) -> Result<T> {
        serde_json::from_value(value).map_err(|e| {
            MediaError::Internal(format!("Invalid {label} payload: {e}"))
        })
    }

    fn map_row(
        admin_policy: Value,
        user_policy: Value,
        pin_policy: Value,
        device_trust_policy: Value,
        updated_at: DateTime<Utc>,
        updated_by: Option<Uuid>,
    ) -> Result<AuthSecuritySettings> {
        Ok(AuthSecuritySettings {
            admin_password_policy: Self::map_json(
                admin_policy,
                "admin password policy",
            )?,
            user_password_policy: Self::map_json(
                user_policy,
                "user password policy",
            )?,
            pin_policy: Self::map_json(pin_policy, "PIN policy")?,
            device_trust_policy: Self::map_json(
                device_trust_policy,
                "device trust policy",
            )?,
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
                pin_policy,
                device_trust_policy,
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
            row.pin_policy,
            row.device_trust_policy,
            row.updated_at,
            row.updated_by,
        )
    }

    async fn update_settings(
        &self,
        update: SecuritySettingsUpdate,
    ) -> Result<AuthSecuritySettings> {
        let id = self.ensure_singleton_id().await?;

        let admin_json = Self::to_json(
            &update.admin_password_policy,
            "admin password policy",
        )?;
        let user_json = Self::to_json(
            &update.user_password_policy,
            "user password policy",
        )?;
        let pin_json = Self::to_json(&update.pin_policy, "PIN policy")?;
        let trust_json =
            Self::to_json(&update.device_trust_policy, "device trust policy")?;

        let row = sqlx::query!(
            r#"
            UPDATE auth_security_settings
            SET admin_password_policy = $1,
                user_password_policy = $2,
                pin_policy = $3,
                device_trust_policy = $4,
                updated_at = NOW(),
                updated_by = $5
            WHERE id = $6
            RETURNING
                admin_password_policy,
                user_password_policy,
                pin_policy,
                device_trust_policy,
                updated_at,
                updated_by
            "#,
            admin_json,
            user_json,
            pin_json,
            trust_json,
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
            row.pin_policy,
            row.device_trust_policy,
            row.updated_at,
            row.updated_by,
        )
    }
}
