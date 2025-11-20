use chrono::{DateTime, Utc};
use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::domain::users::auth::domain::events::AuthEvent;
use crate::domain::users::auth::domain::repositories::{
    AuthAuditEventKind, AuthEventLog,
};

#[derive(Debug, Clone)]
pub struct AuthEventContext {
    pub auth_session_id: Option<Uuid>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub metadata: Value,
}

impl Default for AuthEventContext {
    fn default() -> Self {
        Self {
            auth_session_id: None,
            ip_address: None,
            user_agent: None,
            metadata: json!({}),
        }
    }
}

impl AuthEventContext {
    pub fn with_auth_session_id(mut self, session_id: Uuid) -> Self {
        self.auth_session_id = Some(session_id);
        self
    }

    pub fn insert_metadata(&mut self, key: &str, value: Value) {
        if !self.metadata.is_object() {
            self.metadata = json!({});
        }

        if let Some(map) = self.metadata.as_object_mut() {
            map.insert(key.to_string(), value);
        }
    }
}

pub(crate) fn map_domain_events(
    events: Vec<AuthEvent>,
    context: &AuthEventContext,
) -> Vec<AuthEventLog> {
    events
        .into_iter()
        .filter_map(|event| map_domain_event(event, context))
        .collect()
}

fn map_domain_event(
    event: AuthEvent,
    context: &AuthEventContext,
) -> Option<AuthEventLog> {
    let occurred_at = event.timestamp();
    let user_id = event.user_id();
    match event {
        AuthEvent::DeviceRegistered {
            session_id,
            device_name,
            ..
        } => Some(build_log(
            AuthAuditEventKind::DeviceRegistered,
            user_id,
            Some(session_id),
            true,
            None,
            Some(json!({ "device_name": device_name })),
            occurred_at,
            context,
        )),
        AuthEvent::DeviceTrusted { session_id, .. } => Some(build_log(
            AuthAuditEventKind::DeviceRegistered,
            user_id,
            Some(session_id),
            true,
            None,
            Some(json!({ "trusted": true })),
            occurred_at,
            context,
        )),
        AuthEvent::DeviceRevoked { session_id, .. } => Some(build_log(
            AuthAuditEventKind::DeviceRevoked,
            user_id,
            Some(session_id),
            true,
            None,
            None,
            occurred_at,
            context,
        )),
        AuthEvent::AllDevicesRevoked { .. } => Some(build_log(
            AuthAuditEventKind::DeviceRevoked,
            user_id,
            None,
            true,
            None,
            Some(json!({ "scope": "all" })),
            occurred_at,
            context,
        )),
        AuthEvent::PinSet { session_id, .. } => Some(build_log(
            AuthAuditEventKind::PinSet,
            user_id,
            Some(session_id),
            true,
            None,
            None,
            occurred_at,
            context,
        )),
        AuthEvent::PinRemoved { session_id, .. } => Some(build_log(
            AuthAuditEventKind::PinRemoved,
            user_id,
            Some(session_id),
            true,
            None,
            None,
            occurred_at,
            context,
        )),
        AuthEvent::SessionCreated {
            session_id,
            expires_at,
            ..
        } => Some(build_log(
            AuthAuditEventKind::SessionCreated,
            user_id,
            Some(session_id),
            true,
            None,
            Some(json!({ "expires_at": expires_at.to_rfc3339() })),
            occurred_at,
            context,
        )),
        AuthEvent::SessionRefreshed {
            session_id,
            expires_at,
            ..
        } => Some(build_log(
            AuthAuditEventKind::SessionCreated,
            user_id,
            Some(session_id),
            true,
            None,
            Some(json!({
                "action": "refresh",
                "expires_at": expires_at.to_rfc3339(),
            })),
            occurred_at,
            context,
        )),
        AuthEvent::AuthenticationFailed {
            session_id, reason, ..
        } => {
            let lowered = reason.to_ascii_lowercase();
            let event_type = if lowered.contains("pin") {
                AuthAuditEventKind::PinLoginFailure
            } else {
                AuthAuditEventKind::PasswordLoginFailure
            };

            Some(build_log(
                event_type,
                user_id,
                normalize_session_id(session_id),
                false,
                Some(reason),
                None,
                occurred_at,
                context,
            ))
        }
        AuthEvent::PasswordAuthenticated { .. } => Some(build_log(
            AuthAuditEventKind::PasswordLoginSuccess,
            user_id,
            None,
            true,
            None,
            None,
            occurred_at,
            context,
        )),
        AuthEvent::PasswordChanged { .. } => Some(build_log(
            AuthAuditEventKind::SessionRevoked,
            user_id,
            None,
            true,
            None,
            Some(json!({ "reason": "password_changed" })),
            occurred_at,
            context,
        )),
        AuthEvent::AccountLocked { locked_until, .. } => Some(build_log(
            AuthAuditEventKind::SessionRevoked,
            user_id,
            None,
            true,
            None,
            Some(json!({
                "reason": "account_locked",
                "locked_until": locked_until.to_rfc3339(),
            })),
            occurred_at,
            context,
        )),
        AuthEvent::AccountUnlocked { .. } => None,
        AuthEvent::AccountDeactivated { .. } => Some(build_log(
            AuthAuditEventKind::SessionRevoked,
            user_id,
            None,
            true,
            None,
            Some(json!({ "reason": "account_deactivated" })),
            occurred_at,
            context,
        )),
    }
}

fn normalize_session_id(session_id: Uuid) -> Option<Uuid> {
    if session_id.is_nil() {
        None
    } else {
        Some(session_id)
    }
}

#[allow(clippy::too_many_arguments)]
fn build_log(
    event_type: AuthAuditEventKind,
    user_id: Uuid,
    device_session_id: Option<Uuid>,
    success: bool,
    failure_reason: Option<String>,
    extra_metadata: Option<Value>,
    occurred_at: DateTime<Utc>,
    context: &AuthEventContext,
) -> AuthEventLog {
    AuthEventLog {
        event_type,
        user_id: Some(user_id),
        device_session_id,
        session_id: context.auth_session_id,
        success,
        failure_reason,
        ip_address: context.ip_address.clone(),
        user_agent: context.user_agent.clone(),
        metadata: merge_metadata(&context.metadata, extra_metadata),
        occurred_at,
    }
}

fn merge_metadata(base: &Value, extra: Option<Value>) -> Value {
    let mut map: Map<String, Value> = match base {
        Value::Object(obj) => obj.clone(),
        _ => Map::new(),
    };

    if let Some(Value::Object(extra_map)) = extra {
        for (key, value) in extra_map.into_iter() {
            map.insert(key, value);
        }
    }

    Value::Object(map)
}
