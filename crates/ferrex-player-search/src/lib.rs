//! Search data domain for Ferrex player clients.

pub mod calibrator;
pub mod error;
pub mod keyboard;
pub mod messages;
pub mod metrics;
pub mod service;
pub mod types;
pub mod update;

use ferrex_player_api::services::api::ApiService;
use ferrex_player_foundation::domain::DomainTask;
use ferrex_player_library::repository::{Accessor, ReadOnly};
use std::sync::Arc;

pub use self::keyboard::{
    TenFootKeyboardAction, TenFootKeyboardDirection, TenFootKeyboardKey,
    TenFootKeyboardState,
};
pub use self::messages::{SearchEvent, SearchMessage};
pub use self::service::SearchService;
pub use self::types::{
    SearchMode, SearchPresentation, SearchResponse, SearchState, SearchStrategy,
};

/// Cross-domain event view needed by the search data domain.
pub trait SearchExternalEvent {
    fn selected_library_changed(&self) -> bool {
        false
    }
    fn selected_home(&self) -> bool {
        false
    }
}

/// Search domain state container.
#[derive(Debug)]
pub struct SearchDomain {
    pub state: SearchState,
    pub service: Arc<SearchService>,
}

impl SearchDomain {
    pub fn new(
        api_service: Option<Arc<dyn ApiService>>,
        search_accessor: Option<Arc<Accessor<ReadOnly>>>,
    ) -> Self {
        Self {
            state: SearchState::default(),
            service: Arc::new(SearchService::new(api_service, search_accessor)),
        }
    }

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

    pub fn calibrate(&self) -> DomainTask<SearchMessage> {
        let service = self.service.clone();

        DomainTask::perform(
            async move { calibrator::SearchCalibrator::calibrate(&service).await },
            SearchMessage::_CalibrationComplete,
        )
    }

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

#[derive(Clone, Debug)]
pub enum SearchDomainEvent {
    SearchInProgress(bool),
    NavigateToMedia(ferrex_player_api::api_types::Media),
    NoOp,
}
