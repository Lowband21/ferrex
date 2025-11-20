// Common imports and re-exports for the ferrex-player application

// Iced GUI framework imports
pub use iced::{
    widget::{scrollable, stack, text},
    Font,
};

// Icons
pub use lucide_icons::Icon;

// Media and streaming

// Serialization

// Standard library

// Once cell for lazy statics

// Internal modules re-exports
pub use crate::state::ViewState;

// Helper functions that are commonly used
pub fn icon_char(icon: Icon) -> String {
    icon.unicode().to_string()
}

pub fn icon_text(icon: Icon) -> text::Text<'static> {
    text(icon.unicode()).font(lucide_font()).size(20)
}

// Font helper
pub fn lucide_font() -> Font {
    Font::with_name("lucide")
}
