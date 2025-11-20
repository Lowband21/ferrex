use async_trait::async_trait;

use crate::error::Result;
use crate::query::types::{MediaQuery, MediaWithStatus};

#[async_trait]
pub trait QueryRepository: Send + Sync {
    async fn query_media(&self, query: &MediaQuery) -> Result<Vec<MediaWithStatus>>;
}
