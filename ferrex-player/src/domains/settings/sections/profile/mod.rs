//! Profile settings sub-domain
//!
//! Handles user profile management including:
//! - Account: Display name, email, avatar
//! - Authentication: Logout, switch user

pub mod messages;
pub mod state;
pub mod update;

pub use messages::ProfileMessage;
pub use state::ProfileState;

use crate::common::messages::DomainUpdateResult;
use crate::state::State;

/// Profile section marker for type-safe section identification
#[derive(Debug)]
pub struct ProfileSection;

/// Update profile settings state
pub fn update(
    state: &mut State,
    message: ProfileMessage,
) -> DomainUpdateResult {
    update::handle_message(state, message)
}
