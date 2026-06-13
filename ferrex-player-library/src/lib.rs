//! Library data domain for Ferrex player clients.
//!
//! This crate owns the library state container, library messages,
//! repository snapshot/index structures, and server-backed library scan/media
//! subscriptions. The desktop app supplies app-shell context around these data
//! primitives instead of forcing the data domain to import the final root state.

pub mod media_root_browser;
pub mod messages;
pub mod repo_snapshot;
pub mod repository;
pub mod server;
pub mod types;
pub mod update;
pub mod update_handlers;

use self::{
    media_root_browser::State as MediaRootBrowserState, types::LibraryFormData,
};
#[cfg(feature = "demo")]
use ferrex_core::player_prelude::LibraryId;
use ferrex_core::player_prelude::{
    Library, LibraryMediaCache, ScanConfig, ScanMetrics, ScanProgressEvent,
    ScanSnapshotDto,
};
use ferrex_player_api::services::api::ApiService;
use ferrex_player_foundation::domain::DomainTask;
use repository::accessor::{Accessor, ReadWrite};
use std::{collections::HashMap, sync::Arc};
#[cfg(feature = "demo")]
use std::{collections::HashSet, path::PathBuf};
use uuid::Uuid;

/// Cross-domain events relevant to the library data domain.
pub trait LibraryExternalEvent {
    /// Whether the event represents an authenticated user.
    fn is_user_authenticated(&self) -> bool {
        false
    }

    /// Whether the backing database/cache was cleared.
    fn is_database_cleared(&self) -> bool {
        false
    }

    /// Whether library state should be cleared for the current session.
    fn is_clear_libraries(&self) -> bool {
        false
    }
}

/// Library domain state owned by this crate.
#[derive(Debug)]
pub struct LibraryDomainState {
    pub show_library_management: bool,
    pub library_form_data: Option<LibraryFormData>,
    pub library_form_errors: Vec<String>,
    pub library_form_success: Option<String>,
    pub library_media_cache: HashMap<Uuid, LibraryMediaCache>,
    pub active_scans: HashMap<Uuid, ScanSnapshotDto>,
    pub latest_progress: HashMap<Uuid, ScanProgressEvent>,
    pub load_state: LibrariesLoadState,

    pub scan_metrics: Option<ScanMetrics>,
    pub scan_config: Option<ScanConfig>,
    pub media_root_browser: MediaRootBrowserState,

    pub api_service: Option<Arc<dyn ApiService>>,

    pub libraries: Vec<Library>,

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
    pub demo_library_ids: HashSet<LibraryId>,
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
            show_library_management: false,
            library_form_data: None,
            library_form_errors: Vec::new(),
            library_form_success: None,
            library_media_cache: HashMap::new(),
            active_scans: HashMap::new(),
            latest_progress: HashMap::new(),
            load_state: LibrariesLoadState::NotStarted,
            scan_metrics: None,
            scan_config: None,
            media_root_browser: MediaRootBrowserState::default(),
            api_service,
            libraries: Vec::new(),
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

impl LibraryDomain {
    pub fn new(state: LibraryDomainState) -> Self {
        Self { state }
    }

    /// Handle a cross-domain event through an explicit data-domain view of the event.
    pub fn handle_event<E>(
        &mut self,
        event: &E,
    ) -> DomainTask<messages::LibraryMessage>
    where
        E: LibraryExternalEvent,
    {
        if event.is_database_cleared() || event.is_clear_libraries() {
            self.state.library_media_cache.clear();
            self.state.load_state = LibrariesLoadState::NotStarted;
        }
        DomainTask::none()
    }
}

#[derive(Debug, Clone)]
pub enum LibrariesLoadState {
    NotStarted,
    InProgress,
    Succeeded {
        user_id: Option<Uuid>,
        server_url: String,
    },
    Failed {
        last_error: String,
    },
}
