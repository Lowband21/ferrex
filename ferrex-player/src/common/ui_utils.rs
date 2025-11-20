//! Common UI utility functions

use iced::widget::text;
use iced::Font;
pub use lucide_icons::Icon;

/// Helper function to create icon text
pub fn icon_text(icon: Icon) -> text::Text<'static> {
    text(icon.unicode()).font(lucide_font()).size(20)
}

/// Get the lucide font
pub fn lucide_font() -> Font {
    Font::with_name("lucide")
}
