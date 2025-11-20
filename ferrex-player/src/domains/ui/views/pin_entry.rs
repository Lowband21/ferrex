use crate::{
    domains::auth::messages::Message,
    domains::ui::theme::{FerrexTheme, button_style, container_style},
};
use iced::{
    Alignment, Element, Length, Theme,
    widget::{Space, button, column, container, row, text},
};

/// State for PIN entry view
#[derive(Debug)]
pub struct PinEntryView {
    pub username: String,
    pub display_name: String,
    pin: String,
    error: Option<String>,
    loading: bool,
    max_pin_length: usize,
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::all_functions
)]
impl PinEntryView {
    /// Create a new PIN entry view
    pub fn new(username: String, display_name: String) -> Self {
        Self {
            username,
            display_name,
            pin: String::new(),
            error: None,
            loading: false,
            max_pin_length: 4,
        }
    }

    /// Add a digit to the PIN
    pub fn add_digit(&mut self, digit: char) {
        if self.pin.len() < self.max_pin_length && digit.is_numeric() {
            self.pin.push(digit);
            self.error = None;

            // Auto-submit when PIN is complete
            if self.pin.len() == self.max_pin_length {
                // This will be handled by the parent component
            }
        }
    }

    /// Remove the last digit from the PIN
    pub fn remove_digit(&mut self) {
        self.pin.pop();
        self.error = None;
    }

    /// Clear the PIN
    pub fn clear(&mut self) {
        self.pin.clear();
        self.error = None;
    }

    /// Set error message
    pub fn set_error(&mut self, error: String) {
        self.error = Some(error);
        self.loading = false;
    }

    /// Set loading state
    pub fn set_loading(&mut self, loading: bool) {
        self.loading = loading;
    }

    /// Get the current PIN
    pub fn get_pin(&self) -> &str {
        &self.pin
    }

    /// Check if PIN is complete
    pub fn is_complete(&self) -> bool {
        self.pin.len() == self.max_pin_length
    }

    /// View the PIN entry screen
    pub fn view(self) -> Element<'static, Message> {
        let title = text(format!("Welcome, {}", self.display_name))
            .size(32)
            .color(FerrexTheme::HeaderText.text_color());

        let subtitle = text("Enter your PIN")
            .size(20)
            .color(FerrexTheme::SubduedText.text_color());

        // PIN display
        let mut pin_dots = Vec::new();
        for i in 0..self.max_pin_length {
            let dot = if i < self.pin.len() {
                container(text("●").size(24))
            } else {
                container(text("○").size(24))
            };

            let styled_dot = dot
                .width(40)
                .height(40)
                .style(|_theme: &Theme| container::Style {
                    background: Some(iced::Background::Color(FerrexTheme::card_background())),
                    border: iced::Border {
                        color: FerrexTheme::border_color(),
                        width: 1.0,
                        radius: 20.0.into(),
                    },
                    ..Default::default()
                })
                .align_x(iced::alignment::Horizontal::Center)
                .align_y(iced::alignment::Vertical::Center);

            pin_dots.push(styled_dot.into());
        }

        let pin_display = row(pin_dots).spacing(10).align_y(Alignment::Center);

        // Error message
        let error_message = if let Some(error) = self.error.clone() {
            container(
                text(error)
                    .size(16)
                    .color(FerrexTheme::ErrorText.text_color()),
            )
            .padding(10)
        } else {
            container(Space::with_height(26))
        };

        // Number pad
        let number_pad = Self::create_number_pad(self.is_complete(), self.loading);

        // Back button
        let back_button = button("← Back")
            .on_press(Message::BackToUserSelection)
            .style(button_style())
            .padding([10, 20]);

        let content = column![
            Space::with_height(60),
            title,
            subtitle,
            Space::with_height(40),
            pin_display,
            error_message,
            Space::with_height(20),
            number_pad,
            Space::with_height(40),
            back_button,
        ]
        .align_x(Alignment::Center)
        .spacing(20);

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(iced::alignment::Horizontal::Center)
            .align_y(iced::alignment::Vertical::Center)
            .style(container_style())
            .into()
    }

    /// Create the number pad
    fn create_number_pad(is_complete: bool, loading: bool) -> Element<'static, Message> {
        let button_size = 80u32;
        let button_spacing = 10;

        // Create number buttons
        let create_num_button = move |num: char| {
            let size = button_size;
            button(text(num).size(32))
                .on_press(Message::PinDigitPressed(num))
                .width(size)
                .height(size)
                .style(move |theme: &Theme, status| {
                    let base = button_style()(theme, status);
                    button::Style {
                        border: iced::Border {
                            radius: (size as f32 / 2.0).into(),
                            ..base.border
                        },
                        ..base
                    }
                })
        };

        // Rows of numbers
        let row1 = row![
            create_num_button('1'),
            create_num_button('2'),
            create_num_button('3'),
        ]
        .spacing(button_spacing);

        let row2 = row![
            create_num_button('4'),
            create_num_button('5'),
            create_num_button('6'),
        ]
        .spacing(button_spacing);

        let row3 = row![
            create_num_button('7'),
            create_num_button('8'),
            create_num_button('9'),
        ]
        .spacing(button_spacing);

        // Bottom row with clear, 0, and backspace
        let clear_button = button(text("Clear").size(20))
            .on_press(Message::PinClear)
            .width(button_size)
            .height(button_size)
            .style(move |theme: &Theme, status| {
                let base = button_style()(theme, status);
                button::Style {
                    border: iced::Border {
                        radius: (button_size as f32 / 2.0).into(),
                        ..base.border
                    },
                    ..base
                }
            });

        let backspace_button = button(text("⌫").size(24))
            .on_press(Message::PinBackspace)
            .width(button_size)
            .height(button_size)
            .style(move |theme: &Theme, status| {
                let base = button_style()(theme, status);
                button::Style {
                    border: iced::Border {
                        radius: (button_size as f32 / 2.0).into(),
                        ..base.border
                    },
                    ..base
                }
            });

        let row4 =
            row![clear_button, create_num_button('0'), backspace_button,].spacing(button_spacing);

        // Submit button (only enabled when PIN is complete)
        let submit_button = if is_complete && !loading {
            button(if loading {
                text("Logging in...").size(20)
            } else {
                text("Submit").size(20)
            })
            .on_press(Message::PinSubmit)
            .width(button_size * 3 + button_spacing * 2)
            .style(button_style())
            .padding(15)
        } else {
            button(text("Submit").size(20))
                .width(button_size * 3 + button_spacing * 2)
                .style(|_theme: &Theme, _status| button::Style {
                    background: Some(iced::Background::Color(
                        FerrexTheme::card_background().scale_alpha(0.5),
                    )),
                    text_color: FerrexTheme::SubduedText.text_color().scale_alpha(0.5),
                    border: iced::Border {
                        color: FerrexTheme::border_color().scale_alpha(0.5),
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                })
                .padding(15)
        };

        column![
            row1,
            row2,
            row3,
            row4,
            Space::with_height(20),
            submit_button,
        ]
        .spacing(button_spacing)
        .align_x(Alignment::Center)
        .into()
    }
}

/// Render PIN entry view
pub fn view_pin_entry(
    username: String,
    display_name: String,
    pin: String,
    error: Option<String>,
    loading: bool,
) -> Element<'static, Message> {
    let mut view = PinEntryView {
        username,
        display_name,
        pin: String::new(),
        error,
        loading,
        max_pin_length: 6,
    };

    // Restore PIN state
    for ch in pin.chars() {
        if view.pin.len() < view.max_pin_length && ch.is_numeric() {
            view.pin.push(ch);
        }
    }

    view.view()
}

/// Messages specific to PIN entry
#[derive(Debug, Clone)]
pub enum PinEntryMessage {
    DigitPressed(char),
    Backspace,
    Clear,
    Submit,
    LoginSuccess,
    LoginError(String),
}
