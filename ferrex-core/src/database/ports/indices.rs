use async_trait::async_trait;
use uuid::Uuid;

use crate::{
    api::types::FilterIndicesRequest,
    error::Result,
    query::types::{SortBy, SortOrder},
    types::LibraryID,
};

/// Repository for working with precomputed index data such as
/// `movie_sort_positions` and filtered index lookups.
#[async_trait]
pub trait IndicesRepository: Send + Sync {
    /// Rebuild the precomputed movie sort positions for a library.
    async fn rebuild_movie_sort_positions(
        &self,
        library_id: LibraryID,
    ) -> Result<()>;

    /// Fetch presorted movie indices for the given library using the
    /// precomputed position columns. Returns zero-based offsets.
    async fn fetch_sorted_movie_indices(
        &self,
        library_id: LibraryID,
        sort: SortBy,
        order: SortOrder,
        offset: Option<usize>,
        limit: Option<usize>,
    ) -> Result<Vec<u32>>;

    /// Fetch filtered movie indices for the given library based on the
    /// provided filter specification. Returns zero-based offsets matching the
    /// order defined in the query.
    async fn fetch_filtered_movie_indices(
        &self,
        library_id: LibraryID,
        spec: &FilterIndicesRequest,
        user_id: Option<Uuid>,
    ) -> Result<Vec<u32>>;
}
