//! Users section state (Admin)

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Users management state
#[derive(Debug, Clone, Default)]
pub struct UsersState {
    /// List of users
    pub users: Vec<UserSummary>,
    /// Currently selected user for editing
    pub selected_user_id: Option<Uuid>,
    /// Whether user list is loading
    pub loading: bool,
    /// Error message from last operation
    pub error: Option<String>,
    /// User form state (for add/edit)
    pub form: Option<UserFormState>,
}

/// Summary info for a user
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSummary {
    pub id: Uuid,
    pub username: String,
    pub display_name: Option<String>,
    pub email: Option<String>,
    pub role: UserRole,
    pub created_at: String,
    pub last_login: Option<String>,
    pub is_active: bool,
}

/// User role
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UserRole {
    Admin,
    User,
    Guest,
}

impl UserRole {
    pub const ALL: [UserRole; 3] = [Self::Admin, Self::User, Self::Guest];
}

impl std::fmt::Display for UserRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Admin => write!(f, "Administrator"),
            Self::User => write!(f, "User"),
            Self::Guest => write!(f, "Guest"),
        }
    }
}

/// State for user add/edit form
#[derive(Debug, Clone, Default)]
pub struct UserFormState {
    pub id: Option<Uuid>,
    pub username: String,
    pub display_name: String,
    pub email: String,
    pub password: String,
    pub confirm_password: String,
    pub role: Option<UserRole>,
    pub is_active: bool,
    pub saving: bool,
    pub error: Option<String>,
}
