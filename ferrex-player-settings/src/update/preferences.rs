//! Settings preference reducers.

use ferrex_core::player_prelude::UserScale;

use crate::{ScalePreset, state::PreferencesState};

use super::{SettingsEffect, SettingsUpdate};

/// Request auto-login persistence and mark the preference as busy.
pub fn toggle_auto_login(
    preferences: &mut PreferencesState,
    enabled: bool,
) -> SettingsUpdate {
    preferences.loading = true;
    preferences.error = None;
    SettingsUpdate::effect(SettingsEffect::ToggleAutoLogin { enabled })
}

/// Apply the result of an auto-login persistence request.
pub fn auto_login_toggled(
    preferences: &mut PreferencesState,
    result: Result<bool, String>,
) -> SettingsUpdate {
    preferences.loading = false;
    match result {
        Ok(enabled) => {
            preferences.auto_login_enabled = enabled;
            preferences.error = None;
        }
        Err(error) => {
            preferences.error = Some(error);
        }
    }
    SettingsUpdate::none()
}

/// Store the selected user scale and request app runtime rescaling.
pub fn set_user_scale(
    preferences: &mut PreferencesState,
    user_scale: UserScale,
) -> SettingsUpdate {
    preferences.user_scale = user_scale.clone();
    SettingsUpdate::effect(SettingsEffect::ApplyUserScale(user_scale))
}

/// Request app runtime rescaling from a named preset.
pub fn set_scale_preset(preset: ScalePreset) -> SettingsUpdate {
    SettingsUpdate::effect(SettingsEffect::ApplyScalePreset(preset))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_login_result_updates_state() {
        let mut preferences = PreferencesState::default();
        preferences.loading = true;

        auto_login_toggled(&mut preferences, Ok(true));

        assert!(preferences.auto_login_enabled);
        assert!(!preferences.loading);
        assert!(preferences.error.is_none());
    }
}
