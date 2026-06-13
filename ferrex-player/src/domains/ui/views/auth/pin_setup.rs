//! PIN setup view

use super::components::{
    auth_card, auth_container, error_message, primary_button, secondary_button,
    spacing, title,
};
use crate::common::messages::DomainMessage;
use crate::domains::auth::messages as auth;
use crate::domains::auth::pin_policy::{
    PinPolicyRules, pin_pair_satisfies_policy, pin_satisfies_policy,
    policy_label_for,
};
use crate::domains::auth::security::secure_credential::SecureCredential;
use crate::domains::auth::types::PinEntryTarget;
use crate::state::State;
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
    state: &'a State,
    user: &'a User,
    pin: &'a SecureCredential,
    confirm_pin: &'a SecureCredential,
    pin_entry_target: PinEntryTarget,
    error: Option<&'a str>,
) -> Element<'a, DomainMessage> {
    let fonts = &state.domains.ui.state.size_provider.font;
    let policy: PinPolicyRules = (&state.domains.auth.state.pin_policy).into();
    let max_length = policy.max_length;
    let pin_is_valid = pin_satisfies_policy(pin.as_str(), policy);
    let effective_target =
        if pin_entry_target == PinEntryTarget::ConfirmPin && pin_is_valid {
            PinEntryTarget::ConfirmPin
        } else {
            PinEntryTarget::Pin
        };

    let mut content = column![
        title("Set Up PIN", fonts.title_lg),
        text(format!("Create a secure PIN for {}", user.display_name))
            .size(fonts.body)
            .style(|theme: &Theme| {
                text::Style {
                    color: Some(
                        theme.extended_palette().background.strong.text,
                    ),
                }
            })
            .align_x(iced::alignment::Horizontal::Center),
        text(policy_label_for(policy))
            .size(fonts.caption)
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
        content = content.push(error_message(err, fonts.caption));
        content = content.push(spacing());
    }

    // PIN entry
    content = content.push(text("Enter PIN").size(fonts.caption).style(
        |theme: &Theme| text::Style {
            color: Some(theme.extended_palette().background.strong.text),
        },
    ));

    content = content.push(Space::new().height(Length::Fixed(8.0)));

    content = content.push(
        container(pin_display(pin.as_str(), false, fonts.title, max_length))
            .width(Length::Fill)
            .align_x(iced::alignment::Horizontal::Center),
    );

    content = content.push(spacing());

    // Confirm PIN entry
    content = content.push(text("Confirm PIN").size(fonts.caption).style(
        |theme: &Theme| text::Style {
            color: Some(theme.extended_palette().background.strong.text),
        },
    ));

    content = content.push(Space::new().height(Length::Fixed(8.0)));

    content = content.push(
        container(pin_display(
            confirm_pin.as_str(),
            true,
            fonts.title,
            max_length,
        ))
        .width(Length::Fill)
        .align_x(iced::alignment::Horizontal::Center),
    );

    content = content.push(spacing());

    let edit_pin = secondary_button("Edit PIN", fonts.caption).on_press(
        DomainMessage::Auth(auth::AuthMessage::SelectPinEntryTarget(
            PinEntryTarget::Pin,
        )),
    );
    let mut edit_confirm = secondary_button("Confirm PIN", fonts.caption);
    if pin_is_valid {
        edit_confirm = edit_confirm.on_press(DomainMessage::Auth(
            auth::AuthMessage::SelectPinEntryTarget(PinEntryTarget::ConfirmPin),
        ));
    }
    content = content.push(
        row![edit_pin, edit_confirm]
            .spacing(8)
            .align_y(Alignment::Center),
    );

    content = content.push(spacing());

    // Numeric keypad
    let (keypad_value, is_confirm) = match effective_target {
        PinEntryTarget::Pin => (pin.as_str(), false),
        PinEntryTarget::ConfirmPin => (confirm_pin.as_str(), true),
    };
    let keypad = numeric_keypad(
        keypad_value,
        is_confirm,
        fonts.title,
        fonts.body,
        max_length,
    );

    content = content.push(
        container(keypad)
            .width(Length::Fill)
            .align_x(iced::alignment::Horizontal::Center),
    );

    content = content.push(spacing());

    // Submit button
    let can_submit =
        pin_pair_satisfies_policy(pin.as_str(), confirm_pin.as_str(), policy);
    let submit_button = if can_submit {
        primary_button("Set PIN", fonts.body)
            .on_press(DomainMessage::Auth(auth::AuthMessage::SubmitPin))
    } else {
        primary_button("Set PIN", fonts.body)
    };

    content = content.push(submit_button);

    content = content.push(Space::new().height(Length::Fixed(12.0)));

    // Skip button
    content = content.push(
        secondary_button("Skip for now", fonts.body)
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
pub fn pin_display<'a>(
    value: &str,
    _is_confirm: bool,
    font_size: f32,
    max_length: usize,
) -> Element<'a, DomainMessage> {
    let digits: Vec<Element<'a, DomainMessage>> = (0..max_length)
        .map(|i| {
            let digit = value.chars().nth(i);
            let display = if digit.is_some() { "●" } else { "○" };

            container(
                text(display)
                    .size(font_size)
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
pub fn numeric_keypad<'a>(
    current_value: &str,
    is_confirm: bool,
    digit_font_size: f32,
    label_font_size: f32,
    max_length: usize,
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
                .size(digit_font_size)
                .align_x(iced::alignment::Horizontal::Center),
        )
        .on_press_maybe(if current_value.len() < max_length {
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
            .size(label_font_size)
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
