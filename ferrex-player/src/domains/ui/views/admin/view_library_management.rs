//! Library management view with permission-based controls

use std::sync::Arc;

use crate::{
    domains::{
        auth::permissions::{self, StatePermissionExt},
        library::messages as library,
        ui::{messages::Message, theme, views::admin::view_library_form},
    },
    infrastructure::{
        api_types::LibraryType,
        repository::accessor::{Accessor, ReadOnly, ReadWrite},
    },
    state_refactored::State,
};
use ferrex_core::{
    ArchivedLibraryType, LibraryID,
    types::library::{ArchivedLibrary, Library},
};
use iced::{
    Element, Length,
    widget::{Space, button, column, container, row, scrollable, text},
};
use lucide_icons::Icon;
use rkyv::{deserialize, rancor::Error, util::AlignedVec};
use uuid::Uuid;
use yoke::Yoke;

// Helper function to create icon text
fn icon_text(icon: Icon) -> text::Text<'static> {
    text(icon.unicode()).font(lucide_font()).size(20)
}

// Get the lucide font
fn lucide_font() -> iced::Font {
    iced::Font::with_name("lucide")
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn view_library_management(state: &State) -> Element<Message> {
    let permissions = state.permission_checker();

    // Check if user has permission to view libraries
    if !permissions.can_view_library_settings() {
        return container(
            column![
                text("Access Denied")
                    .size(32)
                    .color(theme::MediaServerTheme::ERROR),
                Space::with_height(20),
                text("You don't have permission to view library settings")
                    .size(16)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY),
                Space::with_height(40),
                button("Back to Admin Dashboard")
                    .on_press(Message::HideLibraryManagement)
                    .style(theme::Button::Secondary.style())
                    .padding([12, 20]),
            ]
            .spacing(20)
            .align_x(iced::Alignment::Center),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into();
    }

    // If form is open, show the form instead
    if let Some(form_data) = &state.domains.library.state.library_form_data {
        return view_library_form(state, form_data);
    }

    let mut content = column![].spacing(20).padding(20);

    // Build header with conditional buttons
    let mut header_row = row![
        button(
            row![icon_text(Icon::ArrowLeft), text(" Back to Library")]
                .spacing(5)
                .align_y(iced::Alignment::Center)
        )
        .on_press(Message::HideLibraryManagement)
        .style(theme::Button::Secondary.style()),
        Space::with_width(Length::Fill),
        text("Library Management")
            .size(28)
            .color(theme::MediaServerTheme::TEXT_PRIMARY),
        Space::with_width(Length::Fill),
    ]
    .align_y(iced::Alignment::Center);

    // Add Create Library button only if user has permission
    if permissions.has_permission("libraries:create") {
        header_row = header_row.push(
            button("Create Library")
                .on_press(Message::ShowLibraryForm(None))
                .style(theme::Button::Primary.style()),
        );
        header_row = header_row.push(Space::with_width(10));
    }

    // Add Clear All Data button only if user can reset database
    if permissions.can_reset_database() {
        header_row = header_row.push(
            button("🗑 Clear All Data")
                .on_press(Message::ShowClearDatabaseConfirm)
                .style(theme::Button::Destructive.style()),
        );
    }

    content = content.push(header_row);

    // Libraries list
    if !state.domains.library.state.repo_accessor.is_initialized() {
        content = content.push(
            container(
                column![
                    text("No Libraries Configured")
                        .size(24)
                        .color(theme::MediaServerTheme::TEXT_SECONDARY),
                    Space::with_height(10),
                    text("Create a library to start managing your media collection.")
                        .size(16)
                        .color(theme::MediaServerTheme::TEXT_SUBDUED),
                ]
                .spacing(10)
                .align_x(iced::Alignment::Center),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill),
        );
    } else {
        let libraries_list = scrollable(
            column(
                state
                    .domains
                    .ui
                    .state
                    .repo_accessor
                    .libraries_index()
                    .expect("Failed to lock repository")
                    .iter()
                    .map(|library_id| {
                        create_library_card(
                            state.domains.ui.state.repo_accessor.clone(),
                            library_id,
                            &permissions,
                        )
                    })
                    .collect::<Vec<_>>(),
            )
            .spacing(15),
        );

        content = content.push(libraries_list);
    }

    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

type LibraryYoke = Yoke<&'static ArchivedLibrary, Arc<AlignedVec>>;

fn create_library_card<'a>(
    repo_accessor: Accessor<ReadOnly>,
    library_id: &Uuid,
    //library: &'a LibraryYoke,
    permissions: &permissions::PermissionChecker,
) -> Element<'a, Message> {
    let library_opt = repo_accessor
        .get_archived_library_yoke(&library_id)
        .unwrap(); // This should be safe but I should handle it anyway

    if let Some(library_yoke) = library_opt {
        let library = *library_yoke.get();

        let library_type_icon = match library.library_type {
            ArchivedLibraryType::Movies => "🎬",
            ArchivedLibraryType::Series => "📺",
        };

        let status_text = if library.enabled {
            text("Enabled").color(theme::MediaServerTheme::SUCCESS)
        } else {
            text("Disabled").color(theme::MediaServerTheme::TEXT_SUBDUED)
        };

        let mut action_buttons = row![].spacing(10);

        // Scan button (only if user has scan permission)
        if permissions.can_scan_libraries() && library.enabled {
            action_buttons = action_buttons.push(
                button("Scan")
                    .on_press(Message::ScanLibrary(LibraryID(library.id.as_uuid())))
                    .style(theme::Button::Secondary.style()),
            );
        }

        // Edit button (only if user has update permission)
        if permissions.has_permission("libraries:update") {
            action_buttons = action_buttons.push(
                button("Edit")
                    .on_press(Message::ShowLibraryForm(Some(
                        deserialize::<Library, Error>(library)
                            .expect("Failed to deserialize library"),
                    )))
                    .style(theme::Button::Secondary.style()),
            );
        }

        // Delete button (only if user has delete permission)
        if permissions.has_permission("libraries:delete") {
            action_buttons = action_buttons.push(
                button("Delete")
                    .on_press(Message::DeleteLibrary(LibraryID(library.id.as_uuid())))
                    .style(theme::Button::Destructive.style()),
            );
        }

        container(
            row![
                // Library icon and info
                row![
                    text(library_type_icon).size(24),
                    column![
                        row![
                            text(library.name.to_string())
                                .size(18)
                                .color(theme::MediaServerTheme::TEXT_PRIMARY),
                            Space::with_width(10),
                            status_text,
                        ]
                        .align_y(iced::Alignment::Center),
                        text(
                            library
                                .paths
                                .first()
                                .expect("Invalid or non existant library path")
                                .to_string()
                        )
                        .size(14)
                        .color(theme::MediaServerTheme::TEXT_SECONDARY),
                    ]
                    .spacing(5),
                ]
                .spacing(15)
                .align_y(iced::Alignment::Center)
                .width(Length::Fill),
                // Action buttons
                action_buttons,
            ]
            .align_y(iced::Alignment::Center)
            .padding(20),
        )
        .style(theme::Container::Card.style())
        .width(Length::Fill)
        .into()
    } else {
        container(
            column![
                text("No Libraries Configured")
                    .size(24)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY),
                Space::with_height(10),
                text("Create a library to start managing your media collection.")
                    .size(16)
                    .color(theme::MediaServerTheme::TEXT_SUBDUED),
            ]
            .spacing(10)
            .align_x(iced::Alignment::Center),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
    }
}
