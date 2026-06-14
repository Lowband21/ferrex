//! Settings sub-domains
//!
//! Each section re-exports shared state and messages with desktop update
//! adapters for compatibility with existing app module paths.

pub mod devices;
pub mod display;
pub mod performance;
pub mod playback;
pub mod profile;
pub mod security;
pub mod theme;

// Re-export section types for convenience
pub use devices::DevicesSection;
pub use display::DisplaySection;
pub use performance::PerformanceSection;
pub use playback::PlaybackSection;
pub use profile::ProfileSection;
pub use security::SecuritySection;
pub use theme::ThemeSection;
