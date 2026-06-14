//! Common UI utility functions

use iced::Font;
use iced::widget::text;
pub use lucide_icons::Icon;

/// Helper function to create icon text with the default size (20px)
pub fn icon_text(icon: Icon) -> text::Text<'static> {
    icon_text_with_size(icon, 20.0)
}

/// Helper function to create icon text with a custom size
pub fn icon_text_with_size(icon: Icon, size: f32) -> text::Text<'static> {
    text(icon.unicode()).font(lucide_font()).size(size)
}

/// Get the lucide font
pub fn lucide_font() -> Font {
    Font::with_name("lucide")
}
