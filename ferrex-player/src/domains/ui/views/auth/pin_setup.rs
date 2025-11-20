//! PIN setup view

use super::components::{
    auth_card, auth_container, error_message, primary_button, secondary_button,
    spacing, title,
};
use crate::common::messages::DomainMessage;
use crate::domains::auth::messages as auth;
use crate::domains::auth::security::secure_credential::SecureCredential;
use ferrex_core::player_prelude::User;
use iced::{
    Alignment, Element, Length, Theme,
    widget::{Space, button, column, container, row, text},
};

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn view_pin_setup<'a>(
    user: &'a User,
    pin: &'a SecureCredential,
    confirm_pin: &'a SecureCredential,
    error: Option<&'a str>,
) -> Element<'a, DomainMessage> {
    let mut content = column![
        title("Set Up PIN"),
        text(format!("Create a 4-digit PIN for {}", user.display_name))
            .size(16)
            .style(|theme: &Theme| {
                text::Style {
                    color: Some(
                        theme.extended_palette().background.strong.text,
                    ),
                }
            })
            .align_x(iced::alignment::Horizontal::Center),
        spacing(),
    ];

    // Error message
    if let Some(err) = error {
        content = content.push(error_message(err));
        content = content.push(spacing());
    }

    // PIN entry
    content =
        content.push(text("Enter PIN").size(14).style(|theme: &Theme| {
            text::Style {
                color: Some(theme.extended_palette().background.strong.text),
            }
        }));

    content = content.push(Space::new().height(Length::Fixed(8.0)));

    content = content.push(
        container(pin_display(pin.as_str(), false))
            .width(Length::Fill)
            .align_x(iced::alignment::Horizontal::Center),
    );

    content = content.push(spacing());

    // Confirm PIN entry
    content =
        content.push(text("Confirm PIN").size(14).style(|theme: &Theme| {
            text::Style {
                color: Some(theme.extended_palette().background.strong.text),
            }
        }));

    content = content.push(Space::new().height(Length::Fixed(8.0)));

    content = content.push(
        container(pin_display(confirm_pin.as_str(), true))
            .width(Length::Fill)
            .align_x(iced::alignment::Horizontal::Center),
    );

    content = content.push(spacing());

    // Numeric keypad
    let keypad = if pin.len() < 4 {
        numeric_keypad(pin.as_str(), false)
    } else {
        numeric_keypad(confirm_pin.as_str(), true)
    };

    content = content.push(
        container(keypad)
            .width(Length::Fill)
            .align_x(iced::alignment::Horizontal::Center),
    );

    content = content.push(spacing());

    // Submit button
    let can_submit = pin.len() == 4 && confirm_pin.len() == 4;
    let submit_button = if can_submit {
        primary_button("Set PIN")
            .on_press(DomainMessage::Auth(auth::AuthMessage::SubmitPin))
    } else {
        primary_button("Set PIN")
    };

    content = content.push(submit_button);

    content = content.push(Space::new().height(Length::Fixed(12.0)));

    // Skip button
    content = content.push(
        secondary_button("Skip for now")
            .on_press(DomainMessage::Auth(auth::AuthMessage::Back)),
    );

    let card = auth_card(content.align_x(Alignment::Center));
    auth_container(card).into()
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
fn pin_display<'a>(
    value: &str,
    is_confirm: bool,
) -> Element<'a, DomainMessage> {
    let digits: Vec<Element<'a, DomainMessage>> = (0..4)
        .map(|i| {
            let digit = value.chars().nth(i);
            let display = if digit.is_some() { "●" } else { "○" };

            container(
                text(display)
                    .size(24)
                    .align_x(iced::alignment::Horizontal::Center),
            )
            .width(Length::Fixed(50.0))
            .height(Length::Fixed(50.0))
            .align_x(iced::alignment::Horizontal::Center)
            .align_y(iced::alignment::Vertical::Center)
            .style(move |theme: &Theme| {
                let palette = theme.extended_palette();
                container::Style {
                    background: if digit.is_some() {
                        Some(palette.primary.weak.color.into())
                    } else {
                        None
                    },
                    border: iced::Border {
                        color: if digit.is_some() {
                            palette.primary.base.color
                        } else {
                            palette.background.strong.color
                        },
                        width: 2.0,
                        radius: 6.0.into(),
                    },
                    ..Default::default()
                }
            })
            .into()
        })
        .collect();

    row(digits).spacing(8).align_y(Alignment::Center).into()
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
fn numeric_keypad<'a>(
    current_value: &str,
    is_confirm: bool,
) -> Element<'a, DomainMessage> {
    let button_size = 60.0;

    let create_digit_button = |digit: char| {
        let message = if is_confirm {
            DomainMessage::Auth(auth::AuthMessage::UpdateConfirmPin(format!(
                "{}{}",
                current_value, digit
            )))
        } else {
            DomainMessage::Auth(auth::AuthMessage::UpdatePin(format!(
                "{}{}",
                current_value, digit
            )))
        };

        button(
            text(digit)
                .size(24)
                .align_x(iced::alignment::Horizontal::Center),
        )
        .on_press_maybe(if current_value.len() < 4 {
            Some(message)
        } else {
            None
        })
        .width(Length::Fixed(button_size))
        .height(Length::Fixed(button_size))
        .style(|theme: &Theme, status| {
            let palette = theme.extended_palette();
            match status {
                button::Status::Active => button::Style {
                    background: Some(palette.background.weak.color.into()),
                    text_color: palette.background.base.text,
                    border: iced::Border {
                        radius: 8.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                button::Status::Hovered => button::Style {
                    background: Some(palette.primary.weak.color.into()),
                    text_color: palette.background.base.text,
                    border: iced::Border {
                        radius: 8.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                button::Status::Pressed => button::Style {
                    background: Some(palette.primary.base.color.into()),
                    text_color: palette.primary.base.text,
                    border: iced::Border {
                        radius: 8.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                button::Status::Disabled => button::Style {
                    background: Some(palette.background.weak.color.into()),
                    text_color: palette.background.strong.text,
                    border: iced::Border {
                        radius: 8.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            }
        })
    };

    let clear_button = button(
        text("Clear")
            .size(16)
            .align_x(iced::alignment::Horizontal::Center),
    )
    .on_press(if is_confirm {
        DomainMessage::Auth(auth::AuthMessage::UpdateConfirmPin(String::new()))
    } else {
        DomainMessage::Auth(auth::AuthMessage::UpdatePin(String::new()))
    })
    .width(Length::Fixed(button_size * 2.0 + 8.0))
    .height(Length::Fixed(button_size))
    .style(|theme: &Theme, _| {
        let palette = theme.extended_palette();
        button::Style {
            background: Some(palette.background.weak.color.into()),
            text_color: palette.background.base.text,
            border: iced::Border {
                radius: 8.0.into(),
                ..Default::default()
            },
            ..Default::default()
        }
    });

    column![
        row![
            create_digit_button('1'),
            create_digit_button('2'),
            create_digit_button('3'),
        ]
        .spacing(8),
        row![
            create_digit_button('4'),
            create_digit_button('5'),
            create_digit_button('6'),
        ]
        .spacing(8),
        row![
            create_digit_button('7'),
            create_digit_button('8'),
            create_digit_button('9'),
        ]
        .spacing(8),
        row![create_digit_button('0'), clear_button,]
            .spacing(8)
            .align_y(Alignment::Center),
    ]
    .spacing(8)
    .align_x(Alignment::Center)
    .into()
}
