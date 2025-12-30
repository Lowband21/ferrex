//! Library management sub-domain (Admin)
//!
//! Handles library administration including:
//! - Library List: Add, edit, delete, scan controls
//! - Scan Settings: Auto-scan, scan intervals
//!
//! Note: This section is only visible to users with library management permissions.

pub mod messages;
pub mod state;
pub mod update;

pub use messages::LibrariesMessage;
pub use state::LibrariesState;

use crate::common::messages::DomainUpdateResult;
use crate::state::State;

/// Libraries section marker for type-safe section identification
#[derive(Debug)]
pub struct LibrariesSection;

/// Update libraries settings state
pub fn update(
    state: &mut State,
    message: LibrariesMessage,
) -> DomainUpdateResult {
    update::handle_message(state, message)
}
