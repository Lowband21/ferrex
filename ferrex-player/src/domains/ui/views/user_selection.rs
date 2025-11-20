use crate::{
    domains::auth::messages::Message,
    domains::ui::theme::{FerrexTheme, button_style, container_style},
};
use ferrex_core::user::User;
use iced::{
    Alignment, Element, Length, Theme,
    widget::{Space, button, column, container, row, scrollable, text},
};

/// State for user selection view
#[derive(Debug)]
pub struct UserSelectionView {
    users: Vec<User>,
    loading: bool,
    error: Option<String>,
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::all_functions
)]
impl Default for UserSelectionView {
    fn default() -> Self {
        Self::new()
    }
}

impl UserSelectionView {
    /// Create a new user selection view
    pub fn new() -> Self {
        Self {
            users: Vec::new(),
            loading: true,
            error: None,
        }
    }

    /// Update the list of users
    pub fn set_users(&mut self, users: Vec<User>) {
        self.users = users;
        self.loading = false;
        self.error = None;
    }

    /// Set error state
    pub fn set_error(&mut self, error: String) {
        self.error = Some(error);
        self.loading = false;
    }

    /// Set loading state
    pub fn set_loading(&mut self, loading: bool) {
        self.loading = loading;
    }

    /// View the user selection screen
    pub fn view(self) -> Element<'static, Message> {
        let content = if self.loading {
            // Loading state
            container(
                column![text("Loading users...").size(24),]
                    .align_x(Alignment::Center)
                    .spacing(20),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(iced::alignment::Horizontal::Center)
            .align_y(iced::alignment::Vertical::Center)
        } else if let Some(error) = self.error.clone() {
            // Error state
            container(
                column![
                    text("Error").size(32),
                    text(error).size(16),
                    button("Retry")
                        .on_press(Message::LoadUsers)
                        .style(button_style())
                        .padding([10, 20]),
                ]
                .align_x(Alignment::Center)
                .spacing(20),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(iced::alignment::Horizontal::Center)
            .align_y(iced::alignment::Vertical::Center)
        } else {
            // User grid
            let title = text("Who's watching?")
                .size(48)
                .color(FerrexTheme::HeaderText.text_color());

            let user_grid = if self.users.is_empty() {
                // No users - show create user option
                container(
                    column![
                        text("No users found").size(24),
                        Space::with_height(20),
                        button("Create New User")
                            .on_press(Message::ShowCreateUser)
                            .style(button_style())
                            .padding([10, 20]),
                    ]
                    .align_x(Alignment::Center)
                    .spacing(20),
                )
                .width(Length::Fill)
            } else {
                // Show user profiles in a grid
                let total_users = self.users.len() + 1; // +1 for "Add Profile" button
                let users_per_row = 5;
                let num_rows = total_users.div_ceil(users_per_row);

                let mut rows = Vec::new();

                for row_idx in 0..num_rows {
                    let start_idx = row_idx * users_per_row;
                    let end_idx = ((row_idx + 1) * users_per_row).min(self.users.len());

                    let mut row_elements = Vec::new();

                    // Add user cards for this row
                    for user_idx in start_idx..end_idx {
                        if let Some(user) = self.users.get(user_idx) {
                            row_elements.push(Self::create_user_card(user));
                        }
                    }

                    // Add the "Add Profile" button in the last row if needed
                    if row_idx == num_rows - 1 && total_users > self.users.len() {
                        row_elements.push(Self::create_add_profile_card());
                    }

                    let row = row(row_elements).spacing(30).align_y(Alignment::Center);
                    rows.push(row.into());
                }

                container(scrollable(
                    column(rows).spacing(30).align_x(Alignment::Center),
                ))
                .width(Length::Fill)
            };

            container(
                column![
                    Space::with_height(60),
                    title,
                    Space::with_height(60),
                    user_grid,
                ]
                .align_x(Alignment::Center)
                .width(Length::Fill),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .style(container_style())
        };

        content.into()
    }

    /// Create a user profile card
    fn create_user_card(user: &User) -> Element<'static, Message> {
        let user_id = user.id;
        let display_name = user.display_name.clone();
        let initial = display_name
            .chars()
            .next()
            .unwrap_or('?')
            .to_uppercase()
            .to_string();

        let profile_button = button(
            column![
                // Profile image placeholder
                container(text(initial).size(48),)
                    .width(150)
                    .height(150)
                    .style(|_theme: &Theme| container::Style {
                        background: Some(iced::Background::Color(FerrexTheme::card_background())),
                        border: iced::Border {
                            color: FerrexTheme::border_color(),
                            width: 2.0,
                            radius: 8.0.into(),
                        },
                        ..Default::default()
                    })
                    .align_x(iced::alignment::Horizontal::Center)
                    .align_y(iced::alignment::Vertical::Center),
                Space::with_height(10),
                text(display_name)
                    .size(18)
                    .color(FerrexTheme::Text.text_color()),
            ]
            .align_x(Alignment::Center),
        )
        .on_press(Message::SelectUser(user_id))
        .style(|theme: &Theme, status| {
            let base = button_style()(theme, status);
            button::Style {
                background: None,
                border: iced::Border {
                    width: 0.0,
                    ..base.border
                },
                ..base
            }
        })
        .padding(10);

        profile_button.into()
    }

    /// Create "Add Profile" card
    fn create_add_profile_card() -> Element<'static, Message> {
        let add_button = button(
            column![
                // Add icon placeholder
                container(text("+").size(72),)
                    .width(150)
                    .height(150)
                    .style(|_theme: &Theme| container::Style {
                        background: Some(iced::Background::Color(
                            FerrexTheme::card_background().scale_alpha(0.5)
                        )),
                        border: iced::Border {
                            color: FerrexTheme::border_color().scale_alpha(0.5),
                            width: 2.0,
                            radius: 8.0.into(),
                        },
                        ..Default::default()
                    })
                    .align_x(iced::alignment::Horizontal::Center)
                    .align_y(iced::alignment::Vertical::Center),
                Space::with_height(10),
                text("Add Profile")
                    .size(18)
                    .color(FerrexTheme::SubduedText.text_color()),
            ]
            .align_x(Alignment::Center),
        )
        .on_press(Message::ShowCreateUser)
        .style(|theme: &Theme, status| {
            let base = button_style()(theme, status);
            button::Style {
                background: None,
                border: iced::Border {
                    width: 0.0,
                    ..base.border
                },
                ..base
            }
        })
        .padding(10);

        add_button.into()
    }
}

/// Render user selection view
pub fn view_user_selection(
    users: Vec<User>,
    loading: bool,
    error: Option<String>,
) -> Element<'static, Message> {
    let view = UserSelectionView {
        users,
        loading,
        error,
    };
    view.view()
}

/// Messages specific to user selection
#[derive(Debug, Clone)]
pub enum UserSelectionMessage {
    UsersLoaded(Vec<User>),
    LoadError(String),
    SelectUser(uuid::Uuid),
    ShowCreateUser,
}
