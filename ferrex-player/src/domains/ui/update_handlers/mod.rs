//! UI update handlers
//!
//! Contains specific update logic for UI-related messages

pub mod all_focus;
pub mod all_tab;
pub mod curated;
#[cfg(feature = "demo")]
pub mod demo_controls;
pub mod navigation_updates;
pub mod scroll_updates;
pub mod search_updates;
pub mod virtual_carousel_helpers;
pub mod virtual_carousel_updates;
pub mod window_update;

// Re-export update functions
pub use all_focus::*;
pub use all_tab::{
    emit_initial_all_tab_snapshots_combined, init_all_tab_view,
    restore_all_tab_carousel_scroll_positions,
};
pub use curated::*;
#[cfg(feature = "demo")]
pub use demo_controls::*;
pub use navigation_updates::*;
pub use scroll_updates::*;
pub use search_updates::*;
pub use virtual_carousel_helpers::*;
pub use virtual_carousel_updates::*;
pub use window_update::*;
