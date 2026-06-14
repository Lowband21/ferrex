//! Virtual Carousel (scaffold)
//!
//! This module provides a reusable, viewport-aware, horizontally scrolling
//! virtual carousel. It mirrors the reliability and performance patterns of
//! the grid (virtual list), with a structure that cleanly separates state,
//! controller logic, view composition, and integration points (registry and
//! planner helpers).
//!
//! Milestone: Scaffold only. Implementations are intentionally lightweight
//! stubs with clear TODOs for subsequent phases.

pub mod animator;
pub mod focus;
pub mod messages;
pub mod planner;
pub mod registry;
pub mod state;
pub mod types;
pub mod view;

pub use focus::CarouselFocus;
pub use messages::VirtualCarouselMessage;
pub use registry::CarouselRegistry;
pub use state::VirtualCarouselState;
pub use types::*;
pub use view::virtual_carousel;
