use std::sync::Arc;

use async_trait::async_trait;

use crate::Result;
use crate::database::ports::query::QueryRepository;
use crate::database::postgres::PostgresDatabase;

#[derive(Clone)]
pub struct PostgresQueryRepository {
    db: Arc<PostgresDatabase>,
}

impl PostgresQueryRepository {
    pub fn new(db: Arc<PostgresDatabase>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl QueryRepository for PostgresQueryRepository {
    async fn query_media(
        &self,
        query: &crate::query::MediaQuery,
    ) -> Result<Vec<crate::query::MediaWithStatus>> {
        self.db.query_media(query).await
    }
}
