//! Scroll position manager for maintaining scroll states across the application
//!
//! This module provides a centralized way to store and restore scroll positions
//! for different views, ensuring a smooth user experience when navigating.

use std::collections::HashMap;

/// Identifies a scrollable view in the application
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub enum ScrollableView {
    /// Main library view - all media
    LibraryAll,
    /// Movies grid view
    MoviesGrid,
    /// TV shows grid view
    TvShowsGrid,
    /// TV show details view
    TvShowDetails(String), // show_id
    /// Season details view
    SeasonDetails(String, String), // show_id, season_id
    /// Search results
    SearchResults(String), // search_query
    /// Custom view identifier
    Custom(String),
}

/// Manages scroll positions across the application
#[derive(Debug, Default)]
pub struct ScrollPositionManager {
    /// Stored scroll positions for each view
    positions: HashMap<ScrollableView, f32>,
}

impl ScrollPositionManager {
    /// Create a new scroll position manager
    pub fn new() -> Self {
        Self {
            positions: HashMap::new(),
        }
    }

    /// Save the scroll position for a view
    pub fn save_position(&mut self, view: ScrollableView, position: f32) {
        // Only save non-zero positions to avoid overwriting with initial values
        if position > 0.0 {
            self.positions.insert(view, position);
        }
    }

    /// Get the saved scroll position for a view
    pub fn get_position(&self, view: &ScrollableView) -> Option<f32> {
        self.positions.get(view).copied()
    }

    /// Clear the scroll position for a view
    pub fn clear_position(&mut self, view: &ScrollableView) {
        self.positions.remove(view);
    }

    /// Clear all saved positions
    pub fn clear_all(&mut self) {
        self.positions.clear();
    }

    /// Get the number of saved positions
    pub fn count(&self) -> usize {
        self.positions.len()
    }

    /// Check if we have a saved position for a view
    pub fn has_position(&self, view: &ScrollableView) -> bool {
        self.positions.contains_key(view)
    }
}

/// Extension trait for easy scroll position management
pub trait ScrollPositionExt {
    fn save_scroll(&mut self, view: ScrollableView, position: f32);
    fn restore_scroll(&self, view: &ScrollableView) -> Option<f32>;
}

impl ScrollPositionExt for crate::state::State {
    fn save_scroll(&mut self, view: ScrollableView, position: f32) {
        self.scroll_manager.save_position(view, position);
    }

    fn restore_scroll(&self, view: &ScrollableView) -> Option<f32> {
        self.scroll_manager.get_position(view)
    }
}
