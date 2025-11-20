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
use crate::domains::media::repository::accessor::{Accessor, ReadWrite};
use crate::infrastructure::adapters::api_client_adapter::ApiClientAdapter;
use ferrex_core::api_types::{LibraryMediaCache, ScanProgress};
use ferrex_core::types::library::Library;
use ferrex_core::{ArchivedLibrary, LibraryID};
use iced::Task;
use rkyv::vec::ArchivedVec;
use std::collections::HashMap;
use std::sync::{Arc, RwLock as StdRwLock};
use uuid::Uuid;

#[derive(Debug)]
pub struct LibraryDomainState {
    // From State struct:
    pub current_library_id: Option<LibraryID>,
    pub show_library_management: bool,
    pub library_form_data: Option<LibraryFormData>,
    pub library_form_errors: Vec<String>,
    pub library_media_cache: HashMap<Uuid, LibraryMediaCache>,
    pub scanning: bool,
    pub active_scan_id: Option<String>,
    pub scan_progress: Option<ScanProgress>,
    pub show_scan_progress: bool,
    pub initial_library_fetch: bool,

    pub api_service: Option<Arc<ApiClientAdapter>>,

    pub repo_accessor: Accessor<ReadWrite>,
}

impl LibraryDomainState {
    pub fn new(
        api_service: Option<Arc<ApiClientAdapter>>,
        repo_accessor: Accessor<ReadWrite>,
    ) -> Self {
        Self {
            current_library_id: None,
            show_library_management: false,
            library_form_data: None,
            library_form_errors: Vec::new(),
            library_media_cache: HashMap::new(),
            scanning: false,
            active_scan_id: None,
            scan_progress: None,
            show_scan_progress: false,
            initial_library_fetch: false,
            api_service,
            repo_accessor,
        }
    }
}

#[derive(Debug)]
pub struct LibraryDomain {
    pub state: LibraryDomainState,
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::all_functions
)]
impl LibraryDomain {
    pub fn new(state: LibraryDomainState) -> Self {
        Self { state }
    }

    //pub fn update(&mut self, message: LibraryMessage) -> Task<DomainMessage> {
    //    // This is a stub - the actual update_library function is called from the main update loop
    //    // We don't call it here to avoid circular dependencies
    //    Task::none()
    //}

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
            // Should probably be depricated in favor of repo centric handling
            CrossDomainEvent::ClearLibraries => {
                // Clear libraries and current_library_id
                self.state.current_library_id = None;
                self.state.library_media_cache.clear();
                Task::none()
            }
            _ => Task::none(),
        }
    }
}
