//! Constants module for centralized configuration values

pub mod layout;
pub mod performance_config;
pub mod routes;
pub mod player;

// Re-export commonly used items
pub use layout::{
    animation, calculations, grid, player_controls, poster, scale_presets, virtual_grid,
};
pub use performance_config::*;
