pub mod postgres;
pub mod surrealdb;
pub mod cache;
pub mod traits;

pub use traits::{MediaDatabaseTrait, DatabaseBackend};
pub use postgres::PostgresDatabase;
pub use surrealdb::SurrealDatabase;
pub use cache::RedisCache;

use crate::Result;
use std::sync::Arc;

#[derive(Clone)]
pub struct MediaDatabase {
    backend: Arc<dyn MediaDatabaseTrait>,
    cache: Option<Arc<RedisCache>>,
}

impl MediaDatabase {
    pub async fn new_postgres(connection_string: &str, with_cache: bool) -> Result<Self> {
        let backend = Arc::new(PostgresDatabase::new(connection_string).await?);
        
        let cache = if with_cache {
            Some(Arc::new(RedisCache::new("redis://127.0.0.1/").await?))
        } else {
            None
        };
        
        Ok(Self { backend, cache })
    }
    
    pub async fn new_surrealdb() -> Result<Self> {
        let backend = Arc::new(SurrealDatabase::new().await?);
        Ok(Self { backend, cache: None })
    }
    
    pub fn backend(&self) -> &dyn MediaDatabaseTrait {
        self.backend.as_ref()
    }
    
    pub fn cache(&self) -> Option<&RedisCache> {
        self.cache.as_ref().map(|c| c.as_ref())
    }
}