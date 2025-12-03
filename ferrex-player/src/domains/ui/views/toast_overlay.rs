//! Toast notification overlay view
//!
//! Renders toast notifications in the top-right corner of the screen.

use iced::widget::{Space, button, column, container, row, text};
use iced::{Alignment, Element, Length, Padding};

use crate::domains::ui::feedback_ui::{FeedbackMessage, ToastId, ToastLevel};
use crate::domains::ui::messages::UiMessage;
use crate::domains::ui::theme::MediaServerTheme;
use crate::state::State;

/// View the toast overlay - renders all active toasts
pub fn view_toast_overlay(state: &State) -> Element<'_, UiMessage> {
    let toasts = &state.domains.ui.state.toast_manager.toasts;

    if toasts.is_empty() {
        return Space::new().width(0).height(0).into();
    }

    let toast_elements: Vec<Element<'_, UiMessage>> = toasts
        .iter()
        .map(|toast| view_single_toast(toast.id, &toast.message, toast.level))
        .collect();

    let toast_column = column(toast_elements).spacing(8).width(Length::Shrink);

    // Position in top-right with padding (top padding to clear header)
    container(toast_column)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(Padding {
            top: 60.0,
            right: 20.0,
            bottom: 20.0,
            left: 20.0,
        })
        .align_x(Alignment::End)
        .align_y(Alignment::Start)
        .into()
}

/// Render a single toast notification
fn view_single_toast(
    id: ToastId,
    message: &str,
    level: ToastLevel,
) -> Element<'_, UiMessage> {
    let (bg_color, border_color, icon) = match level {
        ToastLevel::Info => (
            MediaServerTheme::SURFACE_DIM,
            MediaServerTheme::INFO,
            "\u{e88e}", // info icon
        ),
        ToastLevel::Success => (
            iced::Color::from_rgb(0.1, 0.3, 0.1),
            MediaServerTheme::SUCCESS,
            "\u{e86c}", // check icon
        ),
        ToastLevel::Warning => (
            iced::Color::from_rgb(0.3, 0.25, 0.1),
            MediaServerTheme::WARNING,
            "\u{e002}", // warning icon
        ),
        ToastLevel::Error => (
            iced::Color::from_rgb(0.3, 0.1, 0.1),
            MediaServerTheme::ERROR,
            "\u{e000}", // error icon
        ),
    };

    let dismiss_btn = button(
        text("\u{e5cd}") // close icon
            .font(crate::view::lucide_font())
            .size(14),
    )
    .padding(4)
    .style(|_theme, _status| button::Style {
        background: None,
        text_color: MediaServerTheme::TEXT_SUBDUED,
        ..Default::default()
    })
    .on_press(FeedbackMessage::DismissToast(id).into());

    let content = row![
        text(icon)
            .font(crate::view::lucide_font())
            .size(16)
            .color(border_color),
        Space::new().width(8),
        text(message).size(13).color(MediaServerTheme::TEXT_PRIMARY),
        Space::new().width(12),
        dismiss_btn,
    ]
    .align_y(Alignment::Center);

    container(content)
        .padding(Padding::new(10.0).right(14.0).left(14.0))
        .style(move |_| container::Style {
            background: Some(iced::Background::Color(bg_color)),
            border: iced::Border {
                color: border_color,
                width: 1.0,
                radius: 6.0.into(),
            },
            shadow: iced::Shadow {
                color: iced::Color::from_rgba(0.0, 0.0, 0.0, 0.3),
                offset: iced::Vector::new(0.0, 2.0),
                blur_radius: 8.0,
            },
            ..Default::default()
        })
        .into()
}
