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

use crate::common::messages::DomainMessage;
use crate::domains::auth::types::AuthenticationFlow;
use crate::state_refactored::State;
use iced::Element;

pub use components::*;
pub use credential_entry::view_credential_entry;
pub use loading_users::view_loading_users;
pub use pin_setup::view_pin_setup;
pub use user_carousel::{
    UserCarouselMessage, UserCarouselState, view_user_carousel, view_user_selection_with_carousel,
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
    user_permissions: Option<&'a ferrex_core::rbac::UserPermissions>,
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
            show_password,
            error,
            loading,
        } => view_first_run_setup(
            username,
            password,
            confirm_password,
            display_name,
            setup_token,
            *show_password,
            error.as_deref(),
            *loading,
        ),

        LoadingUsers => view_loading_users(),

        SelectingUser { users, error } => {
            view_user_selection_with_carousel(users, error.as_deref(), user_permissions)
        }

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
    username: &str,
    password: &crate::domains::auth::security::secure_credential::SecureCredential,
    confirm_password: &crate::domains::auth::security::secure_credential::SecureCredential,
    display_name: &str,
    setup_token: &str,
    show_password: bool,
    error: Option<&'a str>,
    loading: bool,
) -> Element<'a, DomainMessage> {
    use crate::domains::auth::messages as auth;
    use crate::domains::ui::theme;
    use iced::widget::{Space, button, checkbox, column, container, row, text, text_input};
    use iced::{Alignment, Length};

    let title = text("Welcome to Ferrex Media Server").size(32);

    let subtitle = text("Let's create your admin account").size(18);

    let username_input = text_input("Username", username)
        .on_input(|s| auth::Message::UpdateSetupField(auth::SetupField::Username(s)).into())
        .padding(12)
        .size(16)
        .style(theme::TextInput::style());

    let display_name_input = text_input("Display Name", display_name)
        .on_input(|s| auth::Message::UpdateSetupField(auth::SetupField::DisplayName(s)).into())
        .padding(12)
        .size(16)
        .style(theme::TextInput::style());

    let password_input = text_input("Password", password.as_str())
        .on_input(|s| auth::Message::UpdateSetupField(auth::SetupField::Password(s)).into())
        .secure(!show_password)
        .padding(12)
        .size(16)
        .style(theme::TextInput::style());

    let confirm_password_input = text_input("Confirm Password", confirm_password.as_str())
        .on_input(|s| auth::Message::UpdateSetupField(auth::SetupField::ConfirmPassword(s)).into())
        .secure(!show_password)
        .padding(12)
        .size(16)
        .style(theme::TextInput::style());

    let show_password_checkbox = checkbox("Show password", show_password)
        .on_toggle(|_| auth::Message::ToggleSetupPasswordVisibility.into())
        .size(16)
        .text_size(14);

    let setup_token_input = text_input("Setup Token (if required)", setup_token)
        .on_input(|s| auth::Message::UpdateSetupField(auth::SetupField::SetupToken(s)).into())
        .padding(12)
        .size(16)
        .style(theme::TextInput::style());

    let ready_to_submit = !loading
        && !username.trim().is_empty()
        && !display_name.trim().is_empty()
        && !password.as_str().is_empty()
        && password.as_str() == confirm_password.as_str();

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
        create_button = create_button.on_press(auth::Message::SubmitSetup.into());
    }

    let mut content = column![
        title,
        Space::with_height(8),
        subtitle,
        Space::with_height(32),
        text("Username").size(14),
        Space::with_height(4),
        username_input,
        Space::with_height(16),
        text("Display Name").size(14),
        Space::with_height(4),
        display_name_input,
        Space::with_height(16),
        text("Password").size(14),
        Space::with_height(4),
        password_input,
        Space::with_height(16),
        text("Confirm Password").size(14),
        Space::with_height(4),
        confirm_password_input,
        Space::with_height(8),
        show_password_checkbox,
        Space::with_height(16),
        text("Setup Token").size(14),
        Space::with_height(4),
        setup_token_input,
        Space::with_height(32),
        create_button,
    ]
    .width(Length::Fixed(400.0))
    .align_x(Alignment::Center);

    if let Some(err) = error {
        content = content.push(Space::with_height(16)).push(
            container(text(err).size(14).size(14))
                .padding(12)
                .style(theme::Container::ErrorBox.style()),
        );
    }

    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(iced::alignment::Horizontal::Center)
        .align_y(iced::alignment::Vertical::Center)
        .into()
}
