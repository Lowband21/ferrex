pub mod update;

use crate::domains::ui::messages::UiMessage;
pub use update::update_library_ui;

use ferrex_core::player_prelude::{
    LibraryId, SortBy, UiDecade, UiGenre, UiResolution, UiWatchStatus,
};

#[derive(Clone)]
pub enum LibraryUiMessage {
    // Sorting
    SetSortBy(SortBy), // Change sort field
    ToggleSortOrder,   // Toggle ascending/descending
    ApplySortedPositions(LibraryId, Option<u64>, Vec<u32>), // Apply position indices with optional cache key
    ApplyFilteredPositions(LibraryId, u64, Vec<u32>), // Apply filtered indices with cache key (Phase 1)
    RequestFilteredPositions, // Trigger fetching filtered positions for active library
    // Filter panel interactions
    ToggleFilterPanel,          // Open/close the filter panel
    ToggleFilterGenre(UiGenre), // Toggle a genre chip
    SetFilterDecade(UiDecade),  // Set a decade
    ClearFilterDecade,          // Clear decade selection
    SetFilterResolution(UiResolution),
    SetFilterWatchStatus(UiWatchStatus),
    ApplyFilters, // Build spec from UI inputs and request filtered positions
    ClearFilters, // Clear UI inputs and reset filters
    SortedIndexFailed(String), // Report fetch failure
}

impl From<LibraryUiMessage> for UiMessage {
    fn from(msg: LibraryUiMessage) -> Self {
        UiMessage::Library(msg)
    }
}

impl LibraryUiMessage {
    pub fn name(&self) -> &'static str {
        match self {
            Self::SetSortBy(_) => "UI::SetSortBy",
            Self::ToggleSortOrder => "UI::ToggleSortOrder",
            Self::ApplySortedPositions(_, _, _) => "UI::ApplySortedPositions",
            Self::ApplyFilteredPositions(_, _, _) => {
                "UI::ApplyFilteredPositions"
            }
            Self::RequestFilteredPositions => "UI::RequestFilteredPositions",
            Self::ToggleFilterPanel => "UI::ToggleFilterPanel",
            Self::ToggleFilterGenre(_) => "UI::ToggleFilterGenre",
            Self::SetFilterDecade(_) => "UI::SetFilterDecade",
            Self::ClearFilterDecade => "UI::ClearFilterDecade",
            Self::SetFilterResolution(_) => "UI::SetFilterResolution",
            Self::SetFilterWatchStatus(_) => "UI::SetFilterWatchStatus",
            Self::ApplyFilters => "UI::ApplyFilters",
            Self::ClearFilters => "UI::ClearFilters",
            Self::SortedIndexFailed(_) => "UI::SortedIndexFailed",
        }
    }
}

impl std::fmt::Debug for LibraryUiMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SetSortBy(sort) => write!(f, "UI::SetSortBy({sort:?})"),
            Self::ToggleSortOrder => write!(f, "UI::ToggleSortOrder"),
            Self::ApplySortedPositions(_, cache_key, _) => match cache_key {
                Some(hash) => {
                    write!(f, "UI::ApplySortedPositions(hash={hash})")
                }
                None => write!(f, "UI::ApplySortedPositions"),
            },
            Self::ApplyFilteredPositions(_, hash, _) => {
                write!(f, "UI::ApplyFilteredPositions(hash={hash})")
            }
            Self::RequestFilteredPositions => {
                write!(f, "UI::RequestFilteredPositions")
            }
            Self::ToggleFilterPanel => write!(f, "UI::ToggleFilterPanel"),
            Self::ToggleFilterGenre(_) => write!(f, "UI::ToggleFilterGenre"),
            Self::SetFilterDecade(_) => write!(f, "UI::SetFilterDecade"),
            Self::ClearFilterDecade => write!(f, "UI::ClearFilterDecade"),
            Self::SetFilterResolution(_) => {
                write!(f, "UI::SetFilterResolution")
            }
            Self::SetFilterWatchStatus(_) => {
                write!(f, "UI::SetFilterWatchStatus")
            }
            Self::ApplyFilters => write!(f, "UI::ApplyFilters"),
            Self::ClearFilters => write!(f, "UI::ClearFilters"),
            Self::SortedIndexFailed(_) => write!(f, "UI::SortedIndexFailed"),
        }
    }
}
