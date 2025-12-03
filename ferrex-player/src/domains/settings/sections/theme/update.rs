//! Theme section update handlers

use crate::infra::color::HarmonyMode;
use crate::infra::shader_widgets::color_picker::AccentColorConfig;
use crate::infra::theme::set_accent;

use super::messages::ThemeMessage;
use crate::common::messages::DomainUpdateResult;
use crate::state::State;

/// Main message handler for theme section
pub fn handle_message(
    state: &mut State,
    message: ThemeMessage,
) -> DomainUpdateResult {
    match message {
        ThemeMessage::SetAccentHueSat { hue, saturation } => {
            set_accent_hue_sat(state, hue, saturation)
        }
        ThemeMessage::SetAccentLightness(lightness) => {
            set_accent_lightness(state, lightness)
        }
        ThemeMessage::SetHarmonyMode(mode) => set_harmony_mode(state, mode),
        ThemeMessage::ResetToDefault => reset_to_default(state),
        ThemeMessage::PickerDragStarted => picker_drag_started(state),
        ThemeMessage::PickerDragEnded => picker_drag_ended(state),
    }
}

fn set_accent_hue_sat(
    state: &mut State,
    hue: f32,
    saturation: f32,
) -> DomainUpdateResult {
    let accent = &mut state.domains.settings.theme.accent_color;
    accent.primary_hue = hue.rem_euclid(360.0);
    accent.primary_saturation = saturation.clamp(0.0, 100.0);
    // Update global accent for live preview
    set_accent(accent.primary_color());
    DomainUpdateResult::none()
}

fn set_accent_lightness(
    state: &mut State,
    lightness: f32,
) -> DomainUpdateResult {
    let accent = &mut state.domains.settings.theme.accent_color;
    accent.lightness = lightness.clamp(0.0, 100.0);
    // Update global accent for live preview
    set_accent(accent.primary_color());
    DomainUpdateResult::none()
}

fn set_harmony_mode(
    state: &mut State,
    mode: HarmonyMode,
) -> DomainUpdateResult {
    state.domains.settings.theme.accent_color.harmony_mode = mode;
    DomainUpdateResult::none()
}

fn reset_to_default(state: &mut State) -> DomainUpdateResult {
    state.domains.settings.theme.accent_color = AccentColorConfig::default();
    state.domains.settings.theme.picker_active = false;
    // Reset global accent
    set_accent(state.domains.settings.theme.accent_color.primary_color());
    DomainUpdateResult::none()
}

fn picker_drag_started(state: &mut State) -> DomainUpdateResult {
    state.domains.settings.theme.picker_active = true;
    DomainUpdateResult::none()
}

fn picker_drag_ended(state: &mut State) -> DomainUpdateResult {
    state.domains.settings.theme.picker_active = false;
    DomainUpdateResult::none()
}
