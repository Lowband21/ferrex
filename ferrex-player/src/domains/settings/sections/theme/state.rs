//! Theme section state
//!
//! Contains all state related to theme/accent color settings.

use iced::Color;
use serde::{Deserialize, Serialize};

use crate::infra::shader_widgets::color_picker::AccentColorConfig;

/// Theme settings state
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ThemeState {
    /// Accent color configuration (HSLuv-based with harmony support)
    pub accent_color: AccentColorConfig,

    /// Whether color picker interaction is active (for UI state)
    #[serde(skip)]
    pub picker_active: bool,
}

impl ThemeState {
    /// Get the effective accent color as sRGB
    pub fn effective_accent(&self) -> Color {
        self.accent_color.primary_color()
    }

    /// Get all harmony colors (primary + complements)
    pub fn all_accent_colors(&self) -> Vec<Color> {
        self.accent_color.all_colors()
    }
}
