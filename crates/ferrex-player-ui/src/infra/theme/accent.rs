//! Accent theme with user-configurable base color
//!
//! Provides a shared accent color system that can be read from any thread.
//! The accent color is stored atomically for lock-free reads in theme functions.

use std::sync::atomic::{AtomicU32, Ordering};

use iced::Color;

use super::colors::{brighten, lighten, with_alpha};

// =============================================================================
// Default Accent Color
// =============================================================================

/// Default accent color (magenta from original theme)
pub const DEFAULT_ACCENT: Color = Color::from_rgb(0.867, 0.0, 0.867);

/// Default accent hover color (slightly lighter)
pub const DEFAULT_ACCENT_HOVER: Color = Color::from_rgb(0.9, 0.0, 0.9);

/// Default accent glow color (with alpha for shadows)
pub const DEFAULT_ACCENT_GLOW: Color = Color {
    r: 0.867,
    g: 0.5,
    b: 0.867,
    a: 0.3,
};

// =============================================================================
// Atomic Accent Color Storage
// =============================================================================

/// Packed RGB as u32: 0x00RRGGBB (each channel 0-255)
static ACCENT_RGB: AtomicU32 = AtomicU32::new(pack_color_const(DEFAULT_ACCENT));

/// Pack a Color into u32 at compile time
const fn pack_color_const(color: Color) -> u32 {
    let r = (color.r * 255.0) as u32;
    let g = (color.g * 255.0) as u32;
    let b = (color.b * 255.0) as u32;
    (r << 16) | (g << 8) | b
}

/// Pack a Color into u32 at runtime
fn pack_color(color: Color) -> u32 {
    let r = (color.r * 255.0) as u32;
    let g = (color.g * 255.0) as u32;
    let b = (color.b * 255.0) as u32;
    (r << 16) | (g << 8) | b
}

/// Unpack u32 into Color
fn unpack_color(packed: u32) -> Color {
    let r = ((packed >> 16) & 0xFF) as u8;
    let g = ((packed >> 8) & 0xFF) as u8;
    let b = (packed & 0xFF) as u8;
    Color::from_rgb8(r, g, b)
}

// =============================================================================
// Public API - Accent Color Access
// =============================================================================

/// Set the global accent color
///
/// This updates the atomic storage that all theme functions read from.
/// Call this when the user changes their accent color preference.
pub fn set_accent(color: Color) {
    ACCENT_RGB.store(pack_color(color), Ordering::Relaxed);
}

/// Get the current accent color
///
/// This is a lock-free atomic read, safe to call from any thread.
pub fn accent() -> Color {
    unpack_color(ACCENT_RGB.load(Ordering::Relaxed))
}

/// Get the accent hover color (slightly lighter than base)
pub fn accent_hover() -> Color {
    lighten(accent(), 0.03)
}

/// Get the accent glow color (base with reduced alpha for shadows)
pub fn accent_glow() -> Color {
    with_alpha(accent(), 0.3)
}

/// Reset the accent color to the default
pub fn reset_accent() {
    ACCENT_RGB.store(pack_color_const(DEFAULT_ACCENT), Ordering::Relaxed);
}

// =============================================================================
// AccentTheme Struct
// =============================================================================

/// Computed accent theme with base, hover, and glow variants
///
/// Use this when you need a snapshot of accent colors that won't change
/// during a render pass.
#[derive(Debug, Clone, Copy)]
pub struct AccentTheme {
    pub accent: Color,
    pub accent_hover: Color,
    pub accent_glow: Color,
}

impl Default for AccentTheme {
    fn default() -> Self {
        Self::from_base(DEFAULT_ACCENT)
    }
}

impl AccentTheme {
    /// Create an AccentTheme from a base color
    pub fn from_base(base: Color) -> Self {
        Self {
            accent: base,
            accent_hover: lighten(base, 0.03),
            accent_glow: with_alpha(base, 0.3),
        }
    }

    /// Create an AccentTheme from RGB u8 values (0-255)
    pub fn from_rgb_u8(r: u8, g: u8, b: u8) -> Self {
        Self::from_base(Color::from_rgb8(r, g, b))
    }

    /// Get the current accent theme from global state
    pub fn current() -> Self {
        Self::from_base(accent())
    }

    /// Create a brighter variant for pressed/active states
    pub fn bright(&self) -> Color {
        brighten(self.accent, 0.2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pack_unpack_roundtrip() {
        let color = Color::from_rgb8(0xDD, 0x00, 0xDD);
        let packed = pack_color(color);
        let unpacked = unpack_color(packed);
        assert!((color.r - unpacked.r).abs() < 0.01);
        assert!((color.g - unpacked.g).abs() < 0.01);
        assert!((color.b - unpacked.b).abs() < 0.01);
    }

    #[test]
    fn test_set_and_get_accent() {
        let custom = Color::from_rgb8(0x00, 0x80, 0xFF);
        set_accent(custom);
        let result = accent();
        assert!((result.r - custom.r).abs() < 0.01);
        assert!((result.g - custom.g).abs() < 0.01);
        assert!((result.b - custom.b).abs() < 0.01);

        // Reset for other tests
        reset_accent();
    }

    #[test]
    fn test_accent_theme_from_base() {
        let theme = AccentTheme::from_base(Color::from_rgb(0.5, 0.0, 0.5));
        assert!((theme.accent.r - 0.5).abs() < 0.01);
        assert!((theme.accent_glow.a - 0.3).abs() < 0.01);
    }
}
