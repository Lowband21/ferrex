use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

use crate::media_library::MediaFile;

/// Cached metadata entry with timestamp
#[derive(Debug, Clone)]
struct CachedEntry {
    data: MediaFile,
    cached_at: Instant,
}

/// Client-side metadata cache
#[derive(Debug, Clone)]
pub struct MetadataCache {
    /// In-memory cache of media files by ID
    cache: Arc<RwLock<HashMap<String, CachedEntry>>>,
    /// Cache TTL (time to live)
    ttl: Duration,
    /// Optional disk cache directory
    disk_cache_dir: Option<PathBuf>,
}

impl MetadataCache {
    /// Create a new metadata cache
    pub fn new(ttl_minutes: u64) -> Self {
        let cache_dir = dirs::cache_dir().map(|d| d.join("ferrex_player").join("metadata"));

        // Create cache directory if it doesn't exist
        if let Some(ref dir) = cache_dir {
            std::fs::create_dir_all(dir).ok();
        }

        Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
            ttl: Duration::from_secs(ttl_minutes * 60),
            disk_cache_dir: cache_dir,
        }
    }

    /// Get a media file from cache
    pub async fn get(&self, id: &str) -> Option<MediaFile> {
        let cache = self.cache.read().await;

        if let Some(entry) = cache.get(id) {
            // Check if entry is still valid
            if entry.cached_at.elapsed() < self.ttl {
                return Some(entry.data.clone());
            }
        }

        // Try loading from disk cache if available
        if let Some(ref cache_dir) = self.disk_cache_dir {
            let cache_file = cache_dir.join(format!("{}.json", id.replace(':', "_")));
            if cache_file.exists() {
                if let Ok(data) = tokio::fs::read_to_string(&cache_file).await {
                    if let Ok(media) = serde_json::from_str::<MediaFile>(&data) {
                        // Check file modification time
                        if let Ok(metadata) = tokio::fs::metadata(&cache_file).await {
                            if let Ok(modified) = metadata.modified() {
                                if let Ok(elapsed) = modified.elapsed() {
                                    if elapsed < self.ttl {
                                        // Re-insert into memory cache
                                        drop(cache);
                                        self.insert(id, media.clone()).await;
                                        return Some(media);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        None
    }

    /// Insert or update a media file in cache
    pub async fn insert(&self, id: &str, media: MediaFile) {
        let mut cache = self.cache.write().await;
        cache.insert(
            id.to_string(),
            CachedEntry {
                data: media.clone(),
                cached_at: Instant::now(),
            },
        );

        // Also save to disk cache if available
        if let Some(ref cache_dir) = self.disk_cache_dir {
            let cache_file = cache_dir.join(format!("{}.json", id.replace(':', "_")));
            if let Ok(json) = serde_json::to_string_pretty(&media) {
                tokio::fs::write(cache_file, json).await.ok();
            }
        }
    }

    /// Batch insert media files
    pub async fn insert_batch(&self, media_files: Vec<MediaFile>) {
        let mut cache = self.cache.write().await;
        let now = Instant::now();

        for media in media_files {
            cache.insert(
                media.id.clone(),
                CachedEntry {
                    data: media.clone(),
                    cached_at: now,
                },
            );

            // Save to disk cache
            if let Some(ref cache_dir) = self.disk_cache_dir {
                let cache_file = cache_dir.join(format!("{}.json", media.id.replace(':', "_")));
                if let Ok(json) = serde_json::to_string_pretty(&media) {
                    tokio::spawn({
                        let cache_file = cache_file.clone();
                        async move {
                            tokio::fs::write(cache_file, json).await.ok();
                        }
                    });
                }
            }
        }
    }

    /// Clear expired entries from cache
    pub async fn cleanup(&self) {
        let mut cache = self.cache.write().await;
        let now = Instant::now();

        cache.retain(|_, entry| now.duration_since(entry.cached_at) < self.ttl);

        // Clean up disk cache
        if let Some(ref cache_dir) = self.disk_cache_dir {
            if let Ok(mut entries) = tokio::fs::read_dir(cache_dir).await {
                while let Ok(Some(entry)) = entries.next_entry().await {
                    if let Ok(metadata) = entry.metadata().await {
                        if let Ok(modified) = metadata.modified() {
                            if let Ok(elapsed) = modified.elapsed() {
                                if elapsed > self.ttl {
                                    tokio::fs::remove_file(entry.path()).await.ok();
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// Get cache statistics
    pub async fn stats(&self) -> CacheStats {
        let cache = self.cache.read().await;
        let total_entries = cache.len();
        let mut expired_entries = 0;
        let now = Instant::now();

        for entry in cache.values() {
            if now.duration_since(entry.cached_at) >= self.ttl {
                expired_entries += 1;
            }
        }

        CacheStats {
            total_entries,
            valid_entries: total_entries - expired_entries,
            expired_entries,
        }
    }

    /// Check if a media file needs refresh
    pub async fn needs_refresh(&self, id: &str) -> bool {
        let cache = self.cache.read().await;

        if let Some(entry) = cache.get(id) {
            // Refresh if more than half the TTL has passed
            entry.cached_at.elapsed() > self.ttl / 2
        } else {
            true
        }
    }
}

#[derive(Debug, Clone)]
pub struct CacheStats {
    pub total_entries: usize,
    pub valid_entries: usize,
    pub expired_entries: usize,
}
