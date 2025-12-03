//! Display settings sub-domain
//!
//! Handles display/UI-related settings including:
//! - Theme: Light/Dark/System
//! - Grid Layout: Grid size, poster titles, show recently watched, show continue watching
//! - Poster: Base dimensions, corner radius, text area height
//! - Spacing: Grid spacing, row spacing, viewport padding
//! - Animation: Hover scale, durations, texture fade

pub mod messages;
pub mod state;
pub mod update;

pub use messages::DisplayMessage;
pub use state::DisplayState;

use crate::common::messages::DomainUpdateResult;
use crate::state::State;

/// Display section marker for type-safe section identification
pub struct DisplaySection;

/// Update display settings state
pub fn update(
    state: &mut State,
    message: DisplayMessage,
) -> DomainUpdateResult {
    update::handle_message(state, message)
}
