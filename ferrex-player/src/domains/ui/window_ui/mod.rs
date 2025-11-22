pub mod update;

use crate::domains::ui::messages::UiMessage;
use iced::{Point, Size};

pub use update::update_window_ui;

#[derive(Clone)]
pub enum WindowUiMessage {
    WindowResized(Size),
    WindowMoved(Option<Point>),
}

impl From<WindowUiMessage> for UiMessage {
    fn from(msg: WindowUiMessage) -> Self {
        UiMessage::Window(msg)
    }
}

impl WindowUiMessage {
    pub fn name(&self) -> &'static str {
        match self {
            Self::WindowResized(_) => "UI::WindowResized",
            Self::WindowMoved(_) => "UI::WindowMoved",
        }
    }
}

impl std::fmt::Debug for WindowUiMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WindowResized(size) => {
                write!(f, "UI::WindowResized({:?})", size)
            }
            Self::WindowMoved(position) => {
                write!(f, "UI::WindowMoved({:?})", position)
            }
        }
    }
}
