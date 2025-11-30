use iced::Point;

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
