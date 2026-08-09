use crate::{
    domains::ui::{
        LibraryMaintenanceAction, messages::UiMessage,
        settings_ui::SettingsUiMessage, theme,
    },
    state::State,
};
use iced::{
    Element, Length,
    widget::{Space, button, column, container, mouse_area, row, text},
};

#[derive(Debug, Clone, PartialEq, Eq)]
struct ConfirmationCopy {
    title: String,
    body: String,
    confirm_label: &'static str,
}

fn confirmation_copy(
    action: LibraryMaintenanceAction,
    library_name: Option<&str>,
) -> ConfirmationCopy {
    let target = library_name.unwrap_or("this library");
    match action {
        LibraryMaintenanceAction::Delete(_) => ConfirmationCopy {
            title: format!("Delete {target}?"),
            body: format!(
                "This permanently deletes {target} and all indexed data owned by it. Media files on disk are not removed."
            ),
            confirm_label: "Delete Library",
        },
        LibraryMaintenanceAction::Reset(_) => ConfirmationCopy {
            title: format!("Reset {target}?"),
            body: format!(
                "This atomically clears {target}'s indexed data, preserves the library identity and settings, and starts a fresh scan."
            ),
            confirm_label: "Reset Library",
        },
        LibraryMaintenanceAction::ClearAllData => ConfirmationCopy {
            title: "Clear all server data?".to_string(),
            body: "This permanently deletes every library and its indexed media, all users, and all sessions. You will be signed out and must run setup again. Media files on disk are not removed."
                .to_string(),
            confirm_label: "Clear All Data",
        },
    }
}

fn library_name(
    state: &State,
    action: LibraryMaintenanceAction,
) -> Option<String> {
    let library_id = match action {
        LibraryMaintenanceAction::Delete(id)
        | LibraryMaintenanceAction::Reset(id) => id,
        LibraryMaintenanceAction::ClearAllData => return None,
    };

    state
        .domains
        .library
        .state
        .repo_accessor
        .get_archived_library_yoke(library_id.as_uuid())
        .ok()
        .flatten()
        .map(|library| library.get().name.to_string())
}

pub fn view_library_maintenance_confirmation(
    state: &State,
) -> Option<Element<'_, UiMessage>> {
    let action = state.domains.ui.state.library_maintenance_confirmation?;
    let name = library_name(state, action);
    let copy = confirmation_copy(action, name.as_deref());

    let dialog = container(
        column![
            text(copy.title)
                .size(24)
                .color(theme::MediaServerTheme::TEXT_PRIMARY),
            text(copy.body)
                .size(15)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
            Space::new().height(8),
            row![
                Space::new().width(Length::Fill),
                button("Cancel")
                    .on_press(
                        SettingsUiMessage::HideLibraryMaintenanceConfirm.into()
                    )
                    .style(theme::Button::Secondary.style()),
                button(copy.confirm_label)
                    .on_press(action.confirmation_message().into())
                    .style(theme::Button::Destructive.style()),
            ]
            .spacing(12)
            .align_y(iced::Alignment::Center),
        ]
        .spacing(16),
    )
    .padding(28)
    .width(Length::Fixed(560.0))
    .style(theme::Container::Modal.style());

    let overlay = container(dialog)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .style(theme::Container::ModalOverlay.style());

    Some(mouse_area(overlay).on_press(UiMessage::NoOp).into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrex_core::player_prelude::LibraryId;
    use uuid::Uuid;

    #[test]
    fn confirmations_name_the_target_and_dispatch_the_real_action() {
        let library_id = LibraryId(Uuid::from_u128(42));

        let delete = confirmation_copy(
            LibraryMaintenanceAction::Delete(library_id),
            Some("Family Movies"),
        );
        assert!(delete.title.contains("Family Movies"));
        assert!(matches!(
            LibraryMaintenanceAction::Delete(library_id)
                .confirmation_message(),
            SettingsUiMessage::DeleteLibrary(id) if id == library_id
        ));

        let reset = confirmation_copy(
            LibraryMaintenanceAction::Reset(library_id),
            Some("Family Movies"),
        );
        assert!(reset.body.contains("Family Movies"));
        assert!(matches!(
            LibraryMaintenanceAction::Reset(library_id)
                .confirmation_message(),
            SettingsUiMessage::ResetLibrary(id) if id == library_id
        ));

        assert!(matches!(
            LibraryMaintenanceAction::ClearAllData.confirmation_message(),
            SettingsUiMessage::ClearDatabase
        ));
    }
}
