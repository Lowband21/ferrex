//! Device management sub-domain
//!
//! Handles device-related settings including:
//! - Trusted Devices: List, revoke, current device indicator

pub mod messages;
pub mod state;
pub mod update;

use crate::common::messages::DomainUpdateResult;
use crate::state::State;
pub use messages::DevicesMessage;

/// Devices section marker for type-safe section identification
#[derive(Debug)]
pub struct DevicesSection;

/// Update devices settings state
pub fn update(
    state: &mut State,
    message: DevicesMessage,
) -> DomainUpdateResult {
    update::handle_message(state, message)
}
