//! Repository traits for data access abstraction
//!
//! This module defines trait-based interfaces for all data access operations,
//! following the Ports & Adapters (Hexagonal) architecture pattern.
//! 
//! These traits enable:
//! - Testability through mock implementations
//! - Decoupling of business logic from data storage
//! - Clear contracts for data operations
//! - Easy switching between different storage backends

pub mod media;
pub mod library;

pub use media::{MediaRepository, MediaQuery, MediaSortOptions, MediaFilterOptions};
pub use library::{LibraryRepository, LibraryQuery};

/// Result type for repository operations
pub type RepositoryResult<T> = Result<T, RepositoryError>;

/// Repository-specific errors with proper context
#[derive(Debug, thiserror::Error)]
pub enum RepositoryError {
    #[error("Entity not found: {entity_type} with id {id}")]
    NotFound { entity_type: String, id: String },
    
    #[error("Query failed: {0}")]
    QueryFailed(String),
    
    #[error("Update failed: {0}")]
    UpdateFailed(String),
    
    #[error("Delete failed: {0}")]
    DeleteFailed(String),
    
    #[error("Create failed: {0}")]
    CreateFailed(String),
    
    #[error("Storage error: {0}")]
    StorageError(String),
    
    #[error("Lock acquisition failed: {0}")]
    LockError(String),
    
    #[error("Serialization error: {0}")]
    SerializationError(String),
    
    #[error("Media error: {0}")]
    MediaError(#[from] ferrex_core::error::MediaError),
}