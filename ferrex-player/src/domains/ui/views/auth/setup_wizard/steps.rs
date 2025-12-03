//! Individual step views for the setup wizard

use crate::common::focus::ids;
use crate::common::messages::DomainMessage;
use crate::domains::auth::messages as auth;
use crate::domains::auth::security::secure_credential::SecureCredential;
use crate::domains::auth::types::SetupClaimStatus;
use crate::domains::ui::theme;
use crate::domains::ui::views::auth::components::{
    error_message, primary_button,
};
use crate::domains::ui::views::auth::pin_setup::{numeric_keypad, pin_display};
use crate::infra::design_tokens::FontTokens;
use crate::state::State;
use iced::widget::{Space, checkbox, column, container, row, text, text_input};
use iced::{Alignment, Element, Length, Theme};

/// Welcome step - brief introduction
pub fn view_welcome_step<'a>(fonts: &FontTokens) -> Element<'a, DomainMessage> {
    column![
        text("Welcome to Ferrex")
            .size(fonts.title_lg)
            .align_x(Alignment::Center),
        Space::new().height(16),
        text("Let's set up your admin account.")
            .size(fonts.body)
            .style(|theme: &Theme| iced::widget::text::Style {
                color: Some(theme.extended_palette().background.strong.text),
            })
            .align_x(Alignment::Center),
        Space::new().height(24),
        text("This only takes a minute.")
            .size(fonts.caption)
            .style(|theme: &Theme| iced::widget::text::Style {
                color: Some(theme.extended_palette().background.weak.text),
            })
            .align_x(Alignment::Center),
    ]
    .align_x(Alignment::Center)
    .width(Length::Fill)
    .into()
}

/// Account step - username, display name, password
pub fn view_account_step<'a>(
    username: &'a str,
    display_name: &'a str,
    password: &'a SecureCredential,
    confirm_password: &'a SecureCredential,
    show_password: bool,
    error: Option<&'a str>,
    fonts: &FontTokens,
) -> Element<'a, DomainMessage> {
    let mut content = column![
        text("Create Your Account")
            .size(fonts.title)
            .align_x(Alignment::Center),
        Space::new().height(24),
        // Username field
        text("Username").size(fonts.caption),
        Space::new().height(4),
        text_input("Enter username", username)
            .on_input(|s| {
                DomainMessage::Auth(auth::AuthMessage::UpdateSetupField(
                    auth::SetupField::Username(s),
                ))
            })
            .id(ids::auth_first_run_username())
            .padding(12)
            .size(fonts.body)
            .style(theme::TextInput::style()),
        Space::new().height(16),
        // Display name field
        text("Display Name").size(fonts.caption),
        Space::new().height(4),
        text_input("Enter display name", display_name)
            .on_input(|s| {
                DomainMessage::Auth(auth::AuthMessage::UpdateSetupField(
                    auth::SetupField::DisplayName(s),
                ))
            })
            .id(ids::auth_first_run_display_name())
            .padding(12)
            .size(fonts.body)
            .style(theme::TextInput::style()),
        Space::new().height(16),
        // Password field
        text("Password").size(fonts.caption),
        Space::new().height(4),
        text_input("Enter password", password.as_str())
            .on_input(|s| {
                DomainMessage::Auth(auth::AuthMessage::UpdateSetupField(
                    auth::SetupField::Password(s),
                ))
            })
            .secure(!show_password)
            .id(ids::auth_first_run_password())
            .padding(12)
            .size(fonts.body)
            .style(theme::TextInput::style()),
        Space::new().height(12),
        // Confirm password field
        text("Confirm Password").size(fonts.caption),
        Space::new().height(4),
        text_input("Confirm password", confirm_password.as_str())
            .on_input(|s| {
                DomainMessage::Auth(auth::AuthMessage::UpdateSetupField(
                    auth::SetupField::ConfirmPassword(s),
                ))
            })
            .secure(!show_password)
            .id(ids::auth_first_run_confirm_password())
            .padding(12)
            .size(fonts.body)
            .style(theme::TextInput::style()),
        Space::new().height(8),
        // Show password checkbox
        checkbox("Show password", show_password)
            .on_toggle(|_| {
                DomainMessage::Auth(
                    auth::AuthMessage::ToggleSetupPasswordVisibility,
                )
            })
            .size(16)
            .text_size(fonts.caption),
    ]
    .width(Length::Fill);

    // Add error message if present
    if let Some(err) = error {
        content = content
            .push(Space::new().height(16))
            .push(error_message(err, fonts.caption));
    }

    content.into()
}

/// Setup token step - for servers requiring a token
pub fn view_setup_token_step<'a>(
    setup_token: &'a str,
    error: Option<&'a str>,
    fonts: &FontTokens,
) -> Element<'a, DomainMessage> {
    let mut content = column![
        text("Server Setup Token")
            .size(fonts.title)
            .align_x(Alignment::Center),
        Space::new().height(16),
        text("Your server requires a setup token.")
            .size(fonts.body)
            .style(|theme: &Theme| iced::widget::text::Style {
                color: Some(theme.extended_palette().background.strong.text),
            })
            .align_x(Alignment::Center),
        Space::new().height(24),
        text("Setup Token").size(fonts.caption),
        Space::new().height(4),
        text_input("Enter setup token", setup_token)
            .on_input(|s| {
                DomainMessage::Auth(auth::AuthMessage::UpdateSetupField(
                    auth::SetupField::SetupToken(s),
                ))
            })
            .id(ids::auth_first_run_setup_token())
            .padding(12)
            .size(fonts.body)
            .style(theme::TextInput::style()),
        Space::new().height(12),
        container(
            text(
                "Run `just show-setup-token` on your server to get this token."
            )
            .size(fonts.small)
            .style(|theme: &Theme| iced::widget::text::Style {
                color: Some(theme.extended_palette().background.weak.text),
            }),
        )
        .padding([8, 12])
        .style(theme::Container::TechDetail.style()),
    ]
    .width(Length::Fill);

    if let Some(err) = error {
        content = content
            .push(Space::new().height(16))
            .push(error_message(err, fonts.caption));
    }

    content.into()
}

/// Device claim step - verify this device can set up the server
pub fn view_device_claim_step<'a>(
    claim_code: Option<&'a str>,
    claim_status: &SetupClaimStatus,
    claim_loading: bool,
    error: Option<&'a str>,
    fonts: &FontTokens,
) -> Element<'a, DomainMessage> {
    let mut content = column![
        text("Secure Device Binding")
            .size(fonts.title)
            .align_x(Alignment::Center),
        Space::new().height(16),
        text("This binds your device to the server setup.")
            .size(fonts.body)
            .style(|theme: &Theme| iced::widget::text::Style {
                color: Some(theme.extended_palette().background.strong.text),
            })
            .align_x(Alignment::Center),
        Space::new().height(24),
    ]
    .align_x(Alignment::Center)
    .width(Length::Fill);

    // Show claim code or loading state
    if claim_loading && claim_code.is_none() {
        content = content.push(
            text("Requesting binding code...")
                .size(fonts.body)
                .style(|theme: &Theme| iced::widget::text::Style {
                    color: Some(theme.extended_palette().background.weak.text),
                })
                .align_x(Alignment::Center),
        );
    } else if let Some(code) = claim_code {
        // Show the claim code prominently
        content =
            content.push(text("Your Binding Code").size(fonts.caption).style(
                |theme: &Theme| iced::widget::text::Style {
                    color: Some(
                        theme.extended_palette().background.strong.text,
                    ),
                },
            ));
        content = content.push(Space::new().height(8));

        // Large code display
        let code_display =
            container(text(code).size(fonts.title_lg * 1.5).style(
                |theme: &Theme| iced::widget::text::Style {
                    color: Some(theme.extended_palette().primary.base.color),
                },
            ))
            .padding([16, 32])
            .style(|theme: &Theme| container::Style {
                background: Some(
                    theme.extended_palette().background.weak.color.into(),
                ),
                border: iced::Border {
                    color: theme.extended_palette().primary.weak.color,
                    width: 2.0,
                    radius: 8.0.into(),
                },
                ..Default::default()
            });

        content = content.push(
            container(code_display)
                .width(Length::Fill)
                .align_x(Alignment::Center),
        );

        content = content.push(Space::new().height(20));

        // Status indicator
        let is_confirmed = matches!(claim_status, SetupClaimStatus::Confirmed);
        let is_pending = matches!(claim_status, SetupClaimStatus::Pending);

        if is_confirmed {
            content = content.push(
                text("Device bound successfully!")
                    .size(fonts.body)
                    .style(|theme: &Theme| iced::widget::text::Style {
                        color: Some(
                            theme.extended_palette().success.base.color,
                        ),
                    })
                    .align_x(Alignment::Center),
            );
            content = content.push(Space::new().height(16));
        } else if is_pending {
            // Explanation text
            content = content.push(
                container(
                    text("This code confirms you have access to this device on your local network. Click below to bind this device to the server setup.")
                        .size(fonts.caption)
                        .style(|theme: &Theme| iced::widget::text::Style {
                            color: Some(theme.extended_palette().background.weak.text),
                        })
                        .align_x(Alignment::Center),
                )
                .padding([8, 16])
                .width(Length::Fill),
            );

            content = content.push(Space::new().height(16));

            // Bind device button
            let button_label = if claim_loading {
                "Binding..."
            } else {
                "Bind This Device"
            };
            let mut bind_btn = primary_button(button_label, fonts.body)
                .width(Length::Fixed(200.0));
            if !claim_loading {
                bind_btn = bind_btn.on_press(DomainMessage::Auth(
                    auth::AuthMessage::ConfirmSetupClaim,
                ));
            }
            content = content.push(
                container(bind_btn)
                    .width(Length::Fill)
                    .align_x(Alignment::Center),
            );
        }
    }

    // Error message
    if let Some(err) = error {
        content = content.push(Space::new().height(16));
        content = content.push(error_message(err, fonts.caption));
    }

    content.into()
}

/// PIN step - optional 4-digit PIN setup
pub fn view_pin_step<'a>(
    state: &'a State,
    pin: &'a SecureCredential,
    confirm_pin: &'a SecureCredential,
    error: Option<&'a str>,
) -> Element<'a, DomainMessage> {
    let fonts = &state.domains.ui.state.size_provider.font;

    let mut content = column![
        text("Quick Access PIN")
            .size(fonts.title)
            .align_x(Alignment::Center),
        Space::new().height(12),
        text("Create a 4-digit PIN for faster logins.")
            .size(fonts.body)
            .style(|theme: &Theme| iced::widget::text::Style {
                color: Some(theme.extended_palette().background.strong.text),
            })
            .align_x(Alignment::Center),
        Space::new().height(8),
        text("You can skip this and set a PIN later.")
            .size(fonts.caption)
            .style(|theme: &Theme| iced::widget::text::Style {
                color: Some(theme.extended_palette().background.weak.text),
            })
            .align_x(Alignment::Center),
        Space::new().height(24),
    ]
    .align_x(Alignment::Center)
    .width(Length::Fill);

    // Error message
    if let Some(err) = error {
        content = content
            .push(error_message(err, fonts.caption))
            .push(Space::new().height(16));
    }

    // PIN entry label
    content =
        content
            .push(text("Enter PIN").size(fonts.caption).style(
                |theme: &Theme| iced::widget::text::Style {
                    color: Some(
                        theme.extended_palette().background.strong.text,
                    ),
                },
            ))
            .push(Space::new().height(8));

    // PIN display
    content = content.push(
        container(pin_display(pin.as_str(), false, fonts.title))
            .width(Length::Fill)
            .align_x(Alignment::Center),
    );

    content = content.push(Space::new().height(16));

    // Confirm PIN label
    content =
        content
            .push(text("Confirm PIN").size(fonts.caption).style(
                |theme: &Theme| iced::widget::text::Style {
                    color: Some(
                        theme.extended_palette().background.strong.text,
                    ),
                },
            ))
            .push(Space::new().height(8));

    // Confirm PIN display
    content = content.push(
        container(pin_display(confirm_pin.as_str(), true, fonts.title))
            .width(Length::Fill)
            .align_x(Alignment::Center),
    );

    content = content.push(Space::new().height(20));

    // Numeric keypad - target PIN or confirm_pin based on which is being filled
    let keypad = if pin.len() < 4 {
        numeric_keypad(pin.as_str(), false, fonts.title, fonts.body)
    } else {
        numeric_keypad(confirm_pin.as_str(), true, fonts.title, fonts.body)
    };

    content = content.push(
        container(keypad)
            .width(Length::Fill)
            .align_x(Alignment::Center),
    );

    content.into()
}

/// Complete step - success confirmation
pub fn view_complete_step<'a>(
    fonts: &FontTokens,
) -> Element<'a, DomainMessage> {
    column![
        text("You're All Set!")
            .size(fonts.title_lg)
            .align_x(Alignment::Center),
        Space::new().height(20),
        text("Your admin account is ready.")
            .size(fonts.body)
            .style(|theme: &Theme| iced::widget::text::Style {
                color: Some(theme.extended_palette().background.strong.text),
            })
            .align_x(Alignment::Center),
        Space::new().height(32),
        text("Click 'Get Started' to begin using Ferrex.")
            .size(fonts.caption)
            .style(|theme: &Theme| iced::widget::text::Style {
                color: Some(theme.extended_palette().background.weak.text),
            })
            .align_x(Alignment::Center),
    ]
    .align_x(Alignment::Center)
    .width(Length::Fill)
    .into()
}
