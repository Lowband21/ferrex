//! Tab manager for coordinating multiple independent tab states

use ferrex_core::ArchivedMediaID;
use ferrex_core::{LibraryID, SortBy, SortOrder};
use std::collections::HashMap;

use super::{TabId, TabState};
use crate::infrastructure::api_types::LibraryType;
use crate::infrastructure::repository::accessor::{Accessor, ReadOnly};

/// Manages all tab states in the application
#[derive(Debug)]
pub struct TabManager {
    /// All tab states indexed by TabId
    tabs: HashMap<TabId, TabState>,

    /// The currently active tab
    active_tab: TabId,

    /// Reference to the media repo accessor for creating new tabs
    repo_accessor: Accessor<ReadOnly>,

    /// Library information for creating new tabs
    /// This is cached to avoid needing to query library domain
    library_info: HashMap<LibraryID, LibraryType>,
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::all_functions
)]
impl TabManager {
    /// Create a new tab manager
    pub fn new(repo_accessor: Accessor<ReadOnly>) -> Self {
        let mut tabs = HashMap::new();

        // Always start with the All tab (without repo accessor initially)
        tabs.insert(TabId::All, TabState::new_all(repo_accessor.clone()));

        Self {
            tabs,
            active_tab: TabId::All,
            repo_accessor,
            library_info: HashMap::new(),
        }
    }

    /// Register library information for tab creation
    pub fn register_library(&mut self, library_id: LibraryID, library_type: LibraryType) {
        self.library_info.insert(library_id, library_type);
    }

    /// Update library information from the repo accessor
    pub fn update_libraries(&mut self) {
        self.library_info.clear(); // Clear BEFORE populating, not after

        if self.repo_accessor.is_initialized()
            && let Ok(libraries) = self.repo_accessor.get_libraries()
        {
            for library in libraries {
                if library.enabled {
                    self.library_info.insert(library.id, library.library_type);
                    self.register_library(library.id, library.library_type);
                }
            }
        }

        // Clean up tabs for libraries that no longer exist
        let valid_ids: Vec<TabId> = self
            .tabs
            .keys()
            .filter(|tab_id| match tab_id {
                TabId::All => true,
                TabId::Library(id) => self.library_info.contains_key(id),
            })
            .cloned()
            .collect();

        self.tabs.retain(|id, _| valid_ids.contains(id));

        // If active tab was removed, switch to All
        if !self.tabs.contains_key(&self.active_tab) {
            self.active_tab = TabId::All;
        }
    }

    /// Get or create a tab state
    pub fn get_or_create_tab(&mut self, tab_id: TabId) -> &mut TabState {
        // If tab doesn't exist, create it
        if !self.tabs.contains_key(&tab_id) {
            match tab_id {
                TabId::All => {
                    // All tab should always exist, but create if needed
                    //self.tabs
                    //    .insert(tab_id, TabState::new_all(self.repo_accessor.as_ref()));
                }
                TabId::Library(library_id) => {
                    // Create library tab if we have the library info
                    if let Some(&library_type) = self.library_info.get(&library_id) {
                        self.tabs.insert(
                            tab_id,
                            TabState::new_library(
                                library_id,
                                library_type,
                                self.repo_accessor.clone(),
                            ),
                        );
                    } else {
                        // Library not registered, log warning and return All tab
                        log::warn!(
                            "Attempted to create tab for unregistered library: {}",
                            library_id
                        );
                        return self.tabs.get_mut(&TabId::All).unwrap();
                    }
                }
            }
        }

        self.tabs.get_mut(&tab_id).unwrap()
    }

    /// Get or create a tab state with scroll restoration
    pub fn get_or_create_tab_with_scroll(
        &mut self,
        tab_id: TabId,
        scroll_manager: &crate::domains::ui::scroll_manager::ScrollPositionManager,
    ) -> &mut TabState {
        let is_new_tab = !self.tabs.contains_key(&tab_id);

        // Create the tab if needed
        let tab = self.get_or_create_tab(tab_id);

        // If this is a newly created tab, restore its scroll position
        if is_new_tab
            && let Some(scroll_state) = scroll_manager.get_tab_scroll(&tab_id)
            && let Some(grid_state) = tab.grid_state_mut()
        {
            grid_state.scroll_position = scroll_state.position;
            log::info!(
                "Restored scroll position for newly created tab {:?}: {}",
                tab_id,
                scroll_state.position
            );
        }

        self.tabs.get_mut(&tab_id).unwrap()
    }

    pub fn set_active_sort(&mut self, sort_by: SortBy, sort_order: SortOrder) {
        if let Some(tab) = self.tabs.get_mut(&self.active_tab) {
            tab.set_sort(sort_by, sort_order);
        }
    }

    /// Get the currently active tab
    pub fn get_active_tab(&mut self) -> &mut TabState {
        self.get_or_create_tab(self.active_tab)
    }

    /// Get the active tab without mutation
    pub fn active_tab(&self) -> &TabState {
        self.tabs
            .get(&self.active_tab)
            .expect("Active tab should always exist")
    }

    /// Get the active tab ID
    pub fn active_tab_id(&self) -> TabId {
        self.active_tab
    }

    /// Get the active tab ID
    pub fn active_tab_type(&self) -> Option<&LibraryType> {
        if let Some(id) = &self.active_tab.library_id() {
            self.library_info.get(id)
        } else {
            None
        }
    }

    /// Set the active tab
    pub fn set_active_tab(&mut self, tab_id: TabId) -> bool {
        // Check if this is a valid tab
        match tab_id {
            TabId::All => {
                self.active_tab = tab_id;
                true
            }
            TabId::Library(library_id) => {
                if self.library_info.contains_key(&library_id) {
                    // Ensure tab exists
                    self.get_or_create_tab(tab_id);
                    self.active_tab = tab_id;
                    true
                } else {
                    log::warn!(
                        "Attempted to activate unregistered library tab: {}",
                        library_id
                    );
                    false
                }
            }
        }
    }

    /// Set the active tab with scroll position management
    /// This method saves the current tab's scroll position and restores the new tab's position
    pub fn set_active_tab_with_scroll(
        &mut self,
        tab_id: TabId,
        scroll_manager: &mut crate::domains::ui::scroll_manager::ScrollPositionManager,
        window_width: f32,
    ) -> bool {
        // Save current tab's scroll position if it's different from the new tab
        if self.active_tab != tab_id {
            // Note: Current scroll position is already being saved in real-time by scroll handlers
            // This is just for logging/debugging
            log::debug!("Switching from tab {:?} to {:?}", self.active_tab, tab_id);
        }

        // Switch to the new tab
        let success = self.set_active_tab(tab_id);

        if success {
            // Ensure the tab's grid has the correct column count for the current window width
            if let Some(tab) = self.get_tab_mut(tab_id)
                && let Some(grid_state) = tab.grid_state_mut()
            {
                // Update columns based on current window width
                grid_state.resize(window_width);

                // Restore the tab's scroll position from ScrollPositionManager
                if let Some(scroll_state) = scroll_manager.get_tab_scroll(&tab_id) {
                    grid_state.scroll_position = scroll_state.position;
                    log::info!(
                        "Restored scroll position for tab {:?}: {}",
                        tab_id,
                        scroll_state.position
                    );
                }
            }
        }

        success
    }

    /// Get a specific tab by ID
    pub fn get_tab(&self, tab_id: TabId) -> Option<&TabState> {
        self.tabs.get(&tab_id)
    }

    /// Get a specific tab by ID (mutable)
    pub fn get_tab_mut(&mut self, tab_id: TabId) -> Option<&mut TabState> {
        self.tabs.get_mut(&tab_id)
    }

    /// Mark a tab as needing refresh
    pub fn mark_tab_needs_refresh(&mut self, tab_id: TabId) {
        if let Some(tab) = self.get_tab_mut(tab_id) {
            match tab {
                TabState::Library(state) => state.mark_needs_refresh(),
                TabState::All(_) => {
                    // AllTabState refresh is handled differently
                }
            }
        }
    }

    /// Mark all tabs as needing refresh
    pub fn mark_all_tabs_need_refresh(&mut self) {
        for (_, tab) in self.tabs.iter_mut() {
            match tab {
                TabState::Library(state) => state.mark_needs_refresh(),
                TabState::All(_) => {
                    // AllTabState refresh is handled differently
                }
            }
        }
    }

    /// Refresh content for the active tab
    pub fn refresh_active_tab(&mut self) {
        match self.get_active_tab() {
            TabState::Library(state) => state.refresh_from_repo(),
            TabState::All(state) => {
                // Refresh All view model
                //state.view_model.refresh_from_repo();
            }
        }
    }

    /// Refresh content for all tabs
    pub fn refresh_all_tabs(&mut self) {
        for (_, tab) in self.tabs.iter_mut() {
            match tab {
                TabState::Library(state) => state.refresh_from_repo(),
                TabState::All(state) => {} //state.view_model.refresh_from_repo(),
            }
        }
    }

    /// Get the number of tabs
    pub fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    /// Check if a tab exists
    pub fn has_tab(&self, tab_id: TabId) -> bool {
        self.tabs.contains_key(&tab_id)
    }

    /// Get all tab IDs
    pub fn tab_ids(&self) -> Vec<TabId> {
        self.tabs.keys().cloned().collect()
    }

    /// Get all tab info
    pub fn library_info(&self) -> &HashMap<LibraryID, LibraryType> {
        &self.library_info
    }

    /// Get the currently visible media items from the active tab
    pub fn get_active_tab_visible_items(&self) -> Vec<ArchivedMediaID> {
        self.tabs
            .get(&self.active_tab)
            .map(|tab| tab.get_visible_items())
            .unwrap_or_default()
    }
}
