use iced::{
    theme,
    widget::{button, container, scrollable, slider, text_input},
    Background, Border, Color, Shadow, Theme,
};

/// Pure black theme with high contrast electric blue accents
#[derive(Debug, Clone, Copy)]
pub struct MediaServerTheme;

impl MediaServerTheme {
    // Core colors
    pub const BLACK: Color = Color::from_rgb(0.0, 0.0, 0.0); // #000000
    pub const ACCENT_BLUE: Color = Color::from_rgb(0.0, 0.5, 1.0); // #0080FF
    pub const ACCENT_BLUE_HOVER: Color = Color::from_rgb(0.0, 0.6, 1.0); // #0099FF
    pub const ACCENT_BLUE_GLOW: Color = Color::from_rgba(0.0, 0.5, 1.0, 0.3); // Blue glow

    // Grays
    pub const CARD_BG: Color = Color::from_rgb(0.1, 0.1, 0.1); // #1A1A1A
    pub const CARD_HOVER: Color = Color::from_rgb(0.15, 0.15, 0.15); // #262626
    pub const BORDER_COLOR: Color = Color::from_rgb(0.2, 0.2, 0.2); // #333333

    // Text colors
    pub const TEXT_PRIMARY: Color = Color::from_rgb(1.0, 1.0, 1.0); // #FFFFFF
    pub const TEXT_SECONDARY: Color = Color::from_rgb(0.7, 0.7, 0.7); // #B3B3B3
    pub const TEXT_DIMMED: Color = Color::from_rgb(0.5, 0.5, 0.5); // #808080

    // Status colors
    pub const SUCCESS: Color = Color::from_rgb(0.0, 0.8, 0.4); // #00CC66
    pub const WARNING: Color = Color::from_rgb(1.0, 0.6, 0.0); // #FF9900
    pub const ERROR: Color = Color::from_rgb(1.0, 0.2, 0.2); // #FF3333
    pub const ERROR_COLOR: Color = Color::from_rgb(1.0, 0.2, 0.2); // #FF3333 - alias for forms
    pub const DESTRUCTIVE: Color = Color::from_rgb(1.0, 0.2, 0.2); // #FF3333 - for destructive actions

    pub fn theme() -> Theme {
        let mut palette = theme::Palette::DARK;
        palette.background = Self::BLACK;
        palette.text = Self::TEXT_PRIMARY;
        palette.primary = Self::ACCENT_BLUE;
        palette.success = Self::SUCCESS;
        palette.danger = Self::ERROR;

        Theme::custom("MediaServer", palette)
    }
}

// Container styles using closures
pub enum Container {
    Default,
    Card,
    CardHovered,
    Selected,
    MediaGrid,
    VideoPlayer,
    ProgressBar,
    ProgressBarBackground,
    Badge,
    Header,
    RoundedImage,
    ErrorBox,
    Modal,
    ModalOverlay,
}

impl Container {
    pub fn style(&self) -> fn(&Theme) -> container::Style {
        match self {
            Container::Default => |_| container::Style {
                text_color: Some(MediaServerTheme::TEXT_PRIMARY),
                background: Some(Background::Color(MediaServerTheme::BLACK)),
                border: Border::default(),
                shadow: Shadow::default(),
                snap: false,
            },
            Container::Card => |_| container::Style {
                text_color: Some(MediaServerTheme::TEXT_PRIMARY),
                background: Some(Background::Color(MediaServerTheme::CARD_BG)),
                border: Border {
                    color: MediaServerTheme::BORDER_COLOR,
                    width: 1.0,
                    radius: 8.0.into(),
                },
                shadow: Shadow::default(),
                snap: false,
            },
            Container::CardHovered => |_| container::Style {
                text_color: Some(MediaServerTheme::TEXT_PRIMARY),
                background: Some(Background::Color(MediaServerTheme::CARD_HOVER)),
                border: Border {
                    color: MediaServerTheme::ACCENT_BLUE,
                    width: 1.0,
                    radius: 8.0.into(),
                },
                shadow: Shadow {
                    color: MediaServerTheme::ACCENT_BLUE_GLOW,
                    offset: iced::Vector::new(0.0, 0.0),
                    blur_radius: 10.0,
                },
                snap: false,
            },
            Container::Selected => |_| container::Style {
                text_color: Some(MediaServerTheme::TEXT_PRIMARY),
                background: Some(Background::Color(MediaServerTheme::CARD_BG)),
                border: Border {
                    color: MediaServerTheme::ACCENT_BLUE,
                    width: 2.0,
                    radius: 8.0.into(),
                },
                shadow: Shadow {
                    color: MediaServerTheme::ACCENT_BLUE_GLOW,
                    offset: iced::Vector::new(0.0, 0.0),
                    blur_radius: 20.0,
                },
                snap: false,
            },
            Container::MediaGrid => |_| container::Style {
                text_color: Some(MediaServerTheme::TEXT_PRIMARY),
                background: Some(Background::Color(MediaServerTheme::BLACK)),
                border: Border::default(),
                shadow: Shadow::default(),
                snap: false,
            },
            Container::VideoPlayer => |_| container::Style {
                text_color: Some(MediaServerTheme::TEXT_PRIMARY),
                background: Some(Background::Color(Color::BLACK)),
                border: Border::default(),
                shadow: Shadow::default(),
                snap: false,
            },
            Container::ProgressBar => |_| container::Style {
                text_color: None,
                background: Some(Background::Color(MediaServerTheme::ACCENT_BLUE)),
                border: Border {
                    color: Color::TRANSPARENT,
                    width: 0.0,
                    radius: 2.0.into(),
                },
                shadow: Shadow {
                    color: MediaServerTheme::ACCENT_BLUE_GLOW,
                    offset: iced::Vector::new(0.0, 0.0),
                    blur_radius: 4.0,
                },
                snap: false,
            },
            Container::ProgressBarBackground => |_| container::Style {
                text_color: None,
                background: Some(Background::Color(MediaServerTheme::CARD_BG)),
                border: Border {
                    color: MediaServerTheme::BORDER_COLOR,
                    width: 1.0,
                    radius: 2.0.into(),
                },
                shadow: Shadow::default(),
                snap: false,
            },
            Container::Badge => |_| container::Style {
                text_color: Some(Color::WHITE),
                background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.8))),
                border: Border {
                    color: Color::TRANSPARENT,
                    width: 0.0,
                    radius: 4.0.into(),
                },
                shadow: Shadow::default(),
                snap: false,
            },
            Container::Header => |_| container::Style {
                text_color: Some(MediaServerTheme::TEXT_PRIMARY),
                background: Some(Background::Color(MediaServerTheme::BLACK)),
                border: Border {
                    color: MediaServerTheme::BLACK,
                    width: 0.0,
                    radius: 0.0.into(),
                },
                shadow: Shadow {
                    color: Color::from_rgba(0.0, 0.0, 0.0, 0.8),
                    offset: iced::Vector::new(0.0, 2.0),
                    blur_radius: 4.0,
                },
                snap: false,
            },
            Container::RoundedImage => |_| container::Style {
                text_color: None,
                background: None,
                border: Border {
                    color: Color::TRANSPARENT,
                    width: 0.0,
                    radius: 8.0.into(),
                },
                shadow: Shadow::default(),
                snap: false,
            },
            Container::ErrorBox => |_| container::Style {
                text_color: Some(MediaServerTheme::ERROR_COLOR),
                background: Some(Background::Color(Color::from_rgba(1.0, 0.2, 0.2, 0.1))),
                border: Border {
                    color: MediaServerTheme::ERROR_COLOR,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                shadow: Shadow::default(),
                snap: false,
            },
            Container::Modal => |_| container::Style {
                text_color: Some(MediaServerTheme::TEXT_PRIMARY),
                background: Some(Background::Color(MediaServerTheme::CARD_BG)),
                border: Border {
                    color: MediaServerTheme::BORDER_COLOR,
                    width: 1.0,
                    radius: 12.0.into(),
                },
                shadow: Shadow {
                    color: Color::from_rgba(0.0, 0.0, 0.0, 0.8),
                    offset: iced::Vector::new(0.0, 4.0),
                    blur_radius: 20.0,
                },
                snap: false,
            },
            Container::ModalOverlay => |_| container::Style {
                text_color: None,
                background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.7))),
                border: Border::default(),
                shadow: Shadow::default(),
                snap: false,
            },
        }
    }
}

// Button styles using closures
pub enum Button {
    Primary,
    Secondary,
    Destructive,
    MediaCard,
    MediaCardHovered,
    Text,
    Icon,
    PlayOverlay,
    Card,
}

impl Button {
    pub fn style(&self) -> fn(&Theme, button::Status) -> button::Style {
        match self {
            Button::Primary => |_, status| {
                let (background, shadow) = match status {
                    button::Status::Active => (
                        MediaServerTheme::ACCENT_BLUE,
                        Shadow {
                            color: MediaServerTheme::ACCENT_BLUE_GLOW,
                            offset: iced::Vector::new(0.0, 2.0),
                            blur_radius: 8.0,
                        },
                    ),
                    button::Status::Hovered => (
                        MediaServerTheme::ACCENT_BLUE_HOVER,
                        Shadow {
                            color: MediaServerTheme::ACCENT_BLUE_GLOW,
                            offset: iced::Vector::new(0.0, 2.0),
                            blur_radius: 16.0,
                        },
                    ),
                    button::Status::Pressed => (
                        Color::from_rgb(0.0, 0.4, 0.8),
                        Shadow {
                            color: MediaServerTheme::ACCENT_BLUE_GLOW,
                            offset: iced::Vector::new(0.0, 2.0),
                            blur_radius: 8.0,
                        },
                    ),
                    _ => (MediaServerTheme::ACCENT_BLUE, Shadow::default()),
                };

                button::Style {
                    text_color: MediaServerTheme::TEXT_PRIMARY,
                    background: Some(Background::Color(background)),
                    border: Border {
                        color: background,
                        width: 1.0,
                        radius: 8.0.into(),
                    },
                    shadow,
                    snap: false,
                }
            },
            Button::Secondary => |_, status| {
                let (background, border_color) = match status {
                    button::Status::Active => {
                        (MediaServerTheme::CARD_BG, MediaServerTheme::BORDER_COLOR)
                    }
                    button::Status::Hovered => {
                        (MediaServerTheme::CARD_HOVER, MediaServerTheme::ACCENT_BLUE)
                    }
                    _ => (MediaServerTheme::CARD_BG, MediaServerTheme::BORDER_COLOR),
                };

                button::Style {
                    text_color: MediaServerTheme::TEXT_PRIMARY,
                    background: Some(Background::Color(background)),
                    border: Border {
                        color: border_color,
                        width: 1.0,
                        radius: 8.0.into(),
                    },
                    shadow: Shadow::default(),
                    snap: false,
                }
            },
            Button::Destructive => |_, status| {
                let (background, shadow) = match status {
                    button::Status::Active => (
                        MediaServerTheme::ERROR,
                        Shadow {
                            color: Color::from_rgba(1.0, 0.2, 0.2, 0.3),
                            offset: iced::Vector::new(0.0, 2.0),
                            blur_radius: 8.0,
                        },
                    ),
                    button::Status::Hovered => (
                        Color::from_rgb(1.0, 0.3, 0.3),
                        Shadow {
                            color: Color::from_rgba(1.0, 0.2, 0.2, 0.3),
                            offset: iced::Vector::new(0.0, 2.0),
                            blur_radius: 16.0,
                        },
                    ),
                    button::Status::Pressed => (
                        Color::from_rgb(0.9, 0.1, 0.1),
                        Shadow {
                            color: Color::from_rgba(1.0, 0.2, 0.2, 0.3),
                            offset: iced::Vector::new(0.0, 2.0),
                            blur_radius: 8.0,
                        },
                    ),
                    _ => (MediaServerTheme::ERROR, Shadow::default()),
                };

                button::Style {
                    text_color: MediaServerTheme::TEXT_PRIMARY,
                    background: Some(Background::Color(background)),
                    border: Border {
                        color: background,
                        width: 1.0,
                        radius: 8.0.into(),
                    },
                    shadow,
                    snap: false,
                }
            },
            Button::MediaCard => |_, status| {
                let background = match status {
                    button::Status::Hovered => {
                        Some(Background::Color(Color::from_rgba(0.0, 0.5, 1.0, 0.1)))
                    }
                    _ => Some(Background::Color(Color::TRANSPARENT)),
                };

                button::Style {
                    text_color: MediaServerTheme::TEXT_PRIMARY,
                    background,
                    border: Border {
                        color: Color::TRANSPARENT,
                        width: 0.0,
                        radius: 8.0.into(),
                    },
                    shadow: Shadow::default(),
                    snap: false,
                }
            },
            Button::MediaCardHovered => |_, _| button::Style {
                text_color: MediaServerTheme::TEXT_PRIMARY,
                background: Some(Background::Color(Color::from_rgba(0.0, 0.5, 1.0, 0.1))),
                border: Border::default(),
                shadow: Shadow::default(),
                snap: false,
            },
            Button::Text => |_, status| {
                let text_color = match status {
                    button::Status::Hovered => MediaServerTheme::TEXT_PRIMARY,
                    _ => MediaServerTheme::TEXT_SECONDARY,
                };

                button::Style {
                    text_color,
                    background: None,
                    border: Border::default(),
                    shadow: Shadow::default(),
                    snap: false,
                }
            },
            Button::Icon => |_, status| {
                let background = match status {
                    button::Status::Hovered => {
                        Some(Background::Color(MediaServerTheme::ACCENT_BLUE))
                    }
                    _ => Some(Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.1))),
                };

                let icon_color = match status {
                    button::Status::Hovered => MediaServerTheme::BLACK,
                    _ => MediaServerTheme::TEXT_PRIMARY,
                };
                button::Style {
                    text_color: icon_color,
                    background,
                    border: Border {
                        color: Color::TRANSPARENT,
                        width: 0.0,
                        radius: 50.0.into(),
                    },
                    shadow: Shadow::default(),
                    snap: false,
                }
            },
            Button::PlayOverlay => |_, status| {
                let (background, shadow) = match status {
                    button::Status::Hovered => (
                        Some(Background::Color(MediaServerTheme::ACCENT_BLUE)),
                        Shadow {
                            color: MediaServerTheme::ACCENT_BLUE_GLOW,
                            offset: iced::Vector::new(0.0, 2.0),
                            blur_radius: 10.0,
                        },
                    ),
                    _ => (
                        Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.6))),
                        Shadow {
                            color: Color::from_rgba(0.0, 0.0, 0.0, 0.3),
                            offset: iced::Vector::new(0.0, 2.0),
                            blur_radius: 8.0,
                        },
                    ),
                };

                button::Style {
                    text_color: MediaServerTheme::TEXT_PRIMARY,
                    background,
                    border: Border {
                        color: Color::TRANSPARENT,
                        width: 0.0,
                        radius: 50.0.into(),
                    },
                    shadow,
                    snap: false,
                }
            },
            Button::Card => |_, status| {
                let background = match status {
                    button::Status::Hovered => {
                        Some(Background::Color(MediaServerTheme::CARD_HOVER))
                    }
                    _ => Some(Background::Color(Color::TRANSPARENT)),
                };

                button::Style {
                    text_color: MediaServerTheme::TEXT_PRIMARY,
                    background,
                    border: Border {
                        color: Color::TRANSPARENT,
                        width: 0.0,
                        radius: 8.0.into(),
                    },
                    shadow: Shadow::default(),
                    snap: false,
                }
            },
        }
    }
}

// Scrollable style
pub struct Scrollable;

impl Scrollable {
    pub fn style() -> fn(&Theme, scrollable::Status) -> scrollable::Style {
        |_, status| {
            let scroller_color = match status {
                scrollable::Status::Hovered { .. } => MediaServerTheme::ACCENT_BLUE_HOVER,
                _ => MediaServerTheme::ACCENT_BLUE,
            };

            scrollable::Style {
                container: container::Style::default(),
                vertical_rail: scrollable::Rail {
                    background: Some(Background::Color(MediaServerTheme::CARD_BG)),
                    border: Border {
                        color: MediaServerTheme::BORDER_COLOR,
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    scroller: scrollable::Scroller {
                        color: scroller_color,
                        border: Border {
                            color: Color::TRANSPARENT,
                            width: 0.0,
                            radius: 4.0.into(),
                        },
                    },
                },
                horizontal_rail: scrollable::Rail {
                    background: Some(Background::Color(MediaServerTheme::CARD_BG)),
                    border: Border {
                        color: MediaServerTheme::BORDER_COLOR,
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    scroller: scrollable::Scroller {
                        color: scroller_color,
                        border: Border {
                            color: Color::TRANSPARENT,
                            width: 0.0,
                            radius: 4.0.into(),
                        },
                    },
                },
                gap: None,
            }
        }
    }
}

// Slider style
pub struct Slider;

impl Slider {
    pub fn style() -> fn(&Theme, slider::Status) -> slider::Style {
        |_, status| {
            let handle_color = match status {
                slider::Status::Active => MediaServerTheme::ACCENT_BLUE,
                slider::Status::Hovered => MediaServerTheme::ACCENT_BLUE_HOVER,
                slider::Status::Dragged => MediaServerTheme::ACCENT_BLUE_HOVER,
            };

            slider::Style {
                rail: slider::Rail {
                    backgrounds: (
                        Background::Color(MediaServerTheme::CARD_BG),
                        Background::Color(MediaServerTheme::ACCENT_BLUE),
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
                    border_width: 2.0,
                    border_color: handle_color,
                },
            }
        }
    }
}

// Text input style
pub struct TextInput;

impl TextInput {
    pub fn style() -> fn(&Theme, text_input::Status) -> text_input::Style {
        |_, status| {
            let (border_color, border_width) = match status {
                text_input::Status::Active => (MediaServerTheme::BORDER_COLOR, 1.0),
                text_input::Status::Hovered => (MediaServerTheme::ACCENT_BLUE, 1.0),
                text_input::Status::Focused { .. } => (MediaServerTheme::ACCENT_BLUE, 2.0),
                text_input::Status::Disabled => (MediaServerTheme::BORDER_COLOR, 1.0),
            };

            let background = match status {
                text_input::Status::Disabled => {
                    Background::Color(Color::from_rgb(0.05, 0.05, 0.05))
                }
                _ => Background::Color(MediaServerTheme::CARD_BG),
            };

            text_input::Style {
                background,
                border: Border {
                    color: border_color,
                    width: border_width,
                    radius: 8.0.into(),
                },
                icon: MediaServerTheme::TEXT_SECONDARY,
                placeholder: MediaServerTheme::TEXT_DIMMED,
                value: MediaServerTheme::TEXT_PRIMARY,
                selection: MediaServerTheme::ACCENT_BLUE,
            }
        }
    }
}

// Text styles
pub fn icon_white<'a>(_theme: &iced::Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(Color::WHITE),
    }
}
