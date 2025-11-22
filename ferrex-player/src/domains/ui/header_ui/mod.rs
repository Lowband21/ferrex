pub mod update;

use crate::domains::ui::messages::UiMessage;

pub use update::update_header_ui;

#[derive(Clone)]
pub enum HeaderMessage {
    ShowLibraryMenu,
    ShowAllLibrariesMenu,
}

impl From<HeaderMessage> for UiMessage {
    fn from(msg: HeaderMessage) -> Self {
        UiMessage::Header(msg)
    }
}

impl HeaderMessage {
    pub fn name(&self) -> &'static str {
        match self {
            Self::ShowLibraryMenu => "UI::ShowLibraryMenu",
            Self::ShowAllLibrariesMenu => "UI::ShowAllLibrariesMenu",
        }
    }
}

impl std::fmt::Debug for HeaderMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ShowLibraryMenu => write!(f, "UI::ShowLibraryMenu"),
            Self::ShowAllLibrariesMenu => write!(f, "UI::ShowAllLibrariesMenu"),
        }
    }
}
