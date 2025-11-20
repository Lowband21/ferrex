//! Common UI types moved from monolithic state

use crate::infrastructure::api_types::MovieReference;
use ferrex_core::{EpisodeID, SeasonID, SeriesID};

/// View state representing which screen/page is currently shown
#[derive(Debug, Clone, Default)]
pub enum ViewState {
    #[default]
    Library,
    LibraryManagement, // New view for library management
    AdminDashboard,    // New comprehensive admin dashboard
    FirstRunSetup,     // First-run admin setup view
    Player,
    LoadingVideo {
        url: String,
    },
    VideoError {
        message: String,
    },
    MovieDetail {
        movie: MovieReference, // Store full reference for efficient access
        backdrop_handle: Option<iced::widget::image::Handle>, // Cached backdrop handle
    },
    TvShowDetail {
        series_id: SeriesID, // Keep as string for now, will convert to SeriesID later
        backdrop_handle: Option<iced::widget::image::Handle>, // Cached backdrop handle
    },
    SeasonDetail {
        series_id: SeriesID,
        season_id: SeasonID,
        backdrop_handle: Option<iced::widget::image::Handle>, // Cached backdrop handle
    },
    EpisodeDetail {
        episode_id: EpisodeID,                                // Keep as string for now
        backdrop_handle: Option<iced::widget::image::Handle>, // Cached backdrop handle
    },
    UserSettings, // User settings and preferences view
}

impl ViewState {
    /// Returns true if this view should show the main header
    pub fn has_header(&self) -> bool {
        matches!(
            self,
            ViewState::Library
                | ViewState::LibraryManagement
                | ViewState::AdminDashboard
                | ViewState::UserSettings
                | ViewState::MovieDetail { .. }
                | ViewState::TvShowDetail { .. }
                | ViewState::SeasonDetail { .. }
                | ViewState::EpisodeDetail { .. }
        )
        // FirstRunSetup has no header
    }

    /// Returns true if this view should show the background shader
    pub fn has_background(&self) -> bool {
        !matches!(
            self,
            ViewState::Player | ViewState::LoadingVideo { .. } | ViewState::FirstRunSetup
        )
    }

    /// Returns header height in pixels if this view has a header
    pub fn header_height(&self) -> Option<f32> {
        if self.has_header() {
            match self {
                ViewState::Library => {
                    Some(crate::infrastructure::constants::layout::header::HEIGHT)
                } // Main library header
                ViewState::MovieDetail { .. }
                | ViewState::TvShowDetail { .. }
                | ViewState::SeasonDetail { .. }
                | ViewState::EpisodeDetail { .. } => {
                    Some(crate::infrastructure::constants::layout::header::HEIGHT)
                } // Same header height
                ViewState::LibraryManagement
                | ViewState::AdminDashboard
                | ViewState::UserSettings => {
                    Some(crate::infrastructure::constants::layout::header::HEIGHT)
                } // Same header height
                _ => None,
            }
        } else {
            None
        }
    }

    /// Returns layout regions for background shader effects
    /// (For future use with shadows and visual divisions)
    pub fn layout_regions(&self) -> LayoutRegions {
        LayoutRegions {
            header_height: self.header_height(),
            has_sidebar: matches!(self, ViewState::AdminDashboard),
            content_padding: match self {
                ViewState::Library => 0.0, // No padding, grid goes edge to edge
                ViewState::Player => 0.0,
                _ => 20.0, // Standard content padding
            },
        }
    }
}

/// Layout information for background shader effects
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub struct LayoutRegions {
    pub header_height: Option<f32>,
    pub has_sidebar: bool,
    pub content_padding: f32,
}



/// Display mode for library-centric content organization
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum DisplayMode {
    /// Show curated collections in carousels (all libraries)
    #[default]
    Curated,
    /// Show content from current selected library
    Library,
    /// Show recommended content (future feature)
    Recommended,
    /// Show recently added content across all libraries
    RecentlyAdded,
    /// Show continue watching content
    ContinueWatching,
}

/// Sort criteria for media content
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortBy {
    #[default]
    DateAdded,
    Title,
    Year,
    Rating,
    Runtime,
    FileSize,
    Resolution,
    LastWatched,
    Genre,
    Popularity,
}

/// Sort order direction
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum SortOrder {
    #[default]
    Descending,
    Ascending,
}

/// Backdrop aspect ratio mode
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BackdropAspectMode {
    /// Automatically select aspect ratio based on window dimensions
    Auto,
    /// Force 21:9 aspect ratio regardless of window dimensions
    Force21x9,
}

impl Default for BackdropAspectMode {
    fn default() -> Self {
        Self::Auto
    }
}
