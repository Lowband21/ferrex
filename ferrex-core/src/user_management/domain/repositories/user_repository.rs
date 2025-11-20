use crate::user_management::domain::{UserAggregate, UserRole, Username};
use async_trait::async_trait;
use uuid::Uuid;

/// Repository trait for user data persistence operations
///
/// This trait defines the interface for user data operations, allowing
/// different implementations (in-memory, PostgreSQL, etc.) while keeping
/// the domain logic independent of persistence details.
#[async_trait]
pub trait UserRepository: Send + Sync {
    /// Create a new user
    async fn create(&self, user: UserAggregate) -> Result<UserAggregate, UserRepositoryError>;

    /// Find a user by their unique identifier
    async fn find_by_id(&self, id: Uuid) -> Result<Option<UserAggregate>, UserRepositoryError>;

    /// Find a user by their username
    async fn find_by_username(
        &self,
        username: &Username,
    ) -> Result<Option<UserAggregate>, UserRepositoryError>;

    /// Find a user by their email address
    async fn find_by_email(
        &self,
        email: &str,
    ) -> Result<Option<UserAggregate>, UserRepositoryError>;

    /// Update an existing user
    async fn update(&self, user: UserAggregate) -> Result<UserAggregate, UserRepositoryError>;

    /// Delete a user by their identifier
    async fn delete(&self, id: Uuid) -> Result<bool, UserRepositoryError>;

    /// List all users with optional filtering
    async fn list(&self, filter: UserListFilter)
    -> Result<Vec<UserAggregate>, UserRepositoryError>;

    /// Count users with optional filtering
    async fn count(&self, filter: UserListFilter) -> Result<u64, UserRepositoryError>;

    /// Check if a username is already taken
    async fn username_exists(&self, username: &Username) -> Result<bool, UserRepositoryError>;

    /// Check if an email is already taken
    async fn email_exists(&self, email: &str) -> Result<bool, UserRepositoryError>;

    /// Find users by role
    async fn find_by_role(&self, role: UserRole)
    -> Result<Vec<UserAggregate>, UserRepositoryError>;

    /// Get users who haven't logged in since the specified date
    async fn find_inactive_since(
        &self,
        days: u32,
    ) -> Result<Vec<UserAggregate>, UserRepositoryError>;
}

/// Filter options for listing users
#[derive(Debug, Clone, Default)]
pub struct UserListFilter {
    /// Filter by active status
    pub active_only: Option<bool>,

    /// Filter by role
    pub role: Option<UserRole>,

    /// Search by username (partial match)
    pub username_search: Option<String>,

    /// Search by display name (partial match)
    pub display_name_search: Option<String>,

    /// Limit number of results
    pub limit: Option<u32>,

    /// Offset for pagination
    pub offset: Option<u32>,

    /// Sort field
    pub sort_by: Option<UserSortBy>,

    /// Sort direction
    pub sort_direction: Option<SortDirection>,
}

/// Sorting options for user queries
#[derive(Debug, Clone)]
pub enum UserSortBy {
    Username,
    DisplayName,
    CreatedAt,
    UpdatedAt,
    LastLogin,
}

/// Sort direction
#[derive(Debug, Clone, Default)]
pub enum SortDirection {
    #[default]
    Ascending,
    Descending,
}

/// Errors that can occur during repository operations
#[derive(Debug, Clone, thiserror::Error)]
pub enum UserRepositoryError {
    #[error("User not found")]
    NotFound,

    #[error("Username already exists")]
    UsernameExists,

    #[error("Email already exists")]
    EmailExists,

    #[error("Database connection error: {0}")]
    ConnectionError(String),

    #[error("Database query error: {0}")]
    QueryError(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Validation error: {0}")]
    ValidationError(String),

    #[error("Internal error: {0}")]
    InternalError(String),
}

impl UserRepositoryError {
    /// Check if this is a not found error
    pub fn is_not_found(&self) -> bool {
        matches!(self, UserRepositoryError::NotFound)
    }

    /// Check if this is a constraint violation (username/email exists)
    pub fn is_constraint_violation(&self) -> bool {
        matches!(
            self,
            UserRepositoryError::UsernameExists | UserRepositoryError::EmailExists
        )
    }
}
