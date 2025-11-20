//! Core types for the media card system

use crate::domains::ui::messages::Message;
use std::time::Duration;

/// Predefined card sizes with associated dimensions
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CardSize {
    /// Small cards - 150x225 (thumbnail size)
    Small,
    /// Medium cards - 200x300 (standard poster)
    Medium,
    /// Large cards - 300x450 (detailed view)
    Large,
    /// Wide cards - 400x225 (episode/backdrop ratio)
    Wide,
    /// Custom size with specific dimensions
    Custom(f32, f32),
}

impl CardSize {
    /// Get the width and height for this card size
    pub fn dimensions(&self) -> (f32, f32) {
        match self {
            CardSize::Small => (150.0, 225.0),
            CardSize::Medium => (200.0, 300.0),
            CardSize::Large => (300.0, 450.0),
            CardSize::Wide => (400.0, 225.0),
            CardSize::Custom(w, h) => (*w, *h),
        }
    }

    /// Get the corner radius appropriate for this size
    pub fn radius(&self) -> f32 {
        match self {
            CardSize::Small => 4.0,
            CardSize::Medium => 4.0,
            CardSize::Large => 8.0,
            CardSize::Wide => 12.0,
            CardSize::Custom(w, _) => {
                // Scale radius based on width
                (w / 200.0 * 8.0).clamp(4.0, 16.0)
            }
        }
    }

    /// Get appropriate text sizes for this card size
    pub fn text_sizes(&self) -> (u32, u32) {
        match self {
            CardSize::Small => (12, 10),
            CardSize::Medium => (14, 12),
            CardSize::Large => (18, 14),
            CardSize::Wide => (16, 13),
            CardSize::Custom(w, _) => {
                let scale = (w / 200.0).clamp(0.75, 2.0);
                ((14.0 * scale) as u32, (12.0 * scale) as u32)
            }
        }
    }
}

/// Card loading and display states
#[derive(Debug, Clone)]
pub enum CardState {
    /// Initial loading state with optional progress
    Loading {
        /// Optional loading progress (0.0 - 1.0)
        progress: Option<f32>,
        /// Show shimmer animation
        shimmer: bool,
    },
    /// Successfully loaded with animation progress
    Loaded {
        /// Animation progress for entrance effects (0.0 - 1.0)
        animation_progress: f32,
        /// Time when loading completed (for staggered animations)
        loaded_at: std::time::Instant,
    },
    /// Error state with message
    Error(String),
    /// Placeholder state (no image available)
    Placeholder,
}

/// Media types supported by the card system
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MediaType {
    Movie,
    Series,
    Season,
    Episode,
}

impl MediaType {
    /// Get the default fallback icon/emoji for this media type
    pub fn default_icon(&self) -> &'static str {
        match self {
            MediaType::Movie => "🎬",
            MediaType::Series => "📺",
            MediaType::Season => "📺",
            MediaType::Episode => "🎞️",
        }
    }

    /// Get the hover icon name for this media type
    pub fn hover_icon(&self) -> lucide_icons::Icon {
        match self {
            MediaType::Movie => lucide_icons::Icon::Play,
            MediaType::Series => lucide_icons::Icon::Tv,
            MediaType::Season => lucide_icons::Icon::LayoutGrid,
            MediaType::Episode => lucide_icons::Icon::Play,
        }
    }
}

/// Animation configuration for cards
#[derive(Debug, Clone)]
pub struct AnimationConfig {
    /// Type of animation to use
    pub animation_type: AnimationType,
    /// Duration of the animation
    pub duration: Duration,
    /// Delay before animation starts (for staggered effects)
    pub delay: Duration,
    /// Easing function
    pub easing: EasingFunction,
}

impl Default for AnimationConfig {
    fn default() -> Self {
        Self {
            animation_type: AnimationType::Flip,
            duration: Duration::from_millis(600), // Increased for better visibility
            delay: Duration::ZERO,
            easing: EasingFunction::EaseOut,
        }
    }
}

impl AnimationConfig {
    /// Configuration for emphasising freshly added content with a single flip.
    pub fn flip_once() -> Self {
        Self {
            animation_type: AnimationType::Flip,
            ..Default::default()
        }
    }
}

/// Types of animations supported
#[derive(Debug, Clone, Copy)]
pub enum AnimationType {
    /// Simple fade in
    FadeIn,
    /// Flip animation (card flip)
    Flip,
    /// Slide in from direction
    SlideIn(Direction),
    /// Scale up from center
    ScaleIn,
    /// Combined fade and scale
    FadeScale,
}

/// Direction for slide animations
#[derive(Debug, Clone, Copy)]
pub enum Direction {
    Left,
    Right,
    Top,
    Bottom,
}

/// Easing functions for animations
#[derive(Debug, Clone, Copy)]
pub enum EasingFunction {
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
    /// Custom cubic bezier (x1, y1, x2, y2)
    CubicBezier(f32, f32, f32, f32),
}

impl EasingFunction {
    /// Apply easing to a progress value (0.0 - 1.0)
    pub fn apply(&self, t: f32) -> f32 {
        match self {
            EasingFunction::Linear => t,
            EasingFunction::EaseIn => t * t,
            EasingFunction::EaseOut => 1.0 - (1.0 - t) * (1.0 - t),
            EasingFunction::EaseInOut => {
                if t < 0.5 {
                    2.0 * t * t
                } else {
                    1.0 - 2.0 * (1.0 - t) * (1.0 - t)
                }
            }
            EasingFunction::CubicBezier(x1, y1, x2, y2) => {
                // Simplified cubic bezier approximation
                let t2 = t * t;
                let t3 = t2 * t;
                3.0 * (1.0 - t) * (1.0 - t) * t * y1 + 3.0 * (1.0 - t) * t2 * y2 + t3
            }
        }
    }
}

/// Image type for cards
#[derive(Debug, Clone, Copy)]
pub enum CardImageType {
    Poster,
    Thumbnail,
    Backdrop,
    Still,
}

/// Priority for lazy loading
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LoadPriority {
    Low = 0,
    Normal = 1,
    High = 2,
    Immediate = 3,
}

/// Button configuration for hover overlays
#[derive(Debug, Clone)]
pub struct OverlayButton {
    pub icon: lucide_icons::Icon,
    pub size: u16,
    pub position: ButtonPosition,
    pub action: Message,
}

/// Position for overlay buttons
#[derive(Debug, Clone, Copy)]
pub enum ButtonPosition {
    TopLeft,
    TopRight,
    Center,
    BottomLeft,
    BottomRight,
}

/// Badge configuration for cards
#[derive(Debug, Clone)]
pub struct CardBadge {
    pub content: String,
    pub position: BadgePosition,
    pub style: BadgeStyle,
}

/// Position for card badges
#[derive(Debug, Clone, Copy)]
pub enum BadgePosition {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// Style for card badges
#[derive(Debug, Clone, Copy)]
pub enum BadgeStyle {
    Default,
    Rating,
    New,
    Custom {
        bg_color: iced::Color,
        text_color: iced::Color,
    },
}
