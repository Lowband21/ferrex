//! Authentication views module
//!
//! This module provides all authentication-related UI components in a modular way.
//! Each authentication flow state has its own view component.

mod components;
mod credential_entry;
mod loading_users;
mod pin_setup;
mod user_carousel;
mod user_selection;

use crate::common::focus::ids;
use crate::common::messages::DomainMessage;
use crate::domains::auth::types::{
    AuthenticationFlow, SetupClaimStatus, SetupClaimUi,
};
use ferrex_core::player_prelude::UserPermissions;
use iced::Element;

pub use components::*;
pub use credential_entry::view_credential_entry;
pub use loading_users::view_loading_users;
pub use pin_setup::view_pin_setup;
pub use user_carousel::{
    UserCarouselMessage, UserCarouselState, view_user_carousel,
    view_user_selection_with_carousel,
};
pub use user_selection::view_user_selection;

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn view_auth<'a>(
    auth_flow: &'a AuthenticationFlow,
    user_permissions: Option<&'a UserPermissions>,
) -> Element<'a, DomainMessage> {
    use AuthenticationFlow::*;

    match auth_flow {
        CheckingSetup => view_loading_users(), // Show loading while checking

        CheckingAutoLogin => view_loading_users(), // Show loading while checking auto-login

        FirstRunSetup {
            username,
            password,
            confirm_password,
            display_name,
            setup_token,
            claim_token,
            show_password,
            error,
            loading,
            claim,
        } => view_first_run_setup(
            username,
            password,
            confirm_password,
            display_name,
            setup_token,
            claim_token,
            *show_password,
            error.as_deref(),
            *loading,
            claim,
        ),

        LoadingUsers => view_loading_users(),

        SelectingUser { users, error } => view_user_selection_with_carousel(
            users,
            error.as_deref(),
            user_permissions,
        ),

        CheckingDevice { user } => {
            view_loading_users() // Show loading while checking device
        }

        EnteringCredentials {
            user,
            input_type,
            input,
            show_password,
            remember_device,
            error,
            attempts_remaining,
            loading,
        } => view_credential_entry(
            user,
            input_type,
            input,
            *show_password,
            *remember_device,
            error.as_deref(),
            *attempts_remaining,
            *loading,
        ),

        SettingUpPin {
            user,
            pin,
            confirm_pin,
            error,
        } => view_pin_setup(user, pin, confirm_pin, error.as_deref()),

        Authenticated { user, mode } => {
            // This state should not render auth views - the app should show main content
            view_loading_users() // Fallback
        }
    }
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn view_first_run_setup<'a>(
    username: &'a str,
    password: &'a crate::domains::auth::security::secure_credential::SecureCredential,
    confirm_password: &'a crate::domains::auth::security::secure_credential::SecureCredential,
    display_name: &'a str,
    setup_token: &'a str,
    claim_token: &'a str,
    show_password: bool,
    error: Option<&'a str>,
    loading: bool,
    claim: &'a SetupClaimUi,
) -> Element<'a, DomainMessage> {
    use crate::domains::auth::messages as auth;
    use crate::domains::ui::theme;
    use chrono::{DateTime, Local};
    use iced::widget::{
        Space, button, checkbox, column, container, row, text, text_input,
    };
    use iced::{Alignment, Length};

    let title = text("Welcome to Ferrex Media Server").size(32);

    let subtitle = text("Let's create your admin account").size(18);

    let username_input = text_input("Username", username)
        .on_input(|s| {
            auth::Message::UpdateSetupField(auth::SetupField::Username(s))
                .into()
        })
        .id(ids::auth_first_run_username())
        .padding(12)
        .size(16)
        .style(theme::TextInput::style());

    let display_name_input = text_input("Display Name", display_name)
        .on_input(|s| {
            auth::Message::UpdateSetupField(auth::SetupField::DisplayName(s))
                .into()
        })
        .id(ids::auth_first_run_display_name())
        .padding(12)
        .size(16)
        .style(theme::TextInput::style());

    let password_input = text_input("Password", password.as_str())
        .on_input(|s| {
            auth::Message::UpdateSetupField(auth::SetupField::Password(s))
                .into()
        })
        .secure(!show_password)
        .id(ids::auth_first_run_password())
        .padding(12)
        .size(16)
        .style(theme::TextInput::style());

    let confirm_password_input =
        text_input("Confirm Password", confirm_password.as_str())
            .on_input(|s| {
                auth::Message::UpdateSetupField(
                    auth::SetupField::ConfirmPassword(s),
                )
                .into()
            })
            .secure(!show_password)
            .id(ids::auth_first_run_confirm_password())
            .padding(12)
            .size(16)
            .style(theme::TextInput::style());

    let show_password_checkbox = checkbox("Show password", show_password)
        .on_toggle(|_| auth::Message::ToggleSetupPasswordVisibility.into())
        .size(16)
        .text_size(14);

    let setup_token_input =
        text_input("Setup Token (if required)", setup_token)
            .on_input(|s| {
                auth::Message::UpdateSetupField(auth::SetupField::SetupToken(s))
                    .into()
            })
            .id(ids::auth_first_run_setup_token())
            .padding(12)
            .size(16)
            .style(theme::TextInput::style());

    let claim_token_display =
        container(text(claim_token).size(16).style(|theme: &iced::Theme| {
            text::Style {
                color: Some(theme.extended_palette().background.strong.text),
            }
        }))
        .padding(12)
        .width(Length::Fill)
        .style(theme::Container::Default.style());

    let mut claim_metadata = column![
        text("Secure Device Binding").size(18),
        Space::new().height(8),
        text(
            "Generate a binding code from the player on the same network. Confirm it here to receive the claim token required for admin creation.",
        )
        .size(14)
        .style(|theme: &iced::Theme| text::Style {
            color: Some(theme.extended_palette().background.strong.text),
        }),
        Space::new().height(12),
        container(
            column![
                text("Binding codes expire after 10 minutes. Request a fresh code if the countdown has passed or the player warns that it is stale.")
                    .size(13)
                    .style(|theme: &iced::Theme| text::Style {
                        color: Some(theme.extended_palette().background.weak.text),
                    }),
                Space::new().height(8),
                text(
                    "Bindings remain scoped to the server's LAN; connect through your trusted VPN/mesh (Tailscale, WireGuard, etc.) or use `ferrex-server --claim-reset` before issuing a new code if a session gets stuck.",
                )
                .size(13)
                .style(|theme: &iced::Theme| text::Style {
                    color: Some(theme.extended_palette().background.weak.text),
                }),
            ],
        )
        .padding(12)
        .style(theme::Container::TechDetail.style()),
        Space::new().height(16),
    ];

    if claim.lan_only {
        claim_metadata = claim_metadata.push(
            container(
                column![
                    text("Local-only binding active")
                        .size(14)
                        .style(|theme: &iced::Theme| text::Style {
                            color: Some(theme.extended_palette().primary.strong.text),
                        }),
                    Space::new().height(4),
                    text(
                        "Confirm while your device is on the server's LAN or an authorized VPN peer (e.g., Tailscale). Without that reachability the confirm step will fail.",
                    )
                    .size(13)
                    .style(|theme: &iced::Theme| text::Style {
                        color: Some(theme.extended_palette().primary.weak.text),
                    }),
                ],
            )
            .padding(12)
            .style(theme::Container::SuccessBox.style()),
        )
        .push(Space::new().height(16));
    }

    let device_name_input =
        text_input("Player Name (appears in audit logs)", &claim.device_name)
            .on_input(|s| auth::Message::UpdateClaimDeviceName(s).into())
            .id(ids::auth_first_run_device_name())
            .padding(12)
            .size(16)
            .style(theme::TextInput::style());

    claim_metadata = claim_metadata
        .push(text("Device Name").size(14))
        .push(Space::new().height(4))
        .push(device_name_input)
        .push(Space::new().height(16));

    let mut request_button = button(text("Request Binding Code").size(14))
        .padding([10, 16])
        .style(theme::Button::Secondary.style());

    if !claim.is_requesting {
        request_button =
            request_button.on_press(auth::Message::StartSetupClaim.into());
    }

    let mut confirm_button = button(text("Confirm Binding").size(14))
        .padding([10, 16])
        .style(theme::Button::Primary.style());

    let can_confirm = claim.claim_code.is_some()
        && !claim.is_confirming
        && !claim.is_expired();
    if can_confirm {
        confirm_button =
            confirm_button.on_press(auth::Message::ConfirmSetupClaim.into());
    } else {
        confirm_button = confirm_button.style(theme::Button::Disabled.style());
    }

    let reset_button = button(text("Reset Binding").size(14))
        .padding([10, 16])
        .style(theme::Button::Text.style())
        .on_press(auth::Message::ResetSetupClaim.into());

    let mut claim_status_row = column![request_button];

    if let Some(code) = &claim.claim_code {
        let expiry_text = claim.expires_at.map(|expires| {
            let local: DateTime<Local> = DateTime::from(expires);
            format!("Expires at {}", local.format("%Y-%m-%d %H:%M:%S %Z"))
        });

        let status_label = match claim.status {
            SetupClaimStatus::Pending => "Binding code active",
            SetupClaimStatus::Confirmed => "Binding confirmed",
            SetupClaimStatus::Expired => "Binding expired",
            SetupClaimStatus::Idle => "Binding idle",
        };

        let mut code_row = row![text(format!("Code: {}", code)).size(18),]
            .spacing(12)
            .align_y(Alignment::Center);

        if claim.lan_only {
            code_row = code_row.push(
                container(text("LAN-only").size(12).style(
                    |theme: &iced::Theme| text::Style {
                        color: Some(
                            theme.extended_palette().primary.strong.text,
                        ),
                    },
                ))
                .padding([4, 8])
                .style(theme::Container::TechDetail.style()),
            );
        }

        claim_status_row = claim_status_row
            .push(code_row)
            .push(Space::new().height(4))
            .push(text(status_label).size(14))
            .push(Space::new().height(4));

        if let Some(expiry) = expiry_text {
            claim_status_row = claim_status_row.push(text(expiry).size(14));
        }

        claim_status_row = claim_status_row
            .push(Space::new().height(12))
            .push(confirm_button)
            .push(Space::new().height(8))
            .push(reset_button);
    } else {
        claim_status_row = claim_status_row.push(Space::new().height(12));
    }

    if let Some(err) = &claim.last_error {
        claim_status_row = claim_status_row.push(Space::new().height(8)).push(
            container(text(err).size(14))
                .padding(12)
                .style(theme::Container::ErrorBox.style()),
        );
    }

    claim_metadata = claim_metadata.push(claim_status_row);

    let ready_to_submit = !loading
        && !username.trim().is_empty()
        && !display_name.trim().is_empty()
        && !password.as_str().is_empty()
        && password.as_str() == confirm_password.as_str()
        && !claim_token.trim().is_empty();

    let mut create_button = button(if loading {
        text("Creating Admin...")
    } else {
        text("Create Admin Account")
    })
    .padding([12, 24])
    .style(if ready_to_submit {
        theme::Button::Primary.style()
    } else {
        theme::Button::Disabled.style()
    });

    if ready_to_submit {
        create_button =
            create_button.on_press(auth::Message::SubmitSetup.into());
    }

    let mut content = column![
        title,
        Space::new().height(8),
        subtitle,
        Space::new().height(32),
        text("Username").size(14),
        Space::new().height(4),
        username_input,
        Space::new().height(16),
        text("Display Name").size(14),
        Space::new().height(4),
        display_name_input,
        Space::new().height(16),
        text("Password").size(14),
        Space::new().height(4),
        password_input,
        Space::new().height(16),
        text("Confirm Password").size(14),
        Space::new().height(4),
        confirm_password_input,
        Space::new().height(8),
        show_password_checkbox,
        Space::new().height(16),
        text("Setup Token").size(14),
        Space::new().height(4),
        setup_token_input,
        Space::new().height(16),
        text("Claim Token").size(14),
        Space::new().height(4),
        claim_token_display,
        Space::new().height(32),
        create_button,
    ]
    .width(Length::Fixed(400.0))
    .align_x(Alignment::Center);

    if let Some(err) = error {
        content = content.push(Space::new().height(16)).push(
            container(text(err).size(14).size(14))
                .padding(12)
                .style(theme::Container::ErrorBox.style()),
        );
    }

    content = content.push(Space::new().height(32)).push(claim_metadata);

    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(iced::alignment::Horizontal::Center)
        .align_y(iced::alignment::Vertical::Center)
        .into()
}
