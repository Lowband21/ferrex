//! Main authentication integration tests
//!
//! This file integrates all auth test modules

#![cfg(feature = "testing")]

mod auth;

// Make auth tests available at this level
pub use auth::*;
