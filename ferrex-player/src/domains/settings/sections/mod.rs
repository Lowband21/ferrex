//! Settings sub-domains
//!
//! Each section has its own isolated state, messages, and update handlers.
//! This enables clean separation of concerns and isolated message routing.

pub mod devices;
pub mod display;
pub mod libraries;
pub mod performance;
pub mod playback;
pub mod profile;
pub mod security;
pub mod server;
pub mod theme;
pub mod users;

// Re-export section types for convenience
pub use devices::DevicesSection;
pub use display::DisplaySection;
pub use libraries::LibrariesSection;
pub use performance::PerformanceSection;
pub use playback::PlaybackSection;
pub use profile::ProfileSection;
pub use security::SecuritySection;
pub use server::ServerSection;
pub use theme::ThemeSection;
pub use users::UsersSection;
