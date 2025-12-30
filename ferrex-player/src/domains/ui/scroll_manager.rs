//! Scroll position manager for maintaining scroll states across the application
//!
//! This module provides a centralized way to store and restore scroll positions
//! for different views, ensuring a smooth user experience when navigating.
//! Supports library-aware keys for grid views and efficient state management.

use crate::common::messages::DomainMessage;
use crate::domains::ui::{
    tabs::TabId, types::ViewState, views::grid::VirtualGridState,
    views::virtual_carousel::types::CarouselKey,
};
use ferrex_core::player_prelude::LibraryId;
use iced::Task;
use iced::widget::scrollable::Viewport;
use std::collections::HashMap;
use uuid::Uuid;

/// Lightweight scroll state for efficient storage and restoration
#[derive(Debug, Clone)]
pub struct ScrollState {
    /// Y-axis scroll position
    pub position: f32,
    /// Viewport dimensions when saved
    pub viewport_width: f32,
    pub viewport_height: f32,
    /// Grid-specific state
    pub columns: Option<usize>,
    /// Visible range for virtual grids (start, end)
    pub visible_range: Option<(usize, usize)>,
}

/// Carousel-specific scroll state for horizontal carousels
#[derive(Debug, Clone)]
pub struct CarouselScrollState {
    /// Horizontal scroll position in pixels
    pub scroll_x: f32,
    /// Item index position (fractional for smooth positioning)
    pub index_position: f32,
    /// Reference index (last committed anchor position)
    pub reference_index: f32,
    /// Viewport width when saved (for validation on restore)
    pub viewport_width: f32,
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::all_functions
)]
impl ScrollState {
    /// Create from just a scroll position (for simple views)
    pub fn from_position(position: f32) -> Self {
        Self {
            position,
            viewport_width: 0.0,
            viewport_height: 0.0,
            columns: None,
            visible_range: None,
        }
    }

    /// Create from viewport (for scrollable views)
    pub fn from_viewport(viewport: Viewport) -> Self {
        Self {
            position: viewport.absolute_offset().y,
            viewport_width: viewport.bounds().width,
            viewport_height: viewport.bounds().height,
            columns: None,
            visible_range: None,
        }
    }

    /// Create from grid state (for virtual grids)
    pub fn from_grid_state(grid_state: &VirtualGridState) -> Self {
        Self {
            position: grid_state.scroll_position,
            viewport_width: grid_state.viewport_width,
            viewport_height: grid_state.viewport_height,
            columns: Some(grid_state.columns),
            visible_range: Some((
                grid_state.visible_range.start,
                grid_state.visible_range.end,
            )),
        }
    }
}

/// Trait for automatic view key generation from ViewState and library context
pub trait ViewIdentifier {
    /// Generate a unique scroll state key for this view
    fn scroll_key(&self, library_id: Option<Uuid>) -> String;

    /// Generate a context-specific key (for views with sub-scrollables)
    fn scroll_key_with_context(
        &self,
        library_id: Option<Uuid>,
        context: &Uuid,
    ) -> String;
}

impl ViewIdentifier for ViewState {
    fn scroll_key(&self, library_id: Option<Uuid>) -> String {
        match self {
            ViewState::Library => {
                ScrollPositionManager::generate_key("library", library_id, None)
            }
            ViewState::MovieDetail { movie_id, .. } => {
                ScrollPositionManager::generate_key(
                    "movie_detail",
                    library_id,
                    Some(movie_id.as_uuid()),
                )
            }
            ViewState::SeriesDetail { series_id, .. } => {
                ScrollPositionManager::generate_key(
                    "tv_show_detail",
                    library_id,
                    Some(series_id.as_uuid()),
                )
            }
            ViewState::SeasonDetail { season_id, .. } => {
                ScrollPositionManager::generate_key(
                    "season_detail",
                    library_id,
                    Some(season_id.as_uuid()),
                )
            }
            ViewState::EpisodeDetail { episode_id, .. } => {
                ScrollPositionManager::generate_key(
                    "episode_detail",
                    library_id,
                    Some(episode_id.as_uuid()),
                )
            }
            ViewState::LibraryManagement => {
                ScrollPositionManager::generate_key(
                    "library_management",
                    None,
                    None,
                )
            }
            ViewState::AdminDashboard => ScrollPositionManager::generate_key(
                "admin_dashboard",
                None,
                None,
            ),
            ViewState::AdminUsers => {
                ScrollPositionManager::generate_key("admin_users", None, None)
            }
            ViewState::UserSettings => {
                ScrollPositionManager::generate_key("user_settings", None, None)
            }
            ViewState::Player
            | ViewState::LoadingVideo { .. }
            | ViewState::VideoError { .. } => {
                // These views don't need scroll persistence
                ScrollPositionManager::generate_key("no_scroll", None, None)
            }
        }
    }

    fn scroll_key_with_context(
        &self,
        library_id: Option<Uuid>,
        context: &Uuid,
    ) -> String {
        match self {
            ViewState::Library => ScrollPositionManager::generate_key(
                "library",
                library_id,
                Some(context),
            ),
            _ => {
                // For other views, append context to the base key
                format!("{}.{}", self.scroll_key(library_id), context)
            }
        }
    }
}

/// Manages scroll positions across the application using string-based keys
#[derive(Debug, Default)]
pub struct ScrollPositionManager {
    /// Stored scroll states for each view (key format: "view_type[.library_id][.context]")
    states: HashMap<String, ScrollState>,
    /// Carousel scroll states keyed by CarouselKey
    carousel_states: HashMap<CarouselKey, CarouselScrollState>,
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::all_functions
)]
impl ScrollPositionManager {
    /// Create a new scroll position manager
    pub fn new() -> Self {
        Self {
            states: HashMap::new(),
            carousel_states: HashMap::new(),
        }
    }

    /// Save scroll state for a view with library context
    pub fn save_state(&mut self, view_key: String, state: ScrollState) {
        // Only save meaningful positions to avoid overwriting with initial values
        if state.position > 0.1 {
            self.states.insert(view_key, state);
        }
    }

    /// Get saved scroll state for a view
    pub fn get_state(&self, view_key: &str) -> Option<&ScrollState> {
        self.states.get(view_key)
    }

    /// Save state for library view (required library_id for specific library, None for Home)
    pub fn save_library_scroll(
        &mut self,
        library_id: Option<LibraryId>,
        state: ScrollState,
    ) {
        let key = Self::library_scroll_key(library_id);
        self.save_state(key, state);
    }

    /// Get state for library view (required library_id for specific library, None for Home)
    pub fn get_library_scroll(
        &self,
        library_id: Option<LibraryId>,
    ) -> Option<&ScrollState> {
        let key = Self::library_scroll_key(library_id);
        self.get_state(&key)
    }

    /// Save scroll state for a specific tab
    pub fn save_tab_scroll(&mut self, tab_id: &TabId, state: ScrollState) {
        let key = Self::tab_scroll_key(tab_id);
        self.save_state(key, state);
    }

    /// Get scroll state for a specific tab
    pub fn get_tab_scroll(&self, tab_id: &TabId) -> Option<&ScrollState> {
        let key = Self::tab_scroll_key(tab_id);
        let state = self.get_state(&key);
        if let Some(s) = state {
            log::debug!(
                "Retrieved scroll state for tab {:?} at position {}",
                tab_id,
                s.position
            );
        }
        state
    }

    /// Clear scroll state for a specific tab
    pub fn clear_tab_scroll(&mut self, tab_id: &TabId) {
        let key = Self::tab_scroll_key(tab_id);
        self.clear_state(&key);
    }

    /// Save state for a specific ViewModel's library view
    pub fn save_viewmodel_library_scroll(
        &mut self,
        source: &str,
        library_id: Option<LibraryId>,
        state: ScrollState,
    ) {
        let key = Self::viewmodel_library_scroll_key(source, library_id);
        self.save_state(key, state);
    }

    /// Get state for a specific ViewModel's library view
    pub fn get_viewmodel_library_scroll(
        &self,
        source: &str,
        library_id: Option<LibraryId>,
    ) -> Option<&ScrollState> {
        let key = Self::viewmodel_library_scroll_key(source, library_id);
        self.get_state(&key)
    }

    /// Clear scroll state for a specific view
    pub fn clear_state(&mut self, view_key: &str) {
        self.states.remove(view_key);
    }

    /// Clear all states for a specific library
    pub fn clear_library_states(&mut self, library_id: LibraryId) {
        let library_prefix = format!(".{}", library_id);
        self.states.retain(|key, _| !key.contains(&library_prefix));
    }

    /// Clear all saved states
    pub fn clear_all(&mut self) {
        self.states.clear();
    }

    /// Get the number of saved states
    pub fn count(&self) -> usize {
        self.states.len()
    }

    /// Check if we have saved state for a view
    pub fn has_state(&self, view_key: &str) -> bool {
        self.states.contains_key(view_key)
    }

    // Key generation helpers
    fn library_scroll_key(library_id: Option<LibraryId>) -> String {
        match library_id {
            Some(id) => format!("library.{}", id),
            None => "library.all".to_string(),
        }
    }

    /// Generate key for tab-specific scroll state
    fn tab_scroll_key(tab_id: &TabId) -> String {
        match tab_id {
            TabId::Home => "tab.all".to_string(),
            TabId::Library(id) => {
                format!("tab.library.{}", id)
            }
        }
    }

    /// Generate key for ViewModel-specific library scroll state
    fn viewmodel_library_scroll_key(
        source: &str,
        library_id: Option<LibraryId>,
    ) -> String {
        match library_id {
            Some(id) => format!("{}.library.{}", source, id),
            None => format!("{}.library.all", source),
        }
    }

    /// Generate key for any view with optional library context
    pub fn generate_key(
        view_type: &str,
        library_id: Option<Uuid>,
        context: Option<&Uuid>,
    ) -> String {
        let mut key = view_type.to_string();

        if let Some(id) = library_id {
            key.push_str(&format!(".{}", id));
        }

        if let Some(ctx) = context {
            key.push_str(&format!(".{}", ctx));
        }

        key
    }

    /// Save scroll state for a ViewState-based view
    pub fn save_view_state(
        &mut self,
        view: &ViewState,
        library_id: Option<LibraryId>,
        state: ScrollState,
    ) {
        if let Some(id) = library_id {
            let key = view.scroll_key(Some(id.to_uuid()));
            self.save_state(key, state);
        }
    }

    /// Get scroll state for a ViewState-based view
    pub fn get_view_state(
        &self,
        view: &ViewState,
        library_id: Option<LibraryId>,
    ) -> Option<&ScrollState> {
        if let Some(id) = library_id {
            let key = view.scroll_key(Some(id.to_uuid()));
            self.get_state(&key)
        } else {
            None
        }
    }

    /// Save scroll state for a ViewState-based view with context
    pub fn save_view_state_with_context(
        &mut self,
        view: &ViewState,
        library_id: Option<LibraryId>,
        context: &Uuid,
        state: ScrollState,
    ) {
        if let Some(id) = library_id {
            let key = view.scroll_key_with_context(Some(id.to_uuid()), context);
            self.save_state(key, state);
        }
    }

    /// Get scroll state for a ViewState-based view with context
    pub fn get_view_state_with_context(
        &self,
        view: &ViewState,
        library_id: Option<LibraryId>,
        context: &Uuid,
    ) -> Option<&ScrollState> {
        if let Some(id) = library_id {
            let key = view.scroll_key_with_context(Some(id.to_uuid()), context);
            self.get_state(&key)
        } else {
            None
        }
    }

    // Carousel scroll state management

    /// Save carousel scroll state
    pub fn save_carousel_scroll(
        &mut self,
        key: CarouselKey,
        state: CarouselScrollState,
    ) {
        // Only save if there's meaningful scroll position
        if state.scroll_x > 0.1 || state.index_position > 0.1 {
            self.carousel_states.insert(key, state);
        }
    }

    /// Get saved carousel scroll state
    pub fn get_carousel_scroll(
        &self,
        key: &CarouselKey,
    ) -> Option<&CarouselScrollState> {
        self.carousel_states.get(key)
    }

    /// Clear carousel scroll state for a specific carousel
    pub fn clear_carousel_scroll(&mut self, key: &CarouselKey) {
        self.carousel_states.remove(key);
    }

    /// Clear all carousel scroll states
    pub fn clear_all_carousel_scrolls(&mut self) {
        self.carousel_states.clear();
    }
}

/// Extension trait for easy scroll state management with library context
pub trait ScrollStateExt {
    fn save_scroll_state(&mut self, view_key: String, state: ScrollState);
    fn restore_scroll_state(&self, view_key: &str) -> Option<&ScrollState>;
    fn save_library_scroll(
        &mut self,
        library_id: Option<LibraryId>,
        state: ScrollState,
    );
    fn restore_library_scroll(
        &self,
        library_id: Option<LibraryId>,
    ) -> Option<&ScrollState>;
    fn save_view_scroll(
        &mut self,
        view: &ViewState,
        library_id: Option<LibraryId>,
        state: ScrollState,
    );
    fn restore_view_scroll(
        &self,
        view: &ViewState,
        library_id: Option<LibraryId>,
    ) -> Option<&ScrollState>;
    fn save_current_view_scroll(&mut self, state: ScrollState);
    fn restore_current_view_scroll(&self) -> Option<&ScrollState>;
    fn restore_library_scroll_state(
        &mut self,
        library_id: Option<LibraryId>,
    ) -> Task<DomainMessage>;

    // ViewModel-specific methods
    fn save_movies_vm_scroll(
        &mut self,
        library_id: Option<LibraryId>,
        state: ScrollState,
    );
    fn restore_movies_vm_scroll(
        &self,
        library_id: Option<LibraryId>,
    ) -> Option<&ScrollState>;
    fn save_tv_vm_scroll(
        &mut self,
        library_id: Option<LibraryId>,
        state: ScrollState,
    );
    fn restore_tv_vm_scroll(
        &self,
        library_id: Option<LibraryId>,
    ) -> Option<&ScrollState>;
    fn save_all_vm_scroll(
        &mut self,
        library_id: Option<LibraryId>,
        state: ScrollState,
    );
    fn restore_all_vm_scroll(
        &self,
        library_id: Option<LibraryId>,
    ) -> Option<&ScrollState>;
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::all_functions
)]
impl ScrollStateExt for crate::state::State {
    fn save_scroll_state(&mut self, view_key: String, state: ScrollState) {
        self.domains
            .ui
            .state
            .scroll_manager
            .save_state(view_key, state);
    }

    fn restore_scroll_state(&self, view_key: &str) -> Option<&ScrollState> {
        self.domains.ui.state.scroll_manager.get_state(view_key)
    }

    fn save_library_scroll(
        &mut self,
        library_id: Option<LibraryId>,
        state: ScrollState,
    ) {
        self.domains
            .ui
            .state
            .scroll_manager
            .save_library_scroll(library_id, state);
    }

    fn restore_library_scroll(
        &self,
        library_id: Option<LibraryId>,
    ) -> Option<&ScrollState> {
        self.domains
            .ui
            .state
            .scroll_manager
            .get_library_scroll(library_id)
    }

    fn save_view_scroll(
        &mut self,
        view: &ViewState,
        library_id: Option<LibraryId>,
        state: ScrollState,
    ) {
        self.domains
            .ui
            .state
            .scroll_manager
            .save_view_state(view, library_id, state);
    }

    fn restore_view_scroll(
        &self,
        view: &ViewState,
        library_id: Option<LibraryId>,
    ) -> Option<&ScrollState> {
        self.domains
            .ui
            .state
            .scroll_manager
            .get_view_state(view, library_id)
    }

    fn save_current_view_scroll(&mut self, state: ScrollState) {
        let view = &self.domains.ui.state.view.clone();
        let library_id = self.domains.ui.state.scope.lib_id();
        self.save_view_scroll(view, library_id, state);
    }

    fn restore_current_view_scroll(&self) -> Option<&ScrollState> {
        let view = &self.domains.ui.state.view;
        let library_id = self.domains.ui.state.scope.lib_id();
        self.restore_view_scroll(view, library_id)
    }

    fn restore_library_scroll_state(
        &mut self,
        library_id: Option<LibraryId>,
    ) -> Task<DomainMessage> {
        let scaled_layout = &self.domains.ui.state.scaled_layout;
        if library_id.is_some() {
            if let Some(lib_id) = library_id {
                self.tab_manager.set_active_tab_with_scroll(
                    TabId::Library(lib_id),
                    &mut self.domains.ui.state.scroll_manager,
                    self.window_size.width,
                    scaled_layout,
                );
            }
        } else {
            self.tab_manager.set_active_tab_with_scroll(
                TabId::Home,
                &mut self.domains.ui.state.scroll_manager,
                self.window_size.width,
                scaled_layout,
            );
        }

        // TabManager has already restored scroll position, just refresh content
        self.tab_manager.refresh_active_tab();

        log::debug!(
            "Restored library scroll state through TabManager for library_id: {:?}",
            library_id
        );

        Task::none()
    }

    // ViewModel-specific implementations
    fn save_movies_vm_scroll(
        &mut self,
        library_id: Option<LibraryId>,
        state: ScrollState,
    ) {
        self.domains
            .ui
            .state
            .scroll_manager
            .save_viewmodel_library_scroll("movies_vm", library_id, state);
    }

    fn restore_movies_vm_scroll(
        &self,
        library_id: Option<LibraryId>,
    ) -> Option<&ScrollState> {
        self.domains
            .ui
            .state
            .scroll_manager
            .get_viewmodel_library_scroll("movies_vm", library_id)
    }

    fn save_tv_vm_scroll(
        &mut self,
        library_id: Option<LibraryId>,
        state: ScrollState,
    ) {
        self.domains
            .ui
            .state
            .scroll_manager
            .save_viewmodel_library_scroll("tv_vm", library_id, state);
    }

    fn restore_tv_vm_scroll(
        &self,
        library_id: Option<LibraryId>,
    ) -> Option<&ScrollState> {
        self.domains
            .ui
            .state
            .scroll_manager
            .get_viewmodel_library_scroll("tv_vm", library_id)
    }

    fn save_all_vm_scroll(
        &mut self,
        library_id: Option<LibraryId>,
        state: ScrollState,
    ) {
        self.domains
            .ui
            .state
            .scroll_manager
            .save_viewmodel_library_scroll("all_vm", library_id, state);
    }

    fn restore_all_vm_scroll(
        &self,
        library_id: Option<LibraryId>,
    ) -> Option<&ScrollState> {
        self.domains
            .ui
            .state
            .scroll_manager
            .get_viewmodel_library_scroll("all_vm", library_id)
    }
}
