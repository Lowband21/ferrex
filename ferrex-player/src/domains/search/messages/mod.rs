//! Search domain messages

use super::metrics::SearchPerformanceMetrics;
use super::types::{SearchMode, SearchResult};
use crate::infra::api_types::Media;

pub mod subscriptions;

/// Search domain messages
#[derive(Clone)]
pub enum Message {
    // User actions
    /// Update search query text
    UpdateQuery(String),
    /// Execute search (called after debounce)
    ExecuteSearch,
    /// Clear search and results
    ClearSearch,
    /// Select a search result
    SelectResult(Media),
    /// Load more results (pagination)
    LoadMore,
    /// Toggle between dropdown and fullscreen modes
    ToggleMode,
    /// Set specific mode
    SetMode(SearchMode),
    /// Navigate selection up
    SelectPrevious,
    /// Navigate selection down
    SelectNext,
    /// Select current highlighted result
    SelectCurrent,

    // Internal events
    /// Debounced search trigger
    SearchDebounced(String),
    /// Results received from search execution
    ResultsReceived {
        query: String,
        results: Vec<SearchResult>,
        total_count: usize,
    },
    /// Search error occurred
    SearchError(String),
    /// Set searching state
    SetSearching(bool),
    /// Record search performance metrics
    RecordMetrics(SearchPerformanceMetrics),

    // Cross-domain coordination
    /// Request media details for result preview
    RequestMediaDetails(Media),
    /// Refresh search when media content changes
    RefreshFromMediaStore,

    // Internal calibration complete
    #[doc(hidden)]
    _CalibrationComplete(super::calibrator::CalibrationResults),

    /// Run calibration to determine optimal search strategy
    RunCalibration,
}

impl std::fmt::Debug for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // User actions
            Self::UpdateQuery(query) => write!(f, "UpdateQuery({})", query),
            Self::ExecuteSearch => write!(f, "ExecuteSearch"),
            Self::ClearSearch => write!(f, "ClearSearch"),
            Self::SelectResult(media_ref) => {
                write!(f, "SelectResult({:?})", media_ref)
            }
            Self::LoadMore => write!(f, "LoadMore"),
            Self::ToggleMode => write!(f, "ToggleMode"),
            Self::SetMode(mode) => write!(f, "SetMode({:?})", mode),
            Self::SelectPrevious => write!(f, "SelectPrevious"),
            Self::SelectNext => write!(f, "SelectNext"),
            Self::SelectCurrent => write!(f, "SelectCurrent"),

            // Internal events
            Self::SearchDebounced(query) => {
                write!(f, "SearchDebounced({})", query)
            }
            Self::ResultsReceived {
                query,
                results,
                total_count,
            } => {
                write!(
                    f,
                    "ResultsReceived(query: {}, results: {}, total: {})",
                    query,
                    results.len(),
                    total_count
                )
            }
            Self::SearchError(error) => write!(f, "SearchError({})", error),
            Self::SetSearching(searching) => {
                write!(f, "SetSearching({})", searching)
            }
            Self::RecordMetrics(_) => write!(f, "RecordMetrics(...)"),

            // Cross-domain coordination
            Self::RequestMediaDetails(media_ref) => {
                write!(f, "RequestMediaDetails({:?})", media_ref)
            }
            Self::RefreshFromMediaStore => write!(f, "RefreshFromMediaStore"),

            // Internal calibration
            Self::_CalibrationComplete(_) => {
                write!(f, "_CalibrationComplete(...)")
            }
            Self::RunCalibration => write!(f, "RunCalibration"),
        }
    }
}

/// Search domain events that other domains can listen to
#[derive(Clone, Debug)]
pub enum SearchEvent {
    /// Search query changed
    QueryChanged(String),
    /// Search started
    SearchStarted,
    /// Search completed with result count
    SearchCompleted(usize),
    /// User selected a search result
    ResultSelected(Media),
    /// Search mode changed
    ModeChanged(SearchMode),
    /// Search cleared
    SearchCleared,
}

impl Message {
    /// Convert to string for debugging
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::UpdateQuery(_) => "UpdateQuery",
            Self::ExecuteSearch => "ExecuteSearch",
            Self::ClearSearch => "ClearSearch",
            Self::SelectResult(_) => "SelectResult",
            Self::LoadMore => "LoadMore",
            Self::ToggleMode => "ToggleMode",
            Self::SetMode(_) => "SetMode",
            Self::SelectPrevious => "SelectPrevious",
            Self::SelectNext => "SelectNext",
            Self::SelectCurrent => "SelectCurrent",
            Self::SearchDebounced(_) => "SearchDebounced",
            Self::ResultsReceived { .. } => "ResultsReceived",
            Self::SearchError(_) => "SearchError",
            Self::SetSearching(_) => "SetSearching",
            Self::RecordMetrics(_) => "RecordMetrics",
            Self::RequestMediaDetails(_) => "RequestMediaDetails",
            Self::RefreshFromMediaStore => "RefreshFromMediaStore",
            Self::_CalibrationComplete(_) => "_CalibrationComplete",
            Self::RunCalibration => "RunCalibration",
        }
    }
}
