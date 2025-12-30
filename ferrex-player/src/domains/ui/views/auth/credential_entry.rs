//! Credential entry view for both password and PIN

use super::components::{
    auth_card, auth_container, error_message, primary_button, secondary_button,
    spacing, title,
};
use crate::common::focus::FocusMessage;
use crate::common::focus::ids;
use crate::common::messages::DomainMessage;
use crate::domains::auth::messages as auth;
use crate::domains::auth::security::secure_credential::SecureCredential;
use crate::domains::auth::types::CredentialType;
use crate::domains::ui::theme;
use crate::domains::ui::views::auth::login_card;
use ferrex_core::player_prelude::User;
use iced::{
    Alignment, Element, Length, Theme,
    widget::{
        Space, button, checkbox, column, container, row, text, text_input,
    },
};

/// Shows the credential entry screen (password or PIN)
#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn view_credential_entry<'a>(
    state: &'a crate::state::State,
    user: &'a User,
    input_type: &'a CredentialType,
    input: &'a SecureCredential,
    show_password: bool,
    remember_device: bool,
    error: Option<&'a str>,
    attempts_remaining: Option<u8>,
    loading: bool,
) -> Element<'a, DomainMessage> {
    let fonts = &state.domains.ui.state.size_provider.font;
    let mut content = column![
        // User info
        container(
            column![
                text(user.display_name.chars().next().unwrap_or('U')).size(48),
            ]
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
        title(&user.display_name, fonts.title_lg),
        text(format!("@{}", user.username))
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

    // Keep a stable widget tree before the input field to avoid focus loss when
    // error/warning content appears/disappears (iced widget state is positional).
    let has_error = error.is_some();
    let error_slot: Element<'a, DomainMessage> = error
        .map(|err| error_message(err, fonts.caption).into())
        .unwrap_or_else(|| Space::new().height(Length::Fixed(0.0)).into());
    content = content.push(error_slot);
    content = content.push(Space::new().height(if has_error {
        Length::Fixed(12.0)
    } else {
        Length::Fixed(0.0)
    }));

    let show_attempts_warning = attempts_remaining.is_some_and(|a| a < 3);
    let attempts_warning: Element<'a, DomainMessage> = if let Some(attempts) =
        attempts_remaining
        && attempts < 3
    {
        text(format!("{} attempts remaining", attempts))
            .size(14)
            .style(|theme: &Theme| text::Style {
                color: Some(theme.extended_palette().danger.base.color),
            })
            .into()
    } else {
        Space::new().height(Length::Fixed(0.0)).into()
    };
    content = content.push(attempts_warning);
    content = content.push(Space::new().height(if show_attempts_warning {
        Length::Fixed(8.0)
    } else {
        Length::Fixed(0.0)
    }));

    // Input field
    match input_type {
        CredentialType::Password => {
            content = content.push(
                text_input("Password", input.as_str())
                    .on_input(|s| {
                        DomainMessage::Auth(
                            auth::AuthMessage::UpdateCredential(s),
                        )
                    })
                    .on_submit(DomainMessage::Auth(
                        auth::AuthMessage::SubmitCredential,
                    ))
                    .secure(!show_password)
                    .id(ids::auth_password_entry())
                    .padding(12)
                    .size(16)
                    .width(Length::Fill),
            );

            content = content.push(Space::new().height(Length::Fixed(8.0)));

            // Password visibility toggle
            content = content.push(
                checkbox(show_password)
                    .label("Show password")
                    .on_toggle(|_| {
                        DomainMessage::Auth(
                            auth::AuthMessage::TogglePasswordVisibility,
                        )
                    })
                    .style(theme::Checkbox::style())
                    .size(16)
                    .text_size(fonts.caption)
                    .spacing(8),
            );

            content = content.push(Space::new().height(Length::Fixed(8.0)));

            // Remember device checkbox
            content = content.push(
                checkbox(remember_device)
                    .label("Remember this device")
                    .on_toggle(|_| {
                        DomainMessage::Auth(
                            auth::AuthMessage::ToggleRememberDevice,
                        )
                    })
                    .style(theme::Checkbox::style())
                    .size(16)
                    .text_size(fonts.caption)
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
        || (matches!(input_type, CredentialType::Pin { .. })
            && input.len() != 4)
    {
        primary_button(submit_label, fonts.body)
    } else {
        primary_button(submit_label, fonts.body)
            .on_press(DomainMessage::Auth(auth::AuthMessage::SubmitCredential))
    };

    content = content.push(submit_button);

    content = content.push(Space::new().height(Length::Fixed(12.0)));

    // Back button
    content = content.push(
        secondary_button("Back", fonts.body)
            .on_press(DomainMessage::Auth(auth::AuthMessage::Back)),
    );

    let card = auth_card(content.align_x(Alignment::Center));
    auth_container(card).into()
}

/// Shows a pre-auth login screen with username and password (no server-provided user yet)
#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn view_pre_auth_login<'a>(
    state: &'a crate::state::State,
    username: &'a str,
    password: &'a SecureCredential,
    show_password: bool,
    remember_device: bool,
    error: Option<&'a str>,
    loading: bool,
) -> Element<'a, DomainMessage> {
    let fonts = &state.domains.ui.state.size_provider.font;
    let mut content = column![title("Sign in", fonts.title_lg), spacing(),];

    // Keep a stable widget tree before inputs to preserve focus when errors are cleared.
    let has_error = error.is_some();
    let error_slot: Element<'a, DomainMessage> = error
        .map(|err| error_message(err, fonts.caption).into())
        .unwrap_or_else(|| Space::new().height(Length::Fixed(0.0)).into());
    content = content.push(error_slot);
    content = content.push(Space::new().height(if has_error {
        Length::Fixed(12.0)
    } else {
        Length::Fixed(0.0)
    }));

    // Username input
    content = content.push(
        text_input("Username", username)
            .on_input(|s| {
                DomainMessage::Auth(auth::AuthMessage::PreAuthUpdateUsername(s))
            })
            .on_submit(DomainMessage::Focus(FocusMessage::Traverse {
                backwards: false,
            }))
            .id(ids::auth_pre_auth_username())
            .padding(12)
            .size(16)
            .width(Length::Fill),
    );

    content = content.push(Space::new().height(Length::Fixed(8.0)));

    // Password input
    content = content.push(
        text_input("Password", password.as_str())
            .on_input(|s| {
                DomainMessage::Auth(auth::AuthMessage::UpdateCredential(s))
            })
            .on_submit(DomainMessage::Auth(auth::AuthMessage::PreAuthSubmit))
            .secure(!show_password)
            .id(ids::auth_pre_auth_password())
            .padding(12)
            .size(16)
            .width(Length::Fill),
    );

    content = content.push(Space::new().height(Length::Fixed(8.0)));

    // Toggles
    content = content.push(
        column![
            checkbox(show_password)
                .label("Show password")
                .on_toggle(|_| DomainMessage::Auth(
                    auth::AuthMessage::PreAuthTogglePasswordVisibility
                ))
                .style(theme::Checkbox::style())
                .size(16)
                .text_size(fonts.caption)
                .spacing(8),
            checkbox(remember_device)
                .label("Remember this device")
                .on_toggle(|_| DomainMessage::Auth(
                    auth::AuthMessage::PreAuthToggleRememberDevice
                ))
                .style(theme::Checkbox::style())
                .size(16)
                .text_size(fonts.caption)
                .spacing(8),
        ]
        .spacing(8),
    );

    content = content.push(spacing());

    // Submit button
    let submit_label = if loading { "Signing in..." } else { "Sign In" };
    let submit_button = if loading {
        primary_button(submit_label, fonts.body)
    } else {
        primary_button(submit_label, fonts.body)
            .on_press(DomainMessage::Auth(auth::AuthMessage::PreAuthSubmit))
    };

    content = content.push(submit_button);

    // Wrap in auth container (centered on screen)
    let card = login_card(
        container(content)
            .padding(24)
            .width(Length::Fill)
            .height(Length::Shrink),
    );

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
        Space::new().height(Length::Fixed(20.0)),
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
            Some(DomainMessage::Auth(auth::AuthMessage::UpdateCredential(
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
        Some(DomainMessage::Auth(auth::AuthMessage::UpdateCredential(
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
            Space::new().width(Length::Fixed(button_size)),
            create_digit_button('0'),
            backspace_button,
        ]
        .spacing(8),
    ]
    .spacing(8)
    .align_x(Alignment::Center)
    .into()
}
