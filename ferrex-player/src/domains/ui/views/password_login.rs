use crate::{
    domains::auth::messages::Message,
    domains::ui::theme::{FerrexTheme, TextInput, button_style, container_style},
};
use iced::{
    Alignment, Element, Length, Theme,
    widget::{Space, button, column, container, row, text, text_input},
};

/// State for password login view
#[derive(Debug, Clone)]
pub struct PasswordLoginView {
    pub username: String,
    pub password: String,
    pub show_password: bool,
    pub remember_device: bool,
    pub error: Option<String>,
    pub loading: bool,
}

impl Default for PasswordLoginView {
    fn default() -> Self {
        Self {
            username: String::new(),
            password: String::new(),
            show_password: false,
            remember_device: true,
            error: None,
            loading: false,
        }
    }
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::all_functions
)]
impl PasswordLoginView {
    /// Create a new password login view
    pub fn new() -> Self {
        Self::default()
    }

    /// Create with pre-filled username
    pub fn with_username(username: String) -> Self {
        Self {
            username,
            ..Default::default()
        }
    }

    /// Set error message
    pub fn set_error(&mut self, error: String) {
        self.error = Some(error);
        self.loading = false;
    }

    /// Set loading state
    pub fn set_loading(&mut self, loading: bool) {
        self.loading = loading;
        if loading {
            self.error = None;
        }
    }

    /// View the password login screen
    pub fn view(self) -> Element<'static, Message> {
        let title = text("Sign In")
            .size(32)
            .color(FerrexTheme::HeaderText.text_color());

        let subtitle = text("Enter your password to continue")
            .size(16)
            .color(FerrexTheme::SubduedText.text_color());

        // Username input
        let username_input = text_input("Username", &self.username)
            .on_input(Message::PasswordLoginUpdateUsername)
            .padding(12)
            .size(16)
            .style(TextInput::style())
            .width(300);

        // Password input with visibility toggle
        let password_input = text_input("Password", &self.password)
            .on_input(Message::PasswordLoginUpdatePassword)
            .on_submit(Message::PasswordLoginSubmit)
            .secure(!self.show_password)
            .padding(12)
            .size(16)
            .style(TextInput::style())
            .width(300);

        let show_password_button =
            button(text(if self.show_password { "🙈" } else { "👁" }).size(20))
                .on_press(Message::PasswordLoginToggleVisibility)
                .style(|theme: &Theme, status| {
                    let base = button_style()(theme, status);
                    button::Style {
                        background: None,
                        text_color: FerrexTheme::SubduedText.text_color(),
                        ..base
                    }
                })
                .padding(8);

        let password_row = row![password_input, show_password_button,]
            .spacing(8)
            .align_y(Alignment::Center);

        // Remember device checkbox
        let remember_device = row![
            // Custom checkbox using button
            button(text(if self.remember_device { "☑" } else { "☐" }).size(20))
                .on_press(Message::PasswordLoginToggleRemember)
                .style(|theme: &Theme, status| {
                    let base = button_style()(theme, status);
                    button::Style {
                        background: None,
                        text_color: FerrexTheme::Text.text_color(),
                        border: iced::Border {
                            width: 0.0,
                            ..base.border
                        },
                        ..base
                    }
                })
                .padding(0),
            text("Remember this device for 30 days")
                .size(14)
                .color(FerrexTheme::SubduedText.text_color()),
        ]
        .spacing(8)
        .align_y(Alignment::Center);

        // Error message
        let error_message = if let Some(error) = self.error {
            container(
                text(error)
                    .size(14)
                    .color(FerrexTheme::ErrorText.text_color()),
            )
            .padding(10)
            .style(|_theme: &Theme| container::Style {
                background: Some(iced::Background::Color(
                    FerrexTheme::ErrorText.text_color().scale_alpha(0.1),
                )),
                border: iced::Border {
                    color: FerrexTheme::ErrorText.text_color().scale_alpha(0.3),
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            })
            .width(350)
        } else {
            container(Space::new().height(0))
        };

        // Submit button
        let submit_button = if self.loading {
            button(text("Signing in...").size(16))
                .style(|_theme: &Theme, _status| button::Style {
                    background: Some(iced::Background::Color(
                        FerrexTheme::card_background().scale_alpha(0.5),
                    )),
                    text_color: FerrexTheme::SubduedText.text_color(),
                    border: iced::Border {
                        color: FerrexTheme::border_color().scale_alpha(0.5),
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                })
                .padding([12, 24])
                .width(Length::Fill)
        } else {
            button(text("Sign In").size(16))
                .on_press(Message::PasswordLoginSubmit)
                .style(button_style())
                .padding([12, 24])
                .width(Length::Fill)
        };

        // Back button
        let back_button = button(text("← Back").size(14))
            .on_press(Message::BackToUserSelection)
            .style(|theme: &Theme, status| {
                let base = button_style()(theme, status);
                button::Style {
                    background: None,
                    text_color: FerrexTheme::SubduedText.text_color(),
                    border: iced::Border {
                        width: 0.0,
                        ..base.border
                    },
                    ..base
                }
            })
            .padding([8, 16]);

        // Main content
        let content = column![
            Space::new().height(60),
            title,
            subtitle,
            Space::new().height(40),
            username_input,
            Space::new().height(16),
            password_row,
            Space::new().height(12),
            remember_device,
            Space::new().height(20),
            error_message,
            Space::new().height(8),
            submit_button,
            Space::new().height(40),
            back_button,
        ]
        .align_x(Alignment::Center)
        .width(350)
        .padding(20);

        // Center the content
        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(iced::alignment::Horizontal::Center)
            .align_y(iced::alignment::Vertical::Center)
            .style(container_style())
            .into()
    }
}

/// Helper function to create password login view
pub fn view_password_login(
    username: String,
    password: String,
    show_password: bool,
    remember_device: bool,
    error: Option<String>,
    loading: bool,
) -> Element<'static, Message> {
    let view = PasswordLoginView {
        username,
        password,
        show_password,
        remember_device,
        error,
        loading,
    };
    view.view()
}

/// Messages specific to password login
#[derive(Debug, Clone)]
pub enum PasswordLoginMessage {
    UpdateUsername(String),
    UpdatePassword(String),
    ToggleVisibility,
    ToggleRemember,
    Submit,
    LoginSuccess,
    LoginError(String),
}
