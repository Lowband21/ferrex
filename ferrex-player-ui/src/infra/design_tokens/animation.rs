//! Animation duration tokens for consistent timing
//!
//! Animation durations scale at the square root rate to prevent
//! animations from becoming too fast at small scales or too slow at large scales.

/// Animation duration tokens
///
/// ## Scaling Behavior
///
/// Animation durations use square root scaling:
/// - At 0.5x scale: durations are ~0.7x (not 0.5x)
/// - At 2.0x scale: durations are ~1.4x (not 2.0x)
///
/// This prevents animations from feeling jarring at extreme scales.
///
/// ## Token Scale
///
/// | Token   | Base Duration | Typical Usage                   |
/// |---------|---------------|--------------------------------|
/// | `fast`  | 150ms         | Micro-interactions, hover      |
/// | `normal`| 300ms         | Standard transitions           |
/// | `slow`  | 600ms         | Emphasized animations          |
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnimationTokens {
    /// Fast/micro interactions - 150ms base
    pub fast: u64,
    /// Standard transitions - 300ms base
    pub normal: u64,
    /// Slow/emphasized - 600ms base
    pub slow: u64,
}

impl AnimationTokens {
    /// Base (unscaled) animation durations in milliseconds
    pub const BASE: Self = Self {
        fast: 150,
        normal: 300,
        slow: 600,
    };

    /// Minimum animation duration (prevents too-fast animations)
    pub const MIN_DURATION: u64 = 50;

    /// Maximum animation duration (prevents too-slow animations)
    pub const MAX_DURATION: u64 = 2000;

    /// Create scaled animation tokens
    ///
    /// Uses square root scaling to keep animations feeling natural
    /// across different UI scales.
    pub fn scaled(scale: f32) -> Self {
        let factor = scale.sqrt();
        Self {
            fast: Self::scale_duration(Self::BASE.fast, factor),
            normal: Self::scale_duration(Self::BASE.normal, factor),
            slow: Self::scale_duration(Self::BASE.slow, factor),
        }
    }

    /// Scale a single duration with min/max clamping
    #[inline]
    fn scale_duration(base: u64, factor: f32) -> u64 {
        let scaled = ((base as f32) * factor).round() as u64;
        scaled.clamp(Self::MIN_DURATION, Self::MAX_DURATION)
    }

    /// Get duration by semantic name
    pub fn get(&self, speed: AnimationSpeed) -> u64 {
        match speed {
            AnimationSpeed::Fast => self.fast,
            AnimationSpeed::Normal => self.normal,
            AnimationSpeed::Slow => self.slow,
        }
    }

    /// Get duration as f32 seconds (for some animation APIs)
    pub fn get_secs(&self, speed: AnimationSpeed) -> f32 {
        self.get(speed) as f32 / 1000.0
    }

    /// Get duration as std::time::Duration
    pub fn get_duration(&self, speed: AnimationSpeed) -> std::time::Duration {
        std::time::Duration::from_millis(self.get(speed))
    }
}

impl Default for AnimationTokens {
    fn default() -> Self {
        Self::BASE
    }
}

/// Semantic animation speed names
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AnimationSpeed {
    Fast,
    Normal,
    Slow,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base_values() {
        assert_eq!(AnimationTokens::BASE.fast, 150);
        assert_eq!(AnimationTokens::BASE.normal, 300);
        assert_eq!(AnimationTokens::BASE.slow, 600);
    }

    #[test]
    fn test_sqrt_scaling() {
        // At 4x scale, sqrt(4) = 2, so durations should double
        let scaled = AnimationTokens::scaled(4.0);
        assert_eq!(scaled.fast, 300); // 150 * 2
        assert_eq!(scaled.normal, 600); // 300 * 2
    }

    #[test]
    fn test_duration_clamping() {
        // Very small scale shouldn't go below MIN_DURATION
        let tiny = AnimationTokens::scaled(0.01);
        assert!(tiny.fast >= AnimationTokens::MIN_DURATION);

        // Very large scale shouldn't exceed MAX_DURATION
        let huge = AnimationTokens::scaled(100.0);
        assert!(huge.slow <= AnimationTokens::MAX_DURATION);
    }

    #[test]
    fn test_get_secs() {
        let tokens = AnimationTokens::BASE;
        assert!((tokens.get_secs(AnimationSpeed::Fast) - 0.15).abs() < 0.001);
    }
}
