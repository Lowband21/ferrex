//! Credential entry view for both password and PIN

use super::components::{
    auth_card, auth_container, error_message, primary_button, secondary_button,
    spacing, title,
};
use crate::common::focus::FocusMessage;
use crate::common::focus::ids;
use crate::common::messages::DomainMessage;
use crate::domains::auth::messages as auth;
use crate::domains::auth::pin_policy::{
    PinPolicyRules, validate_pin_with_policy,
};
use crate::domains::auth::security::secure_credential::SecureCredential;
use crate::domains::auth::types::CredentialType;
use crate::domains::ui::theme;
use crate::domains::ui::views::auth::login_card;
use ferrex_core::player_prelude::User;
use iced::{
    Alignment, Element, Length, Theme,
    widget::{Space, checkbox, column, container, text, text_input},
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
        CredentialType::Pin { .. } => {
            content = content.push(
                text_input("PIN", input.as_str())
                    .on_input(|s| {
                        DomainMessage::Auth(
                            auth::AuthMessage::UpdateCredential(s),
                        )
                    })
                    .on_submit(DomainMessage::Auth(
                        auth::AuthMessage::SubmitCredential,
                    ))
                    .secure(true)
                    .id(ids::auth_pin_entry())
                    .padding(12)
                    .size(16)
                    .width(Length::Fill)
                    .style(theme::TextInput::style()),
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

    let base_pin_policy: PinPolicyRules =
        (&state.domains.auth.state.pin_policy).into();
    let pin_can_submit = match input_type {
        CredentialType::Password => true,
        CredentialType::Pin {
            min_length,
            max_length,
        } => {
            let pin_policy = PinPolicyRules {
                min_length: *min_length,
                max_length: *max_length,
                ..base_pin_policy
            };
            validate_pin_with_policy(input.as_str(), pin_policy).is_ok()
        }
    };
    let submit_button = if loading || !pin_can_submit {
        primary_button(submit_label, fonts.body)
    } else {
        primary_button(submit_label, fonts.body)
            .on_press(DomainMessage::Auth(auth::AuthMessage::SubmitCredential))
    };

    content = content.push(submit_button);

    content = content.push(Space::new().height(Length::Fixed(12.0)));

    if matches!(input_type, CredentialType::Pin { .. }) {
        content = content.push(
            secondary_button("Use password instead", fonts.body).on_press(
                DomainMessage::Auth(auth::AuthMessage::UsePasswordLogin),
            ),
        );
        content = content.push(Space::new().height(Length::Fixed(8.0)));
    }

    if error.is_some() {
        content = content.push(
            secondary_button("Retry", fonts.body)
                .on_press(DomainMessage::Auth(auth::AuthMessage::Retry)),
        );
        content = content.push(Space::new().height(Length::Fixed(8.0)));
    }

    content = content.push(
        secondary_button("Reset local auth state", fonts.body).on_press(
            DomainMessage::Auth(auth::AuthMessage::ResetLocalAuthState),
        ),
    );

    content = content.push(Space::new().height(Length::Fixed(8.0)));

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
    content = content.push(Space::new().height(Length::Fixed(12.0)));
    content = content.push(
        secondary_button("Reset local auth state", fonts.body).on_press(
            DomainMessage::Auth(auth::AuthMessage::ResetLocalAuthState),
        ),
    );

    // Wrap in auth container (centered on screen)
    let card = login_card(
        container(content)
            .padding(24)
            .width(Length::Fill)
            .height(Length::Shrink),
    );

    auth_container(card).into()
}
