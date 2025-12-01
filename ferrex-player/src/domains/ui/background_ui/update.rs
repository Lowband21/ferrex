use iced::Task;
use std::time::Instant;

use crate::{
    common::messages::DomainUpdateResult,
    domains::ui::{
        background_ui::BackgroundMessage, types::BackdropAspectMode,
    },
    infra::constants::menu::MENU_KEEPALIVE_MS,
    state::State,
};

pub fn update_background_ui(
    state: &mut State,
    message: BackgroundMessage,
) -> DomainUpdateResult {
    match message {
        BackgroundMessage::UpdateTransitions => {
            let ui_state = &mut state.domains.ui.state;
            let now = Instant::now();

            // Advance poster menu flip states
            let mut menu_active = false;
            ui_state.poster_menu_states.retain(|id, menu_state| {
                let active = menu_state.step(now);
                if active {
                    menu_active = true;
                }
                // Keep entry if still active or still targeted for open face
                active || ui_state.poster_menu_open.as_ref() == Some(id)
            });
            if menu_active {
                ui_state.poster_anim_active_until = Some(
                    now + std::time::Duration::from_millis(MENU_KEEPALIVE_MS),
                );
            }

            let poster_anim_active = match ui_state.poster_anim_active_until {
                Some(until) if until > now => true,
                Some(_) => {
                    ui_state.poster_anim_active_until = None;
                    false
                }
                None => false,
            };

            let shader_state = &mut ui_state.background_shader_state;
            let transitions_active =
                shader_state.color_transitions.is_transitioning()
                    || shader_state.backdrop_transitions.is_transitioning()
                    || shader_state.gradient_transitions.is_transitioning();

            if !poster_anim_active && !transitions_active && !menu_active {
                return DomainUpdateResult::task(Task::none());
            }

            shader_state.color_transitions.update();
            shader_state.backdrop_transitions.update();
            shader_state.gradient_transitions.update();

            // Update the actual colors based on transition progress
            let (primary, secondary) =
                shader_state.color_transitions.get_interpolated_colors();
            shader_state.primary_color = primary;
            shader_state.secondary_color = secondary;

            // Update the gradient center based on transition progress
            shader_state.gradient_center =
                shader_state.gradient_transitions.get_interpolated_center();

            DomainUpdateResult::task(Task::none())
        }
        BackgroundMessage::ToggleBackdropAspectMode => {
            state
                .domains
                .ui
                .state
                .background_shader_state
                .backdrop_aspect_mode = match state
                .domains
                .ui
                .state
                .background_shader_state
                .backdrop_aspect_mode
            {
                BackdropAspectMode::Auto => BackdropAspectMode::Force21x9,
                BackdropAspectMode::Force21x9 => BackdropAspectMode::Auto,
            };
            DomainUpdateResult::task(Task::none())
        }
        BackgroundMessage::UpdateBackdropHandle(_handle) => {
            // Deprecated - backdrops are now pulled reactively from image service
            // This message handler kept for compatibility but does nothing
            DomainUpdateResult::task(Task::none())
        }
    }
}
