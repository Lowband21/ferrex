//! UI update handlers
//!
//! Contains specific update logic for UI-related messages

pub mod curated;
#[cfg(feature = "demo")]
pub mod demo_controls;
pub mod home_focus;
pub mod home_tab;
pub mod navigation_updates;
pub mod scroll_prefetch;
pub mod scroll_updates;
pub mod search_updates;
pub mod virtual_carousel_helpers;
pub mod virtual_carousel_updates;
pub mod window_update;

// Re-export update functions
pub use curated::*;
#[cfg(feature = "demo")]
pub use demo_controls::*;
pub use home_focus::*;
pub use home_tab::{
    emit_initial_all_tab_snapshots_combined, init_all_tab_view,
    restore_all_tab_carousel_scroll_positions,
};
pub use navigation_updates::*;
pub use scroll_updates::*;
pub use search_updates::*;
pub use virtual_carousel_helpers::*;
pub use virtual_carousel_updates::*;
pub use window_update::*;
