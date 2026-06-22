//! Search data domain for Ferrex player clients.
//!
//! This crate owns search state, service strategy selection, calibration,
//! keyboard navigation helpers, metrics, messages, and UI-agnostic reducer logic.
//! App shells provide API/repository access and render the resulting state.

/// Search strategy calibration helpers.
pub mod calibrator;
/// Search error types.
pub mod error;
/// 10-foot on-screen keyboard model.
pub mod keyboard;
/// Search messages and subscriptions.
pub mod messages;
/// Search timing/strategy metrics.
pub mod metrics;
/// Search service that chooses server or repository-backed execution.
pub mod service;
/// Search state and DTO types.
pub mod types;
/// UI-agnostic search reducer logic.
pub mod update;

use ferrex_player_api::services::api::ApiService;
use ferrex_player_foundation::domain::DomainTask;
use ferrex_player_library::repository::{Accessor, ReadOnly};
use std::sync::Arc;

/// 10-foot keyboard types re-exported for UI crates.
pub use self::keyboard::{
    TenFootKeyboardAction, TenFootKeyboardDirection, TenFootKeyboardKey,
    TenFootKeyboardState,
};
/// Search message/event types.
pub use self::messages::{SearchEvent, SearchMessage};
/// Search service type.
pub use self::service::SearchService;
/// Search state, presentation, response, and strategy types.
pub use self::types::{
    SearchMode, SearchPresentation, SearchResponse, SearchState, SearchStrategy,
};

/// Cross-domain event view needed by the search data domain.
pub trait SearchExternalEvent {
    /// Whether the selected library changed and stale search results should refresh.
    fn selected_library_changed(&self) -> bool {
        false
    }
    /// Whether the user navigated to the home surface and active queries should rerun.
    fn selected_home(&self) -> bool {
        false
    }
}

/// Search domain state container.
#[derive(Debug)]
pub struct SearchDomain {
    /// Mutable search state used by reducers and views.
    pub state: SearchState,
    /// Search service used to execute queries.
    pub service: Arc<SearchService>,
}

impl SearchDomain {
    /// Build a search domain using default strategy selection.
    pub fn new(
        api_service: Option<Arc<dyn ApiService>>,
        search_accessor: Option<Arc<Accessor<ReadOnly>>>,
    ) -> Self {
        Self {
            state: SearchState::default(),
            service: Arc::new(SearchService::new(api_service, search_accessor)),
        }
    }

    /// Build a search domain with metrics-enabled decision tracking.
    pub fn new_with_metrics(
        api_service: Option<Arc<dyn ApiService>>,
        search_accessor: Option<Arc<Accessor<ReadOnly>>>,
    ) -> Self {
        let state = SearchState {
            decision_engine: types::SearchDecisionEngine::new_with_metrics(),
            ..SearchState::default()
        };

        Self {
            state,
            service: Arc::new(SearchService::new(api_service, search_accessor)),
        }
    }

    /// Calibrate search strategy thresholds using the configured service.
    pub fn calibrate(&self) -> DomainTask<SearchMessage> {
        let service = self.service.clone();

        DomainTask::perform(
            async move { calibrator::SearchCalibrator::calibrate(&service).await },
            SearchMessage::_CalibrationComplete,
        )
    }

    /// Respond to cross-domain events that should refresh active searches.
    pub fn handle_event<E>(&mut self, event: &E) -> DomainTask<SearchMessage>
    where
        E: SearchExternalEvent,
    {
        if (event.selected_library_changed() || event.selected_home())
            && !self.state.query.is_empty()
        {
            DomainTask::done(SearchMessage::ExecuteSearch)
        } else {
            DomainTask::none()
        }
    }

    /// Translate internal search events into app-shell domain events.
    pub fn emit_event(&self, event: SearchEvent) -> SearchDomainEvent {
        match event {
            SearchEvent::ResultSelected(media_ref) => {
                SearchDomainEvent::NavigateToMedia(media_ref)
            }
            SearchEvent::SearchStarted => {
                SearchDomainEvent::SearchInProgress(true)
            }
            SearchEvent::SearchCompleted(_) => {
                SearchDomainEvent::SearchInProgress(false)
            }
            _ => SearchDomainEvent::NoOp,
        }
    }
}

/// Events emitted from the search domain back to an app shell.
#[derive(Clone, Debug)]
pub enum SearchDomainEvent {
    /// Search execution started or stopped.
    SearchInProgress(bool),
    /// User selected a media result and the shell should navigate to it.
    NavigateToMedia(ferrex_player_api::api_types::Media),
    /// No shell action is required.
    NoOp,
}
