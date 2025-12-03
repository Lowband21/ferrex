//! Performance settings sub-domain
//!
//! Handles performance tuning settings including:
//! - Scrolling: Debounce, velocity, easing, boost multiplier
//! - Texture Upload: GPU budget per frame
//! - Prefetch: Virtual grid row prefetch settings
//! - Carousel: Motion, snap, and prefetch settings

pub mod messages;
pub mod state;
pub mod update;

pub use messages::PerformanceMessage;
pub use state::PerformanceState;

use crate::common::messages::DomainUpdateResult;
use crate::state::State;

/// Performance section marker for type-safe section identification
pub struct PerformanceSection;

/// Update performance settings state
pub fn update(
    state: &mut State,
    message: PerformanceMessage,
) -> DomainUpdateResult {
    update::handle_message(state, message)
}
