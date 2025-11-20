use crate::domain::users::user_management::domain::value_objects::{
    DisplayName, UserRole, Username,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// User aggregate for managing user CRUD operations and business logic
///
/// This aggregate encapsulates all user-related business rules and ensures
/// data consistency for user management operations. It handles user creation,
/// updates, role changes, and status management.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserAggregate {
    /// Unique user identifier
    pub id: Uuid,
    /// Validated username
    pub username: Username,
    /// User's display name
    pub display_name: DisplayName,
    /// User's role in the system
    pub role: UserRole,
    /// Whether the user account is active
    pub is_active: bool,
    /// Optional email address
    pub email: Option<String>,
    /// Optional URL to user's avatar image
    pub avatar_url: Option<String>,
    /// Timestamp of account creation
    pub created_at: DateTime<Utc>,
    /// Timestamp of last profile update
    pub updated_at: DateTime<Utc>,
    /// Timestamp of most recent login
    pub last_login: Option<DateTime<Utc>>,
}

impl UserAggregate {
    /// Create a new user with validated inputs
    pub fn new(
        username: Username,
        display_name: DisplayName,
        role: UserRole,
    ) -> Self {
        let now = Utc::now();

        Self {
            id: Uuid::now_v7(),
            username,
            display_name,
            role,
            is_active: true,
            email: None,
            avatar_url: None,
            created_at: now,
            updated_at: now,
            last_login: None,
        }
    }

    /// Update the user's display name
    pub fn update_display_name(
        &mut self,
        display_name: DisplayName,
    ) -> Result<(), UserAggregateError> {
        if !self.is_active {
            return Err(UserAggregateError::UserInactive);
        }

        self.display_name = display_name;
        self.updated_at = Utc::now();
        Ok(())
    }

    /// Update the user's role
    pub fn update_role(
        &mut self,
        role: UserRole,
    ) -> Result<(), UserAggregateError> {
        if !self.is_active {
            return Err(UserAggregateError::UserInactive);
        }

        self.role = role;
        self.updated_at = Utc::now();
        Ok(())
    }

    /// Update the user's email
    pub fn update_email(
        &mut self,
        email: Option<String>,
    ) -> Result<(), UserAggregateError> {
        if !self.is_active {
            return Err(UserAggregateError::UserInactive);
        }

        // Basic email validation if provided
        if let Some(ref email_str) = email
            && (!email_str.contains('@') || email_str.len() > 254)
        {
            return Err(UserAggregateError::InvalidEmail);
        }

        self.email = email;
        self.updated_at = Utc::now();
        Ok(())
    }

    /// Update the user's avatar URL
    pub fn update_avatar_url(
        &mut self,
        avatar_url: Option<String>,
    ) -> Result<(), UserAggregateError> {
        if !self.is_active {
            return Err(UserAggregateError::UserInactive);
        }

        self.avatar_url = avatar_url;
        self.updated_at = Utc::now();
        Ok(())
    }

    /// Deactivate the user account
    pub fn deactivate(&mut self) -> Result<(), UserAggregateError> {
        if !self.is_active {
            return Err(UserAggregateError::UserAlreadyInactive);
        }

        self.is_active = false;
        self.updated_at = Utc::now();
        Ok(())
    }

    /// Reactivate the user account
    pub fn reactivate(&mut self) -> Result<(), UserAggregateError> {
        if self.is_active {
            return Err(UserAggregateError::UserAlreadyActive);
        }

        self.is_active = true;
        self.updated_at = Utc::now();
        Ok(())
    }

    /// Record a login timestamp
    pub fn record_login(&mut self) {
        self.last_login = Some(Utc::now());
    }

    /// Check if the user has the specified role
    pub fn has_role(&self, role: &UserRole) -> bool {
        self.role == *role
    }

    /// Check if the user is an admin
    pub fn is_admin(&self) -> bool {
        matches!(self.role, UserRole::Admin)
    }

    /// Check if the user can perform administrative actions
    pub fn can_admin(&self) -> bool {
        self.is_active && self.is_admin()
    }
}

/// Errors that can occur during user aggregate operations
#[derive(Debug, Clone, thiserror::Error)]
pub enum UserAggregateError {
    #[error("User is inactive")]
    UserInactive,

    #[error("User is already inactive")]
    UserAlreadyInactive,

    #[error("User is already active")]
    UserAlreadyActive,

    #[error("Invalid email address")]
    InvalidEmail,

    #[error("Operation not permitted")]
    OperationNotPermitted,
}
