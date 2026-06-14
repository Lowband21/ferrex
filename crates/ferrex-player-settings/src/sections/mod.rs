//! Settings sub-domains
//!
//! Each section has its own isolated state and messages.
//! This enables clean separation of concerns and shared message routing.

pub mod devices;
pub mod display;
pub mod performance;
pub mod playback;
pub mod profile;
pub mod security;
pub mod theme;
