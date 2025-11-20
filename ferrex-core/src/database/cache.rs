use crate::{MediaError, Result};
use redis::{aio::ConnectionManager, AsyncCommands};
use serde::{de::DeserializeOwned, Serialize};
use std::time::Duration;
use tracing::{debug, info, warn};

#[derive(Clone)]
pub struct RedisCache {
    conn: ConnectionManager,
}

impl RedisCache {
    pub async fn new(redis_url: &str) -> Result<Self> {
        info!("Connecting to Redis cache at {}", redis_url);

        let client = redis::Client::open(redis_url).map_err(|e| {
            MediaError::InvalidMedia(format!("Failed to create Redis client: {e}"))
        })?;

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
            .query_async::<_, ()>(&mut self.conn)
            .await
            .map_err(|e| MediaError::InvalidMedia(format!("Redis FLUSHDB failed: {e}")))?;

        Ok(())
    }
}

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
}
