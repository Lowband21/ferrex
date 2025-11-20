use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Compact admin-facing user info used by the admin users endpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminUserInfo {
    pub id: Uuid,
    pub username: String,
    pub display_name: String,
    pub roles: Vec<String>,
    pub created_at: i64,
    pub session_count: i64,
}

/// Request payload to create a user via admin endpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateUserRequest {
    pub username: String,
    pub display_name: String,
    pub password: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub role_ids: Vec<Uuid>,
    #[serde(default = "default_true")]
    pub is_active: bool,
}

/// Request payload to update a user via admin endpoints.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpdateUserRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_active: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role_ids: Option<Vec<Uuid>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_password: Option<String>,
}

fn default_true() -> bool {
    true
}

