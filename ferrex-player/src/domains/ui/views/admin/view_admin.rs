//! Admin dashboard view with permission-based rendering
//!
//! This view shows administrative controls based on the user's permissions.
//! Different sections are shown/hidden based on what the user can access.

use crate::{
    domains::auth::permissions::StatePermissionExt, domains::ui::messages::Message,
    domains::ui::theme, state_refactored::State,
};
use iced::{
    widget::{button, column, container, row, text, Space},
    Element, Length,
};
use lucide_icons::Icon;

fn icon_text(icon: Icon) -> text::Text<'static> {
    text(icon.unicode()).font(lucide_font()).size(20)
}

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
pub fn view_admin_dashboard(state: &State) -> Element<Message> {
    let permissions = state.permission_checker();

    // Check if user has any admin permissions
    if !permissions.can_view_admin_dashboard() {
        return container(
            column![
                text("Access Denied")
                    .size(32)
                    .color(theme::MediaServerTheme::ERROR),
                Space::with_height(20),
                text("You don't have permission to access admin settings")
                    .size(16)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY),
                Space::with_height(40),
                button("Back to Library")
                    .on_press(Message::HideAdminDashboard)
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

    let mut content = column![].spacing(30).padding(20);

    // Header with back button
    content = content.push(
        row![
            button(
                row![icon_text(Icon::ArrowLeft), text(" Back to Library")]
                    .spacing(5)
                    .align_y(iced::Alignment::Center)
            )
            .on_press(Message::HideAdminDashboard)
            .style(theme::Button::Secondary.style()),
            Space::with_width(Length::Fill),
            text("Admin Dashboard")
                .size(32)
                .color(theme::MediaServerTheme::TEXT_PRIMARY),
            Space::with_width(Length::Fill),
            Space::with_width(Length::Fixed(100.0)), // Balance the back button
        ]
        .align_y(iced::Alignment::Center),
    );

    // Build sections based on permissions
    let mut sections_row_1 = row![].spacing(20).align_y(iced::Alignment::Start);

    let mut sections_row_2 = row![].spacing(20).align_y(iced::Alignment::Start);

    let mut has_row_1_content = false;
    let mut has_row_2_content = false;

    // Library Management section (only if user can manage libraries)
    if permissions.can_view_library_settings() {
        sections_row_1 = sections_row_1.push(
            container(
                column![
                    row![
                        text("📚").size(32),
                        Space::with_width(15),
                        column![
                            text("Library Management")
                                .size(20)
                                .color(theme::MediaServerTheme::TEXT_PRIMARY),
                            text("Manage media libraries, scanning, and organization")
                                .size(14)
                                .color(theme::MediaServerTheme::TEXT_SECONDARY),
                        ]
                        .spacing(5),
                    ]
                    .align_y(iced::Alignment::Center),
                    Space::with_height(20),
                    button("Manage Libraries")
                        .on_press(Message::ShowLibraryManagement)
                        .style(theme::Button::Primary.style())
                        .padding([12, 20])
                        .width(Length::Fill),
                ]
                .spacing(15)
                .padding(20),
            )
            .style(theme::Container::Card.style())
            .width(Length::Fill),
        );
        has_row_1_content = true;
    }

    // User Management section (only if user can manage users)
    if permissions.can_view_users() {
        if has_row_1_content {
            sections_row_1 = sections_row_1.push(Space::with_width(20));
        }
        sections_row_1 = sections_row_1.push(
            container(
                column![
                    row![
                        text("👥").size(32),
                        Space::with_width(15),
                        column![
                            text("User Management")
                                .size(20)
                                .color(theme::MediaServerTheme::TEXT_PRIMARY),
                            text("Create users, manage roles and permissions")
                                .size(14)
                                .color(theme::MediaServerTheme::TEXT_SECONDARY),
                        ]
                        .spacing(5),
                    ]
                    .align_y(iced::Alignment::Center),
                    Space::with_height(20),
                    button("Manage Users")
                        .on_press(Message::NoOp) // TODO: Implement user management view
                        .style(theme::Button::Primary.style())
                        .padding([12, 20])
                        .width(Length::Fill),
                ]
                .spacing(15)
                .padding(20),
            )
            .style(theme::Container::Card.style())
            .width(Length::Fill),
        );
        has_row_1_content = true;
    }

    // Server Settings section (only if user can access server settings)
    if permissions.can_access_server_settings() {
        sections_row_2 = sections_row_2.push(
            container(
                column![
                    row![
                        text("⚙️").size(32),
                        Space::with_width(15),
                        column![
                            text("Server Settings")
                                .size(20)
                                .color(theme::MediaServerTheme::TEXT_PRIMARY),
                            text("Configure server settings, API, and performance")
                                .size(14)
                                .color(theme::MediaServerTheme::TEXT_SECONDARY),
                        ]
                        .spacing(5),
                    ]
                    .align_y(iced::Alignment::Center),
                    Space::with_height(20),
                    button("Server Settings")
                        .on_press(Message::NoOp) // TODO: Implement server settings
                        .style(theme::Button::Secondary.style())
                        .padding([12, 20])
                        .width(Length::Fill),
                ]
                .spacing(15)
                .padding(20),
            )
            .style(theme::Container::Card.style())
            .width(Length::Fill),
        );
        has_row_2_content = true;
    }

    // Dev Tools section (only if user can reset database or is admin)
    if permissions.can_reset_database() {
        if has_row_2_content {
            sections_row_2 = sections_row_2.push(Space::with_width(20));
        }
        sections_row_2 = sections_row_2.push(
            container(
                column![
                    row![
                        text("🔧").size(32),
                        Space::with_width(15),
                        column![
                            text("Developer Tools")
                                .size(20)
                                .color(theme::MediaServerTheme::TEXT_PRIMARY),
                            text("Database reset and development utilities")
                                .size(14)
                                .color(theme::MediaServerTheme::TEXT_SECONDARY),
                        ]
                        .spacing(5),
                    ]
                    .align_y(iced::Alignment::Center),
                    Space::with_height(20),
                    button("Dev Tools")
                        .on_press(Message::ShowClearDatabaseConfirm)
                        .style(theme::Button::Danger.style())
                        .padding([12, 20])
                        .width(Length::Fill),
                ]
                .spacing(15)
                .padding(20),
            )
            .style(theme::Container::Card.style())
            .width(Length::Fill),
        );
        has_row_2_content = true;
    }

    // Add sections to content
    if has_row_1_content {
        content = content.push(sections_row_1);
    }

    if has_row_2_content {
        content = content.push(sections_row_2);
    }

    // Show helpful message if user has limited permissions
    if !has_row_1_content && !has_row_2_content {
        content = content.push(
            container(
                column![
                    text("Limited Access")
                        .size(24)
                        .color(theme::MediaServerTheme::TEXT_PRIMARY),
                    Space::with_height(10),
                    text("You have access to the admin dashboard but no specific admin features are available with your current permissions.")
                        .size(16)
                        .color(theme::MediaServerTheme::TEXT_SECONDARY),
                    Space::with_height(20),
                    text("Contact an administrator to request additional permissions.")
                        .size(14)
                        .color(theme::MediaServerTheme::TEXT_SUBDUED),
                ]
                .align_x(iced::Alignment::Center)
                .spacing(10)
            )
            .padding(40)
            .width(Length::Fill)
            .style(theme::Container::Card.style())
        );
    }

    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
