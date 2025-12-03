//! Settings section view functions
//!
//! Each module provides a view function for rendering its settings section content.
//! These are used by the main settings view with sidebar.

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

pub use devices::view_devices_section;
pub use display::view_display_section;
pub use libraries::view_libraries_section;
pub use performance::view_performance_section;
pub use playback::view_playback_section;
pub use profile::view_profile_section;
pub use security::view_security_section;
pub use server::view_server_section;
pub use theme::view_theme_section;
pub use users::view_users_section;
