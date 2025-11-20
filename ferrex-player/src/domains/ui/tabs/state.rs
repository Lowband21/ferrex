//! Tab state definitions for independent tab management

use std::sync::{Arc, RwLock};
use uuid::Uuid;

use crate::domains::media::store::MediaStore;
use crate::domains::ui::view_models::AllViewModel;
use crate::domains::ui::views::grid::virtual_list::VirtualGridState;
use crate::infrastructure::api_types::{
    Library, LibraryType, MediaReference, MovieReference, SeriesReference,
};

/// State for an individual tab
#[derive(Debug)]
pub enum TabState {
    /// State for the "All" tab showing curated content
    All(AllTabState),

    /// State for a library-specific tab
    Library(LibraryTabState),
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::all_functions
)]
impl TabState {
    /// Create a new All tab state
    pub fn new_all(store: Arc<RwLock<MediaStore>>) -> Self {
        TabState::All(AllTabState::new(store))
    }

    /// Create a new Library tab state
    pub fn new_library(
        library_id: Uuid,
        library_type: LibraryType,
        store: Arc<RwLock<MediaStore>>,
    ) -> Self {
        TabState::Library(LibraryTabState::new(library_id, library_type, store))
    }

    /// Get the grid state if this is a library tab
    pub fn grid_state(&self) -> Option<&VirtualGridState> {
        match self {
            TabState::Library(state) => Some(&state.grid_state),
            TabState::All(_) => None,
        }
    }

    /// Get mutable grid state if this is a library tab
    pub fn grid_state_mut(&mut self) -> Option<&mut VirtualGridState> {
        match self {
            TabState::Library(state) => Some(&mut state.grid_state),
            TabState::All(_) => None,
        }
    }

    /// Get the currently visible media items for this tab
    pub fn get_visible_items(&self) -> Vec<MediaReference> {
        match self {
            TabState::Library(state) => state.get_visible_items(),
            TabState::All(_) => {
                // All tab uses carousel view, not virtual grid
                // Return empty for now - could be extended to return carousel visible items
                Vec::new()
            }
        }
    }
}

/// State for the "All" tab showing curated content
#[derive(Debug)]
pub struct AllTabState {
    /// The All view model (existing implementation)
    pub view_model: AllViewModel,

    /// Navigation history specific to this tab
    pub navigation_history: Vec<String>,
}

impl AllTabState {
    /// Create a new All tab state
    pub fn new(store: Arc<RwLock<MediaStore>>) -> Self {
        Self {
            view_model: AllViewModel::new(store),
            navigation_history: Vec::new(),
        }
    }
}

/// State for a library-specific tab
#[derive(Debug)]
pub struct LibraryTabState {
    /// The library ID this tab represents
    pub library_id: Uuid,

    /// The type of library (Movies or TvShows)
    pub library_type: LibraryType,

    /// Virtual grid state for this specific tab
    pub grid_state: VirtualGridState,

    /// Cached media items for this library
    /// This is an enum to support both movie and TV libraries
    pub cached_media: CachedMedia,

    /// Whether this tab needs refresh
    pub needs_refresh: bool,

    /// Navigation history specific to this tab
    pub navigation_history: Vec<String>,

    /// Reference to the media store for data access
    store: Arc<RwLock<MediaStore>>,
}

/// Cached media items for a library tab
#[derive(Debug)]
pub enum CachedMedia {
    /// Cached movies for a movie library
    Movies(Vec<MovieReference>),

    /// Cached TV series for a TV library
    TvShows(Vec<SeriesReference>),
}

impl CachedMedia {
    /// Get the count of cached items
    pub fn len(&self) -> usize {
        match self {
            CachedMedia::Movies(movies) => movies.len(),
            CachedMedia::TvShows(shows) => shows.len(),
        }
    }

    /// Check if cache is empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl LibraryTabState {
    /// Create a new library tab state
    pub fn new(
        library_id: Uuid,
        library_type: LibraryType,
        store: Arc<RwLock<MediaStore>>,
    ) -> Self {
        // Initialize with appropriate cached media type
        let cached_media = match library_type {
            LibraryType::Movies => CachedMedia::Movies(Vec::new()),
            LibraryType::TvShows => CachedMedia::TvShows(Vec::new()),
        };

        // Create grid state with deterministic scrollable_id based on library ID
        let scrollable_id = iced::widget::scrollable::Id::new(format!("library-{}", library_id));
        let grid_state = VirtualGridState::with_id(
            0, // Will be updated when content loads
            5, // Default columns
            crate::infrastructure::constants::virtual_grid::ROW_HEIGHT,
            scrollable_id,
        );

        let mut state = Self {
            library_id,
            library_type,
            grid_state,
            cached_media,
            needs_refresh: true,
            navigation_history: Vec::new(),
            store,
        };

        // Initial refresh from store
        state.refresh_from_store();

        state
    }

    /// Refresh cached media from the store
    pub fn refresh_from_store(&mut self) {
        if !self.needs_refresh {
            return;
        }

        let store = self.store.read().unwrap();

        match self.library_type {
            LibraryType::Movies => {
                // Get movies filtered by library - MediaStore handles the filtering
                let movies: Vec<MovieReference> = store
                    .get_movies(Some(self.library_id))
                    .into_iter()
                    .cloned()
                    .collect();

                // Movies are already sorted by MediaStore, no need to sort again

                // Update grid state
                self.grid_state.total_items = movies.len();

                // Update cache
                self.cached_media = CachedMedia::Movies(movies);
            }
            LibraryType::TvShows => {
                // Get TV shows filtered by library - MediaStore handles the filtering
                let shows: Vec<SeriesReference> = store
                    .get_series(Some(self.library_id))
                    .into_iter()
                    .cloned()
                    .collect();

                // Series are already sorted by MediaStore, no need to sort again

                // Update grid state
                self.grid_state.total_items = shows.len();

                // Update cache
                self.cached_media = CachedMedia::TvShows(shows);
            }
        }

        self.needs_refresh = false;
    }

    /// Mark this tab as needing refresh
    pub fn mark_needs_refresh(&mut self) {
        self.needs_refresh = true;
    }

    /// Update the grid state's scroll position
    pub fn update_scroll(&mut self, viewport: iced::widget::scrollable::Viewport) {
        self.grid_state.update_scroll(viewport);
    }

    /// Get movies if this is a movie library
    pub fn movies(&self) -> Option<&[MovieReference]> {
        match &self.cached_media {
            CachedMedia::Movies(movies) => Some(movies),
            _ => None,
        }
    }

    /// Get TV shows if this is a TV library
    pub fn tv_shows(&self) -> Option<&[SeriesReference]> {
        match &self.cached_media {
            CachedMedia::TvShows(shows) => Some(shows),
            _ => None,
        }
    }

    /// Get the currently visible media items based on the grid's visible range
    pub fn get_visible_items(&self) -> Vec<MediaReference> {
        let range = self.grid_state.visible_range.clone();
        
        match &self.cached_media {
            CachedMedia::Movies(movies) => {
                movies
                    .get(range)
                    .map(|slice| slice.iter().map(|m| MediaReference::Movie(m.clone())).collect())
                    .unwrap_or_default()
            }
            CachedMedia::TvShows(shows) => {
                shows
                    .get(range)
                    .map(|slice| slice.iter().map(|s| MediaReference::Series(s.clone())).collect())
                    .unwrap_or_default()
            }
        }
    }
}
