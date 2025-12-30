//! Server settings sub-domain (Admin)
//!
//! Handles server-wide settings including:
//! - Session Policies: Token lifetimes, max concurrent sessions
//! - Device Policies: Trust duration, max trusted devices
//! - Password Policies: Min length, complexity requirements
//! - Curated Content: Max carousel items, head window
//!
//! Note: This section is only visible to users with server settings permissions.

pub mod messages;
pub mod state;
pub mod update;

pub use messages::ServerMessage;
pub use state::ServerState;

use crate::common::messages::DomainUpdateResult;
use crate::state::State;

/// Server section marker for type-safe section identification
#[derive(Debug)]
pub struct ServerSection;

/// Update server settings state
pub fn update(state: &mut State, message: ServerMessage) -> DomainUpdateResult {
    update::handle_message(state, message)
}
