use iced::{
    Background, Border, Color, Shadow, Vector,
    widget::{button, container, pick_list, slider, text, toggler},
};

// Container styles
pub fn container_player(_theme: &iced::Theme) -> container::Style {
    container::Style {
        // Transparent background to allow Wayland subsurface video to show through
        background: Some(Background::Color(Color::TRANSPARENT)),
        text_color: Some(Color::WHITE),
        ..Default::default()
    }
}

pub fn container_controls_overlay(_theme: &iced::Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color::from_rgba(
            0.0, 0.0, 0.0, 0.8,
        ))),
        ..Default::default()
    }
}

pub fn container_gradient_top(_theme: &iced::Theme) -> container::Style {
    container::Style {
        background: Some(Background::Gradient(
            iced::gradient::Linear::new(iced::Radians(
                std::f32::consts::PI / 2.0,
            ))
            .add_stop(0.0, Color::from_rgba(0.0, 0.0, 0.0, 0.8))
            .add_stop(1.0, Color::from_rgba(0.0, 0.0, 0.0, 0.0))
            .into(),
        )),
        ..Default::default()
    }
}

pub fn container_gradient_bottom(_theme: &iced::Theme) -> container::Style {
    container::Style {
        background: Some(Background::Gradient(
            iced::gradient::Linear::new(iced::Radians(
                3.0 * std::f32::consts::PI / 2.0,
            ))
            .add_stop(0.0, Color::from_rgba(0.0, 0.0, 0.0, 0.8))
            .add_stop(1.0, Color::from_rgba(0.0, 0.0, 0.0, 0.0))
            .into(),
        )),
        ..Default::default()
    }
}

pub fn container_notification(_theme: &iced::Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color::from_rgba(
            0.0, 0.0, 0.0, 0.9,
        ))),
        border: Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.2),
            width: 1.0,
            radius: 8.0.into(),
        },
        ..Default::default()
    }
}

pub fn container_settings_panel(_theme: &iced::Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color::from_rgba(
            0.1, 0.1, 0.1, 0.95,
        ))),
        border: Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.2),
            width: 1.0,
            radius: 8.0.into(),
        },
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.5),
            offset: Vector::new(0.0, 5.0),
            blur_radius: 15.0,
        },
        ..Default::default()
    }
}

pub fn container_settings_panel_wrapper(
    _theme: &iced::Theme,
) -> container::Style {
    container::Style::default()
}

pub fn container_subtitle_menu(_theme: &iced::Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color::from_rgba(
            0.1, 0.1, 0.1, 0.98,
        ))),
        border: Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.3),
            width: 1.0,
            radius: 8.0.into(),
        },
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.8),
            offset: Vector::new(0.0, 4.0),
            blur_radius: 12.0,
        },
        ..Default::default()
    }
}

pub fn container_subtitle_menu_wrapper(
    _theme: &iced::Theme,
) -> container::Style {
    container::Style::default()
}

pub fn container_subtle(_theme: &iced::Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color::from_rgba(
            1.0, 1.0, 1.0, 0.05,
        ))),
        border: Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.1),
            width: 1.0,
            radius: 6.0.into(),
        },
        ..Default::default()
    }
}

pub fn container_hdr_badge(_theme: &iced::Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color::from_rgba(
            0.0, 0.0, 0.0, 0.6,
        ))),
        border: Border {
            color: Color::from_rgba(1.0, 0.8, 0.0, 0.5),
            width: 1.0,
            radius: 4.0.into(),
        },
        ..Default::default()
    }
}

// Seek bar container styles
pub fn container_seek_bar_background(
    _theme: &iced::Theme,
    hovered: bool,
) -> container::Style {
    let background_color = if hovered {
        // Light grey when hovered
        Color::from_rgba(0.6, 0.6, 0.6, 0.6)
    } else {
        // Dark grey when not hovered
        Color::from_rgba(0.3, 0.3, 0.3, 0.5)
    };

    container::Style {
        background: Some(Background::Color(background_color)),
        ..Default::default()
    }
}

pub fn container_seek_bar_buffered(_theme: &iced::Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color::from_rgba(
            1.0, 1.0, 1.0, 0.25,
        ))),
        ..Default::default()
    }
}

pub fn container_seek_bar_progress(_theme: &iced::Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color::from_rgb(0.0, 0.5, 1.0))),
        ..Default::default()
    }
}

pub fn container_subtitle(_theme: &iced::Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color::from_rgba(
            0.0, 0.0, 0.0, 0.85,
        ))),
        border: Border {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.2),
            width: 2.0,
            radius: 4.0.into(),
        },
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.1),
            offset: Vector::new(0.0, 2.0),
            blur_radius: 4.0,
        },
        ..Default::default()
    }
}

// Button styles
pub fn button_player(
    _theme: &iced::Theme,
    status: button::Status,
) -> button::Style {
    match status {
        button::Status::Active => button::Style {
            background: Some(Background::Color(Color::from_rgba(
                1.0, 1.0, 1.0, 0.1,
            ))),
            text_color: Color::WHITE,
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        },
        button::Status::Hovered => button::Style {
            background: Some(Background::Color(Color::from_rgba(
                1.0, 1.0, 1.0, 0.2,
            ))),
            text_color: Color::WHITE,
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        },
        button::Status::Pressed => button::Style {
            background: Some(Background::Color(Color::from_rgba(
                1.0, 1.0, 1.0, 0.3,
            ))),
            text_color: Color::WHITE,
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        },
        button::Status::Disabled => button::Style {
            background: Some(Background::Color(Color::from_rgba(
                1.0, 1.0, 1.0, 0.05,
            ))),
            text_color: Color::from_rgba(1.0, 1.0, 1.0, 0.3),
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        },
    }
}

pub fn button_player_active(
    _theme: &iced::Theme,
    status: button::Status,
) -> button::Style {
    match status {
        button::Status::Active => button::Style {
            background: Some(Background::Color(Color::from_rgba(
                0.0, 0.5, 1.0, 0.01,
            ))),
            text_color: Color::from_rgb(0.0, 0.6, 1.0),
            border: Border {
                color: Color::from_rgba(0.0, 0.5, 1.0, 0.1),
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        },
        button::Status::Hovered => button::Style {
            background: Some(Background::Color(Color::from_rgba(
                0.0, 0.5, 1.0, 0.4,
            ))),
            text_color: Color::from_rgb(0.0, 0.7, 1.0),
            border: Border {
                color: Color::from_rgba(0.0, 0.5, 1.0, 0.6),
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        },
        _ => button_player(_theme, status),
    }
}

pub fn button_player_disabled(
    _theme: &iced::Theme,
    _status: button::Status,
) -> button::Style {
    button::Style {
        background: Some(Background::Color(Color::from_rgba(
            1.0, 1.0, 1.0, 0.02,
        ))),
        text_color: Color::from_rgba(1.0, 1.0, 1.0, 0.2),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: 4.0.into(),
        },
        ..Default::default()
    }
}

pub fn button_transparent(
    _theme: &iced::Theme,
    status: button::Status,
) -> button::Style {
    button::Style {
        background: None,
        border: Border::default(),
        text_color: match status {
            button::Status::Hovered => Color::from_rgba(1.0, 1.0, 1.0, 0.8),
            _ => Color::WHITE,
        },
        ..Default::default()
    }
}

pub fn button_ghost(
    _theme: &iced::Theme,
    status: button::Status,
) -> button::Style {
    match status {
        button::Status::Active => button::Style {
            background: Some(Background::Color(Color::TRANSPARENT)),
            text_color: Color::from_rgba(1.0, 1.0, 1.0, 0.8),
            border: Border::default(),
            ..Default::default()
        },
        button::Status::Hovered => button::Style {
            background: Some(Background::Color(Color::from_rgba(
                1.0, 1.0, 1.0, 0.1,
            ))),
            text_color: Color::WHITE,
            border: Border::default(),
            ..Default::default()
        },
        button::Status::Pressed => button::Style {
            background: Some(Background::Color(Color::from_rgba(
                1.0, 1.0, 1.0, 0.2,
            ))),
            text_color: Color::WHITE,
            border: Border::default(),
            ..Default::default()
        },
        button::Status::Disabled => button::Style {
            background: Some(Background::Color(Color::TRANSPARENT)),
            text_color: Color::from_rgba(1.0, 1.0, 1.0, 0.3),
            border: Border::default(),
            ..Default::default()
        },
    }
}

pub fn button_menu_item(
    _theme: &iced::Theme,
    status: button::Status,
) -> button::Style {
    match status {
        button::Status::Active => button::Style {
            background: Some(Background::Color(Color::TRANSPARENT)),
            text_color: Color::from_rgba(1.0, 1.0, 1.0, 0.9),
            border: Border::default(),
            ..Default::default()
        },
        button::Status::Hovered => button::Style {
            background: Some(Background::Color(Color::from_rgba(
                0.0, 0.5, 1.0, 0.2,
            ))),
            text_color: Color::WHITE,
            border: Border::default(),
            ..Default::default()
        },
        button::Status::Pressed => button::Style {
            background: Some(Background::Color(Color::from_rgba(
                0.0, 0.5, 1.0, 0.3,
            ))),
            text_color: Color::WHITE,
            border: Border::default(),
            ..Default::default()
        },
        button::Status::Disabled => button::Style {
            background: Some(Background::Color(Color::TRANSPARENT)),
            text_color: Color::from_rgba(1.0, 1.0, 1.0, 0.3),
            border: Border::default(),
            ..Default::default()
        },
    }
}

// Seek bar button style (transparent, no visible button styling)
pub fn button_seek_bar(
    _theme: &iced::Theme,
    _status: button::Status,
) -> button::Style {
    button::Style {
        background: Some(Background::Color(Color::TRANSPARENT)),
        text_color: Color::TRANSPARENT,
        border: Border::default(),
        shadow: Shadow::default(),
        snap: false,
    }
}

// Slider styles
pub fn slider_seek(
    _theme: &iced::Theme,
    status: slider::Status,
) -> slider::Style {
    let handle_color = match status {
        slider::Status::Active => Color::from_rgb(0.0, 0.5, 1.0),
        slider::Status::Hovered => Color::from_rgb(0.0, 0.6, 1.0),
        slider::Status::Dragged => Color::from_rgb(0.0, 0.7, 1.0),
    };

    slider::Style {
        rail: slider::Rail {
            backgrounds: (
                Background::Color(Color::from_rgb(0.0, 0.5, 1.0)),
                Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.3)),
            ),
            width: 4.0,
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: 2.0.into(),
            },
        },
        handle: slider::Handle {
            shape: slider::HandleShape::Circle { radius: 8.0 },
            background: Background::Color(handle_color),
            border_color: Color::TRANSPARENT,
            border_width: 0.0,
        },
    }
}

// Invisible slider for seek bar interaction
pub fn slider_seek_invisible(
    _theme: &iced::Theme,
    _status: slider::Status,
) -> slider::Style {
    slider::Style {
        rail: slider::Rail {
            backgrounds: (
                Background::Color(Color::TRANSPARENT),
                Background::Color(Color::TRANSPARENT),
            ),
            width: 4.0,
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: 0.0.into(),
            },
        },
        handle: slider::Handle {
            shape: slider::HandleShape::Rectangle {
                width: 0,
                border_radius: 0.0.into(),
            },
            background: Background::Color(Color::TRANSPARENT),
            border_color: Color::TRANSPARENT,
            border_width: 0.0,
        },
    }
}

pub fn slider_volume(
    _theme: &iced::Theme,
    status: slider::Status,
) -> slider::Style {
    let handle_color = match status {
        slider::Status::Active => Color::from_rgba(1.0, 1.0, 1.0, 0.8),
        slider::Status::Hovered => Color::WHITE,
        slider::Status::Dragged => Color::WHITE,
    };

    slider::Style {
        rail: slider::Rail {
            backgrounds: (
                Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.2)),
                Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.8)),
            ),
            width: 3.0,
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: 1.5.into(),
            },
        },
        handle: slider::Handle {
            shape: slider::HandleShape::Circle { radius: 6.0 },
            background: Background::Color(handle_color),
            border_color: Color::TRANSPARENT,
            border_width: 0.0,
        },
    }
}

// Glassy button style (for settings panel)
pub fn button_glassy(
    _theme: &iced::Theme,
    _status: button::Status,
) -> button::Style {
    button::Style {
        background: Some(Background::Color(Color::from_rgba(
            0.0, 0.0, 0.0, 0.3,
        ))),
        border: Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.1),
            width: 1.0,
            radius: 4.0.into(),
        },
        text_color: Color::WHITE,
        ..button::Style::default()
    }
}

// Active setting button style
pub fn button_active_setting(
    _theme: &iced::Theme,
    _status: button::Status,
) -> button::Style {
    button::Style {
        background: Some(Background::Color(Color::from_rgba(
            1.0, 1.0, 1.0, 0.2,
        ))),
        border: Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.3),
            width: 1.0,
            radius: 4.0.into(),
        },
        text_color: Color::WHITE,
        ..button::Style::default()
    }
}

// Text styles
pub fn text_bright(_theme: &iced::Theme) -> text::Style {
    text::Style {
        color: Some(Color::WHITE),
    }
}

pub fn text_muted(_theme: &iced::Theme) -> text::Style {
    text::Style {
        color: Some(Color::from_rgba(1.0, 1.0, 1.0, 0.6)),
    }
}

pub fn text_dim(_theme: &iced::Theme) -> text::Style {
    text::Style {
        color: Some(Color::from_rgba(1.0, 1.0, 1.0, 0.4)),
    }
}

// Pick list style
pub fn pick_list_dark<T>(
    _theme: &iced::Theme,
    _status: pick_list::Status,
) -> pick_list::Style {
    pick_list::Style {
        text_color: Color::WHITE,
        placeholder_color: Color::from_rgba(1.0, 1.0, 1.0, 0.5),
        handle_color: Color::from_rgba(1.0, 1.0, 1.0, 0.8),
        background: Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.5)),
        border: Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.3),
            width: 1.0,
            radius: 4.0.into(),
        },
    }
}

// Toggler style
pub fn toggler_dark(
    _theme: &iced::Theme,
    status: toggler::Status,
) -> toggler::Style {
    // Extract the actual toggle state from the status
    let is_toggled = match status {
        toggler::Status::Active { is_toggled } => is_toggled,
        toggler::Status::Hovered { is_toggled } => is_toggled,
        toggler::Status::Disabled => false,
    };

    toggler::Style {
        background: if is_toggled {
            Color::from_rgb(0.0, 0.5, 1.0)
        } else {
            Color::from_rgba(1.0, 1.0, 1.0, 0.2)
        },
        background_border_width: 0.0,
        background_border_color: Color::TRANSPARENT,
        foreground: Color::WHITE,
        foreground_border_width: 0.0,
        foreground_border_color: Color::TRANSPARENT,
    }
}
