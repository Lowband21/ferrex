use async_trait::async_trait;

use crate::Result;

#[async_trait]
pub trait QueryRepository: Send + Sync {
    async fn query_media(
        &self,
        query: &crate::query::MediaQuery,
    ) -> Result<Vec<crate::query::MediaWithStatus>>;
}
