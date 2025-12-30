//! Display section messages

use ferrex_model::PosterSize;

use super::state::{GridSize, ThemePreference};

/// Messages for the display settings section
#[derive(Debug, Clone)]
pub enum DisplayMessage {
    // Theme subsection
    /// Set theme preference
    SetTheme(ThemePreference),

    // Grid Layout subsection
    /// Set grid size preset
    SetGridSize(GridSize),
    /// Toggle poster titles on hover
    SetPosterTitlesOnHover(bool),
    /// Toggle show recently watched
    SetShowRecentlyWatched(bool),
    /// Toggle show continue watching
    SetShowContinueWatching(bool),
    /// Toggle sidebar collapsed by default
    SetSidebarCollapsed(bool),

    // Poster subsection (String for validation in domain handler)
    /// Set poster base width
    SetPosterBaseWidth(String),
    /// Set poster base height
    SetPosterBaseHeight(String),
    /// Set poster corner radius
    SetPosterCornerRadius(String),
    /// Set poster text area height
    SetPosterTextAreaHeight(f32),

    // Spacing subsection (String for validation in domain handler)
    /// Set grid poster gap
    SetGridPosterGap(String),
    /// Set grid row spacing
    SetGridRowSpacing(String),
    /// Set grid viewport padding
    SetGridViewportPadding(f32),
    /// Set grid top padding
    SetGridTopPadding(f32),
    /// Set grid bottom padding
    SetGridBottomPadding(f32),

    // Animation subsection (String for validation in domain handler)
    /// Set hover scale factor
    SetAnimationHoverScale(String),
    /// Set default animation duration
    SetAnimationDefaultDuration(String),
    /// Set initial texture fade duration
    SetAnimationTextureFadeInitial(u64),
    /// Set texture fade duration
    SetAnimationTextureFade(u64),

    // Poster Quality subsection
    /// Set poster quality for library/carousel views
    SetLibraryPosterQuality(PosterSize),
    /// Set poster quality for detail views
    SetDetailPosterQuality(PosterSize),

    // Scrollbar subsection (String for validation in domain handler)
    /// Set minimum scroller length in pixels
    SetScrollbarScrollerMinLength(String),
}

impl DisplayMessage {
    pub fn name(&self) -> &'static str {
        match self {
            Self::SetTheme(_) => "Display::SetTheme",
            Self::SetGridSize(_) => "Display::SetGridSize",
            Self::SetPosterTitlesOnHover(_) => {
                "Display::SetPosterTitlesOnHover"
            }
            Self::SetShowRecentlyWatched(_) => {
                "Display::SetShowRecentlyWatched"
            }
            Self::SetShowContinueWatching(_) => {
                "Display::SetShowContinueWatching"
            }
            Self::SetSidebarCollapsed(_) => "Display::SetSidebarCollapsed",
            Self::SetPosterBaseWidth(_) => "Display::SetPosterBaseWidth",
            Self::SetPosterBaseHeight(_) => "Display::SetPosterBaseHeight",
            Self::SetPosterCornerRadius(_) => "Display::SetPosterCornerRadius",
            Self::SetPosterTextAreaHeight(_) => {
                "Display::SetPosterTextAreaHeight"
            }
            Self::SetGridPosterGap(_) => "Display::SetGridPosterGap",
            Self::SetGridRowSpacing(_) => "Display::SetGridRowSpacing",
            Self::SetGridViewportPadding(_) => {
                "Display::SetGridViewportPadding"
            }
            Self::SetGridTopPadding(_) => "Display::SetGridTopPadding",
            Self::SetGridBottomPadding(_) => "Display::SetGridBottomPadding",
            Self::SetAnimationHoverScale(_) => {
                "Display::SetAnimationHoverScale"
            }
            Self::SetAnimationDefaultDuration(_) => {
                "Display::SetAnimationDefaultDuration"
            }
            Self::SetAnimationTextureFadeInitial(_) => {
                "Display::SetAnimationTextureFadeInitial"
            }
            Self::SetAnimationTextureFade(_) => {
                "Display::SetAnimationTextureFade"
            }
            Self::SetLibraryPosterQuality(_) => {
                "Display::SetLibraryPosterQuality"
            }
            Self::SetDetailPosterQuality(_) => {
                "Display::SetDetailPosterQuality"
            }
            Self::SetScrollbarScrollerMinLength(_) => {
                "Display::SetScrollbarScrollerMinLength"
            }
        }
    }
}
