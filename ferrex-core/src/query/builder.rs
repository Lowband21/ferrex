use super::types::*;
use crate::{LibraryID, api_types::ScalarRange, watch_status::WatchStatusFilter};
use uuid::Uuid;

/// Fluent API for building media queries
#[derive(Debug, Clone)]
pub struct MediaQueryBuilder {
    query: MediaQuery,
}

impl MediaQueryBuilder {
    /// Create a new query builder
    pub fn new() -> Self {
        Self {
            query: MediaQuery::default(),
        }
    }

    /// Set user context for watch-status aware queries
    pub fn for_user(mut self, user_id: Uuid) -> Self {
        self.query.user_context = Some(user_id);
        self
    }

    // === Filter methods ===

    /// Filter to only show movies
    pub fn movies_only(mut self) -> Self {
        self.query.filters.media_type = Some(MediaTypeFilter::Movie);
        self
    }

    /// Filter to only show series
    pub fn series_only(mut self) -> Self {
        self.query.filters.media_type = Some(MediaTypeFilter::Series);
        self
    }

    /// Filter to only show episodes
    pub fn episodes_only(mut self) -> Self {
        self.query.filters.media_type = Some(MediaTypeFilter::Episode);
        self
    }

    /// Filter by watch status
    pub fn watch_status(mut self, status: WatchStatusFilter) -> Self {
        self.query.filters.watch_status = Some(status);
        self
    }

    /// Convenience method for "continue watching"
    pub fn watching(mut self) -> Self {
        self.query.filters.watch_status = Some(WatchStatusFilter::InProgress);
        self.sort_by(SortBy::LastWatched, SortOrder::Descending)
    }

    /// Filter by genre
    pub fn genre(mut self, genre: impl Into<String>) -> Self {
        self.query.filters.genres.push(genre.into());
        self
    }

    /// Filter by multiple genres
    pub fn genres(mut self, genres: Vec<String>) -> Self {
        self.query.filters.genres = genres;
        self
    }

    /// Filter by year range
    pub fn year_range(mut self, start: u16, end: u16) -> Self {
        self.query.filters.year_range = Some(ScalarRange::new(start, end));
        self
    }

    /// Filter by rating range
    pub fn rating_range(mut self, min: f32, max: f32) -> Self {
        self.query.filters.rating_range = Some(ScalarRange::new(min, max).to_rating_value());
        self
    }

    /// Filter by resolution range (vertical pixels)
    pub fn resolution_range(mut self, min: u16, max: u16) -> Self {
        self.query.filters.resolution_range = Some(ScalarRange::new(min, max));
        self
    }

    /// Filter by library
    pub fn in_library(mut self, library_id: LibraryID) -> Self {
        self.query.filters.library_ids.push(library_id.as_uuid());
        self
    }

    /// Filter by multiple libraries
    pub fn in_libraries(mut self, library_ids: Vec<Uuid>) -> Self {
        self.query.filters.library_ids = library_ids;
        self
    }

    // === Search methods ===

    /// Add text search
    pub fn search(mut self, text: impl Into<String>) -> Self {
        self.query.search = Some(SearchQuery {
            text: text.into(),
            fields: vec![SearchField::All],
            fuzzy: true,
        });
        self
    }

    /// Add text search with specific fields
    pub fn search_in(mut self, text: impl Into<String>, fields: Vec<SearchField>) -> Self {
        self.query.search = Some(SearchQuery {
            text: text.into(),
            fields,
            fuzzy: true,
        });
        self
    }

    /// Add exact text search (no fuzzy matching)
    pub fn search_exact(mut self, text: impl Into<String>) -> Self {
        self.query.search = Some(SearchQuery {
            text: text.into(),
            fields: vec![SearchField::All],
            fuzzy: false,
        });
        self
    }

    // === Sort methods ===

    /// Set primary sort field and order
    pub fn sort_by(mut self, field: SortBy, order: SortOrder) -> Self {
        self.query.sort.primary = field;
        self.query.sort.order = order;
        self
    }

    /// Add secondary sort for stable sorting
    pub fn then_by(mut self, field: SortBy) -> Self {
        self.query.sort.secondary = Some(field);
        self
    }

    // === Pagination methods ===

    /// Set result limit
    pub fn limit(mut self, limit: usize) -> Self {
        self.query.pagination.limit = limit;
        self
    }

    /// Set result offset
    pub fn offset(mut self, offset: usize) -> Self {
        self.query.pagination.offset = offset;
        self
    }

    /// Set page (convenience method)
    pub fn page(mut self, page: usize, per_page: usize) -> Self {
        self.query.pagination.offset = page * per_page;
        self.query.pagination.limit = per_page;
        self
    }

    // === Build method ===

    /// Build the final query
    pub fn build(self) -> MediaQuery {
        self.query
    }
}

impl Default for MediaQueryBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// === Convenience constructors ===

impl MediaQuery {
    /// Create a "continue watching" query
    pub fn continue_watching(user_id: Uuid, limit: usize) -> Self {
        MediaQueryBuilder::new()
            .for_user(user_id)
            .watching()
            .limit(limit)
            .build()
    }

    /// Create a simple search query
    pub fn search(text: impl Into<String>) -> Self {
        MediaQueryBuilder::new().search(text).build()
    }

    /// Create a library browse query
    pub fn browse_library(library_id: LibraryID, page: usize, per_page: usize) -> Self {
        MediaQueryBuilder::new()
            .in_library(library_id)
            .page(page, per_page)
            .build()
    }
}
