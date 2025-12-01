//! Icon size tokens for consistent icon scaling
//!
//! This is an extension point - icon sizes can be scaled along with the UI.

/// Icon size tokens
///
/// ## Token Scale
///
/// | Token | Base Size | Typical Usage                    |
/// |-------|-----------|----------------------------------|
/// | `sm`  | 16px      | Inline icons, button icons       |
/// | `md`  | 24px      | Standard icons, navigation       |
/// | `lg`  | 32px      | Prominent icons, empty states    |
/// | `xl`  | 48px      | Hero icons, feature highlights   |
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IconTokens {
    /// Small icons - 16px base
    pub sm: f32,
    /// Medium icons - 24px base
    pub md: f32,
    /// Large icons - 32px base
    pub lg: f32,
    /// Extra large/hero icons - 48px base
    pub xl: f32,
}

impl IconTokens {
    /// Base (unscaled) icon sizes
    pub const BASE: Self = Self {
        sm: 16.0,
        md: 24.0,
        lg: 32.0,
        xl: 48.0,
    };

    /// Create scaled icon tokens
    pub fn scaled(scale: f32) -> Self {
        Self {
            sm: Self::BASE.sm * scale,
            md: Self::BASE.md * scale,
            lg: Self::BASE.lg * scale,
            xl: Self::BASE.xl * scale,
        }
    }

    /// Get icon size by semantic name
    pub fn get(&self, size: IconSize) -> f32 {
        match size {
            IconSize::Sm => self.sm,
            IconSize::Md => self.md,
            IconSize::Lg => self.lg,
            IconSize::Xl => self.xl,
        }
    }

    /// Get icon size as u16 (for Iced Length::Fixed)
    pub fn get_u16(&self, size: IconSize) -> u16 {
        self.get(size).round() as u16
    }
}

impl Default for IconTokens {
    fn default() -> Self {
        Self::BASE
    }
}

/// Semantic icon size names
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IconSize {
    Sm,
    Md,
    Lg,
    Xl,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base_values() {
        assert!((IconTokens::BASE.sm - 16.0).abs() < 0.001);
        assert!((IconTokens::BASE.md - 24.0).abs() < 0.001);
    }

    #[test]
    fn test_scaling() {
        let scaled = IconTokens::scaled(2.0);
        assert!((scaled.sm - 32.0).abs() < 0.001);
        assert!((scaled.md - 48.0).abs() < 0.001);
    }
}
