use crate::{watch_status::WatchStatusFilter, LibraryID};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Main query structure that works everywhere
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MediaQuery {
    pub filters: MediaFilters,
    pub sort: SortCriteria,
    pub search: Option<SearchQuery>,
    pub pagination: Pagination,
    pub user_context: Option<Uuid>, // For watch status integration
}

/// Media filtering options
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MediaFilters {
    pub media_type: Option<MediaTypeFilter>,
    pub watch_status: Option<WatchStatusFilter>,
    pub genres: Vec<String>,
    pub year_range: Option<(u16, u16)>,
    pub rating_range: Option<(f32, f32)>,
    pub library_ids: Vec<Uuid>,
}

/// Filter by media type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MediaTypeFilter {
    Movie,
    Series,
    Season,
    Episode,
}

/// Sort criteria for queries
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SortCriteria {
    pub primary: SortField,
    pub order: SortOrder,
    pub secondary: Option<SortField>, // For stable sorting
}

impl Default for SortCriteria {
    fn default() -> Self {
        Self {
            primary: SortField::Title,
            order: SortOrder::Ascending,
            secondary: None,
        }
    }
}

/// Fields available for sorting
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortField {
    Title,
    DateAdded,
    ReleaseDate,
    LastWatched,   // Requires user context
    WatchProgress, // Requires user context
    Rating,
    Runtime,
}

/// Sort order
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SortOrder {
    Ascending,
    Descending,
}

/// Search query options
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchQuery {
    pub text: String,
    pub fields: Vec<SearchField>,
    pub fuzzy: bool,
}

/// Fields to search in
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchField {
    Title,
    Overview,
    Cast,
    Crew,
    Genre,
    All,
}

/// Pagination options
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Pagination {
    pub offset: usize,
    pub limit: usize,
}

impl Default for Pagination {
    fn default() -> Self {
        Self {
            offset: 0,
            limit: 20,
        }
    }
}

/// Query result types - indicates where data comes from
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QueryResult<T> {
    /// Client-side immediate results
    Local(Vec<T>),
    /// Server query needed
    Remote(QueryEndpoint),
    /// Hybrid approach - some local, may need more from server
    Partial {
        local: Vec<T>,
        remote: Option<QueryEndpoint>,
    },
}

/// Remote query endpoint specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryEndpoint {
    pub url: String,
    pub params: HashMap<String, String>,
}

/// Combined media reference with user watch status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaWithStatus {
    pub media: crate::Media,
    pub watch_status: Option<crate::watch_status::InProgressItem>,
    pub is_completed: bool,
}

/// Query execution error
#[derive(Debug, Clone, Serialize, Deserialize, thiserror::Error)]
pub enum QueryError {
    #[error("Invalid query parameters")]
    InvalidQuery(String),

    #[error("Network error")]
    NetworkError,

    #[error("Server error")]
    ServerError(String),

    #[error("Deserialization error")]
    DeserializationError,
}
