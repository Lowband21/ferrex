//! User Management Domain Module
//!
//! This module provides domain-driven design components for user management,
//! including user CRUD operations, role management, and user lifecycle events.
//!
//! ## Domain Structure
//!
//! - **Aggregates**: Core domain entities that enforce business rules
//! - **Value Objects**: Immutable types representing domain concepts
//! - **Repositories**: Interfaces for data persistence
//! - **Services**: Domain services for complex business logic
//! - **Events**: Domain events for decoupled communication

pub mod domain;

pub use domain::*;
