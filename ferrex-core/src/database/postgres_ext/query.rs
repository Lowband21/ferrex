use crate::{
    database::postgres::PostgresDatabase,
    error::Result,
    query::types::{MediaQuery, MediaWithStatus},
};

impl PostgresDatabase {
    /// Execute a media query - delegates to optimized implementation
    pub async fn query_media(&self, query: &MediaQuery) -> Result<Vec<MediaWithStatus>> {
        // Use the optimized query implementation that leverages indexes
        self.query_media_optimized(query).await
    }
}
