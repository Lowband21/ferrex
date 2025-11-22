//! Tab management system for independent state per library/view
//!
//! This module provides a tab-based architecture where each library
//! and the Home view have completely independent state, including
//! scroll positions, grid states, and cached content.

use ferrex_core::player_prelude::LibraryId;
use std::fmt;

pub mod home_focus;
pub mod manager;
pub mod state;

pub use home_focus::{HomeFocusState, ordered_keys_for_home};
pub use manager::TabManager;
pub use state::{HomeTabState, LibraryTabState, TabState};

/// Unique identifier for each tab in the application
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TabId {
    /// The Home tab showing curated content from all libraries
    #[default]
    Home,

    /// A specific library tab
    Library(LibraryId),
}

impl TabId {
    /// Check if this is the Home tab
    pub fn is_all(&self) -> bool {
        matches!(self, TabId::Home)
    }

    /// Check if this is a library tab
    pub fn is_library(&self) -> bool {
        matches!(self, TabId::Library(_))
    }

    /// Get the library ID if this is a library tab
    pub fn library_id(&self) -> Option<LibraryId> {
        match self {
            TabId::Library(id) => Some(*id),
            TabId::Home => None,
        }
    }
}

impl fmt::Display for TabId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TabId::Home => write!(f, "Home"),
            TabId::Library(id) => write!(f, "Library({})", id),
        }
    }
}

impl From<LibraryId> for TabId {
    fn from(library_id: LibraryId) -> Self {
        TabId::Library(library_id)
    }
}
