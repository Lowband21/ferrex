use std::fmt;
use std::net::IpAddr;

use anyhow::Result;
use async_trait::async_trait;
use ipnetwork::IpNetwork;
use sqlx::PgPool;

use crate::auth::AuthEventType;
use crate::auth::domain::repositories::{
    AuthAuditEventKind, AuthEventLog, AuthEventRepository,
};

pub struct PostgresAuthEventRepository {
    pool: PgPool,
}

impl fmt::Debug for PostgresAuthEventRepository {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PostgresAuthEventRepository").finish()
    }
}

impl PostgresAuthEventRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn map_event_kind(kind: AuthAuditEventKind) -> AuthEventType {
    match kind {
        AuthAuditEventKind::PasswordLoginSuccess => {
            AuthEventType::PasswordLoginSuccess
        }
        AuthAuditEventKind::PasswordLoginFailure => {
            AuthEventType::PasswordLoginFailure
        }
        AuthAuditEventKind::PinLoginSuccess => AuthEventType::PinLoginSuccess,
        AuthAuditEventKind::PinLoginFailure => AuthEventType::PinLoginFailure,
        AuthAuditEventKind::DeviceRegistered => AuthEventType::DeviceRegistered,
        AuthAuditEventKind::DeviceRevoked => AuthEventType::DeviceRevoked,
        AuthAuditEventKind::PinSet => AuthEventType::PinSet,
        AuthAuditEventKind::PinRemoved => AuthEventType::PinRemoved,
        AuthAuditEventKind::SessionCreated => AuthEventType::SessionCreated,
        AuthAuditEventKind::SessionRevoked => AuthEventType::SessionRevoked,
        AuthAuditEventKind::AutoLogin => AuthEventType::AutoLogin,
    }
}

fn parse_ip(value: &Option<String>) -> Option<IpNetwork> {
    value
        .as_deref()
        .and_then(|raw| raw.parse::<IpAddr>().ok())
        .map(IpNetwork::from)
}

#[async_trait]
impl AuthEventRepository for PostgresAuthEventRepository {
    async fn record(&self, events: Vec<AuthEventLog>) -> Result<()> {
        if events.is_empty() {
            return Ok(());
        }

        for event in events {
            let event_type = map_event_kind(event.event_type);
            let ip_address = parse_ip(&event.ip_address);

            sqlx::query!(
                r#"
                INSERT INTO auth_events (
                    user_id,
                    device_session_id,
                    session_id,
                    event_type,
                    success,
                    failure_reason,
                    ip_address,
                    user_agent,
                    metadata,
                    created_at
                )
                VALUES ($1, $2, $3, $4::auth_event_type, $5, $6, $7, $8, $9, $10)
                "#,
                event.user_id,
                event.device_session_id,
                event.session_id,
                // Cast keeps `sqlx::query!` aware that this parameter maps to the
                // `auth_event_type` enum in Postgres while still type-checking the Rust enum.
                event_type as AuthEventType,
                event.success,
                event.failure_reason,
                ip_address,
                event.user_agent,
                event.metadata,
                event.occurred_at
            )
            .execute(&self.pool)
            .await?;
        }

        Ok(())
    }
}
