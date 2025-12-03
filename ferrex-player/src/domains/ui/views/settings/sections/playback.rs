//! Playback section view
//!
//! Renders the playback settings section content including:
//! - Seeking: Forward/backward seek amounts (coarse and fine)

use iced::widget::{Space, column, container};
use iced::{Element, Length};

use crate::domains::ui::messages::UiMessage;
use crate::domains::ui::settings_ui::RuntimeConfigMessage;
use crate::domains::ui::theme::{self, MediaServerTheme};
use crate::domains::ui::widgets::setting_controls::{
    setting_row, setting_section, setting_slider_f64,
};
use crate::state::State;

/// Render the playback settings section
pub fn view_playback_section<'a>(state: &'a State) -> Element<'a, UiMessage> {
    let config = &state.runtime_config;
    let fonts = state.domains.ui.state.size_provider.font;

    let mut content = column![].spacing(24).padding(20).max_width(600);

    // ========== SEEKING ==========
    content = content.push(setting_section(
        "Seeking",
        Some("Skip amounts for keyboard shortcuts and controls"),
        fonts,
    ));

    // Coarse seek (arrow keys)
    content = content.push(
        column![
            iced::widget::text("Coarse Seek (Arrow Keys)")
                .size(fonts.caption)
                .color(MediaServerTheme::TEXT_SECONDARY),
            Space::new().height(4),
            setting_row(vec![
                setting_slider_f64(
                    "Forward",
                    config.seek_forward_coarse(),
                    5.0..=120.0,
                    "sec",
                    0,
                    |v| RuntimeConfigMessage::SeekForwardCoarse(v).into(),
                    fonts,
                ),
                setting_slider_f64(
                    "Backward",
                    config.seek_backward_coarse(),
                    5.0..=120.0,
                    "sec",
                    0,
                    |v| RuntimeConfigMessage::SeekBackwardCoarse(v).into(),
                    fonts,
                ),
            ]),
        ]
        .spacing(8),
    );

    content = content.push(Space::new().height(8));

    // Fine seek (shift + arrow keys)
    content = content.push(
        column![
            iced::widget::text("Fine Seek (Shift + Arrow Keys)")
                .size(fonts.caption)
                .color(MediaServerTheme::TEXT_SECONDARY),
            Space::new().height(4),
            setting_row(vec![
                setting_slider_f64(
                    "Forward",
                    config.seek_forward_fine(),
                    1.0..=60.0,
                    "sec",
                    0,
                    |v| RuntimeConfigMessage::SeekForwardFine(v).into(),
                    fonts,
                ),
                setting_slider_f64(
                    "Backward",
                    config.seek_backward_fine(),
                    1.0..=60.0,
                    "sec",
                    0,
                    |v| RuntimeConfigMessage::SeekBackwardFine(v).into(),
                    fonts,
                ),
            ]),
        ]
        .spacing(8),
    );

    container(content)
        .width(Length::Fill)
        .style(theme::Container::Default.style())
        .into()
}
