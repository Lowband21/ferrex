//! Performance section messages

use super::state::EasingKind;

/// Messages for the performance settings section
#[derive(Debug, Clone)]
pub enum PerformanceMessage {
    // Scrolling subsection (String for UI-visible fields)
    SetScrollDebounceMs(String),
    SetScrollTickNs(u64),
    SetScrollDecayTauMs(String),
    SetScrollBaseVelocity(f32),
    SetScrollMaxVelocity(String),
    SetScrollMinStopVelocity(f32),
    SetScrollRampMs(u64),
    SetScrollBoostMultiplier(f32),
    SetScrollEasing(EasingKind),

    // Texture Upload subsection (legacy - now dynamically calculated based on framerate)
    SetTextureMaxUploadsPerFrame(u32),

    // Prefetch subsection
    SetPrefetchRowsAbove(usize),
    SetPrefetchRowsBelow(usize),
    SetPrefetchKeepAliveMs(u64),

    // Carousel subsection
    SetCarouselPrefetchItems(usize),
    SetCarouselBackgroundItems(usize),
    SetCarouselBaseVelocity(f32),
    SetCarouselMaxVelocity(f32),
    SetCarouselBoostMultiplier(f32),
    SetCarouselRampMs(u64),
    SetCarouselDecayTauMs(u64),
    SetCarouselItemSnapMs(u64),
    SetCarouselPageSnapMs(u64),
    SetCarouselHoldTapThresholdMs(u64),
    SetCarouselSnapEpsilon(f32),
    SetCarouselAnchorSettleMs(u64),

    // Animation Effects subsection
    SetAnimationHoverScale(f32),
    SetAnimationHoverTransitionMs(u64),
    SetAnimationHoverScaleDownDelayMs(u64),
}

impl PerformanceMessage {
    pub fn name(&self) -> &'static str {
        match self {
            Self::SetScrollDebounceMs(_) => "Performance::SetScrollDebounceMs",
            Self::SetScrollTickNs(_) => "Performance::SetScrollTickNs",
            Self::SetScrollDecayTauMs(_) => "Performance::SetScrollDecayTauMs",
            Self::SetScrollBaseVelocity(_) => {
                "Performance::SetScrollBaseVelocity"
            }
            Self::SetScrollMaxVelocity(_) => {
                "Performance::SetScrollMaxVelocity"
            }
            Self::SetScrollMinStopVelocity(_) => {
                "Performance::SetScrollMinStopVelocity"
            }
            Self::SetScrollRampMs(_) => "Performance::SetScrollRampMs",
            Self::SetScrollBoostMultiplier(_) => {
                "Performance::SetScrollBoostMultiplier"
            }
            Self::SetScrollEasing(_) => "Performance::SetScrollEasing",
            Self::SetTextureMaxUploadsPerFrame(_) => {
                "Performance::SetTextureMaxUploadsPerFrame"
            }
            Self::SetPrefetchRowsAbove(_) => {
                "Performance::SetPrefetchRowsAbove"
            }
            Self::SetPrefetchRowsBelow(_) => {
                "Performance::SetPrefetchRowsBelow"
            }
            Self::SetPrefetchKeepAliveMs(_) => {
                "Performance::SetPrefetchKeepAliveMs"
            }
            Self::SetCarouselPrefetchItems(_) => {
                "Performance::SetCarouselPrefetchItems"
            }
            Self::SetCarouselBackgroundItems(_) => {
                "Performance::SetCarouselBackgroundItems"
            }
            Self::SetCarouselBaseVelocity(_) => {
                "Performance::SetCarouselBaseVelocity"
            }
            Self::SetCarouselMaxVelocity(_) => {
                "Performance::SetCarouselMaxVelocity"
            }
            Self::SetCarouselBoostMultiplier(_) => {
                "Performance::SetCarouselBoostMultiplier"
            }
            Self::SetCarouselRampMs(_) => "Performance::SetCarouselRampMs",
            Self::SetCarouselDecayTauMs(_) => {
                "Performance::SetCarouselDecayTauMs"
            }
            Self::SetCarouselItemSnapMs(_) => {
                "Performance::SetCarouselItemSnapMs"
            }
            Self::SetCarouselPageSnapMs(_) => {
                "Performance::SetCarouselPageSnapMs"
            }
            Self::SetCarouselHoldTapThresholdMs(_) => {
                "Performance::SetCarouselHoldTapThresholdMs"
            }
            Self::SetCarouselSnapEpsilon(_) => {
                "Performance::SetCarouselSnapEpsilon"
            }
            Self::SetCarouselAnchorSettleMs(_) => {
                "Performance::SetCarouselAnchorSettleMs"
            }
            Self::SetAnimationHoverScale(_) => {
                "Performance::SetAnimationHoverScale"
            }
            Self::SetAnimationHoverTransitionMs(_) => {
                "Performance::SetAnimationHoverTransitionMs"
            }
            Self::SetAnimationHoverScaleDownDelayMs(_) => {
                "Performance::SetAnimationHoverScaleDownDelayMs"
            }
        }
    }
}
