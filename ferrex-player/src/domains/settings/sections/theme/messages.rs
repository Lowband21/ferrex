//! Theme section messages

use crate::infra::color::HarmonyMode;

/// Messages for the theme settings section
#[derive(Debug, Clone)]
pub enum ThemeMessage {
    /// Set accent color hue and saturation (from color picker wheel)
    SetAccentHueSat { hue: f32, saturation: f32 },
    /// Set accent color lightness (from slider)
    SetAccentLightness(f32),
    /// Set color harmony mode
    SetHarmonyMode(HarmonyMode),
    /// Reset accent color to default
    ResetToDefault,
    /// Color picker drag started
    PickerDragStarted,
    /// Color picker drag ended
    PickerDragEnded,
}

impl ThemeMessage {
    pub fn name(&self) -> &'static str {
        match self {
            Self::SetAccentHueSat { .. } => "Theme::SetAccentHueSat",
            Self::SetAccentLightness(_) => "Theme::SetAccentLightness",
            Self::SetHarmonyMode(_) => "Theme::SetHarmonyMode",
            Self::ResetToDefault => "Theme::ResetToDefault",
            Self::PickerDragStarted => "Theme::PickerDragStarted",
            Self::PickerDragEnded => "Theme::PickerDragEnded",
        }
    }
}
