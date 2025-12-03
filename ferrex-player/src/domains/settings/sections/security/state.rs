//! Security section state
//!
//! Note: This re-exports the existing SecurityState from the parent module
//! to maintain backwards compatibility during the refactor.

// Re-export existing types
pub use crate::domains::settings::state::{
    PasswordChangeState, PinChangeState, SecurityState,
};
