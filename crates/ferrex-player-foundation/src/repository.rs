//! Shared repository result and error primitives for player-side data access.
//!
//! These types are intentionally independent of concrete persistence backends,
//! UI crates, and Ferrex domain model crates. Adapters should map backend- or
//! domain-specific errors into these variants at crate boundaries.

/// Result type for repository operations.
pub type RepositoryResult<T> = Result<T, RepositoryError>;

/// Repository-specific errors with enough context for callers and logs.
#[derive(Debug, thiserror::Error)]
pub enum RepositoryError {
    /// A requested entity was not present in the repository.
    #[error("Entity not found: {entity_type} with id {id}")]
    NotFound {
        /// Human-readable entity kind.
        entity_type: String,
        /// Stable identifier used in the failed lookup.
        id: String,
    },

    /// A read/query operation failed.
    #[error("Query failed: {0}")]
    QueryFailed(String),

    /// Serialized data could not be decoded into the expected type.
    #[error("Deserialization failed: {0}")]
    DeserializationError(String),

    /// An update/write operation failed.
    #[error("Update failed: {0}")]
    UpdateFailed(String),

    /// A delete operation failed.
    #[error("Delete failed: {0}")]
    DeleteFailed(String),

    /// A create/insert operation failed.
    #[error("Create failed: {0}")]
    CreateFailed(String),

    /// Storage was unavailable or in an invalid state.
    #[error("Storage error: {0}")]
    StorageError(String),

    /// A lock could not be acquired.
    #[error("Lock acquisition failed: {0}")]
    LockError(String),

    /// Data could not be serialized into the repository representation.
    #[error("Serialization error: {0}")]
    SerializationError(String),

    /// Media-layer failures mapped at the player boundary.
    #[error("Media error: {0}")]
    MediaError(String),
}
