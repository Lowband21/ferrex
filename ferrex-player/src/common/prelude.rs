// Common imports and re-exports for the ferrex-player application

// Iced GUI framework imports
pub use iced::{
    Font,
    widget::{scrollable, stack, text},
};

// Icons
pub use lucide_icons::Icon;

// Media and streaming

// Serialization

// Standard library

// Once cell for lazy statics

// Internal modules re-exports
pub use crate::domains::ui::types::ViewState;

// Helper functions that are commonly used
pub fn icon_char(icon: Icon) -> String {
    icon.unicode().to_string()
}

pub use crate::common::ui_utils::{
    icon_text, icon_text_with_size, lucide_font,
};
