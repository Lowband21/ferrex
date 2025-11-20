//! Library management domain
//!
//! Contains all library-related state and logic moved from the monolithic State

pub mod messages;
pub mod server;
pub mod types;
pub mod update;
pub mod update_handlers;

use self::messages::Message as LibraryMessage;
use self::types::LibraryFormData;
use crate::common::messages::{CrossDomainEvent, DomainMessage};
use crate::domains::media::store::MediaStore;
use crate::infrastructure::adapters::api_client_adapter::ApiClientAdapter;
use ferrex_core::api_types::{LibraryMediaCache, ScanProgress};
use ferrex_core::library::Library;
use iced::Task;
use std::collections::HashMap;
use std::sync::{Arc, RwLock as StdRwLock};
use uuid::Uuid;

/// Cache for library media to enable instant switching
//#[derive(Debug)]
//pub struct LibraryMediaCache {
//    pub media_files: Vec<crate::infrastructure::api_types::MediaFile>,
//    pub last_updated: std::time::Instant,
//}

/// Library domain state - moved from monolithic State
#[derive(Debug)]
pub struct LibraryDomainState {
    // From State struct:
    pub libraries: Vec<Library>,
    pub current_library_id: Option<Uuid>,
    pub show_library_management: bool,
    pub library_form_data: Option<LibraryFormData>,
    pub library_form_errors: Vec<String>,
    pub library_media_cache: HashMap<Uuid, LibraryMediaCache>,
    pub scanning: bool,
    pub active_scan_id: Option<String>,
    pub scan_progress: Option<ScanProgress>,
    pub show_scan_progress: bool,

    // Shared references needed by library domain
    pub media_store: Arc<StdRwLock<MediaStore>>,
    pub api_service: Option<Arc<ApiClientAdapter>>,
}

impl LibraryDomainState {
    pub fn new(media_store: Arc<StdRwLock<MediaStore>>, api_service: Option<Arc<ApiClientAdapter>>) -> Self {
        Self {
            libraries: Vec::new(),
            current_library_id: None,
            show_library_management: false,
            library_form_data: None,
            library_form_errors: Vec::new(),
            library_media_cache: HashMap::new(),
            scanning: false,
            active_scan_id: None,
            scan_progress: None,
            show_scan_progress: false,
            media_store,
            api_service,
        }
    }
}

#[derive(Debug)]
pub struct LibraryDomain {
    pub state: LibraryDomainState,
}

impl LibraryDomain {
    pub fn new(state: LibraryDomainState) -> Self {
        Self { state }
    }

    /// Update function - delegates to existing update_library logic
    pub fn update(&mut self, message: LibraryMessage) -> Task<DomainMessage> {
        // This is a stub - the actual update_library function is called from the main update loop
        // We don't call it here to avoid circular dependencies
        Task::none()
    }

    pub fn handle_event(&mut self, event: &CrossDomainEvent) -> Task<DomainMessage> {
        match event {
            CrossDomainEvent::UserAuthenticated(_user, _permissions) => {
                // Could trigger library refresh here
                Task::none()
            }
            CrossDomainEvent::DatabaseCleared => {
                // Clear library cache
                self.state.library_media_cache.clear();
                self.state.current_library_id = None;
                Task::none()
            }
            CrossDomainEvent::ClearLibraries => {
                // Clear libraries and current_library_id
                self.state.libraries.clear();
                self.state.current_library_id = None;
                self.state.library_media_cache.clear();
                Task::none()
            }
            _ => Task::none(),
        }
    }
}
