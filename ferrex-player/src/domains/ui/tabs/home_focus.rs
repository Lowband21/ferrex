use iced::widget::Id;

use crate::domains::ui::tabs::{TabId, TabState};
use crate::domains::ui::views::virtual_carousel::animator::SnapAnimator;
use crate::domains::ui::views::virtual_carousel::types::CarouselKey;
use crate::infra::api_types::LibraryType;
use crate::infra::constants::virtual_carousel::layout as vcl;
use crate::state::State;

/// Focus and vertical snap state for the Home view
#[derive(Debug, Clone)]
pub struct HomeFocusState {
    pub scrollable_id: Id,
    pub active_carousel: Option<CarouselKey>,
    pub ordered_keys: Vec<CarouselKey>,
    pub vertical_animator: SnapAnimator,
    pub viewport_height: f32,
    pub scroll_y: f32,
}

impl Default for HomeFocusState {
    fn default() -> Self {
        Self {
            scrollable_id: Id::unique(),
            active_carousel: None,
            ordered_keys: Vec::new(),
            vertical_animator: SnapAnimator::new(),
            viewport_height: 0.0,
            scroll_y: 0.0,
        }
    }
}

impl HomeFocusState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Rebuild the display order of all carousels shown in Home view
    pub fn rebuild_ordered_keys(&mut self, state: &State) {
        self.ordered_keys.clear();

        // Curated sections first (only include non-empty lists)
        if let Some(tab) = state.tab_manager.get_tab(TabId::Home)
            && let TabState::Home(home) = tab
        {
            if !home.continue_watching.is_empty() {
                self.ordered_keys
                    .push(CarouselKey::Custom("ContinueWatching"));
            }
            if !home.recent_movies.is_empty() {
                self.ordered_keys
                    .push(CarouselKey::Custom("RecentlyAddedMovies"));
            }
            if !home.recent_series.is_empty() {
                self.ordered_keys
                    .push(CarouselKey::Custom("RecentlyAddedSeries"));
            }
            if !home.released_movies.is_empty() {
                self.ordered_keys
                    .push(CarouselKey::Custom("RecentlyReleasedMovies"));
            }
            if !home.released_series.is_empty() {
                self.ordered_keys
                    .push(CarouselKey::Custom("RecentlyReleasedSeries"));
            }
        }

        // Then one section per library (Movies / Series)
        for (lib_id, lib_type) in state.tab_manager.library_info() {
            match lib_type {
                LibraryType::Movies => {
                    self.ordered_keys
                        .push(CarouselKey::LibraryMovies(lib_id.to_uuid()));
                }
                LibraryType::Series => {
                    self.ordered_keys
                        .push(CarouselKey::LibrarySeries(lib_id.to_uuid()));
                }
            }
        }

        // Initialize focus if needed
        if self.active_carousel.is_none() {
            self.active_carousel = self.ordered_keys.first().cloned();
        }
    }

    pub fn next_key(&self) -> Option<CarouselKey> {
        let Some(active) = &self.active_carousel else {
            return self.ordered_keys.first().cloned();
        };
        let idx = self.ordered_keys.iter().position(|k| k == active)?;
        if idx + 1 < self.ordered_keys.len() {
            Some(self.ordered_keys[idx + 1].clone())
        } else {
            // Clamp (no wrap)
            Some(self.ordered_keys[idx].clone())
        }
    }

    pub fn prev_key(&self) -> Option<CarouselKey> {
        let Some(active) = &self.active_carousel else {
            return self.ordered_keys.first().cloned();
        };
        let idx = self.ordered_keys.iter().position(|k| k == active)?;
        if idx > 0 {
            Some(self.ordered_keys[idx - 1].clone())
        } else {
            // Clamp (no wrap)
            Some(self.ordered_keys[0].clone())
        }
    }

    /// Compute the approximate top Y position (in px) of the section for the given
    /// key within the Home view's scrollable content, based on known layout constants
    /// and the section index in `ordered_keys`.
    pub fn section_top_y(&self, key: &CarouselKey) -> Option<f32> {
        let idx = self.ordered_keys.iter().position(|k| k == key)? as f32;
        let padding_top = 20.0; // view_all_content uses .padding(20)
        let section_height = vcl::HEADER_HEIGHT_EST
            + vcl::HEADER_SCROLL_SPACING
            + vcl::SCROLL_HEIGHT
            + vcl::SECTION_GAP;
        Some(padding_top + idx * section_height)
    }
}

/// Compute the ordered list of carousel keys shown in the Home view without
/// borrowing the Home tab mutably. This avoids borrow checker conflicts when
/// assigning into `HomeFocusState` later.
pub fn ordered_keys_for_home(state: &State) -> Vec<CarouselKey> {
    let mut keys: Vec<CarouselKey> = Vec::new();

    if let Some(TabState::Home(all)) = state.tab_manager.get_tab(TabId::Home) {
        if !all.continue_watching.is_empty() {
            keys.push(CarouselKey::Custom("ContinueWatching"));
        }
        if !all.recent_movies.is_empty() {
            keys.push(CarouselKey::Custom("RecentlyAddedMovies"));
        }
        if !all.recent_series.is_empty() {
            keys.push(CarouselKey::Custom("RecentlyAddedSeries"));
        }
        if !all.released_movies.is_empty() {
            keys.push(CarouselKey::Custom("RecentlyReleasedMovies"));
        }
        if !all.released_series.is_empty() {
            keys.push(CarouselKey::Custom("RecentlyReleasedSeries"));
        }
    }

    // Libraries
    for (lib_id, lib_type) in state.tab_manager.library_info() {
        match lib_type {
            LibraryType::Movies => {
                keys.push(CarouselKey::LibraryMovies(lib_id.to_uuid()))
            }
            LibraryType::Series => {
                keys.push(CarouselKey::LibrarySeries(lib_id.to_uuid()))
            }
        }
    }

    keys
}
