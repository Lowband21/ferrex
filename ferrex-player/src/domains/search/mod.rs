//! Search domain - handles all search functionality

pub mod calibrator;
pub mod messages;
pub mod metrics;
pub mod service;
pub mod types;
pub mod update;

use crate::common::messages::{CrossDomainEvent, DomainMessage};
use iced::Task;
use std::sync::Arc;

pub use self::messages::{Message, SearchEvent};
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
        //media_store: Arc<StdRwLock<crate::domains::media::store::MediaStore>>,
        api_service: Option<Arc<dyn crate::infrastructure::services::api::ApiService>>,
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
        //media_store: Arc<StdRwLock<crate::domains::media::store::MediaStore>>,
        api_service: Option<Arc<dyn crate::infrastructure::services::api::ApiService>>,
    ) -> Self {
        let mut state = SearchState::default();
        // Enable the enhanced decision engine with metrics
        state.decision_engine = types::SearchDecisionEngine::new_with_metrics();

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
                DomainMessage::Search(Message::_CalibrationComplete(results))
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
    pub fn handle_event(&mut self, event: &CrossDomainEvent) -> Task<DomainMessage> {
        match event {
            // NOTE: MediaStoreRefreshed moved to direct Search messages in Task 2.10
            CrossDomainEvent::LibrarySelected(library_id) => {
                // Update search scope to selected library
                self.state.library_id = Some(*library_id);
                if !self.state.query.is_empty() {
                    // Re-run search with new library scope
                    Task::done(DomainMessage::Search(Message::ExecuteSearch))
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
            SearchEvent::SearchStarted => CrossDomainEvent::SearchInProgress(true),
            SearchEvent::SearchCompleted(_) => CrossDomainEvent::SearchInProgress(false),
            _ => CrossDomainEvent::NoOp,
        }
    }
}
