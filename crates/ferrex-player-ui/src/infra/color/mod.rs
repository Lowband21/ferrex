//! Color utilities for perceptually uniform color handling.
//!
//! Core color DTOs and harmony helpers are owned by `ferrex-player-settings`
//! so settings state and UI rendering share one type surface.

pub use ferrex_player_settings::color::{
    ColorPoint, HarmonyMode, HsluvColor, get_harmony_colors, handle_linked_drag,
};
