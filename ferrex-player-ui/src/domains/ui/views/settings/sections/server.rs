//! Server section view (Admin)
//!
//! Renders the server settings section content including:
//! - Session Policies: Token lifetimes, max concurrent sessions
//! - Device Policies: Trust duration, max trusted devices
//! - Password Policies: Min length, complexity requirements
//! - Curated Content: Max carousel items, head window

use iced::Element;
use iced::widget::{column, container, text};

use crate::domains::ui::messages::UiMessage;
use crate::domains::ui::theme::{self, MediaServerTheme};
use crate::state::State;

/// Render the server settings section (admin only)
pub fn view_server_section<'a>(state: &'a State) -> Element<'a, UiMessage> {
    let fonts = state.domains.ui.state.size_provider.font;

    // TODO: Implement server section UI
    // - Session Policies subsection
    //   - Access token lifetime input
    //   - Refresh token lifetime input
    //   - Max concurrent sessions input (optional)
    // - Device Policies subsection
    //   - Device trust duration input
    //   - Max trusted devices input (optional)
    //   - Require PIN for new device toggle
    // - Password Policies subsection
    //   - Admin policy
    //     - Enforce toggle
    //     - Min length input
    //     - Require uppercase toggle
    //     - Require lowercase toggle
    //     - Require number toggle
    //     - Require special toggle
    //   - User policy (same fields)
    // - Curated Content subsection
    //   - Max carousel items input
    //   - Head window input
    // - Save button

    let content = column![
        text("Server Settings")
            .size(fonts.title)
            .color(MediaServerTheme::TEXT_PRIMARY),
        text("Session Policies")
            .size(fonts.body_lg)
            .color(MediaServerTheme::TEXT_PRIMARY),
        text("Token lifetime and session limit settings will go here.")
            .size(fonts.caption)
            .color(MediaServerTheme::TEXT_SUBDUED),
        text("Device Policies")
            .size(fonts.body_lg)
            .color(MediaServerTheme::TEXT_PRIMARY),
        text("Device trust settings will go here.")
            .size(fonts.caption)
            .color(MediaServerTheme::TEXT_SUBDUED),
        text("Password Policies")
            .size(fonts.body_lg)
            .color(MediaServerTheme::TEXT_PRIMARY),
        text("Password complexity settings for admins and users will go here.")
            .size(fonts.caption)
            .color(MediaServerTheme::TEXT_SUBDUED),
        text("Curated Content")
            .size(fonts.body_lg)
            .color(MediaServerTheme::TEXT_PRIMARY),
        text("Content carousel settings will go here.")
            .size(fonts.caption)
            .color(MediaServerTheme::TEXT_SUBDUED),
    ]
    .spacing(16)
    .padding(20);

    container(content)
        .style(theme::Container::Default.style())
        .into()
}
