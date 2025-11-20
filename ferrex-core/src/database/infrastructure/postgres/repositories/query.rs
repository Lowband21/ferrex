use std::fmt;
use std::sync::Arc;

use async_trait::async_trait;

use crate::{
    database::{ports::query::QueryRepository, postgres::PostgresDatabase},
    error::Result,
    query::types::{MediaQuery, MediaWithStatus},
};

#[derive(Clone)]
pub struct PostgresQueryRepository {
    db: Arc<PostgresDatabase>,
}

impl PostgresQueryRepository {
    pub fn new(db: Arc<PostgresDatabase>) -> Self {
        Self { db }
    }
}

impl fmt::Debug for PostgresQueryRepository {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let pool = self.db.pool();
        f.debug_struct("PostgresQueryRepository")
            .field("pool_size", &pool.size())
            .field("idle_connections", &pool.num_idle())
            .finish()
    }
}

#[async_trait]
impl QueryRepository for PostgresQueryRepository {
    async fn query_media(&self, query: &MediaQuery) -> Result<Vec<MediaWithStatus>> {
        self.db.query_media(query).await
    }
}
