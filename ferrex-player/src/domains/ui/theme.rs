use iced::{
    Background, Border, Color, Shadow, Theme, Vector, theme,
    widget::{button, checkbox, container, scrollable, slider, text_input},
};

use crate::domains::ui::types::ViewState;
use crate::infra::theme::{accent, accent_glow, accent_hover};
use crate::state::State;

/// Pure black theme with high contrast electric blue accents
#[derive(Debug, Clone, Copy)]
pub struct MediaServerTheme;

impl MediaServerTheme {
    // Core colors
    pub const BLACK: Color = Color::from_rgb(0.0, 0.0, 0.0); // #000000
    pub const BACKGROUND_DARK: Color = Color::from_rgb(0.1, 0.1, 0.10);
    pub const BACKGROUND_ACCENT: Color = Color::from_rgb(0.01, 0.01, 0.01);
    pub const ACCENT: Color = Color::from_rgb(0.867, 0.0, 0.867);
    pub const ACCENT_HOVER: Color = Color::from_rgb(0.9, 0.0, 0.9);
    pub const ACCENT_GLOW: Color = Color::from_rgba(0.867, 0.5, 0.867, 0.3);

    // Grays
    pub const CARD_BG: Color = Color::from_rgb(0.1, 0.1, 0.1); // #1A1A1A
    pub const CARD_HOVER: Color = Color::from_rgb(0.15, 0.15, 0.15); // #262626
    pub const BORDER_COLOR: Color = Color::from_rgb(0.2, 0.2, 0.2); // #333333

    // Soft grays for backgrounds - with subtle blue tint
    pub const SOFT_GREY_DARK: Color = Color::from_rgb(0.01, 0.01, 0.02); // Much darker for contrast
    pub const SOFT_GREY_LIGHT: Color = Color::from_rgb(0.06, 0.05, 0.07); // Much lighter for visible gradient
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
        // Default to an opaque background to avoid app-wide transparency.
        palette.background = Self::BACKGROUND;
        palette.text = Self::TEXT_PRIMARY;
        palette.primary = Self::ACCENT;
        palette.success = Self::SUCCESS;
        palette.danger = Self::ERROR;

        Theme::custom("Ferrex Dark", palette)
    }

    /// Choose a theme based on application state and window.
    ///
    /// - Uses an opaque background everywhere by default.
    /// - Switches to a transparent background only for the main window
    ///   when showing the Player view with the Subwave Wayland backend
    ///   active (so the video subsurface can render behind the controls).
    pub fn theme_for_state(
        state: &State,
        window: Option<iced::window::Id>,
    ) -> Theme {
        let mut palette = theme::Palette::DARK;

        // Default to opaque background
        let mut use_transparent_bg = false;

        // Only consider transparency on the main window when actually
        // presenting the player view with a Wayland subsurface backend.
        if let Some(main_id) = state
            .windows
            .get(crate::domains::ui::windows::WindowKind::Main)
            && window.map(|w| w == main_id).unwrap_or(true)
            && matches!(state.domains.ui.state.view, ViewState::Player)
            && let Some(video) = state.domains.player.state.video_opt.as_ref()
        {
            // Only make the background transparent when using Wayland.
            // Treat any preference other than ForceAppsink on Wayland as Wayland-backed,
            // which includes PreferWayland.
            let pref = video.backend();
            if std::env::var("WAYLAND_DISPLAY").is_ok() {
                use_transparent_bg = !matches!(
                    pref,
                    subwave_unified::video::BackendPreference::ForceAppsink
                );
            } else {
                // On non-Wayland, only enable if explicitly forced to Wayland
                use_transparent_bg = matches!(
                    pref,
                    subwave_unified::video::BackendPreference::ForceWayland
                );
            }
        }

        palette.background = if use_transparent_bg {
            Color::TRANSPARENT
        } else {
            Self::BACKGROUND
        };
        palette.text = Self::TEXT_PRIMARY;
        palette.primary = Self::ACCENT;
        palette.success = Self::SUCCESS;
        palette.danger = Self::ERROR;

        Theme::custom("Ferrex Dark", palette)
    }
}

// Container styles using closures
#[derive(Debug)]
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
                    color: accent(),
                    width: 1.0,
                    radius: 8.0.into(),
                },
                shadow: Shadow {
                    color: accent_glow(),
                    offset: iced::Vector::new(0.0, 0.0),
                    blur_radius: 10.0,
                },
                snap: false,
            },
            Container::ProgressBar => |_| container::Style {
                text_color: None,
                background: Some(Background::Color(accent())),
                border: Border {
                    color: Color::TRANSPARENT,
                    width: 0.0,
                    radius: 2.0.into(),
                },
                shadow: Shadow {
                    color: accent_glow(),
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
#[derive(Debug)]
pub enum Button {
    Primary,
    Secondary,
    Destructive,
    MediaCard,
    Text,
    Icon,
    PlayOverlay,
    HeaderIcon,
    // Active tab in header: primary color but square corners
    HeaderTabActive,
    // Menu triggers in library header: square corners
    HeaderMenuPrimary,
    HeaderMenuSecondary,
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
                        accent(),
                        Shadow {
                            color: accent_glow(),
                            offset: iced::Vector::new(0.0, 2.0),
                            blur_radius: 8.0,
                        },
                    ),
                    button::Status::Hovered => (
                        accent_hover(),
                        Shadow {
                            color: accent_glow(),
                            offset: iced::Vector::new(0.0, 2.0),
                            blur_radius: 16.0,
                        },
                    ),
                    button::Status::Pressed => (
                        crate::infra::theme::darken(accent(), 0.2),
                        Shadow {
                            color: accent_glow(),
                            offset: iced::Vector::new(0.0, 2.0),
                            blur_radius: 8.0,
                        },
                    ),
                    _ => (accent(), Shadow::default()),
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
            Button::HeaderTabActive => |_, status| {
                let (background, shadow) = match status {
                    button::Status::Active => (
                        accent(),
                        Shadow {
                            color: accent_glow(),
                            offset: iced::Vector::new(0.0, 2.0),
                            blur_radius: 8.0,
                        },
                    ),
                    button::Status::Hovered => (
                        accent_hover(),
                        Shadow {
                            color: accent_glow(),
                            offset: iced::Vector::new(0.0, 2.0),
                            blur_radius: 16.0,
                        },
                    ),
                    button::Status::Pressed => (
                        crate::infra::theme::darken(accent(), 0.2),
                        Shadow {
                            color: accent_glow(),
                            offset: iced::Vector::new(0.0, 2.0),
                            blur_radius: 8.0,
                        },
                    ),
                    _ => (accent(), Shadow::default()),
                };

                button::Style {
                    text_color: MediaServerTheme::TEXT_PRIMARY,
                    background: Some(Background::Color(background)),
                    border: Border {
                        color: background,
                        width: 1.0,
                        radius: 0.0.into(),
                    },
                    shadow,
                    snap: false,
                }
            },
            Button::HeaderMenuPrimary => |_, status| {
                let (background, shadow) = match status {
                    button::Status::Active => (
                        accent(),
                        Shadow {
                            color: accent_glow(),
                            offset: iced::Vector::new(0.0, 2.0),
                            blur_radius: 8.0,
                        },
                    ),
                    button::Status::Hovered => (
                        accent_hover(),
                        Shadow {
                            color: accent_glow(),
                            offset: iced::Vector::new(0.0, 2.0),
                            blur_radius: 16.0,
                        },
                    ),
                    button::Status::Pressed => (
                        crate::infra::theme::darken(accent(), 0.2),
                        Shadow {
                            color: accent_glow(),
                            offset: iced::Vector::new(0.0, 2.0),
                            blur_radius: 8.0,
                        },
                    ),
                    _ => (accent(), Shadow::default()),
                };

                button::Style {
                    text_color: MediaServerTheme::TEXT_PRIMARY,
                    background: Some(Background::Color(background)),
                    border: Border {
                        color: Color::TRANSPARENT,
                        width: 1.0,
                        radius: 0.0.into(),
                    },
                    shadow,
                    snap: false,
                }
            },
            Button::HeaderMenuSecondary => |_, status| {
                let (background, border_color) = match status {
                    button::Status::Active => (
                        MediaServerTheme::CARD_BG,
                        MediaServerTheme::BORDER_COLOR,
                    ),
                    button::Status::Hovered => {
                        (MediaServerTheme::CARD_HOVER, accent())
                    }
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
                        radius: 0.0.into(),
                    },
                    shadow: Shadow::default(),
                    snap: false,
                }
            },
            Button::Secondary => |_, status| {
                let (background, border_color) = match status {
                    button::Status::Active => (
                        MediaServerTheme::CARD_BG,
                        MediaServerTheme::BORDER_COLOR,
                    ),
                    button::Status::Hovered => {
                        (MediaServerTheme::CARD_HOVER, accent())
                    }
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
                        Some(Background::Color(accent()))
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
                        Some(Background::Color(accent())),
                        Shadow {
                            color: accent_glow(),
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
                        accent(),
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
                        Some(Background::Color(
                            crate::infra::theme::with_alpha(accent(), 0.1),
                        )),
                        MediaServerTheme::TEXT_PRIMARY,
                    ),
                    button::Status::Hovered => (
                        Some(Background::Color(accent())),
                        MediaServerTheme::TEXT_PRIMARY,
                    ),
                    button::Status::Pressed => (
                        Some(Background::Color(crate::infra::theme::darken(
                            accent(),
                            0.2,
                        ))),
                        MediaServerTheme::TEXT_PRIMARY,
                    ),
                    _ => (
                        Some(Background::Color(
                            crate::infra::theme::with_alpha(accent(), 0.08),
                        )),
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
                        accent(),
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
#[derive(Debug)]
pub struct Scrollable;

impl Scrollable {
    pub fn style() -> fn(&Theme, scrollable::Status) -> scrollable::Style {
        |_, status| {
            let scroller_color = match status {
                scrollable::Status::Hovered { .. } => accent_hover(),
                _ => accent(),
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
                        background: Background::Color(scroller_color),
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
                        background: Background::Color(scroller_color),
                        border: Border {
                            color: Color::TRANSPARENT,
                            width: 0.0,
                            radius: 4.0.into(),
                        },
                    },
                },
                gap: None,
                auto_scroll: scrollable::AutoScroll {
                    background: Background::Color(MediaServerTheme::CARD_BG),
                    border: Border {
                        color: MediaServerTheme::BORDER_COLOR,
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    shadow: Shadow {
                        color: Color::BLACK,
                        offset: Vector::ZERO,
                        blur_radius: 2.0,
                    },
                    icon: MediaServerTheme::TEXT_PRIMARY,
                },
            }
        }
    }
}

// Slider style
#[derive(Debug)]
pub struct Slider;

impl Slider {
    pub fn style() -> fn(&Theme, slider::Status) -> slider::Style {
        |_, status| {
            let handle_color = match status {
                slider::Status::Active => accent(),
                slider::Status::Hovered => accent_hover(),
                slider::Status::Dragged => accent_hover(),
            };

            slider::Style {
                rail: slider::Rail {
                    backgrounds: (
                        Background::Color(MediaServerTheme::CARD_BG),
                        Background::Color(accent()),
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

// Checkbox style
#[derive(Debug)]
pub struct Checkbox;

impl Checkbox {
    pub fn style() -> fn(&Theme, checkbox::Status) -> checkbox::Style {
        |_, status| {
            let (is_checked, is_hovered, is_disabled) = match status {
                checkbox::Status::Active { is_checked } => {
                    (is_checked, false, false)
                }
                checkbox::Status::Hovered { is_checked } => {
                    (is_checked, true, false)
                }
                checkbox::Status::Disabled { is_checked } => {
                    (is_checked, false, true)
                }
            };

            let (background, border_color, icon_color, text_color) =
                if is_disabled {
                    let background = if is_checked {
                        crate::infra::theme::darken(accent(), 0.35)
                    } else {
                        MediaServerTheme::SURFACE_DIM
                    };

                    (
                        background,
                        MediaServerTheme::BORDER_COLOR,
                        MediaServerTheme::TEXT_DIMMED,
                        MediaServerTheme::TEXT_DIMMED,
                    )
                } else if is_checked {
                    let accent =
                        if is_hovered { accent_hover() } else { accent() };

                    (
                        accent,
                        accent,
                        MediaServerTheme::TEXT_PRIMARY,
                        MediaServerTheme::TEXT_PRIMARY,
                    )
                } else {
                    let background = if is_hovered {
                        MediaServerTheme::CARD_HOVER
                    } else {
                        MediaServerTheme::CARD_BG
                    };

                    let border_color = if is_hovered {
                        accent()
                    } else {
                        MediaServerTheme::BORDER_COLOR
                    };

                    (
                        background,
                        border_color,
                        MediaServerTheme::TEXT_PRIMARY,
                        MediaServerTheme::TEXT_SECONDARY,
                    )
                };

            checkbox::Style {
                background: Background::Color(background),
                icon_color,
                border: Border {
                    color: border_color,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                text_color: Some(text_color),
            }
        }
    }
}

// Text input style
#[derive(Debug)]
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
                text_input::Status::Hovered => (accent(), 1.0),
                text_input::Status::Focused { .. } => (accent(), 2.0),
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
                selection: accent(),
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
                selection: accent(),
            }
        }
    }
}

// Text styles
pub fn icon_white(_theme: &iced::Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(Color::WHITE),
    }
}
