//! Library management domain
//!
//! Contains all library-related state and logic moved from the monolithic State

pub mod messages;
pub mod server;
pub mod types;
pub mod update;
pub mod update_handlers;

use self::types::LibraryFormData;
use crate::common::messages::{CrossDomainEvent, DomainMessage};
use crate::infrastructure::adapters::api_client_adapter::ApiClientAdapter;
use crate::infrastructure::repository::accessor::{Accessor, ReadWrite};
use crate::infrastructure::services::api::ApiService;
use ferrex_core::player_prelude::{
    LibraryID, LibraryMediaCache, ScanConfig, ScanMetrics, ScanProgressEvent, ScanSnapshotDto,
};
use iced::Task;
use std::collections::HashMap;
#[cfg(feature = "demo")]
use std::collections::HashSet;
#[cfg(feature = "demo")]
use std::path::PathBuf;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug)]
pub struct LibraryDomainState {
    // From State struct:
    pub current_library_id: Option<LibraryID>,
    pub show_library_management: bool,
    pub library_form_data: Option<LibraryFormData>,
    pub library_form_errors: Vec<String>,
    pub library_form_success: Option<String>,
    pub library_media_cache: HashMap<Uuid, LibraryMediaCache>,
    pub active_scans: HashMap<Uuid, ScanSnapshotDto>,
    pub latest_progress: HashMap<Uuid, ScanProgressEvent>,
    pub initial_library_fetch: bool,

    // Diagnostics
    pub scan_metrics: Option<ScanMetrics>,
    pub scan_config: Option<ScanConfig>,

    pub api_service: Option<Arc<dyn ApiService>>,

    pub repo_accessor: Accessor<ReadWrite>,
    #[cfg(feature = "demo")]
    pub demo_controls: DemoControlsState,
}

#[cfg(feature = "demo")]
#[derive(Debug, Clone, Default)]
pub struct DemoControlsState {
    pub is_loading: bool,
    pub is_updating: bool,
    pub error: Option<String>,
    pub demo_library_ids: HashSet<LibraryID>,
    pub movies_current: Option<usize>,
    pub series_current: Option<usize>,
    pub movies_input: String,
    pub series_input: String,
    pub demo_root: Option<PathBuf>,
    pub demo_username: Option<String>,
}

impl LibraryDomainState {
    pub fn new(
        api_service: Option<Arc<dyn ApiService>>,
        repo_accessor: Accessor<ReadWrite>,
    ) -> Self {
        Self {
            current_library_id: None,
            show_library_management: false,
            library_form_data: None,
            library_form_errors: Vec::new(),
            library_form_success: None,
            library_media_cache: HashMap::new(),
            active_scans: HashMap::new(),
            latest_progress: HashMap::new(),
            initial_library_fetch: false,
            scan_metrics: None,
            scan_config: None,
            api_service,
            repo_accessor,
            #[cfg(feature = "demo")]
            demo_controls: DemoControlsState::default(),
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
