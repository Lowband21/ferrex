//! Streaming update handlers
//!
//! Contains specific update logic for streaming-related messages

pub mod transcoding;

// Re-export update functions
pub use transcoding::*;