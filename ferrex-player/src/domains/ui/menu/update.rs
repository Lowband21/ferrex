use crate::{
    common::messages::DomainUpdateResult,
    domains::ui::menu::{MenuButton, PosterMenuMessage, PosterMenuState},
    infra::{
        constants::menu::MENU_KEEPALIVE_MS, shader_widgets::poster::PosterFace,
    },
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
    entry.force_to(now, PosterFace::Front);

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
        PosterMenuMessage::Close(media_id) => {
            // Force close target poster
            let entry = ui_state
                .poster_menu_states
                .entry(media_id)
                .or_insert_with(|| PosterMenuState::new(now));
            entry.force_to(now, PosterFace::Front);

            // Clear open menu state
            if ui_state.poster_menu_open == Some(media_id) {
                ui_state.poster_menu_open = None;
            }
        }
        PosterMenuMessage::Start(media_id) => {
            // Close previous open poster if exists
            if let Some(open_id) = ui_state.poster_menu_open
                && open_id != media_id
            {
                let entry_prev = ui_state
                    .poster_menu_states
                    .entry(open_id)
                    .or_insert_with(|| PosterMenuState::new(now));
                entry_prev.force_to(now, PosterFace::Front);
            }

            // Start hold on target poster
            let entry = ui_state
                .poster_menu_states
                .entry(media_id)
                .or_insert_with(|| PosterMenuState::new(now));
            entry.mark_begin(now);

            // Always set poster_menu_open to the provided target
            ui_state.poster_menu_open = Some(media_id);
        }
        PosterMenuMessage::End(media_id) => {
            if let Some(entry) = ui_state.poster_menu_states.get_mut(&media_id)
            {
                entry.mark_end(now);
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
