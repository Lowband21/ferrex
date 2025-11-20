pub mod cache;
pub mod infrastructure;
pub mod ports;
pub mod postgres;
pub mod postgres_ext;
pub mod traits;

pub use cache::RedisCache;
pub use postgres::{PoolStats, PostgresDatabase};
pub use traits::MediaDatabaseTrait;

use crate::{LibraryID, MovieID, Result, SeriesID};
use std::{fmt, sync::Arc};

#[derive(Clone)]
pub struct MediaDatabase {
    backend: Arc<dyn MediaDatabaseTrait>,
    cache: Option<Arc<RedisCache>>,
}

impl fmt::Debug for MediaDatabase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let backend_type = std::any::type_name_of_val(self.backend.as_ref());
        f.debug_struct("MediaDatabase")
            .field("backend_type", &backend_type)
            .field("cache_configured", &self.cache.is_some())
            .finish()
    }
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

    /// Construct a media database from an arbitrary backend implementation.
    /// Primarily used in tests where spinning up a real database is unnecessary.
    pub fn with_backend(backend: Arc<dyn MediaDatabaseTrait>) -> Self {
        Self {
            backend,
            cache: None,
        }
    }

    pub fn backend(&self) -> &dyn MediaDatabaseTrait {
        self.backend.as_ref()
    }

    pub fn cache(&self) -> Option<&RedisCache> {
        self.cache.as_ref().map(|c| c.as_ref())
    }

    /// Get the backend as Any for downcasting
    pub fn as_any(&self) -> &dyn std::any::Any {
        self.backend.as_any()
    }

    /// Execute a media query with caching support
    pub async fn query_media_with_cache(
        &self,
        query: &crate::query::MediaQuery,
        library_id: LibraryID,
    ) -> Result<Vec<crate::query::MediaWithStatus>> {
        use crate::query::QueryComplexityGuard;
        use cache::CacheKeys;
        use tracing::{debug, warn};

        // Check query complexity before execution
        let complexity_guard = QueryComplexityGuard::new();
        complexity_guard.check_query(query)?;

        // If no cache is configured, fall back to direct query
        if self.cache.is_none() {
            return self.backend.query_media(query).await;
        }

        // Generate cache key based on query parameters
        let cache_key = CacheKeys::media_query(query, Some(library_id), query.user_context);
        debug!("Query cache key: {}", cache_key);

        // Try to get from cache
        if let Some(cache) = &self.cache {
            // Clone the Arc to get a mutable reference inside
            let mut cache_conn = cache.as_ref().clone();

            match cache_conn
                .get::<Vec<crate::query::MediaWithStatus>>(&cache_key)
                .await
            {
                Ok(Some(cached_results)) => {
                    debug!("Query cache hit for key: {}", cache_key);
                    return Ok(cached_results);
                }
                Ok(None) => {
                    debug!("Query cache miss for key: {}", cache_key);
                }
                Err(e) => {
                    warn!("Cache read error: {}, falling back to database", e);
                }
            }

            // Execute the query
            let results = self.backend.query_media(query).await?;

            // Store in cache with TTL based on query complexity
            let ttl = self.calculate_cache_ttl(query);

            match cache_conn.set(&cache_key, &results, Some(ttl)).await {
                Ok(_) => {
                    debug!("Query results cached with TTL: {:?}", ttl);
                }
                Err(e) => {
                    warn!("Failed to cache query results: {}", e);
                    // Continue anyway - caching is optional
                }
            }

            Ok(results)
        } else {
            // No cache configured, execute directly
            self.backend.query_media(query).await
        }
    }

    /// Calculate appropriate cache TTL based on query characteristics
    fn calculate_cache_ttl(&self, query: &crate::query::MediaQuery) -> std::time::Duration {
        use std::time::Duration;

        // Base TTL is 5 minutes
        let mut ttl_seconds = 300;

        // Reduce TTL for queries with user context (watch status changes frequently)
        if query.user_context.is_some() {
            ttl_seconds = ttl_seconds.min(60);
        }

        // Reduce TTL for queries with watch status filters
        if query.filters.watch_status.is_some() {
            ttl_seconds = ttl_seconds.min(30);
        }

        // Longer TTL for large paginated results (less likely to change)
        if query.pagination.limit > 100 {
            ttl_seconds *= 2;
        }

        // Shorter TTL for search queries (users refine searches frequently)
        if query.search.is_some() {
            ttl_seconds = ttl_seconds.min(120);
        }

        Duration::from_secs(ttl_seconds)
    }

    /// Invalidate all cached queries - called when media is added/updated/deleted
    pub async fn invalidate_query_cache(&self) -> Result<()> {
        use cache::CacheKeys;
        use tracing::{info, warn};

        if let Some(cache) = &self.cache {
            let mut cache_conn = cache.as_ref().clone();
            let pattern = CacheKeys::media_query_pattern();

            match cache_conn.delete_pattern(&pattern).await {
                Ok(_) => {
                    info!("Invalidated all query cache entries");
                    Ok(())
                }
                Err(e) => {
                    warn!("Failed to invalidate query cache: {}", e);
                    // Convert to our error type
                    Err(e)
                }
            }
        } else {
            // No cache configured, nothing to invalidate
            Ok(())
        }
    }

    /// Invalidate cached queries for a specific library
    pub async fn invalidate_library_query_cache(&self, library_id: LibraryID) -> Result<()> {
        use cache::CacheKeys;
        use tracing::{info, warn};

        if let Some(cache) = &self.cache {
            let mut cache_conn = cache.as_ref().clone();
            // Since we can't filter by library ID in the cache key pattern,
            // we have to invalidate all queries when a library is updated
            let pattern = CacheKeys::media_query_by_library_pattern(library_id);

            match cache_conn.delete_pattern(&pattern).await {
                Ok(_) => {
                    info!("Invalidated query cache for library: {}", library_id);
                    Ok(())
                }
                Err(e) => {
                    warn!("Failed to invalidate library query cache: {}", e);
                    Err(e)
                }
            }
        } else {
            Ok(())
        }
    }

    // Wrapper methods that invalidate cache after modifications

    /// Store a movie reference and invalidate related caches
    pub async fn store_movie_reference_with_cache_invalidation(
        &self,
        movie: &crate::types::media::MovieReference,
    ) -> Result<()> {
        // Store the movie
        self.backend.store_movie_reference(movie).await?;

        // Invalidate query cache
        self.invalidate_query_cache().await?;

        Ok(())
    }

    /// Store a series reference and invalidate related caches
    pub async fn store_series_reference_with_cache_invalidation(
        &self,
        series: &crate::types::media::SeriesReference,
    ) -> Result<()> {
        // Store the series
        self.backend.store_series_reference(series).await?;

        // Invalidate query cache
        self.invalidate_query_cache().await?;

        Ok(())
    }

    /// Delete media and invalidate related caches
    pub async fn delete_media_with_cache_invalidation(&self, id: &str) -> Result<()> {
        // Delete the media
        self.backend.delete_media(id).await?;

        // Invalidate query cache
        self.invalidate_query_cache().await?;

        Ok(())
    }

    /// Update movie TMDB ID and invalidate related caches
    pub async fn update_movie_tmdb_id_with_cache_invalidation(
        &self,
        id: &MovieID,
        tmdb_id: u64,
    ) -> Result<()> {
        // Update the movie
        self.backend.update_movie_tmdb_id(id, tmdb_id).await?;

        // Invalidate query cache
        self.invalidate_query_cache().await?;

        Ok(())
    }

    /// Update series TMDB ID and invalidate related caches
    pub async fn update_series_tmdb_id_with_cache_invalidation(
        &self,
        id: &SeriesID,
        tmdb_id: u64,
    ) -> Result<()> {
        // Update the series
        self.backend.update_series_tmdb_id(id, tmdb_id).await?;

        // Invalidate query cache
        self.invalidate_query_cache().await?;

        Ok(())
    }
}
