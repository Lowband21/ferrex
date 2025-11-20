//! Credential entry view for both password and PIN

use super::components::{
    auth_card, auth_container, error_message, primary_button, secondary_button, spacing, subtitle,
    title,
};
use crate::messages::{auth, DomainMessage};
use crate::security::SecureCredential;
use crate::state::CredentialType;
use ferrex_core::user::User;
use iced::{
    widget::{button, checkbox, column, container, row, text, text_input, Space},
    Alignment, Element, Length, Theme,
};

/// Shows the credential entry screen (password or PIN)
pub fn view_credential_entry<'a>(
    user: &'a User,
    input_type: &'a CredentialType,
    input: &'a SecureCredential,
    show_password: bool,
    remember_device: bool,
    error: Option<&'a str>,
    attempts_remaining: Option<u8>,
    loading: bool,
) -> Element<'a, DomainMessage> {
    let mut content = column![
        // User info
        container(
            column![text(user.display_name.chars().next().unwrap_or('U')).size(48),]
                .align_x(Alignment::Center)
        )
        .width(Length::Fill)
        .align_x(iced::alignment::Horizontal::Center)
        .style(|theme: &Theme| {
            let palette = theme.extended_palette();
            container::Style {
                background: Some(palette.primary.weak.color.into()),
                border: iced::Border {
                    radius: 40.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        })
        .width(Length::Fixed(80.0))
        .height(Length::Fixed(80.0))
        .align_y(iced::alignment::Vertical::Center),
        spacing(),
        title(&user.display_name),
        text(format!("@{}", user.username))
            .size(16)
            .style(|theme: &Theme| {
                text::Style {
                    color: Some(theme.extended_palette().background.strong.text),
                }
            })
            .align_x(iced::alignment::Horizontal::Center),
        spacing(),
    ];

    // Error message
    if let Some(err) = error {
        content = content.push(error_message(err));
        content = content.push(Space::with_height(Length::Fixed(12.0)));
    }

    // Attempts remaining warning
    if let Some(attempts) = attempts_remaining {
        if attempts < 3 {
            content = content.push(
                text(format!("{} attempts remaining", attempts))
                    .size(14)
                    .style(|theme: &Theme| text::Style {
                        color: Some(theme.extended_palette().danger.base.color),
                    }),
            );
            content = content.push(Space::with_height(Length::Fixed(8.0)));
        }
    }

    // Input field
    match input_type {
        CredentialType::Password => {
            content = content.push(
                text_input("Password", input.as_str())
                    .on_input(|s| DomainMessage::Auth(auth::Message::UpdateCredential(s)))
                    .on_submit(DomainMessage::Auth(auth::Message::SubmitCredential))
                    .secure(!show_password)
                    .padding(12)
                    .size(16)
                    .width(Length::Fill),
            );

            content = content.push(Space::with_height(Length::Fixed(8.0)));

            // Password visibility toggle
            content = content.push(
                checkbox("Show password", show_password)
                    .on_toggle(|_| DomainMessage::Auth(auth::Message::TogglePasswordVisibility))
                    .size(16)
                    .spacing(8),
            );

            content = content.push(Space::with_height(Length::Fixed(8.0)));

            // Remember device checkbox
            content = content.push(
                checkbox("Remember this device", remember_device)
                    .on_toggle(|_| DomainMessage::Auth(auth::Message::ToggleRememberDevice))
                    .size(16)
                    .spacing(8),
            );
        }
        CredentialType::Pin { max_length } => {
            content = content.push(
                container(pin_input(input.as_str(), *max_length as u8))
                    .width(Length::Fill)
                    .align_x(iced::alignment::Horizontal::Center),
            );
        }
    }

    content = content.push(spacing());

    // Submit button
    let submit_label = if loading {
        "Signing in..."
    } else {
        match input_type {
            CredentialType::Password => "Sign In",
            CredentialType::Pin { .. } => "Enter",
        }
    };

    let submit_button = if loading
        || (matches!(input_type, CredentialType::Pin { .. }) && input.len() != 4)
    {
        primary_button(submit_label)
    } else {
        primary_button(submit_label).on_press(DomainMessage::Auth(auth::Message::SubmitCredential))
    };

    content = content.push(submit_button);

    content = content.push(Space::with_height(Length::Fixed(12.0)));

    // Back button
    content =
        content.push(secondary_button("Back").on_press(DomainMessage::Auth(auth::Message::Back)));

    let card = auth_card(content.align_x(Alignment::Center));
    auth_container(card).into()
}

/// Creates a PIN input display
fn pin_input<'a>(value: &str, max_length: u8) -> Element<'a, DomainMessage> {
    let digits: Vec<Element<'a, DomainMessage>> = (0..max_length)
        .map(|i| {
            let digit = value.chars().nth(i as usize);
            let display = if digit.is_some() { "●" } else { "○" };

            container(
                text(display)
                    .size(32)
                    .align_x(iced::alignment::Horizontal::Center),
            )
            .width(Length::Fixed(60.0))
            .height(Length::Fixed(60.0))
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
                        radius: 8.0.into(),
                    },
                    ..Default::default()
                }
            })
            .into()
        })
        .collect();

    column![
        row(digits).spacing(12).align_y(Alignment::Center),
        Space::with_height(Length::Fixed(20.0)),
        // Numeric keypad
        numeric_keypad(value)
    ]
    .align_x(Alignment::Center)
    .into()
}

/// Creates a numeric keypad for PIN entry
fn numeric_keypad<'a>(current_value: &str) -> Element<'a, DomainMessage> {
    let button_size = 60.0;

    let create_digit_button = |digit: char| {
        button(
            text(digit)
                .size(24)
                .align_x(iced::alignment::Horizontal::Center),
        )
        .on_press_maybe(if current_value.len() < 4 {
            Some(DomainMessage::Auth(auth::Message::UpdateCredential(
                format!("{}{}", current_value, digit),
            )))
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

    let backspace_button = button(
        text("⌫")
            .size(24)
            .align_x(iced::alignment::Horizontal::Center),
    )
    .on_press_maybe(if !current_value.is_empty() {
        let mut new_value = current_value.to_string();
        new_value.pop();
        Some(DomainMessage::Auth(auth::Message::UpdateCredential(
            new_value,
        )))
    } else {
        None
    })
    .width(Length::Fixed(button_size))
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
        row![
            Space::with_width(Length::Fixed(button_size)),
            create_digit_button('0'),
            backspace_button,
        ]
        .spacing(8),
    ]
    .spacing(8)
    .align_x(Alignment::Center)
    .into()
}
