use crate::{
    common::messages::DomainUpdateResult,
    domains::ui::menu::{
        MENU_KEEPALIVE_MS, PosterMenuMessage, PosterMenuState,
    },
    infra::shader_widgets::poster::PosterFace,
    state::State,
};

use iced::Task;
use std::time::Instant;

pub fn poster_menu_update(
    state: &mut State,
    menu_msg: PosterMenuMessage,
) -> DomainUpdateResult {
    let ui_state = &mut state.domains.ui.state;
    let now = Instant::now();

    match menu_msg {
        PosterMenuMessage::Toggle(media_id) => {
            let was_open = ui_state.poster_menu_open;

            // Close previous open target if different
            if let Some(open_id) = was_open {
                if open_id != media_id {
                    let entry = ui_state
                        .poster_menu_states
                        .entry(open_id)
                        .or_insert_with(|| PosterMenuState::new(now));
                    entry.apply_impulse(PosterFace::Front, now);
                    entry.hold_active = false;
                }
            }

            ui_state.poster_menu_open = Some(media_id);
            let entry = ui_state
                .poster_menu_states
                .entry(media_id)
                .or_insert_with(|| PosterMenuState::new(now));
            entry.apply_impulse(PosterFace::Back, now);
        }
        PosterMenuMessage::Close(media_id) => {
            if ui_state.poster_menu_open == Some(media_id) {
                ui_state.poster_menu_open = None;
            }
            let entry = ui_state
                .poster_menu_states
                .entry(media_id)
                .or_insert_with(|| PosterMenuState::new(now));
            entry.hold_active = false;
            entry.apply_impulse(PosterFace::Front, now);
        }
        PosterMenuMessage::HoldStart(media_id) => {
            if let Some(open_id) = ui_state.poster_menu_open {
                if open_id == media_id {
                    // Toggle closed if already open
                    ui_state.poster_menu_open = None;
                    let entry = ui_state
                        .poster_menu_states
                        .entry(media_id)
                        .or_insert_with(|| PosterMenuState::new(now));
                    entry.apply_impulse(PosterFace::Front, now);
                    entry.hold_active = true;
                    entry.target_face = PosterFace::Front;
                } else {
                    // Close previous, open new
                    let entry_prev = ui_state
                        .poster_menu_states
                        .entry(open_id)
                        .or_insert_with(|| PosterMenuState::new(now));
                    entry_prev.apply_impulse(PosterFace::Front, now);
                    entry_prev.hold_active = false;
                    ui_state.poster_menu_open = Some(media_id);
                    let entry = ui_state
                        .poster_menu_states
                        .entry(media_id)
                        .or_insert_with(|| PosterMenuState::new(now));
                    entry.apply_impulse(PosterFace::Back, now);
                    entry.hold_active = true;
                    entry.target_face = PosterFace::Back;
                }
            } else {
                ui_state.poster_menu_open = Some(media_id);
                let entry = ui_state
                    .poster_menu_states
                    .entry(media_id)
                    .or_insert_with(|| PosterMenuState::new(now));
                entry.apply_impulse(PosterFace::Back, now);
                entry.hold_active = true;
                entry.target_face = PosterFace::Back;
            }
        }
        PosterMenuMessage::HoldEnd(media_id) => {
            if let Some(entry) = ui_state.poster_menu_states.get_mut(&media_id)
            {
                entry.hold_active = false;
            }
        }
    }

    ui_state.poster_anim_active_until =
        Some(now + std::time::Duration::from_millis(MENU_KEEPALIVE_MS));
    DomainUpdateResult::task(Task::none())
}
