use iced::Task;

use crate::{
    common::messages::DomainUpdateResult,
    domains::ui::header_ui::HeaderMessage, state::State,
};

pub fn update_header_ui(
    state: &mut State,
    message: HeaderMessage,
) -> DomainUpdateResult {
    match message {
        HeaderMessage::ShowLibraryMenu => {
            state.domains.ui.state.show_library_menu =
                !state.domains.ui.state.show_library_menu;
            DomainUpdateResult::task(Task::none())
        }
        HeaderMessage::ShowAllLibrariesMenu => {
            state.domains.ui.state.show_library_menu =
                !state.domains.ui.state.show_library_menu;
            state.domains.ui.state.library_menu_target = None;
            DomainUpdateResult::task(Task::none())
        }
    }
}
