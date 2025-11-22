//! All tab (Home) orchestration helpers
//!
//! Consolidates initialization and initial snapshot emission across
//! curated carousels and per-library carousels used in the All view.

use crate::domains::ui::update_handlers::curated::{
    emit_initial_curated_snapshots, recompute_and_init_curated_carousels,
};
use crate::domains::ui::update_handlers::virtual_carousel_helpers::{
    emit_initial_all_tab_snapshots, init_all_tab_virtual_carousels,
};
use crate::state::State;

// Re-export for convenience
pub use crate::domains::ui::update_handlers::virtual_carousel_helpers::restore_all_tab_carousel_scroll_positions;

/// Initialize all All-tab carousels (curated + per-library) with current width
/// and recompute curated lists.
pub fn init_all_tab_view(state: &mut State) {
    // Per-library carousels (Movies/Series) backed by library tab caches
    init_all_tab_virtual_carousels(state);
    // Curated carousels (Continue Watching, Recently Added/Released)
    recompute_and_init_curated_carousels(state);
}

/// Emit initial DemandPlanner snapshots for both per-library and curated
/// carousels so posters begin loading immediately.
pub fn emit_initial_all_tab_snapshots_combined(state: &mut State) {
    // Per-library carousels
    emit_initial_all_tab_snapshots(state);
    // Curated carousels
    emit_initial_curated_snapshots(state);
}
