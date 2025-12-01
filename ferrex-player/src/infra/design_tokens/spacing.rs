//! Spacing tokens for consistent margins, padding, and gaps
//!
//! Uses a semantic scale rather than arbitrary pixel values.

/// Semantic spacing tokens for margins, padding, and gaps
///
/// ## Token Scale
///
/// | Token | Base Size | Typical Usage                      |
/// |-------|-----------|-----------------------------------|
/// | `xs`  | 4px       | Micro spacing, tight groups       |
/// | `sm`  | 8px       | Small gaps, compact layouts       |
/// | `md`  | 16px      | Standard spacing, form fields     |
/// | `lg`  | 24px      | Large gaps, card padding          |
/// | `xl`  | 40px      | Section separation                |
/// | `xxl` | 64px      | Major section breaks              |
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpacingTokens {
    /// Micro spacing - 4px base
    pub xs: f32,
    /// Small spacing - 8px base
    pub sm: f32,
    /// Medium spacing - 16px base
    pub md: f32,
    /// Large spacing - 24px base
    pub lg: f32,
    /// Extra large - 40px base
    pub xl: f32,
    /// Section spacing - 64px base
    pub xxl: f32,
}

impl SpacingTokens {
    /// Base (unscaled) spacing values
    pub const BASE: Self = Self {
        xs: 4.0,
        sm: 8.0,
        md: 16.0,
        lg: 24.0,
        xl: 40.0,
        xxl: 64.0,
    };

    /// Create scaled spacing tokens
    pub fn scaled(scale: f32) -> Self {
        Self {
            xs: Self::BASE.xs * scale,
            sm: Self::BASE.sm * scale,
            md: Self::BASE.md * scale,
            lg: Self::BASE.lg * scale,
            xl: Self::BASE.xl * scale,
            xxl: Self::BASE.xxl * scale,
        }
    }

    /// Get spacing by semantic name (for dynamic access)
    pub fn get(&self, size: SpacingSize) -> f32 {
        match size {
            SpacingSize::Xs => self.xs,
            SpacingSize::Sm => self.sm,
            SpacingSize::Md => self.md,
            SpacingSize::Lg => self.lg,
            SpacingSize::Xl => self.xl,
            SpacingSize::Xxl => self.xxl,
        }
    }

    /// Get spacing as u16 (rounded, for Iced padding/spacing)
    pub fn get_u16(&self, size: SpacingSize) -> u16 {
        self.get(size).round() as u16
    }
}

impl Default for SpacingTokens {
    fn default() -> Self {
        Self::BASE
    }
}

/// Semantic spacing size names for dynamic access
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SpacingSize {
    Xs,
    Sm,
    Md,
    Lg,
    Xl,
    Xxl,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base_values() {
        assert!((SpacingTokens::BASE.xs - 4.0).abs() < 0.001);
        assert!((SpacingTokens::BASE.md - 16.0).abs() < 0.001);
        assert!((SpacingTokens::BASE.xxl - 64.0).abs() < 0.001);
    }

    #[test]
    fn test_scaling() {
        let scaled = SpacingTokens::scaled(1.5);
        assert!((scaled.xs - 6.0).abs() < 0.001); // 4 * 1.5
        assert!((scaled.md - 24.0).abs() < 0.001); // 16 * 1.5
    }

    #[test]
    fn test_dynamic_access() {
        let tokens = SpacingTokens::BASE;
        assert!((tokens.get(SpacingSize::Md) - 16.0).abs() < 0.001);
    }
}
