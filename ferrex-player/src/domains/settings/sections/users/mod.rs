//! User management sub-domain (Admin)
//!
//! Handles user administration including:
//! - User List: Create, edit, delete users
//! - Roles & Permissions: Role assignment
//!
//! Note: This section is only visible to users with user management permissions.

pub mod messages;
pub mod state;
pub mod update;

pub use messages::UsersMessage;
pub use state::UsersState;

use crate::common::messages::DomainUpdateResult;
use crate::state::State;

/// Users section marker for type-safe section identification
pub struct UsersSection;

/// Update users settings state
pub fn update(state: &mut State, message: UsersMessage) -> DomainUpdateResult {
    update::handle_message(state, message)
}
