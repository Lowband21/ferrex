use crate::{
    error::{MediaError, Result},
    query::types::MediaQuery,
    types::ids::LibraryID,
};
use redis::{AsyncCommands, aio::ConnectionManager};
use serde::{Serialize, de::DeserializeOwned};
use std::collections::hash_map::DefaultHasher;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::time::Duration;
use tracing::{debug, info, warn};
use uuid::Uuid;

#[derive(Clone)]
pub struct RedisCache {
    conn: ConnectionManager,
}

impl fmt::Debug for RedisCache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RedisCache")
            .field("connection", &"ConnectionManager")
            .finish()
    }
}

impl RedisCache {
    pub async fn new(redis_url: &str) -> Result<Self> {
        info!("Connecting to Redis cache at {}", redis_url);

        let client = redis::Client::open(redis_url)
            .map_err(|e| MediaError::InvalidMedia(format!("Failed to create Redis client: {e}")))?;

        let conn = ConnectionManager::new(client)
            .await
            .map_err(|e| MediaError::InvalidMedia(format!("Failed to connect to Redis: {e}")))?;

        info!("Successfully connected to Redis cache");

        Ok(Self { conn })
    }

    pub async fn get<T: DeserializeOwned>(&mut self, key: &str) -> Result<Option<T>> {
        debug!("Cache GET: {}", key);

        let data: Option<String> = self
            .conn
            .get(key)
            .await
            .map_err(|e| MediaError::InvalidMedia(format!("Redis GET failed: {e}")))?;

        match data {
            Some(json) => {
                let value = serde_json::from_str(&json).map_err(|e| {
                    MediaError::InvalidMedia(format!("Failed to deserialize cache data: {e}"))
                })?;
                debug!("Cache HIT: {}", key);
                Ok(Some(value))
            }
            None => {
                debug!("Cache MISS: {}", key);
                Ok(None)
            }
        }
    }

    pub async fn set<T: Serialize>(
        &mut self,
        key: &str,
        value: &T,
        ttl: Option<Duration>,
    ) -> Result<()> {
        debug!("Cache SET: {} (TTL: {:?})", key, ttl);

        let json = serde_json::to_string(value).map_err(|e| {
            MediaError::InvalidMedia(format!("Failed to serialize cache data: {e}"))
        })?;

        if let Some(ttl) = ttl {
            self.conn
                .set_ex::<_, _, ()>(key, json, ttl.as_secs())
                .await
                .map_err(|e| MediaError::InvalidMedia(format!("Redis SETEX failed: {e}")))?;
        } else {
            self.conn
                .set::<_, _, ()>(key, json)
                .await
                .map_err(|e| MediaError::InvalidMedia(format!("Redis SET failed: {e}")))?;
        }

        Ok(())
    }

    pub async fn delete(&mut self, key: &str) -> Result<()> {
        debug!("Cache DELETE: {}", key);

        self.conn
            .del::<_, ()>(key)
            .await
            .map_err(|e| MediaError::InvalidMedia(format!("Redis DEL failed: {e}")))?;

        Ok(())
    }

    pub async fn delete_pattern(&mut self, pattern: &str) -> Result<()> {
        debug!("Cache DELETE pattern: {}", pattern);

        let keys: Vec<String> = self
            .conn
            .keys(pattern)
            .await
            .map_err(|e| MediaError::InvalidMedia(format!("Redis KEYS failed: {e}")))?;

        if !keys.is_empty() {
            debug!("Deleting {} keys matching pattern: {}", keys.len(), pattern);
            let _: () = self
                .conn
                .del(keys)
                .await
                .map_err(|e| MediaError::InvalidMedia(format!("Redis DEL failed: {e}")))?;
        }

        Ok(())
    }

    pub async fn flush_all(&mut self) -> Result<()> {
        warn!("Flushing entire Redis cache");

        redis::cmd("FLUSHDB")
            .query_async::<()>(&mut self.conn)
            .await
            .map_err(|e| MediaError::InvalidMedia(format!("Redis FLUSHDB failed: {e}")))?;

        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct CacheKeys;

impl CacheKeys {
    pub fn media_file(id: &str) -> String {
        format!("media:file:{id}")
    }

    pub fn media_list(filters_hash: &str) -> String {
        format!("media:list:{filters_hash}")
    }

    pub fn media_stats() -> String {
        "media:stats".to_string()
    }

    pub fn tv_show(tmdb_id: &str) -> String {
        format!("tv:show:{tmdb_id}")
    }

    pub fn tmdb_search(query: &str) -> String {
        format!("tmdb:search:{query}")
    }

    pub fn tmdb_movie(id: &str) -> String {
        format!("tmdb:movie:{id}")
    }

    pub fn tmdb_tv(id: &str) -> String {
        format!("tmdb:tv:{id}")
    }

    /// Generate a cache key for a media query
    /// The key includes all fields that affect query results
    pub fn media_query(
        query: &MediaQuery,
        library_scope: Option<LibraryID>,
        user_id: Option<Uuid>,
    ) -> String {
        let mut hasher = DefaultHasher::new();

        if let Some(library_id) = library_scope {
            library_id.hash(&mut hasher);
        }

        // Hash all filter fields
        query.filters.library_ids.hash(&mut hasher);
        if let Some(ref media_type) = query.filters.media_type {
            format!("{:?}", media_type).hash(&mut hasher);
        }
        query.filters.genres.hash(&mut hasher);
        query.filters.year_range.hash(&mut hasher);
        query.filters.resolution_range.hash(&mut hasher);

        if let Some(range) = query.filters.rating_range {
            range.hash(&mut hasher);
        }

        if let Some(ref watch_status) = query.filters.watch_status {
            format!("{:?}", watch_status).hash(&mut hasher);
        }

        // Hash sort criteria
        format!("{:?}", query.sort.primary).hash(&mut hasher);
        format!("{:?}", query.sort.order).hash(&mut hasher);
        if let Some(ref secondary) = query.sort.secondary {
            format!("{:?}", secondary).hash(&mut hasher);
        }

        // Hash search if present
        if let Some(ref search) = query.search {
            search.text.to_lowercase().hash(&mut hasher);
            search
                .fields
                .iter()
                .map(|f| format!("{:?}", f))
                .collect::<Vec<_>>()
                .hash(&mut hasher);
            search.fuzzy.hash(&mut hasher);
        }

        // Hash pagination
        query.pagination.offset.hash(&mut hasher);
        query.pagination.limit.hash(&mut hasher);

        // Include user context if present (for watch status queries)
        if let Some(user_id) = user_id.or(query.user_context) {
            user_id.hash(&mut hasher);
        }

        let hash = hasher.finish();
        let library_segment = library_scope
            .map(|library_id| library_id.to_string())
            .unwrap_or_else(|| "global".to_string());

        format!("query:media:v1:{}:{:x}", library_segment, hash)
    }

    /// Generate a pattern to match all query cache keys
    pub fn media_query_pattern() -> String {
        "query:media:v1:*".to_string()
    }

    /// Generate cache keys for invalidation based on library
    pub fn media_query_by_library_pattern(library_id: LibraryID) -> String {
        format!("query:media:v1:{}:*", library_id)
    }
}
