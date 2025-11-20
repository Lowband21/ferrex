//! Performance configuration constants for the Ferrex Player
//!
//! This module centralizes all performance-related settings to make tuning easier.
//! Adjust these values based on your target hardware and performance requirements.

/// Scrolling performance configuration
pub mod scrolling {
    /// Time to wait before considering scroll stopped (milliseconds)
    /// Lower values = quicker poster loading after scroll
    pub const SCROLL_STOP_DEBOUNCE_MS: u64 = 20;
}

/// Texture upload budgeting configuration
pub mod texture_upload {
    /// Maximum texture uploads allowed per frame
    /// Each upload takes ~3ms, with all other operations being relatively insignificant
    /// So 3 uploads = 9ms (close to our target ~8ms frame budget)
    /// Adjust based on target hardware:
    /// - Low-end: 1-2 uploads
    /// - Mid-range: 2-3 uploads
    /// - High-end: 4+ uploads
    pub const MAX_UPLOADS_PER_FRAME: u32 = 5;
}
