// Centralized service abstractions and compatibility utilities
// RUS-136 Phase 0: Compatibility layer

pub mod api;
pub mod auth;
pub mod metadata;
pub mod settings;
pub mod streaming;
pub mod user_management;

pub use ferrex_player_api::services::{
    CompatToggles, ServiceBuilder, ServiceRef,
};
