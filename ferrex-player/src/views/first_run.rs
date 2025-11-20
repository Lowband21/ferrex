//! First-run setup view for creating the initial admin user
//!
//! This view is shown when the server has no admin user configured,
//! allowing the user to create the first administrator account.

use crate::{messages::auth::Message, theme, State};
use iced::{
    widget::{button, column, container, row, text, text_input, Space},
    Alignment, Element, Length,
};

/// State for the first-run setup view
#[derive(Debug, Clone, Default)]
pub struct FirstRunState {
    pub username: String,
    pub display_name: String,
    pub password: String,
    pub confirm_password: String,
    pub show_password: bool,
    pub loading: bool,
    pub error: Option<String>,
}

/// Render the first-run setup view
pub fn view_first_run<'a>(
    state: &'a State,
    setup_state: &'a FirstRunState,
) -> Element<'a, Message> {
    let title = text("Welcome to Ferrex Media Server")
        .size(32)
        .color(theme::MediaServerTheme::TEXT_PRIMARY);

    let subtitle = text("Let's create your administrator account")
        .size(18)
        .color(theme::MediaServerTheme::TEXT_SECONDARY);

    // Username input
    let username_input = container(
        column![
            text("Username")
                .size(14)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
            text_input("Choose a username", &setup_state.username)
                .on_input(Message::FirstRunUpdateUsername)
                .padding(12)
                .size(16)
                .style(theme::TextInput::style())
                .width(Length::Fixed(400.0)),
        ]
        .spacing(8),
    );

    // Display name input
    let display_name_input = container(
        column![
            text("Display Name")
                .size(14)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
            text_input("Your name", &setup_state.display_name)
                .on_input(Message::FirstRunUpdateDisplayName)
                .padding(12)
                .size(16)
                .style(theme::TextInput::style())
                .width(Length::Fixed(400.0)),
        ]
        .spacing(8),
    );

    // Password input
    let password_input = container(
        column![
            text("Password")
                .size(14)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
            text_input("Create a strong password", &setup_state.password)
                .on_input(Message::FirstRunUpdatePassword)
                .secure(true)
                .padding(12)
                .size(16)
                .style(theme::TextInput::style())
                .width(Length::Fixed(400.0)),
            text("Must be at least 8 characters with uppercase, lowercase, and numbers")
                .size(12)
                .color(theme::MediaServerTheme::TEXT_SUBDUED),
        ]
        .spacing(8),
    );

    // Confirm password input
    let confirm_password_input = container(
        column![
            text("Confirm Password")
                .size(14)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
            text_input("Confirm your password", &setup_state.confirm_password)
                .on_input(Message::FirstRunUpdateConfirmPassword)
                .secure(true)
                .padding(12)
                .size(16)
                .style(theme::TextInput::style())
                .width(Length::Fixed(400.0)),
        ]
        .spacing(8),
    );

    // Password strength indicator
    let password_strength: Element<'_, Message> = if setup_state.password.is_empty() {
        Space::with_height(20).into()
    } else {
        let strength = calculate_password_strength(&setup_state.password);
        let (color, strength_text) = match strength {
            0..=25 => (theme::MediaServerTheme::ERROR, "Weak"),
            26..=50 => (theme::MediaServerTheme::WARNING, "Fair"),
            51..=75 => (theme::MediaServerTheme::INFO, "Good"),
            _ => (theme::MediaServerTheme::SUCCESS, "Strong"),
        };

        container(
            row![
                container(Space::with_width(1))
                    .width(Length::FillPortion(strength as u16))
                    .height(4)
                    .style(move |_theme| container::Style {
                        background: Some(color.into()),
                        ..Default::default()
                    }),
                container(Space::with_width(1))
                    .width(Length::FillPortion((100 - strength) as u16))
                    .height(4)
                    .style(|_theme: &iced::Theme| container::Style {
                        background: Some(theme::MediaServerTheme::SURFACE_DIM.into()),
                        ..Default::default()
                    }),
                Space::with_width(10),
                text(strength_text).size(14).color(color),
            ]
            .align_y(Alignment::Center),
        )
        .width(Length::Fixed(400.0))
        .into()
    };

    // Error message
    let error_message: Element<'_, Message> = if let Some(error) = &setup_state.error {
        container(text(error).size(14).color(theme::MediaServerTheme::ERROR))
            .padding([10, 20])
            .width(Length::Fixed(400.0))
            .style(|_theme: &iced::Theme| container::Style {
                background: Some(theme::MediaServerTheme::ERROR.scale_alpha(0.1).into()),
                border: iced::Border {
                    color: theme::MediaServerTheme::ERROR,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            })
            .into()
    } else {
        Space::with_height(0).into()
    };

    // Submit button
    let submit_button = button(
        text(if setup_state.loading {
            "Creating..."
        } else {
            "Create Admin Account"
        })
        .size(16),
    )
    .on_press_maybe(if can_submit(setup_state) && !setup_state.loading {
        Some(Message::FirstRunSubmit)
    } else {
        None
    })
    .padding([12, 24])
    .style(if can_submit(setup_state) && !setup_state.loading {
        theme::Button::Primary.style()
    } else {
        theme::Button::Disabled.style()
    })
    .width(Length::Fixed(400.0));

    // Main content
    let content = column![
        title,
        Space::with_height(10),
        subtitle,
        Space::with_height(40),
        username_input,
        Space::with_height(20),
        display_name_input,
        Space::with_height(20),
        password_input,
        Space::with_height(10),
        password_strength,
        Space::with_height(20),
        confirm_password_input,
        Space::with_height(20),
        error_message,
        Space::with_height(20),
        submit_button,
    ]
    .align_x(Alignment::Center);

    // Center everything on screen
    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .style(|_theme: &iced::Theme| container::Style {
            background: Some(theme::MediaServerTheme::BACKGROUND.into()),
            ..Default::default()
        })
        .into()
}

/// Calculate password strength (0-100)
fn calculate_password_strength(password: &str) -> u32 {
    let mut strength = 0;

    // Length bonus
    strength += (password.len() as u32).min(20) * 2;

    // Character variety bonus
    let has_lower = password.chars().any(|c| c.is_lowercase());
    let has_upper = password.chars().any(|c| c.is_uppercase());
    let has_digit = password.chars().any(|c| c.is_numeric());
    let has_special = password.chars().any(|c| !c.is_alphanumeric());

    if has_lower {
        strength += 15;
    }
    if has_upper {
        strength += 15;
    }
    if has_digit {
        strength += 15;
    }
    if has_special {
        strength += 15;
    }

    strength.min(100)
}

/// Check if the form can be submitted
fn can_submit(state: &FirstRunState) -> bool {
    !state.username.is_empty()
        && !state.display_name.is_empty()
        && state.password.len() >= 8
        && state.password == state.confirm_password
        && state.password.chars().any(|c| c.is_uppercase())
        && state.password.chars().any(|c| c.is_lowercase())
        && state.password.chars().any(|c| c.is_numeric())
}
