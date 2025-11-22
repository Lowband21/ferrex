use crate::{
    common::messages::{DomainMessage, DomainUpdateResult},
    domains::ui::{
        background_ui::update_background_ui, feedback_ui::update_feedback_ui,
        header_ui::update_header_ui, interaction_ui::update_interaction_ui,
        library_ui::update_library_ui, menu::poster_menu_update,
        messages::UiMessage, playback_ui::update_playback_ui,
        settings_ui::update_settings_ui, shell_ui::update_shell_ui,
        update_handlers::handle_virtual_carousel_message,
        utils::bump_keep_alive, view_model_ui::update_view_model_ui,
        window_ui::update_window_ui,
    },
    state::State,
};

use iced::Task;

#[cfg(feature = "demo")]
use crate::domains::ui::update_handlers::demo_controls;

pub fn update_ui(state: &mut State, message: UiMessage) -> DomainUpdateResult {
    match message {
        UiMessage::Shell(shell_msg) => update_shell_ui(state, shell_msg),
        UiMessage::Interaction(interaction_msg) => {
            update_interaction_ui(state, interaction_msg)
        }
        UiMessage::Library(library_msg) => {
            update_library_ui(state, library_msg)
        }
        UiMessage::Feedback(feedback_msg) => {
            update_feedback_ui(state, feedback_msg)
        }
        UiMessage::Window(window_msg) => update_window_ui(state, window_msg),
        UiMessage::Header(header_msg) => update_header_ui(state, header_msg),
        UiMessage::VirtualCarousel(vc_msg) => {
            bump_keep_alive(state);
            DomainUpdateResult::task(
                handle_virtual_carousel_message(state, vc_msg)
                    .map(DomainMessage::Ui),
            )
        }
        UiMessage::Background(background_msg) => {
            update_background_ui(state, background_msg)
        }
        UiMessage::PosterMenu(menu_msg) => poster_menu_update(state, menu_msg),
        UiMessage::ViewModels(view_model_msg) => {
            update_view_model_ui(state, view_model_msg)
        }
        UiMessage::Playback(play_msg) => update_playback_ui(state, play_msg),
        UiMessage::NoOp => DomainUpdateResult::task(Task::none()),
        UiMessage::Settings(settings_ui_message) => {
            update_settings_ui(state, settings_ui_message)
        }
    }
}
