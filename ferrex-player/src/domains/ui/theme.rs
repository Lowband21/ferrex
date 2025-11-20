use iced::{
    Background, Border, Color, Shadow, Theme, theme,
    widget::{button, container, scrollable, slider, text_input},
};

/// Pure black theme with high contrast electric blue accents
#[derive(Debug, Clone, Copy)]
pub struct MediaServerTheme;

impl MediaServerTheme {
    // Core colors
    pub const BLACK: Color = Color::from_rgb(0.0, 0.0, 0.0); // #000000
    pub const BACKGROUND_DARK: Color = Color::from_rgb(0.1, 0.1, 0.10);
    //pub const BACKGROUND_ACCENT: Color = Color::from_rgb(0.31, 0.094, 0.333);
    //pub const BACKGROUND_ACCENT: Color = Color::from_rgb(0.20, 0.05, 0.30);
    pub const BACKGROUND_ACCENT: Color = Color::from_rgb(0.12, 0.05, 0.16);
    pub const ACCENT_BLUE: Color = Color::from_rgb(0.0, 0.5, 1.0); // #0080FF
    pub const ACCENT_BLUE_HOVER: Color = Color::from_rgb(0.0, 0.6, 1.0); // #0099FF
    pub const ACCENT_BLUE_GLOW: Color = Color::from_rgba(0.0, 0.5, 1.0, 0.3); // Blue glow

    // Grays
    pub const CARD_BG: Color = Color::from_rgb(0.1, 0.1, 0.1); // #1A1A1A
    pub const CARD_HOVER: Color = Color::from_rgb(0.15, 0.15, 0.15); // #262626
    pub const BORDER_COLOR: Color = Color::from_rgb(0.2, 0.2, 0.2); // #333333

    // Soft grays for backgrounds - with subtle blue tint
    pub const SOFT_GREY_DARK: Color = Color::from_rgb(0.05, 0.05, 0.08); // Much darker for contrast
    pub const SOFT_GREY_LIGHT: Color = Color::from_rgb(0.20, 0.20, 0.25); // Much lighter for visible gradient
    pub const SOFT_GREY_MEDIUM: Color = Color::from_rgb(0.10, 0.10, 0.12); // Medium with blue tint

    // Text colors
    pub const TEXT_PRIMARY: Color = Color::from_rgb(1.0, 1.0, 1.0); // #FFFFFF
    pub const TEXT_SECONDARY: Color = Color::from_rgb(0.7, 0.7, 0.7); // #B3B3B3
    pub const TEXT_DIMMED: Color = Color::from_rgb(0.5, 0.5, 0.5); // #808080
    pub const TEXT_SUBDUED: Color = Color::from_rgb(0.6, 0.6, 0.6); // #999999

    // Status colors
    pub const SUCCESS: Color = Color::from_rgb(0.0, 0.8, 0.4); // #00CC66
    pub const WARNING: Color = Color::from_rgb(1.0, 0.6, 0.0); // #FF9900
    pub const ERROR: Color = Color::from_rgb(1.0, 0.2, 0.2); // #FF3333
    pub const ERROR_COLOR: Color = Color::from_rgb(1.0, 0.2, 0.2); // #FF3333 - alias for forms
    pub const DESTRUCTIVE: Color = Color::from_rgb(1.0, 0.2, 0.2); // #FF3333 - for destructive actions
    pub const INFO: Color = Color::from_rgb(0.2, 0.6, 1.0); // #3399FF - informational

    // Background colors
    pub const BACKGROUND: Color = Color::from_rgb(0.0, 0.0, 0.0); // #000000 - same as BLACK
    pub const SURFACE_DIM: Color = Color::from_rgb(0.08, 0.08, 0.08); // #141414 - slightly lighter than black

    // View-specific default gradient colors
    pub const LIBRARY_BG_PRIMARY: Color = Self::SOFT_GREY_DARK; // Library view primary gradient
    pub const LIBRARY_BG_SECONDARY: Color = Self::SOFT_GREY_LIGHT; // Library view secondary gradient

    pub fn theme() -> Theme {
        let mut palette = theme::Palette::DARK;
        palette.background = Color::TRANSPARENT;
        palette.text = Self::TEXT_PRIMARY;
        palette.primary = Self::ACCENT_BLUE;
        palette.success = Self::SUCCESS;
        palette.danger = Self::ERROR;

        Theme::custom("Ferrex Dark", palette)
    }
}

// Container styles using closures
pub enum Container {
    Default,
    Card,
    CardHovered,
    ProgressBar,
    ProgressBarBackground,
    Header,
    HeaderAccent,
    ErrorBox,
    SuccessBox,
    Modal,
    ModalOverlay,
    TechDetail,
}

impl Container {
    pub fn style(&self) -> fn(&Theme) -> container::Style {
        match self {
            Container::Default => |_| container::Style {
                text_color: Some(MediaServerTheme::TEXT_PRIMARY),
                background: Some(Background::Color(Color::TRANSPARENT)),
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
                background: Some(Background::Color(
                    MediaServerTheme::CARD_HOVER,
                )),
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
            Container::ProgressBar => |_| container::Style {
                text_color: None,
                background: Some(Background::Color(
                    MediaServerTheme::ACCENT_BLUE,
                )),
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
            Container::Header => |_| container::Style {
                text_color: Some(MediaServerTheme::TEXT_PRIMARY),
                background: Some(Background::Color(
                    MediaServerTheme::BACKGROUND_DARK,
                )),
                border: Border {
                    color: Color::from_rgba(0.0, 0.0, 0.0, 0.2),
                    width: 0.0,
                    radius: 0.0.into(),
                },
                shadow: Shadow::default(),
                snap: false,
            },
            Container::HeaderAccent => |_| container::Style {
                text_color: Some(MediaServerTheme::TEXT_PRIMARY),
                background: Some(Background::Color(
                    MediaServerTheme::BACKGROUND_ACCENT,
                )),
                border: Border {
                    color: Color::from_rgba(0.0, 0.0, 0.0, 0.2),
                    width: 0.0,
                    radius: 0.0.into(),
                },
                shadow: Shadow::default(),
                snap: false,
            },
            Container::ErrorBox => |_| container::Style {
                text_color: Some(MediaServerTheme::ERROR_COLOR),
                background: Some(Background::Color(Color::from_rgba(
                    1.0, 0.2, 0.2, 0.1,
                ))),
                border: Border {
                    color: MediaServerTheme::ERROR_COLOR,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                shadow: Shadow::default(),
                snap: false,
            },
            Container::SuccessBox => |_| container::Style {
                text_color: Some(MediaServerTheme::TEXT_PRIMARY),
                background: Some(Background::Color(Color::from_rgba(
                    0.0, 0.8, 0.4, 0.12,
                ))),
                border: Border {
                    color: MediaServerTheme::SUCCESS,
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
                background: Some(Background::Color(Color::from_rgba(
                    0.0, 0.0, 0.0, 0.7,
                ))),
                border: Border::default(),
                shadow: Shadow::default(),
                snap: false,
            },
            Container::TechDetail => |_| container::Style {
                text_color: Some(MediaServerTheme::TEXT_PRIMARY),
                background: Some(Background::Color(Color::from_rgba(
                    1.0, 1.0, 1.0, 0.02,
                ))),
                border: Border {
                    color: Color::TRANSPARENT,
                    width: 0.0,
                    radius: 0.0.into(), // Sharp corners to match header buttons
                },
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
    Text,
    Icon,
    PlayOverlay,
    HeaderIcon,
    DetailAction,
    BackdropControl,
    Disabled,
    Danger,
}

// Helper functions for views
pub fn button_style() -> fn(&Theme, button::Status) -> button::Style {
    Button::Primary.style()
}

pub fn container_style() -> fn(&Theme) -> container::Style {
    Container::Default.style()
}

// FerrexTheme enum for text styles
#[derive(Debug, Clone, Copy)]
pub enum FerrexTheme {
    Text,
    HeaderText,
    SubduedText,
    ErrorText,
}

impl FerrexTheme {
    pub fn text_color(&self) -> Color {
        match self {
            FerrexTheme::Text => MediaServerTheme::TEXT_PRIMARY,
            FerrexTheme::HeaderText => MediaServerTheme::TEXT_PRIMARY,
            FerrexTheme::SubduedText => MediaServerTheme::TEXT_SECONDARY,
            FerrexTheme::ErrorText => MediaServerTheme::ERROR,
        }
    }

    pub fn card_background() -> Color {
        MediaServerTheme::CARD_BG
    }

    pub fn border_color() -> Color {
        MediaServerTheme::BORDER_COLOR
    }
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
                    button::Status::Active => (
                        MediaServerTheme::CARD_BG,
                        MediaServerTheme::BORDER_COLOR,
                    ),
                    button::Status::Hovered => (
                        MediaServerTheme::CARD_HOVER,
                        MediaServerTheme::ACCENT_BLUE,
                    ),
                    _ => (
                        MediaServerTheme::CARD_BG,
                        MediaServerTheme::BORDER_COLOR,
                    ),
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
            Button::MediaCard => |_, _status| {
                // Always use transparent background - hover effects are handled by the shader
                let background = Some(Background::Color(Color::TRANSPARENT));

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
                    _ => Some(Background::Color(Color::from_rgba(
                        1.0, 1.0, 1.0, 0.1,
                    ))),
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
                        Some(Background::Color(Color::from_rgba(
                            0.0, 0.0, 0.0, 0.6,
                        ))),
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
            Button::HeaderIcon => |_, status| {
                let (background, text_color) = match status {
                    button::Status::Active => (
                        Some(Background::Color(Color::from_rgba(
                            1.0, 1.0, 1.0, 0.02,
                        ))),
                        MediaServerTheme::TEXT_PRIMARY,
                    ),
                    button::Status::Hovered => (
                        Some(Background::Color(Color::from_rgba(
                            1.0, 1.0, 1.0, 0.05,
                        ))),
                        MediaServerTheme::TEXT_PRIMARY,
                    ),
                    button::Status::Pressed => (
                        Some(Background::Color(Color::from_rgba(
                            1.0, 1.0, 1.0, 0.08,
                        ))),
                        MediaServerTheme::ACCENT_BLUE,
                    ),
                    _ => (None, MediaServerTheme::TEXT_SECONDARY),
                };

                button::Style {
                    text_color,
                    background,
                    border: Border {
                        color: Color::TRANSPARENT,
                        width: 0.0,
                        radius: 0.0.into(), // Sharp corners
                    },
                    shadow: Shadow::default(), // No glow
                    snap: false,
                }
            },
            Button::DetailAction => |_, status| {
                let (background, text_color) = match status {
                    button::Status::Active => (
                        Some(Background::Color(Color::from_rgba(
                            0.0, 0.5, 1.0, 0.1,
                        ))), // Brighter blue glow
                        MediaServerTheme::TEXT_PRIMARY,
                    ),
                    button::Status::Hovered => (
                        Some(Background::Color(MediaServerTheme::ACCENT_BLUE)), // Solid blue on hover
                        MediaServerTheme::TEXT_PRIMARY,
                    ),
                    button::Status::Pressed => (
                        Some(Background::Color(Color::from_rgb(0.0, 0.4, 0.8))),
                        MediaServerTheme::TEXT_PRIMARY,
                    ),
                    _ => (
                        Some(Background::Color(Color::from_rgba(
                            0.0, 0.5, 1.0, 0.08,
                        ))),
                        MediaServerTheme::TEXT_PRIMARY,
                    ),
                };

                button::Style {
                    text_color,
                    background,
                    border: Border {
                        color: Color::TRANSPARENT,
                        width: 0.0,
                        radius: 0.0.into(), // Sharp corners
                    },
                    shadow: Shadow::default(), // No glow
                    snap: false,
                }
            },
            Button::BackdropControl => |_, status| {
                let (background, text_color) = match status {
                    button::Status::Active => (
                        Some(Background::Color(Color::from_rgba(
                            1.0, 1.0, 1.0, 0.02,
                        ))),
                        MediaServerTheme::TEXT_PRIMARY,
                    ),
                    button::Status::Hovered => (
                        Some(Background::Color(Color::from_rgba(
                            1.0, 1.0, 1.0, 0.05,
                        ))),
                        MediaServerTheme::TEXT_PRIMARY,
                    ),
                    button::Status::Pressed => (
                        Some(Background::Color(Color::from_rgba(
                            1.0, 1.0, 1.0, 0.08,
                        ))),
                        MediaServerTheme::ACCENT_BLUE,
                    ),
                    _ => (None, MediaServerTheme::TEXT_SECONDARY),
                };

                button::Style {
                    text_color,
                    background,
                    border: Border {
                        color: Color::TRANSPARENT,
                        width: 0.0,
                        radius: 0.0.into(), // Sharp corners
                    },
                    shadow: Shadow::default(), // No glow
                    snap: false,
                }
            },
            Button::Disabled => |_, _| button::Style {
                text_color: MediaServerTheme::TEXT_DIMMED,
                background: Some(Background::Color(MediaServerTheme::CARD_BG)),
                border: Border {
                    color: MediaServerTheme::BORDER_COLOR,
                    width: 1.0,
                    radius: 8.0.into(),
                },
                shadow: Shadow::default(),
                snap: false,
            },
            Button::Danger => |_, status| {
                let (background, shadow) = match status {
                    button::Status::Active => (
                        MediaServerTheme::DESTRUCTIVE,
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
                        Color::from_rgb(0.8, 0.1, 0.1),
                        Shadow {
                            color: Color::from_rgba(1.0, 0.2, 0.2, 0.3),
                            offset: iced::Vector::new(0.0, 2.0),
                            blur_radius: 8.0,
                        },
                    ),
                    _ => (MediaServerTheme::DESTRUCTIVE, Shadow::default()),
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
        }
    }
}

// Scrollable style
pub struct Scrollable;

impl Scrollable {
    pub fn style() -> fn(&Theme, scrollable::Status) -> scrollable::Style {
        |_, status| {
            let scroller_color = match status {
                scrollable::Status::Hovered { .. } => {
                    MediaServerTheme::ACCENT_BLUE_HOVER
                }
                _ => MediaServerTheme::ACCENT_BLUE,
            };

            scrollable::Style {
                container: container::Style::default(),
                vertical_rail: scrollable::Rail {
                    background: Some(Background::Color(
                        MediaServerTheme::CARD_BG,
                    )),
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
                    background: Some(Background::Color(
                        MediaServerTheme::CARD_BG,
                    )),
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

impl Default for TextInput {
    fn default() -> Self {
        Self
    }
}

impl TextInput {
    pub fn style() -> fn(&Theme, text_input::Status) -> text_input::Style {
        |_, status| {
            let (border_color, border_width) = match status {
                text_input::Status::Active => {
                    (MediaServerTheme::BORDER_COLOR, 1.0)
                }
                text_input::Status::Hovered => {
                    (MediaServerTheme::ACCENT_BLUE, 1.0)
                }
                text_input::Status::Focused { .. } => {
                    (MediaServerTheme::ACCENT_BLUE, 2.0)
                }
                text_input::Status::Disabled => {
                    (MediaServerTheme::BORDER_COLOR, 1.0)
                }
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

    pub fn header_search() -> fn(&Theme, text_input::Status) -> text_input::Style
    {
        |_, status| {
            let (background, text_color) = match status {
                text_input::Status::Active => (
                    Some(Background::Color(Color::from_rgba(
                        1.0, 1.0, 1.0, 0.02,
                    ))),
                    MediaServerTheme::TEXT_SECONDARY,
                ),
                text_input::Status::Hovered => (
                    Some(Background::Color(Color::from_rgba(
                        1.0, 1.0, 1.0, 0.05,
                    ))),
                    MediaServerTheme::TEXT_PRIMARY,
                ),
                text_input::Status::Focused { .. } => (
                    Some(Background::Color(Color::from_rgba(
                        1.0, 1.0, 1.0, 0.08,
                    ))),
                    MediaServerTheme::TEXT_PRIMARY,
                ),
                text_input::Status::Disabled => {
                    (None, MediaServerTheme::TEXT_DIMMED)
                }
            };

            text_input::Style {
                background: background
                    .unwrap_or(Background::Color(Color::TRANSPARENT)),
                border: Border {
                    color: Color::TRANSPARENT,
                    width: 0.0,
                    radius: 0.0.into(), // Sharp corners
                },
                icon: MediaServerTheme::TEXT_SECONDARY,
                placeholder: MediaServerTheme::TEXT_DIMMED,
                value: text_color,
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
