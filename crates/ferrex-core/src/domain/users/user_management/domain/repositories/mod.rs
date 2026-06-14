//! User management repository traits.
//!
//! These define the interfaces for data persistence operations, following the
//! Repository pattern for clean architecture.

mod user_repository;

pub use user_repository::{UserRepository, UserRepositoryError};
