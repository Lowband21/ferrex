//! Security settings sub-domain
//!
//! Handles security-related settings including:
//! - PIN: Set/change PIN
//! - Password: Change password
//!
//! Note: This wraps the existing security state from the monolithic settings.

pub mod messages;
pub mod state;
pub mod update;

pub use messages::SecurityMessage;
pub use state::SecurityState;

use crate::common::messages::DomainUpdateResult;
use crate::state::State;

/// Security section marker for type-safe section identification
#[derive(Debug)]
pub struct SecuritySection;

/// Update security settings state
pub fn update(
    state: &mut State,
    message: SecurityMessage,
) -> DomainUpdateResult {
    update::handle_message(state, message)
}
