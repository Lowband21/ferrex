//! Theme settings sub-domain
//!
//! Handles theme-related settings including:
//! - Accent color selection via color picker

pub mod messages;
pub mod state;
pub mod update;

pub use messages::ThemeMessage;
pub use state::ThemeState;

use crate::common::messages::DomainUpdateResult;
use crate::state::State;

/// Theme section marker for type-safe section identification
pub struct ThemeSection;

/// Update theme settings state
pub fn update(state: &mut State, message: ThemeMessage) -> DomainUpdateResult {
    update::handle_message(state, message)
}
