//! Shared theme infra for accent colors
//!
//! This module provides the accent color system used by both the UI and Player domains.
//! It enables user-configurable accent colors with live updates.
//!
//! # Architecture
//!
//! - `accent.rs`: Atomic accent color storage and `AccentTheme` struct
//! - `colors.rs`: Color manipulation utilities (lighten, brighten, with_alpha)
//!
//! # Usage
//!
//! ```rust,ignore
//! use crate::infra::theme::{accent, accent_hover, accent_glow, set_accent, AccentTheme};
//!
//! // Read current accent (lock-free atomic read)
//! let color = accent();
//!
//! // Set new accent color (from settings)
//! set_accent(Color::from_rgb8(0x00, 0x80, 0xFF));
//!
//! // Get a snapshot for consistent rendering
//! let theme = AccentTheme::current();
//! ```

pub mod accent;
pub mod colors;
pub mod media_theme_color;

// Re-export primary types and functions
pub use accent::{
    AccentTheme, DEFAULT_ACCENT, DEFAULT_ACCENT_GLOW, DEFAULT_ACCENT_HOVER,
    accent, accent_glow, accent_hover, reset_accent, set_accent,
};
pub use colors::{brighten, darken, lighten, with_alpha};
pub use media_theme_color::fallback_theme_color_for;
