use crate::domains::ui::views::virtual_carousel::types::CarouselKey;
use iced::Point;
use uuid::Uuid;

/// Uniquely identifies a poster widget instance in the UI.
/// A single media item can have multiple PosterInstanceKeys when displayed
/// in multiple carousels simultaneously (e.g., same movie in "Continue Watching"
/// and "Recently Added").
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PosterInstanceKey {
    /// The underlying media UUID (for operations like Play, Details)
    pub media_id: Uuid,
    /// Which carousel this instance belongs to (None for non-carousel contexts)
    pub carousel_key: Option<CarouselKey>,
}

impl PosterInstanceKey {
    pub fn new(media_id: Uuid, carousel_key: Option<CarouselKey>) -> Self {
        Self {
            media_id,
            carousel_key,
        }
    }

    /// Create a key for a poster not in a carousel (detail pages, grids, etc.)
    pub fn standalone(media_id: Uuid) -> Self {
        Self {
            media_id,
            carousel_key: None,
        }
    }
}

/// State for tracking mouse position within the shader widget
#[derive(Debug, Clone, Default)]
pub struct PosterState {
    /// Current mouse position relative to widget bounds
    pub mouse_position: Option<Point>,
    /// Whether mouse is over the widget
    pub is_hovered: bool,
    /// Whether the primary button was pressed inside this widget
    pub pressed_inside: bool,
    /// Whether the right button was pressed inside this widget
    pub right_pressed_inside: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PosterFace {
    Front,
    Back,
}

impl PosterFace {
    /// Get face opposite of current
    pub fn opposite(self) -> Self {
        match self {
            Self::Front => Self::Back,
            Self::Back => Self::Front,
        }
    }
}
