//! Profile settings reducers.

use crate::state::ProfileState;

use super::{SettingsEffect, SettingsUpdate};

pub fn update_display_name(
    profile: &mut ProfileState,
    name: String,
) -> SettingsUpdate {
    profile.display_name = name;
    profile.error = None;
    profile.success_message = None;
    SettingsUpdate::none()
}

pub fn update_email(
    profile: &mut ProfileState,
    email: String,
) -> SettingsUpdate {
    profile.email = email;
    profile.error = None;
    profile.success_message = None;
    SettingsUpdate::none()
}

pub fn submit_profile_changes(profile: &mut ProfileState) -> SettingsUpdate {
    if profile.display_name.trim().is_empty() {
        profile.error = Some("Display name is required".to_string());
        profile.loading = false;
        return SettingsUpdate::none();
    }

    if !profile.email.trim().is_empty() && !profile.email.contains('@') {
        profile.error = Some("Email address must contain @".to_string());
        profile.loading = false;
        return SettingsUpdate::none();
    }

    profile.loading = true;
    profile.error = None;
    profile.success_message = None;
    SettingsUpdate::effect(SettingsEffect::SubmitProfileChanges)
}

pub fn profile_change_result(
    profile: &mut ProfileState,
    result: Result<(), String>,
) -> SettingsUpdate {
    profile.loading = false;
    match result {
        Ok(()) => {
            profile.error = None;
            profile.success_message = Some("Profile updated".to_string());
        }
        Err(error) => {
            profile.error = Some(error);
            profile.success_message = None;
        }
    }
    SettingsUpdate::none()
}
