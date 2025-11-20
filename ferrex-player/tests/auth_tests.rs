//! Main authentication integration tests
//!
//! This file integrates all auth test modules

#![cfg(feature = "testing")]

mod auth;

// Cross-domain refactoring tests
mod task_1_11_auth_migration {
    include!("auth/task_1_11_auth_migration.rs");
}

// Make auth tests available at this level
pub use auth::*;
