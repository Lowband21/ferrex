//! Tab management system for independent state per library/view
//!
//! This module provides a tab-based architecture where each library
//! and the "All" view have completely independent state, including
//! scroll positions, grid states, and cached content.

use std::fmt;
use uuid::Uuid;

pub mod manager;
pub mod state;

pub use manager::TabManager;
pub use state::{TabState, AllTabState, LibraryTabState};

/// Unique identifier for each tab in the application
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TabId {
    /// The "All" tab showing curated content from all libraries
    All,
    
    /// A specific library tab
    Library(Uuid),
}

impl TabId {
    /// Check if this is the All tab
    pub fn is_all(&self) -> bool {
        matches!(self, TabId::All)
    }
    
    /// Check if this is a library tab
    pub fn is_library(&self) -> bool {
        matches!(self, TabId::Library(_))
    }
    
    /// Get the library ID if this is a library tab
    pub fn library_id(&self) -> Option<Uuid> {
        match self {
            TabId::Library(id) => Some(*id),
            TabId::All => None,
        }
    }
}

impl fmt::Display for TabId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TabId::All => write!(f, "All"),
            TabId::Library(id) => write!(f, "Library({})", id),
        }
    }
}

impl From<Uuid> for TabId {
    fn from(library_id: Uuid) -> Self {
        TabId::Library(library_id)
    }
}

impl Default for TabId {
    fn default() -> Self {
        TabId::All
    }
}