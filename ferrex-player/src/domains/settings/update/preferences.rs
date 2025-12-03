use super::super::messages::SettingsMessage;
use crate::common::messages::{DomainMessage, DomainUpdateResult};
use crate::domains::auth::errors::{AuthError, NetworkError};
use crate::domains::auth::manager::AutoLoginScope;
use crate::infra::{
    constants::layout::calculations::ScaledLayout,
    design_tokens::{ScalePreset, SizeProvider},
    shader_widgets::poster,
};
use crate::state::State;
use ferrex_core::player_prelude::UserScale;
use iced::Task;

/// Handle toggle auto-login preference
#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn handle_toggle_auto_login(
    state: &mut State,
    enabled: bool,
) -> DomainUpdateResult {
    let auth_service = state.domains.settings.auth_service.clone();
    // We need to update both the device-specific setting and synced preference via auth service
    let task = Task::perform(
        async move {
            // First update the device-specific setting
            auth_service
                .set_auto_login_scope(enabled, AutoLoginScope::UserDefault)
                .await
                .map_err(|e| {
                    AuthError::Network(NetworkError::RequestFailed(
                        e.to_string(),
                    ))
                })?;

            auth_service
                .set_auto_login_scope(enabled, AutoLoginScope::DeviceOnly)
                .await
                .map_err(|e| {
                    AuthError::Network(NetworkError::RequestFailed(
                        e.to_string(),
                    ))
                })?;

            Ok(enabled)
        },
        |result| {
            SettingsMessage::AutoLoginToggled(
                result.map_err(|e: AuthError| e.to_string()),
            )
        },
    );
    DomainUpdateResult::task(task.map(DomainMessage::Settings))
}

/// Handle auto-login toggled result
pub fn handle_auto_login_toggled(
    state: &mut State,
    result: Result<bool, String>,
) -> DomainUpdateResult {
    match result {
        Ok(enabled) => {
            // Update UI state to reflect the change
            state.domains.settings.preferences.auto_login_enabled = enabled;
            state.domains.auth.state.auto_login_enabled = enabled;

            if let crate::domains::auth::types::AuthenticationFlow::EnteringCredentials {
                remember_device,
                ..
            } = &mut state.domains.auth.state.auth_flow
            {
                *remember_device = enabled;
            }

            log::info!(
                "Auto-login is now {}",
                if enabled { "enabled" } else { "disabled" }
            );
        }
        Err(error) => {
            log::error!("Failed to toggle auto-login: {}", error);
            // TODO: Show error to user
        }
    }

    DomainUpdateResult::task(Task::none())
}

/// Handle grid size / UI scale change
///
/// This updates:
/// 1. The preferences state (for persistence)
/// 2. The scaling context user_scale
/// 3. The size provider (pre-computed token values)
/// 4. The scaled layout (pre-computed layout dimensions)
/// 5. All virtual grid states (tab grids)
#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn handle_set_user_scale(
    state: &mut State,
    user_scale: UserScale,
) -> DomainUpdateResult {
    // 1. Update preferences state
    state.domains.settings.preferences.user_scale = user_scale.clone();

    // 2. Get the new user scale factor
    let user_scale = user_scale.scale_factor();

    // 3. Update the scaling context
    state
        .domains
        .ui
        .state
        .scaling_context
        .set_user_scale(user_scale);

    // 4. Get the effective scale (combines user, system, accessibility)
    let effective_scale =
        state.domains.ui.state.scaling_context.effective_scale();

    // 5. Recompute the size provider with new scale
    state.domains.ui.state.size_provider =
        SizeProvider::new(state.domains.ui.state.scaling_context);

    // 6. Recompute the scaled layout for virtual grids/carousels
    state.domains.ui.state.scaled_layout = ScaledLayout::new(effective_scale);

    // 6.5. Update poster text scale for GPU uniform
    poster::set_text_scale(effective_scale);

    log::info!(
        "UI scale changed: user_scale={:?}, user_scale={}, effective_scale={}",
        user_scale,
        user_scale,
        effective_scale
    );

    // 7. Update all tab grids with new scaled dimensions
    for tab_id in state.tab_manager.tab_ids() {
        if let Some(tab) = state.tab_manager.get_tab_mut(tab_id)
            && let Some(grid_state) = tab.grid_state_mut()
        {
            grid_state.update_for_scale(&state.domains.ui.state.scaled_layout);
        }
    }

    // 8. Update virtual carousels with new dimensions
    for key in state.domains.ui.state.carousel_registry.keys() {
        if let Some(vc) = state.domains.ui.state.carousel_registry.get_mut(&key)
        {
            vc.update_dimensions(state.window_size.width.max(1.0));
        }
    }

    DomainUpdateResult::task(Task::none())
}

/// Handle scale preset selection
///
/// Applies the ScalingContext from the preset (Compact, Default, Large, Huge, TV).
#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn handle_set_scale_preset(
    state: &mut State,
    preset: ScalePreset,
) -> DomainUpdateResult {
    // 1. Get the ScalingContext from the preset
    let context = preset.to_context();

    // 2. Apply the context
    state.domains.ui.state.scaling_context = context;

    // 3. Get the effective scale
    let effective_scale = context.effective_scale();

    // 4. Recompute the size provider with new context
    state.domains.ui.state.size_provider = SizeProvider::new(context);

    // 5. Recompute the scaled layout for virtual grids/carousels
    state.domains.ui.state.scaled_layout = ScaledLayout::new(effective_scale);

    // 6. Update poster text scale for GPU uniform
    poster::set_text_scale(effective_scale);

    log::info!(
        "Scale preset applied: {:?}, effective_scale={}",
        preset,
        effective_scale
    );

    // 7. Update all tab grids with new scaled dimensions
    for tab_id in state.tab_manager.tab_ids() {
        if let Some(tab) = state.tab_manager.get_tab_mut(tab_id)
            && let Some(grid_state) = tab.grid_state_mut()
        {
            grid_state.update_for_scale(&state.domains.ui.state.scaled_layout);
        }
    }

    // 8. Update virtual carousels with new dimensions
    for key in state.domains.ui.state.carousel_registry.keys() {
        if let Some(vc) = state.domains.ui.state.carousel_registry.get_mut(&key)
        {
            vc.update_dimensions(state.window_size.width.max(1.0));
        }
    }

    DomainUpdateResult::task(Task::none())
}
