//! Search domain messages

use crate::infrastructure::api_types::MediaReference;
use super::types::{SearchResult, SearchMode};
use super::metrics::SearchPerformanceMetrics;

/// Search domain messages
#[derive(Clone, Debug)]
pub enum Message {
    // User actions
    /// Update search query text
    UpdateQuery(String),
    /// Execute search (called after debounce)
    ExecuteSearch,
    /// Clear search and results
    ClearSearch,
    /// Select a search result
    SelectResult(MediaReference),
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
    RequestMediaDetails(MediaReference),
    /// Refresh search from MediaStore changes
    RefreshFromMediaStore,
    
    // Internal calibration complete
    #[doc(hidden)]
    _CalibrationComplete(super::calibrator::CalibrationResults),
    
    /// Run calibration to determine optimal search strategy
    RunCalibration,
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
    ResultSelected(MediaReference),
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