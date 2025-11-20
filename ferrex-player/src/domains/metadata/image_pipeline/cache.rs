//! Multi-level cache implementation for images

use super::{CacheKey, CacheStats, ImageCache, ProcessedImage, Result};
use moka::future::Cache;
use std::sync::Arc;
use std::time::Duration;

/// Multi-level cache with memory and disk storage
pub struct MultiLevelCache {
    /// In-memory cache using moka
    memory_cache: Cache<CacheKey, Arc<ProcessedImage>>,
    // TODO: Disk cache implementation
    // disk_cache: DiskCache,
}

impl MultiLevelCache {
    /// Create a new multi-level cache
    pub fn new(max_memory_items: u64, _max_memory_bytes: u64) -> Self {
        let memory_cache = Cache::builder()
            .max_capacity(max_memory_items)
            .weigher(|_key: &CacheKey, value: &Arc<ProcessedImage>| -> u32 {
                // Estimate memory usage (very rough)
                let (width, height) = value.processed_size;
                let bytes = (width * height * 4) as u32; // RGBA
                (bytes / 1024).max(1) // Return weight in KB
            })
            .time_to_live(Duration::from_secs(3600)) // 1 hour TTL
            .time_to_idle(Duration::from_secs(600)) // 10 min idle time
            .build();

        Self { memory_cache }
    }
}

#[async_trait::async_trait]
impl ImageCache for MultiLevelCache {
    async fn get(&self, key: &CacheKey) -> Option<Arc<ProcessedImage>> {
        // Try memory cache first
        if let Some(image) = self.memory_cache.get(key).await {
            return Some(image);
        }

        // TODO: Try disk cache
        // if let Some(image) = self.disk_cache.get(key).await {
        //     // Promote to memory cache
        //     self.memory_cache.insert(key.clone(), image.clone()).await;
        //     return Some(image);
        // }

        None
    }

    async fn insert(&self, key: CacheKey, image: Arc<ProcessedImage>) -> Result<()> {
        // Insert into memory cache
        self.memory_cache.insert(key.clone(), image.clone()).await;

        // TODO: Also insert into disk cache for persistence
        // self.disk_cache.insert(key, image).await?;

        Ok(())
    }

    async fn remove(&self, key: &CacheKey) -> Result<()> {
        self.memory_cache.remove(key).await;

        // TODO: Remove from disk cache
        // self.disk_cache.remove(key).await?;

        Ok(())
    }

    async fn clear(&self) -> Result<()> {
        self.memory_cache.invalidate_all();
        self.memory_cache.run_pending_tasks().await;

        // TODO: Clear disk cache
        // self.disk_cache.clear().await?;

        Ok(())
    }

    async fn stats(&self) -> CacheStats {
        let memory_items = self.memory_cache.entry_count();
        let memory_bytes = self.memory_cache.weighted_size();

        // Calculate hit rate from moka's stats
        // Note: This is a simplified version
        let hit_rate = 0.0; // TODO: Implement proper hit rate tracking

        CacheStats {
            memory_items: memory_items as usize,
            memory_bytes: (memory_bytes * 1024) as usize, // Convert from KB to bytes
            disk_items: 0,                                // TODO: Implement disk cache
            disk_bytes: 0,
            hit_rate,
        }
    }
}
