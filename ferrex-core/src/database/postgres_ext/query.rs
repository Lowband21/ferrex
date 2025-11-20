use crate::{Result, database::postgres::PostgresDatabase, query::*};

impl PostgresDatabase {
    /// Execute a media query - delegates to optimized implementation
    pub async fn query_media(&self, query: &MediaQuery) -> Result<Vec<MediaWithStatus>> {
        // Use the optimized query implementation that leverages indexes
        self.query_media_optimized(query).await
    }
}
