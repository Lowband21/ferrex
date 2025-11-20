//! User selection carousel view

use super::components::{auth_card, auth_container, error_message, spacing, title};
use crate::{messages::{auth, DomainMessage}};
use crate::views::carousel::CarouselState;
use ferrex_core::user::User;
use iced::{
    widget::{button, column, container, row, scrollable, text, Space},
    Alignment, Element, Length, Theme,
};
use iced::widget::scrollable::Id as ScrollableId;
use lucide_icons::Icon;

/// State for user carousel
#[derive(Debug, Clone)]
pub struct UserCarouselState {
    pub carousel_state: CarouselState,
    pub users: Vec<User>,
    pub selected_index: Option<usize>,
    pub error: Option<String>,
}

impl Default for UserCarouselState {
    fn default() -> Self {
        Self {
            carousel_state: CarouselState::new_with_dimensions(0, 120.0, 20.0), // Avatar-sized items
            users: Vec::new(),
            selected_index: None,
            error: None,
        }
    }
}

impl UserCarouselState {
    /// Create new state with users
    pub fn new(users: Vec<User>) -> Self {
        let mut carousel_state = CarouselState::new_with_dimensions(users.len(), 120.0, 20.0);
        carousel_state.items_per_page = 5; // Show 5 users at a time
        
        Self {
            carousel_state,
            users,
            selected_index: None,
            error: None,
        }
    }

    /// Update users and refresh carousel state
    pub fn set_users(&mut self, users: Vec<User>) {
        self.users = users;
        self.carousel_state.set_total_items(self.users.len());
        self.selected_index = None;
    }

    /// Set selected user index
    pub fn set_selected(&mut self, index: usize) {
        if index < self.users.len() {
            self.selected_index = Some(index);
        }
    }

    /// Set error state
    pub fn set_error(&mut self, error: Option<String>) {
        self.error = error;
    }

    /// Get selected user
    pub fn selected_user(&self) -> Option<&User> {
        self.selected_index.and_then(|i| self.users.get(i))
    }
}

/// Messages for user carousel
#[derive(Debug, Clone)]
pub enum UserCarouselMessage {
    Previous,
    Next,
    SelectUser(usize),
    Scrolled(scrollable::Viewport),
}

/// Shows the user selection carousel
pub fn view_user_carousel<'a>(
    state: &'a UserCarouselState,
    user_permissions: Option<&'a ferrex_core::rbac::UserPermissions>,
) -> Element<'a, DomainMessage> {
    let mut content = column![
        title("Select User"),
        spacing(),
    ];

    // Show error if present
    if let Some(error) = &state.error {
        content = content.push(error_message(error));
        content = content.push(spacing());
    }

    // User carousel
    if state.users.is_empty() {
        content = content.push(
            container(
                text("No users found")
                    .size(16)
                    .style(|theme: &Theme| text::Style {
                        color: Some(theme.extended_palette().background.strong.text),
                    }),
            )
            .width(Length::Fill)
            .padding(40)
            .align_x(iced::alignment::Horizontal::Center),
        );
    } else {
        let carousel = create_user_carousel(state, user_permissions);
        content = content.push(carousel);
    }

    let card = auth_card(content);
    auth_container(card).into()
}

/// Create the user carousel component
fn create_user_carousel<'a>(state: &'a UserCarouselState, user_permissions: Option<&'a ferrex_core::rbac::UserPermissions>) -> Element<'a, DomainMessage> {
    let carousel_state = &state.carousel_state;
    
    // Create navigation buttons
    let left_button = if carousel_state.can_go_left() {
        button(
            text(icon_char(Icon::ChevronLeft))
                .font(lucide_font())
                .size(20)
        )
        .on_press(DomainMessage::Auth(auth::Message::SelectUser(state.users[0].id))) // TODO: Proper navigation
        .padding(8)
        .style(button_style)
    } else {
        button(
            text(icon_char(Icon::ChevronLeft))
                .font(lucide_font())
                .size(20)
        )
        .padding(8)
        .style(button_style_disabled)
    };

    let right_button = if carousel_state.can_go_right() {
        button(
            text(icon_char(Icon::ChevronRight))
                .font(lucide_font())
                .size(20)
        )
        .on_press(DomainMessage::Auth(auth::Message::SelectUser(state.users[0].id))) // TODO: Proper navigation
        .padding(8)
        .style(button_style)
    } else {
        button(
            text(icon_char(Icon::ChevronRight))
                .font(lucide_font())
                .size(20)
        )
        .padding(8)
        .style(button_style_disabled)
    };

    // Create user items row
    let mut user_row = row![].spacing(carousel_state.item_spacing as f32);
    
    // Get visible range for virtualization
    let visible_range = carousel_state.get_visible_range();
    
    // Add visible user items
    for (index, user) in state.users.iter().enumerate() {
        if visible_range.contains(&index) {
            let is_selected = state.selected_index == Some(index);
            user_row = user_row.push(create_user_avatar(user, index, is_selected));
        }
    }
    
    // Add "Add User" button for admins
    if let Some(permissions) = user_permissions {
        if permissions.has_role("admin") || permissions.has_permission("users:create") {
            user_row = user_row.push(create_add_user_button());
        }
    }

    // Create scrollable carousel
    let carousel_content = scrollable(
        container(user_row)
            .padding([20, 40])
            .width(Length::Fill)
    )
    .id(carousel_state.scrollable_id.clone())
    .direction(scrollable::Direction::Horizontal(
        scrollable::Scrollbar::new()
            .width(0) // Hide scrollbar
            .scroller_width(0),
    ))
    .width(Length::Fill)
    .height(Length::Fixed(200.0));

    // Build complete carousel layout
    column![
        // Navigation row
        container(
            row![
                Space::with_width(Length::Fill),
                left_button,
                Space::with_width(10),
                right_button,
                Space::with_width(Length::Fill),
            ]
            .align_y(Alignment::Center)
        )
        .padding([10, 0]),
        
        // Carousel content
        carousel_content,
        
        spacing(),
    ]
    .width(Length::Fill)
    .into()
}

/// Create a user avatar item for the carousel
fn create_user_avatar<'a>(
    user: &'a User, 
    index: usize, 
    is_selected: bool
) -> Element<'a, DomainMessage> {
    let avatar_content = column![
        // Avatar circle
        container(
            text(user.display_name.chars().next().unwrap_or('U'))
                .size(32)
        )
        .width(Length::Fixed(80.0))
        .height(Length::Fixed(80.0))
        .align_x(iced::alignment::Horizontal::Center)
        .align_y(iced::alignment::Vertical::Center)
        .style(move |theme: &Theme| {
            let palette = theme.extended_palette();
            container::Style {
                background: Some(if is_selected {
                    palette.primary.base.color.into()
                } else {
                    palette.primary.weak.color.into()
                }),
                border: iced::Border {
                    radius: 40.0.into(),
                    width: if is_selected { 3.0 } else { 0.0 },
                    color: palette.primary.strong.color,
                },
                ..Default::default()
            }
        }),
        
        Space::with_height(8),
        
        // User name
        text(&user.display_name)
            .size(14)
            .align_x(iced::alignment::Horizontal::Center)
            .style(move |theme: &Theme| text::Style {
                color: Some(if is_selected {
                    theme.extended_palette().primary.base.text
                } else {
                    theme.extended_palette().background.base.text
                }),
            }),
    ]
    .align_x(Alignment::Center)
    .width(Length::Fixed(120.0));

    button(avatar_content)
        .on_press(DomainMessage::Auth(auth::Message::SelectUser(user.id)))
        .style(|_theme: &Theme, _status| button::Style {
            background: None,
            ..Default::default()
        })
        .into()
}

// Helper functions
fn lucide_font() -> iced::Font {
    iced::Font::with_name("lucide")
}

fn icon_char(icon: Icon) -> String {
    icon.unicode().to_string()
}

fn button_style(theme: &Theme, status: button::Status) -> button::Style {
    let palette = theme.extended_palette();
    match status {
        button::Status::Active | button::Status::Pressed => button::Style {
            background: Some(palette.background.weak.color.into()),
            text_color: palette.background.base.text,
            border: iced::Border {
                radius: 6.0.into(),
                ..Default::default()
            },
            ..Default::default()
        },
        button::Status::Hovered => button::Style {
            background: Some(palette.background.strong.color.into()),
            text_color: palette.background.base.text,
            border: iced::Border {
                radius: 6.0.into(),
                ..Default::default()
            },
            ..Default::default()
        },
        button::Status::Disabled => button::Style {
            background: Some(palette.background.weak.color.into()),
            text_color: palette.background.strong.text,
            border: iced::Border {
                radius: 6.0.into(),
                ..Default::default()
            },
            ..Default::default()
        },
    }
}

fn button_style_disabled(theme: &Theme, _status: button::Status) -> button::Style {
    let palette = theme.extended_palette();
    button::Style {
        background: Some(palette.background.weak.color.into()),
        text_color: palette.background.strong.text,
        border: iced::Border {
            radius: 6.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// Create an "Add User" button for admin users
fn create_add_user_button<'a>() -> Element<'a, DomainMessage> {
    let add_user_content = column![
        // Add icon circle
        container(
            text("+")
                .size(48)
                .style(|theme: &Theme| text::Style {
                    color: Some(theme.extended_palette().primary.base.text),
                })
        )
        .width(Length::Fixed(80.0))
        .height(Length::Fixed(80.0))
        .align_x(iced::alignment::Horizontal::Center)
        .align_y(iced::alignment::Vertical::Center)
        .style(|theme: &Theme| {
            let palette = theme.extended_palette();
            container::Style {
                background: Some(palette.background.strong.color.into()),
                border: iced::Border {
                    radius: 40.0.into(),
                    width: 2.0,
                    color: palette.primary.base.color,
                },
                ..Default::default()
            }
        }),
        
        Space::with_height(8),
        
        // "Add User" text
        text("Add User")
            .size(14)
            .align_x(iced::alignment::Horizontal::Center)
            .style(|theme: &Theme| text::Style {
                color: Some(theme.extended_palette().background.base.text),
            }),
    ]
    .align_x(Alignment::Center)
    .width(Length::Fixed(120.0));

    button(add_user_content)
        .on_press(DomainMessage::Auth(auth::Message::ShowCreateUser))
        .style(|theme: &Theme, status| {
            let palette = theme.extended_palette();
            match status {
                button::Status::Active => button::Style {
                    background: None,
                    ..Default::default()
                },
                button::Status::Hovered => button::Style {
                    background: Some(palette.background.weak.color.into()),
                    border: iced::Border {
                        radius: 8.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                button::Status::Pressed => button::Style {
                    background: Some(palette.primary.weak.color.into()),
                    border: iced::Border {
                        radius: 8.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                button::Status::Disabled => button::Style {
                    background: None,
                    ..Default::default()
                },
            }
        })
        .into()
}