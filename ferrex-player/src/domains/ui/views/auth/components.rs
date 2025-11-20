//! Shared components for authentication views

use crate::common::messages::DomainMessage;
use iced::{
    Element, Length, Theme,
    widget::{Button, Container, Space, button, container, text},
};

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn auth_container<'a>(
    content: impl Into<Element<'a, DomainMessage>>,
) -> Container<'a, DomainMessage> {
    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(iced::alignment::Horizontal::Center)
        .align_y(iced::alignment::Vertical::Center)
        .padding(20)
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn auth_card<'a>(
    content: impl Into<Element<'a, DomainMessage>>,
) -> Container<'a, DomainMessage> {
    container(content)
        .width(Length::Fixed(400.0))
        .padding(30)
        .style(|theme: &Theme| {
            let palette = theme.extended_palette();
            container::Style {
                background: Some(palette.background.weak.color.into()),
                border: iced::Border {
                    color: palette.background.strong.color,
                    width: 1.0,
                    radius: 8.0.into(),
                },
                ..Default::default()
            }
        })
}

/// Creates a primary button with consistent styling
pub fn primary_button<'a>(label: &'a str) -> Button<'a, DomainMessage> {
    button(
        text(label)
            .size(16)
            .align_x(iced::alignment::Horizontal::Center),
    )
    .width(Length::Fill)
    .padding([12, 24])
    .style(|theme: &Theme, status| {
        let palette = theme.extended_palette();
        match status {
            button::Status::Active => button::Style {
                background: Some(palette.primary.base.color.into()),
                text_color: palette.primary.base.text,
                border: iced::Border {
                    radius: 4.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            },
            button::Status::Hovered => button::Style {
                background: Some(palette.primary.strong.color.into()),
                text_color: palette.primary.strong.text,
                border: iced::Border {
                    radius: 4.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            },
            button::Status::Pressed => button::Style {
                background: Some(palette.primary.weak.color.into()),
                text_color: palette.primary.weak.text,
                border: iced::Border {
                    radius: 4.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            },
            button::Status::Disabled => button::Style {
                background: Some(palette.background.strong.color.into()),
                text_color: palette.background.strong.text,
                border: iced::Border {
                    radius: 4.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            },
        }
    })
}

/// Creates a secondary button with consistent styling
pub fn secondary_button<'a>(label: &'a str) -> Button<'a, DomainMessage> {
    button(
        text(label)
            .size(16)
            .align_x(iced::alignment::Horizontal::Center),
    )
    .width(Length::Fill)
    .padding([12, 24])
    .style(|theme: &Theme, status| {
        let palette = theme.extended_palette();
        match status {
            button::Status::Active => button::Style {
                background: None,
                text_color: palette.primary.base.color,
                border: iced::Border {
                    color: palette.primary.base.color,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            },
            button::Status::Hovered => button::Style {
                background: Some(palette.primary.weak.color.into()),
                text_color: palette.primary.base.color,
                border: iced::Border {
                    color: palette.primary.base.color,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            },
            button::Status::Pressed => button::Style {
                background: Some(palette.primary.base.color.into()),
                text_color: palette.primary.base.text,
                border: iced::Border {
                    color: palette.primary.base.color,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            },
            button::Status::Disabled => button::Style {
                background: None,
                text_color: palette.background.strong.text,
                border: iced::Border {
                    color: palette.background.strong.color,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            },
        }
    })
}

/// Creates an error message display
pub fn error_message<'a>(error: &'a str) -> Container<'a, DomainMessage> {
    container(text(error).size(14).style(|theme: &Theme| text::Style {
        color: Some(theme.extended_palette().danger.base.color),
    }))
    .width(Length::Fill)
    .padding([8, 12])
    .style(|theme: &Theme| {
        let danger = theme.extended_palette().danger.weak;
        container::Style {
            background: Some(danger.color.into()),
            border: iced::Border {
                color: theme.extended_palette().danger.base.color,
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        }
    })
}

/// Creates a title text element
pub fn title<'a>(text_content: &'a str) -> Element<'a, DomainMessage> {
    text(text_content)
        .size(28)
        .align_x(iced::alignment::Horizontal::Center)
        .into()
}

/// Creates a subtitle text element
pub fn subtitle<'a>(text_content: &'a str) -> Element<'a, DomainMessage> {
    text(text_content)
        .size(16)
        .style(|theme: &Theme| text::Style {
            color: Some(theme.extended_palette().background.strong.text),
        })
        .align_x(iced::alignment::Horizontal::Center)
        .into()
}

/// Creates consistent vertical spacing
pub fn spacing() -> Space {
    Space::with_height(Length::Fixed(20.0))
}
