//! Constants module for centralized configuration values

pub mod curated;
pub mod layout;
pub mod menu;
pub mod performance_config;
pub mod player;
pub mod virtual_carousel;

// Re-export commonly used items
pub use layout::{
    animation, calculations, grid, player_controls, poster, scale_presets,
    search, virtual_grid,
};
pub use performance_config::*;
