//! Users section view (Admin)
//!
//! Renders the user management section content including:
//! - User List: Create, edit, delete users
//! - Roles & Permissions: Role assignment

use iced::Element;
use iced::widget::{column, container, text};

use crate::domains::ui::messages::UiMessage;
use crate::domains::ui::theme::{self, MediaServerTheme};
use crate::state::State;

/// Render the users settings section (admin only)
pub fn view_users_section<'a>(state: &'a State) -> Element<'a, UiMessage> {
    let fonts = state.domains.ui.state.size_provider.font;

    // TODO: Implement users section UI
    // - User list
    //   - Username
    //   - Display name
    //   - Email
    //   - Role (Admin/User/Guest)
    //   - Created at
    //   - Last login
    //   - Active status toggle
    //   - Edit button
    //   - Delete button
    // - Add user button
    // - User form (add/edit)
    //   - Username input
    //   - Display name input
    //   - Email input
    //   - Password input (for new users)
    //   - Role dropdown
    //   - Active toggle
    //   - Save/Cancel buttons

    let content = column![
        text("User Management")
            .size(fonts.title)
            .color(MediaServerTheme::TEXT_PRIMARY),
        text("Users")
            .size(fonts.body_lg)
            .color(MediaServerTheme::TEXT_PRIMARY),
        text("List of users with create/edit/delete controls will go here.")
            .size(fonts.caption)
            .color(MediaServerTheme::TEXT_SUBDUED),
        text("Add User")
            .size(fonts.body_lg)
            .color(MediaServerTheme::TEXT_PRIMARY),
        text("Form to add a new user will go here.")
            .size(fonts.caption)
            .color(MediaServerTheme::TEXT_SUBDUED),
    ]
    .spacing(16)
    .padding(20);

    container(content)
        .style(theme::Container::Default.style())
        .into()
}
