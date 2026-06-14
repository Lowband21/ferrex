//! Desktop settings update adapter.
//!
//! `ferrex-player-settings` owns the UI-agnostic reducers. This module maps the
//! reducer effects to desktop tasks, cross-domain events, and runtime UI caches.

pub mod device_management;
pub mod navigation;
pub mod preferences;
pub mod profile;
pub mod security;

use crate::{
    common::messages::{CrossDomainEvent, DomainMessage, DomainUpdateResult},
    domains::{
        auth::types::AuthenticationFlow, settings::messages::SettingsMessage,
    },
    infra::{
        constants::layout::calculations::ScaledLayout,
        design_tokens::{ScalingContext, SizeProvider},
        shader_widgets::poster,
        theme::set_accent,
    },
    state::State,
};
use ferrex_player_api::auth::AutoLoginScope;
use ferrex_player_settings::update::{
    SettingsEffect, SettingsUpdate, SettingsUpdateContext, SettingsUpdateTarget,
};
use iced::Task;

/// Main settings update handler.
pub fn update_settings(
    state: &mut State,
    message: SettingsMessage,
) -> DomainUpdateResult {
    if let SettingsMessage::AutoLoginToggled(result) = &message {
        sync_auto_login_auth_state(state, result);
    }

    let pin_policy = (&state.domains.auth.state.pin_policy).into();
    let update = {
        let settings = &mut state.domains.settings;
        ferrex_player_settings::update::update_settings(
            SettingsUpdateTarget {
                current_section: &mut settings.current_section,
                security: &mut settings.security,
                profile: &mut settings.profile,
                preferences: &mut settings.preferences,
                device_management: &mut settings.device_management_state,
                playback: &mut settings.playback,
                display: &mut settings.display,
                theme: &mut settings.theme,
                performance: &mut settings.performance,
            },
            message,
            SettingsUpdateContext { pin_policy },
        )
    };

    apply_settings_update(state, update)
}

pub(crate) fn apply_settings_update(
    state: &mut State,
    update: SettingsUpdate,
) -> DomainUpdateResult {
    let mut tasks = Vec::new();
    let mut events = Vec::new();

    for effect in update.effects {
        match effect {
            SettingsEffect::CheckUserHasPin => {
                tasks.push(security::check_user_has_pin_task(state));
            }
            SettingsEffect::LoadDevices => {
                tasks.push(device_management::load_devices_task(state));
            }
            SettingsEffect::RevokeDevice {
                device_id,
                original,
            } => {
                tasks.push(device_management::revoke_device_task(
                    state, device_id, original,
                ));
            }
            SettingsEffect::ToggleAutoLogin { enabled } => {
                tasks.push(toggle_auto_login_task(state, enabled));
            }
            SettingsEffect::AuthCommandRequested(command) => {
                events.push(CrossDomainEvent::AuthCommandRequested(command));
            }
            SettingsEffect::ApplyUserScale(user_scale) => {
                apply_user_scale_runtime(state, user_scale.scale_factor());
            }
            SettingsEffect::ApplyScalePreset(preset) => {
                apply_scale_preset_runtime(state, preset);
            }
            SettingsEffect::RefreshGridLayout => refresh_grid_layout(state),
            SettingsEffect::ApplyAccentColor(color) => set_accent(color),
            SettingsEffect::ApplyHoverAnimation {
                scale,
                transition_ms,
                scale_down_delay_ms,
            } => apply_hover_animation(
                state,
                scale,
                transition_ms,
                scale_down_delay_ms,
            ),
            SettingsEffect::SubmitProfileChanges => {
                tasks.push(Task::done(DomainMessage::Settings(
                    SettingsMessage::ProfileChangeResult(Err(
                        "Profile updates are not available from this client yet"
                            .to_string(),
                    )),
                )));
            }
            SettingsEffect::Logout => {
                events.push(CrossDomainEvent::UserLoggedOut)
            }
            SettingsEffect::SwitchUser => {
                events.push(CrossDomainEvent::UserLoggedOut)
            }
        }
    }

    let task = if tasks.is_empty() {
        Task::none()
    } else {
        Task::batch(tasks)
    };

    DomainUpdateResult::with_events(task, events)
}

fn toggle_auto_login_task(state: &State, enabled: bool) -> Task<DomainMessage> {
    let auth_service = state.domains.settings.auth_service.clone();
    Task::perform(
        async move {
            auth_service
                .set_auto_login_scope(enabled, AutoLoginScope::UserDefault)
                .await
                .map_err(|error| error.to_string())?;
            auth_service
                .set_auto_login_scope(enabled, AutoLoginScope::DeviceOnly)
                .await
                .map_err(|error| error.to_string())?;
            Ok(enabled)
        },
        |result| {
            DomainMessage::Settings(SettingsMessage::AutoLoginToggled(result))
        },
    )
}

fn apply_user_scale_runtime(state: &mut State, user_scale: f32) {
    state
        .domains
        .ui
        .state
        .scaling_context
        .set_user_scale(user_scale);
    refresh_scaled_runtime(state);
}

fn apply_scale_preset_runtime(
    state: &mut State,
    preset: ferrex_player_settings::ScalePreset,
) {
    state.domains.ui.state.scaling_context =
        ScalingContext::from_preset(preset);
    refresh_scaled_runtime(state);
}

fn refresh_scaled_runtime(state: &mut State) {
    let effective_scale =
        state.domains.ui.state.scaling_context.effective_scale();
    let poster_gap = state.domains.settings.display.grid_poster_gap;
    state.domains.ui.state.size_provider =
        SizeProvider::new(state.domains.ui.state.scaling_context);
    state.domains.ui.state.scaled_layout =
        ScaledLayout::new(effective_scale, poster_gap);
    poster::set_text_scale(effective_scale);
    refresh_grid_instances(state);
    refresh_carousel_instances(state);
}

pub(crate) fn refresh_grid_layout(state: &mut State) {
    let effective_scale =
        state.domains.ui.state.scaling_context.effective_scale();
    let poster_gap = state.domains.settings.display.grid_poster_gap;
    state.domains.ui.state.scaled_layout =
        ScaledLayout::new(effective_scale, poster_gap);
    refresh_grid_instances(state);
}

fn refresh_grid_instances(state: &mut State) {
    for tab_id in state.tab_manager.tab_ids() {
        if let Some(tab) = state.tab_manager.get_tab_mut(tab_id)
            && let Some(grid_state) = tab.grid_state_mut()
        {
            grid_state.update_for_scale(&state.domains.ui.state.scaled_layout);
        }
    }
}

fn refresh_carousel_instances(state: &mut State) {
    for key in state.domains.ui.state.carousel_registry.keys() {
        if let Some(carousel) =
            state.domains.ui.state.carousel_registry.get_mut(&key)
        {
            carousel.update_dimensions(state.window_size.width.max(1.0));
        }
    }
}

fn apply_hover_animation(
    state: &mut State,
    scale: Option<f32>,
    transition_ms: Option<u64>,
    scale_down_delay_ms: Option<u64>,
) {
    if let Some(scale) = scale {
        state.runtime_config.animation_hover_scale = Some(scale);
        poster::set_hover_scale(scale);
    }
    if let Some(transition_ms) = transition_ms {
        state.runtime_config.animation_hover_transition_ms =
            Some(transition_ms);
        poster::set_hover_transition_ms(transition_ms);
    }
    if let Some(scale_down_delay_ms) = scale_down_delay_ms {
        state.runtime_config.animation_hover_scale_down_delay_ms =
            Some(scale_down_delay_ms);
        poster::set_hover_scale_down_delay_ms(scale_down_delay_ms);
    }
    state.runtime_config.mark_dirty();
}

pub(crate) fn sync_auto_login_auth_state(
    state: &mut State,
    result: &Result<bool, String>,
) {
    if let Ok(enabled) = result {
        state.domains.auth.state.auto_login_enabled = *enabled;
        if let AuthenticationFlow::EnteringCredentials {
            remember_device, ..
        } = &mut state.domains.auth.state.auth_flow
        {
            *remember_device = *enabled;
        }
    }
}
