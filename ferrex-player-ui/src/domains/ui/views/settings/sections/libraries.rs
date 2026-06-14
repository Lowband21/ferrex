//! Libraries section view (Admin)
//!
//! Renders the library management section content including:
//! - Library List: Add, edit, delete, scan controls
//! - Scan Settings: Auto-scan, scan intervals

use iced::Element;
use iced::widget::{column, container, text};

use crate::domains::ui::messages::UiMessage;
use crate::domains::ui::theme::{self, MediaServerTheme};
use crate::state::State;

/// Render the libraries settings section (admin only)
pub fn view_libraries_section<'a>(state: &'a State) -> Element<'a, UiMessage> {
    let fonts = state.domains.ui.state.size_provider.font;

    // TODO: Implement library list + add/edit UI.

    let content = column![
        text("Library Management")
            .size(fonts.title)
            .color(MediaServerTheme::TEXT_PRIMARY),
        text("Libraries")
            .size(fonts.body_lg)
            .color(MediaServerTheme::TEXT_PRIMARY),
        text(
            "List of libraries with add/edit/delete/scan controls will go here."
        )
        .size(fonts.caption)
        .color(MediaServerTheme::TEXT_SUBDUED),
        text("Add Library")
            .size(fonts.body_lg)
            .color(MediaServerTheme::TEXT_PRIMARY),
        text("Form to add a new library will go here.")
            .size(fonts.caption)
            .color(MediaServerTheme::TEXT_SUBDUED),
    ]
    .spacing(16)
    .padding(20);

    container(content)
        .style(theme::Container::Default.style())
        .into()
}
