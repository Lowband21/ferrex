//! Font size tokens for consistent typography scaling
//!
//! Uses semantic names rather than pixel values to ensure consistent
//! usage across the application.

/// Semantic font size tokens
///
/// All sizes have an 8px floor for readability at extreme scales.
///
/// ## Token Mapping
///
/// | Token       | Base Size | Typical Usage                    |
/// |-------------|-----------|----------------------------------|
/// | `display`   | 32px      | Hero text, large view headings   |
/// | `title_lg`  | 28px      | Page titles, major headings      |
/// | `title`     | 24px      | Section titles, modal headers    |
/// | `subtitle`  | 20px      | Subtitles, nav items             |
/// | `body_lg`   | 18px      | Emphasized body text             |
/// | `body`      | 16px      | Default body text                |
/// | `caption`   | 14px      | Captions, labels, card text      |
/// | `small`     | 12px      | Small labels, metadata           |
/// | `micro`     | 10px      | Tiny metadata, timestamps        |
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FontTokens {
    /// Hero/display text (32px base)
    pub display: f32,
    /// Large titles (28px base)
    pub title_lg: f32,
    /// Section titles (24px base)
    pub title: f32,
    /// Subtitles, navigation (20px base)
    pub subtitle: f32,
    /// Emphasized body text (18px base)
    pub body_lg: f32,
    /// Default body text (16px base)
    pub body: f32,
    /// Captions, labels (14px base)
    pub caption: f32,
    /// Small labels, badges (12px base)
    pub small: f32,
    /// Tiny text, metadata (10px base)
    pub micro: f32,
}

impl FontTokens {
    /// Base (unscaled) font sizes
    pub const BASE: Self = Self {
        display: 32.0,
        title_lg: 28.0,
        title: 24.0,
        subtitle: 20.0,
        body_lg: 18.0,
        body: 16.0,
        caption: 14.0,
        small: 12.0,
        micro: 10.0,
    };

    /// Minimum font size floor (for readability)
    pub const MIN_SIZE: f32 = 8.0;

    /// Create scaled font tokens
    ///
    /// All values are scaled by the given factor, with a minimum of 8px
    /// to ensure readability.
    pub fn scaled(scale: f32) -> Self {
        Self {
            display: Self::scale_size(Self::BASE.display, scale),
            title_lg: Self::scale_size(Self::BASE.title_lg, scale),
            title: Self::scale_size(Self::BASE.title, scale),
            subtitle: Self::scale_size(Self::BASE.subtitle, scale),
            body_lg: Self::scale_size(Self::BASE.body_lg, scale),
            body: Self::scale_size(Self::BASE.body, scale),
            caption: Self::scale_size(Self::BASE.caption, scale),
            small: Self::scale_size(Self::BASE.small, scale),
            micro: Self::scale_size(Self::BASE.micro, scale),
        }
    }

    /// Scale a single font size with minimum floor
    #[inline]
    fn scale_size(base: f32, scale: f32) -> f32 {
        (base * scale).round().max(Self::MIN_SIZE)
    }

    /// Get a font size by semantic name (for dynamic access)
    pub fn get(&self, name: FontSize) -> f32 {
        match name {
            FontSize::Display => self.display,
            FontSize::TitleLg => self.title_lg,
            FontSize::Title => self.title,
            FontSize::Subtitle => self.subtitle,
            FontSize::BodyLg => self.body_lg,
            FontSize::Body => self.body,
            FontSize::Caption => self.caption,
            FontSize::Small => self.small,
            FontSize::Micro => self.micro,
        }
    }
}

impl Default for FontTokens {
    fn default() -> Self {
        Self::BASE
    }
}

/// Semantic font size names for dynamic access
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FontSize {
    Display,
    TitleLg,
    Title,
    Subtitle,
    BodyLg,
    Body,
    Caption,
    Small,
    Micro,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base_values() {
        assert_eq!(FontTokens::BASE.display, 32.0);
        assert_eq!(FontTokens::BASE.title_lg, 28.0);
        assert_eq!(FontTokens::BASE.body, 16.0);
        assert_eq!(FontTokens::BASE.caption, 14.0);
        assert_eq!(FontTokens::BASE.micro, 10.0);
    }

    #[test]
    fn test_scaling() {
        let scaled = FontTokens::scaled(2.0);
        assert_eq!(scaled.display, 64.0); // 32 * 2
        assert_eq!(scaled.body, 32.0); // 16 * 2
        assert_eq!(scaled.caption, 28.0); // 14 * 2
    }

    #[test]
    fn test_minimum_floor() {
        let scaled = FontTokens::scaled(0.5);
        // 10 * 0.5 = 5, but should be floored to 8
        assert_eq!(scaled.micro, 8.0);
    }

    #[test]
    fn test_dynamic_access() {
        let tokens = FontTokens::BASE;
        assert_eq!(tokens.get(FontSize::Body), 16.0);
        assert_eq!(tokens.get(FontSize::Display), 32.0);
        assert_eq!(tokens.get(FontSize::Caption), 14.0);
    }
}
