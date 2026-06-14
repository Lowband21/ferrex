//! Settings navigation reducers.

use crate::state::{SecurityState, SettingsSection};

use super::{SettingsEffect, SettingsUpdate};

/// Change the active settings section and request section-specific loads.
pub fn navigate(
    current_section: &mut SettingsSection,
    security: &mut SecurityState,
    section: SettingsSection,
) -> SettingsUpdate {
    *current_section = section;

    if section != SettingsSection::Security {
        security.clear_sensitive_data();
        security.password_error = None;
        security.pin_error = None;
        security.password_loading = false;
        security.pin_loading = false;
        security.showing_password_change = false;
        security.showing_pin_change = false;
    }

    match section {
        SettingsSection::Security => {
            security.checking_pin_status = true;
            SettingsUpdate::effect(SettingsEffect::CheckUserHasPin)
        }
        SettingsSection::Devices => {
            SettingsUpdate::effect(SettingsEffect::LoadDevices)
        }
        _ => SettingsUpdate::none(),
    }
}

/// Clear sensitive settings state without changing the current section.
pub fn clear_sensitive_security(
    security: &mut SecurityState,
) -> SettingsUpdate {
    security.clear_sensitive_data();
    security.password_error = None;
    security.pin_error = None;
    security.password_loading = false;
    security.pin_loading = false;
    SettingsUpdate::none()
}
