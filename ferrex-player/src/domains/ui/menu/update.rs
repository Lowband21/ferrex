use crate::{
    common::messages::DomainUpdateResult,
    domains::ui::menu::{
        MenuButton, MENU_KEEPALIVE_MS, PosterMenuMessage, PosterMenuState,
    },
    infra::shader_widgets::poster::PosterFace,
    state::State,
};

use iced::Task;
use std::time::Instant;
use uuid::Uuid;

/// Handle a menu button click - close menu and dispatch action
fn handle_button_click(
    state: &mut State,
    media_id: Uuid,
    button: MenuButton,
    now: Instant,
) -> DomainUpdateResult {
    let ui_state = &mut state.domains.ui.state;

    // Close the menu (flip back to front)
    if ui_state.poster_menu_open == Some(media_id) {
        ui_state.poster_menu_open = None;
    }
    let entry = ui_state
        .poster_menu_states
        .entry(media_id)
        .or_insert_with(|| PosterMenuState::new(now));
    entry.hold_active = false;
    entry.apply_impulse(PosterFace::Front, now);

    // Log the action for now - actual dispatch will be wired to domain messages
    match button {
        MenuButton::Play => {
            log::info!("[Menu] Play clicked for media {:?}", media_id);
            // TODO: Dispatch play message - needs media reference lookup
        }
        MenuButton::Details => {
            log::info!("[Menu] Details clicked for media {:?}", media_id);
            // TODO: Navigate to details view
        }
        MenuButton::Watched => {
            log::info!("[Menu] Toggle watched for media {:?}", media_id);
            // TODO: Toggle watch status
        }
        MenuButton::Watchlist | MenuButton::Edit => {
            // These are disabled, shouldn't reach here
            log::warn!("[Menu] Disabled button {:?} clicked", button);
        }
    }

    ui_state.poster_anim_active_until =
        Some(now + std::time::Duration::from_millis(MENU_KEEPALIVE_MS));
    DomainUpdateResult::task(Task::none())
}

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
        PosterMenuMessage::ButtonClicked(media_id, button) => {
            return handle_button_click(state, media_id, button, now);
        }
    }

    ui_state.poster_anim_active_until =
        Some(now + std::time::Duration::from_millis(MENU_KEEPALIVE_MS));
    DomainUpdateResult::task(Task::none())
}
