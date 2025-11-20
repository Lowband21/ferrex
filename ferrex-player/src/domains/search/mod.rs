//! Search domain - coordinates global, server-backed search

pub mod calibrator;
pub mod messages;
pub mod metrics;
pub mod service;
pub mod types;
pub mod update;

use crate::common::messages::{CrossDomainEvent, DomainMessage};
use iced::Task;
use std::sync::Arc;

pub use self::messages::{SearchMessage, SearchEvent};
pub use self::service::SearchService;
pub use self::types::{SearchMode, SearchResult, SearchState, SearchStrategy};

/// Search domain state container
#[derive(Debug)]
pub struct SearchDomain {
    /// Search state
    pub state: SearchState,
    /// Search service for executing searches
    pub service: Arc<SearchService>,
}

impl SearchDomain {
    #[cfg_attr(
        any(
            feature = "profile-with-puffin",
            feature = "profile-with-tracy",
            feature = "profile-with-tracing"
        ),
        profiling::function
    )]
    pub fn new(
        api_service: Option<Arc<dyn crate::infra::services::api::ApiService>>,
    ) -> Self {
        Self {
            state: SearchState::default(),
            service: Arc::new(SearchService::new(api_service)),
        }
    }

    #[cfg_attr(
        any(
            feature = "profile-with-puffin",
            feature = "profile-with-tracy",
            feature = "profile-with-tracing"
        ),
        profiling::function
    )]
    pub fn new_with_metrics(
        api_service: Option<Arc<dyn crate::infra::services::api::ApiService>>,
    ) -> Self {
        let state = SearchState {
            decision_engine: types::SearchDecisionEngine::new_with_metrics(),
            ..SearchState::default()
        };

        Self {
            state,
            service: Arc::new(SearchService::new(api_service)),
        }
    }

    pub async fn calibrate(&mut self) -> Task<DomainMessage> {
        let service = self.service.clone();

        Task::perform(
            async move { calibrator::SearchCalibrator::calibrate(&service).await },
            move |results| {
                // Store calibration results in the decision engine
                DomainMessage::Search(SearchMessage::_CalibrationComplete(results))
            },
        )
    }

    /// Handle cross-domain events
    #[cfg_attr(
        any(
            feature = "profile-with-puffin",
            feature = "profile-with-tracy",
            feature = "profile-with-tracing"
        ),
        profiling::function
    )]
    pub fn handle_event(
        &mut self,
        event: &CrossDomainEvent,
    ) -> Task<DomainMessage> {
        match event {
            CrossDomainEvent::LibrarySelected(_library_id) => {
                // Keep search global; no library scoping.
                if !self.state.query.is_empty() {
                    Task::done(DomainMessage::Search(SearchMessage::ExecuteSearch))
                } else {
                    Task::none()
                }
            }
            CrossDomainEvent::LibrarySelectAll => {
                // Already global; just rerun if needed.
                if !self.state.query.is_empty() {
                    Task::done(DomainMessage::Search(SearchMessage::ExecuteSearch))
                } else {
                    Task::none()
                }
            }
            _ => Task::none(),
        }
    }

    /// Emit cross-domain event
    #[cfg_attr(
        any(
            feature = "profile-with-puffin",
            feature = "profile-with-tracy",
            feature = "profile-with-tracing"
        ),
        profiling::function
    )]
    pub fn emit_event(&self, event: SearchEvent) -> CrossDomainEvent {
        match event {
            SearchEvent::ResultSelected(media_ref) => {
                // Notify UI to navigate to the selected media
                CrossDomainEvent::NavigateToMedia(media_ref)
            }
            SearchEvent::SearchStarted => {
                CrossDomainEvent::SearchInProgress(true)
            }
            SearchEvent::SearchCompleted(_) => {
                CrossDomainEvent::SearchInProgress(false)
            }
            _ => CrossDomainEvent::NoOp,
        }
    }
}
