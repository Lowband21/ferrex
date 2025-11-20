//! User selection view

use super::components::{auth_card, auth_container, error_message, spacing, title};
use crate::common::messages::DomainMessage;
use crate::domains::auth::dto::UserListItemDto;
use crate::domains::auth::messages as auth;
use iced::{
    Alignment, Element, Length, Theme,
    widget::{Space, button, column, container, row, scrollable, text},
};

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn view_user_selection<'a>(
    users: &'a [UserListItemDto],
    error: Option<&'a str>,
    user_permissions: Option<&'a ferrex_core::rbac::UserPermissions>,
) -> Element<'a, DomainMessage> {
    view_user_selection_with_admin_state(users, error, false, user_permissions)
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn view_user_selection_with_admin_state<'a>(
    users: &'a [UserListItemDto],
    error: Option<&'a str>,
    admin_pin_unlock_enabled: bool,
    user_permissions: Option<&'a ferrex_core::rbac::UserPermissions>,
) -> Element<'a, DomainMessage> {
    let mut content = column![
        title("Select User"),
        spacing(),
        admin_session_indicator(admin_pin_unlock_enabled),
        spacing(),
    ];

    // Show error if present
    if let Some(err) = error {
        content = content.push(error_message(err));
        content = content.push(spacing());
    }

    // User list
    if users.is_empty() {
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
        let mut user_items: Vec<Element<'a, DomainMessage>> = users
            .iter()
            .map(|user| user_button_with_auth_method(user, admin_pin_unlock_enabled))
            .collect();

        // Add "Add User" button for admins
        if let Some(permissions) = user_permissions
            && (permissions.has_role("admin") || permissions.has_permission("users:create"))
        {
            user_items.push(add_user_button());
        }

        let user_list = scrollable(column(user_items).spacing(8))
            .height(Length::FillPortion(1))
            .style(|theme: &Theme, _| {
                let palette = theme.extended_palette();
                scrollable::Style {
                    container: container::Style {
                        background: None,
                        ..Default::default()
                    },
                    vertical_rail: scrollable::Rail {
                        background: Some(palette.background.weak.color.into()),
                        border: iced::Border::default(),
                        scroller: scrollable::Scroller {
                            color: palette.background.strong.color,
                            border: iced::Border::default(),
                        },
                    },
                    horizontal_rail: scrollable::Rail {
                        background: Some(palette.background.weak.color.into()),
                        border: iced::Border::default(),
                        scroller: scrollable::Scroller {
                            color: palette.background.strong.color,
                            border: iced::Border::default(),
                        },
                    },
                    gap: None,
                }
            });

        content = content.push(
            container(user_list)
                .height(Length::Fixed(400.0))
                .width(Length::Fill),
        );
    }

    let card = auth_card(content);
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
fn admin_session_indicator<'a>(admin_pin_unlock_enabled: bool) -> Element<'a, DomainMessage> {
    let (icon_text, status_text) = if admin_pin_unlock_enabled {
        ("🔓", "PIN Available")
    } else {
        ("🔐", "Password Required")
    };

    let theme_color = if admin_pin_unlock_enabled {
        |theme: &Theme| theme.extended_palette().success.base.color
    } else {
        |theme: &Theme| theme.extended_palette().background.strong.color
    };

    container(
        row![
            text(icon_text)
                .size(16)
                .style(move |theme: &Theme| text::Style {
                    color: Some(theme_color(theme)),
                }),
            Space::new().width(Length::Fixed(8.0)),
            text(status_text)
                .size(14)
                .style(move |theme: &Theme| text::Style {
                    color: Some(theme_color(theme)),
                }),
        ]
        .align_y(Alignment::Center),
    )
    .padding(8)
    .style(move |theme: &Theme| {
        let color = theme_color(theme);
        container::Style {
            background: Some(color.scale_alpha(0.1).into()),
            border: iced::Border {
                color,
                width: 1.0,
                radius: 6.0.into(),
            },
            ..Default::default()
        }
    })
    .into()
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
fn user_button_with_auth_method<'a>(
    user: &'a UserListItemDto,
    admin_pin_unlock_enabled: bool,
) -> Element<'a, DomainMessage> {
    user_button_internal(user, admin_pin_unlock_enabled)
}

/// Creates a user selection button (legacy compatibility)
fn user_button<'a>(user: &'a UserListItemDto) -> Element<'a, DomainMessage> {
    user_button_internal(user, false)
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
fn user_button_internal<'a>(
    user: &'a UserListItemDto,
    admin_pin_unlock_enabled: bool,
) -> Element<'a, DomainMessage> {
    let auth_method_text = if admin_pin_unlock_enabled {
        "PIN or Password"
    } else {
        "Password Required"
    };

    let auth_method_color = if admin_pin_unlock_enabled {
        |theme: &Theme| theme.extended_palette().success.weak.color
    } else {
        |theme: &Theme| theme.extended_palette().background.strong.text
    };
    button(
        row![
            // User avatar placeholder
            container(text(user.display_name.chars().next().unwrap_or('U')).size(24))
                .width(Length::Fixed(48.0))
                .height(Length::Fixed(48.0))
                .align_x(iced::alignment::Horizontal::Center)
                .align_y(iced::alignment::Vertical::Center)
                .style(|theme: &Theme| {
                    let palette = theme.extended_palette();
                    container::Style {
                        background: Some(palette.primary.weak.color.into()),
                        border: iced::Border {
                            radius: 24.0.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    }
                }),
            Space::new().width(Length::Fixed(16.0)),
            column![
                text(&user.display_name).size(18),
                text(&user.username).size(14).style(|theme: &Theme| {
                    text::Style {
                        color: Some(theme.extended_palette().background.strong.text),
                    }
                }),
                text(auth_method_text)
                    .size(12)
                    .style(move |theme: &Theme| text::Style {
                        color: Some(auth_method_color(theme)),
                    }),
            ]
            .align_x(Alignment::Start),
        ]
        .align_y(Alignment::Center)
        .width(Length::Fill)
        .padding(12),
    )
    .on_press(DomainMessage::Auth(auth::Message::SelectUser(user.id)))
    .width(Length::Fill)
    .style(|theme: &Theme, status| {
        let palette = theme.extended_palette();
        match status {
            button::Status::Active => button::Style {
                background: None,
                text_color: palette.background.base.text,
                border: iced::Border {
                    color: palette.background.strong.color,
                    width: 1.0,
                    radius: 8.0.into(),
                },
                ..Default::default()
            },
            button::Status::Hovered => button::Style {
                background: Some(palette.background.weak.color.into()),
                text_color: palette.background.base.text,
                border: iced::Border {
                    color: palette.primary.weak.color,
                    width: 1.0,
                    radius: 8.0.into(),
                },
                ..Default::default()
            },
            button::Status::Pressed => button::Style {
                background: Some(palette.primary.weak.color.into()),
                text_color: palette.background.base.text,
                border: iced::Border {
                    color: palette.primary.base.color,
                    width: 1.0,
                    radius: 8.0.into(),
                },
                ..Default::default()
            },
            button::Status::Disabled => button::Style {
                background: None,
                text_color: palette.background.strong.text,
                border: iced::Border {
                    color: palette.background.strong.color,
                    width: 1.0,
                    radius: 8.0.into(),
                },
                ..Default::default()
            },
        }
    })
    .into()
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
fn add_user_button<'a>() -> Element<'a, DomainMessage> {
    button(
        row![
            // Add icon
            container(text("+").size(24).style(|theme: &Theme| text::Style {
                color: Some(theme.extended_palette().primary.base.text),
            }))
            .width(Length::Fixed(48.0))
            .height(Length::Fixed(48.0))
            .align_x(iced::alignment::Horizontal::Center)
            .align_y(iced::alignment::Vertical::Center)
            .style(|theme: &Theme| {
                let palette = theme.extended_palette();
                container::Style {
                    background: Some(palette.background.strong.color.into()),
                    border: iced::Border {
                        radius: 24.0.into(),
                        width: 2.0,
                        color: palette.primary.base.color,
                    },
                    ..Default::default()
                }
            }),
            Space::new().width(Length::Fixed(16.0)),
            column![
                text("Add User").size(18),
                text("admin").size(14).style(|theme: &Theme| {
                    text::Style {
                        color: Some(theme.extended_palette().background.strong.text),
                    }
                }),
                text("Create a new user account")
                    .size(12)
                    .style(|theme: &Theme| text::Style {
                        color: Some(theme.extended_palette().primary.base.color),
                    }),
            ]
            .align_x(Alignment::Start),
        ]
        .align_y(Alignment::Center)
        .width(Length::Fill)
        .padding(12),
    )
    .on_press(DomainMessage::Auth(auth::Message::ShowCreateUser))
    .width(Length::Fill)
    .style(|theme: &Theme, status| {
        let palette = theme.extended_palette();
        match status {
            button::Status::Active => button::Style {
                background: None,
                text_color: palette.background.base.text,
                border: iced::Border {
                    color: palette.primary.weak.color,
                    width: 1.0,
                    radius: 8.0.into(),
                },
                ..Default::default()
            },
            button::Status::Hovered => button::Style {
                background: Some(palette.primary.weak.color.into()),
                text_color: palette.background.base.text,
                border: iced::Border {
                    color: palette.primary.base.color,
                    width: 1.0,
                    radius: 8.0.into(),
                },
                ..Default::default()
            },
            button::Status::Pressed => button::Style {
                background: Some(palette.primary.base.color.into()),
                text_color: palette.background.base.text,
                border: iced::Border {
                    color: palette.primary.strong.color,
                    width: 1.0,
                    radius: 8.0.into(),
                },
                ..Default::default()
            },
            button::Status::Disabled => button::Style {
                background: None,
                text_color: palette.background.strong.text,
                border: iced::Border {
                    color: palette.background.strong.color,
                    width: 1.0,
                    radius: 8.0.into(),
                },
                ..Default::default()
            },
        }
    })
    .into()
}
