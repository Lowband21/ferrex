use anyhow::{Context, Result};
use futures::future::BoxFuture;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::fs;
use tracing::{info, warn};

/// Cache manager for transcoded content
pub struct CacheManager {
    cache_dir: PathBuf,
    max_size_mb: u64,
    max_age_days: u32,
}

#[derive(Debug)]
pub struct CacheStats {
    pub total_size_mb: f64,
    pub file_count: usize,
    pub oldest_file_age_days: Option<u32>,
    pub media_count: usize,
}

#[derive(Debug, Clone)]
struct CacheEntry {
    path: PathBuf,
    size: u64,
    modified: SystemTime,
}

impl CacheManager {
    pub fn new(cache_dir: PathBuf, max_size_mb: u64, max_age_days: u32) -> Self {
        Self {
            cache_dir,
            max_size_mb,
            max_age_days,
        }
    }

    /// Get cache statistics
    pub async fn get_stats(&self) -> Result<CacheStats> {
        let mut total_size = 0u64;
        let mut file_count = 0;
        let mut oldest_modified = SystemTime::now();
        let mut media_dirs = 0;

        let mut entries = fs::read_dir(&self.cache_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.is_dir() {
                media_dirs += 1;
                let stats = self.get_dir_stats(&path).await?;
                total_size += stats.0;
                file_count += stats.1;
                if let Some(modified) = stats.2 {
                    if modified < oldest_modified {
                        oldest_modified = modified;
                    }
                }
            }
        }

        let oldest_age = SystemTime::now()
            .duration_since(oldest_modified)
            .ok()
            .map(|d| (d.as_secs() / 86400) as u32);

        Ok(CacheStats {
            total_size_mb: total_size as f64 / 1_048_576.0,
            file_count,
            oldest_file_age_days: oldest_age,
            media_count: media_dirs,
        })
    }

    /// Get directory statistics
    fn get_dir_stats<'a>(
        &'a self,
        dir: &'a Path,
    ) -> BoxFuture<'a, Result<(u64, usize, Option<SystemTime>)>> {
        let dir = dir.to_path_buf();
        Box::pin(async move {
            let mut total_size = 0u64;
            let mut file_count = 0;
            let mut oldest_modified = None;

            let mut entries = fs::read_dir(&dir).await?;
            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();

                if path.is_file() {
                    if let Ok(metadata) = entry.metadata().await {
                        total_size += metadata.len();
                        file_count += 1;

                        if let Ok(modified) = metadata.modified() {
                            match oldest_modified {
                                None => oldest_modified = Some(modified),
                                Some(current) if modified < current => {
                                    oldest_modified = Some(modified)
                                }
                                _ => {}
                            }
                        }
                    }
                } else if path.is_dir() {
                    let sub_stats = self.get_dir_stats(&path).await?;
                    total_size += sub_stats.0;
                    file_count += sub_stats.1;
                    if let Some(modified) = sub_stats.2 {
                        match oldest_modified {
                            None => oldest_modified = Some(modified),
                            Some(current) if modified < current => oldest_modified = Some(modified),
                            _ => {}
                        }
                    }
                }
            }

            Ok((total_size, file_count, oldest_modified))
        })
    }

    /// Clean up cache based on size and age constraints
    pub async fn cleanup(&self) -> Result<CleanupResult> {
        info!("Starting cache cleanup");

        let entries = self.collect_cache_entries().await?;
        let total_size: u64 = entries.iter().map(|e| e.size).sum();
        let max_size_bytes = self.max_size_mb * 1_048_576;

        let mut removed_count = 0;
        let mut removed_size = 0u64;

        // Remove old files first
        let max_age = Duration::from_secs(self.max_age_days as u64 * 86400);
        let now = SystemTime::now();

        for entry in &entries {
            if let Ok(age) = now.duration_since(entry.modified) {
                if age > max_age {
                    match self.remove_entry(&entry.path).await { Err(e) => {
                        warn!("Failed to remove old cache entry: {}", e);
                    } _ => {
                        removed_count += 1;
                        removed_size += entry.size;
                    }}
                }
            }
        }

        // If still over size limit, remove oldest files
        if total_size - removed_size > max_size_bytes {
            let mut sorted_entries = entries.clone();
            sorted_entries.sort_by_key(|e| e.modified);

            let mut current_size = total_size - removed_size;

            for entry in sorted_entries {
                if current_size <= max_size_bytes {
                    break;
                }

                // Skip if already removed
                if !entry.path.exists() {
                    continue;
                }

                match self.remove_entry(&entry.path).await { Err(e) => {
                    warn!("Failed to remove cache entry: {}", e);
                } _ => {
                    removed_count += 1;
                    removed_size += entry.size;
                    current_size -= entry.size;
                }}
            }
        }

        info!(
            "Cache cleanup completed: removed {} files, freed {:.2} MB",
            removed_count,
            removed_size as f64 / 1_048_576.0
        );

        Ok(CleanupResult {
            removed_count,
            removed_size_mb: removed_size as f64 / 1_048_576.0,
        })
    }

    /// Collect all cache entries
    async fn collect_cache_entries(&self) -> Result<Vec<CacheEntry>> {
        let mut entries = Vec::new();
        self.collect_entries_recursive(&self.cache_dir, &mut entries)
            .await?;
        Ok(entries)
    }

    /// Recursively collect cache entries
    fn collect_entries_recursive<'a>(
        &'a self,
        dir: &'a Path,
        entries: &'a mut Vec<CacheEntry>,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let mut dir_entries = fs::read_dir(dir).await?;

            while let Some(entry) = dir_entries.next_entry().await? {
                let path = entry.path();

                if path.is_file() {
                    if let Ok(metadata) = entry.metadata().await {
                        if let Ok(modified) = metadata.modified() {
                            entries.push(CacheEntry {
                                path,
                                size: metadata.len(),
                                modified,
                            });
                        }
                    }
                } else if path.is_dir() {
                    self.collect_entries_recursive(&path, entries).await?;
                }
            }

            Ok(())
        })
    }

    /// Remove a cache entry (file or directory)
    async fn remove_entry(&self, path: &Path) -> Result<()> {
        if path.is_file() {
            fs::remove_file(path)
                .await
                .context("Failed to remove file")?;
        } else if path.is_dir() {
            fs::remove_dir_all(path)
                .await
                .context("Failed to remove directory")?;
        }

        // Try to remove empty parent directories
        if let Some(parent) = path.parent() {
            if parent != self.cache_dir {
                if let Ok(mut entries) = fs::read_dir(parent).await {
                    if entries.next_entry().await?.is_none() {
                        let _ = fs::remove_dir(parent).await;
                    }
                }
            }
        }

        Ok(())
    }

    /// Clear entire cache
    pub async fn clear(&self) -> Result<()> {
        info!("Clearing entire cache");

        let mut entries = fs::read_dir(&self.cache_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.is_dir() {
                fs::remove_dir_all(path).await?;
            } else {
                fs::remove_file(path).await?;
            }
        }

        info!("Cache cleared");
        Ok(())
    }

    /// Get cache path for a specific media and profile
    pub fn get_cache_path(&self, media_id: &str, profile_name: &str) -> PathBuf {
        self.cache_dir.join(media_id).join(profile_name)
    }

    /// Check if cached version exists
    pub async fn has_cached_version(&self, media_id: &str, profile_name: &str) -> bool {
        let cache_path = self.get_cache_path(media_id, profile_name);
        let playlist_path = cache_path.join("playlist.m3u8");
        playlist_path.exists()
    }

    /// Remove cached version for specific media and profile
    pub async fn remove_cached_version(&self, media_id: &str, profile_name: &str) -> Result<()> {
        let cache_path = self.get_cache_path(media_id, profile_name);
        if cache_path.exists() {
            fs::remove_dir_all(cache_path).await?;

            // Try to remove media directory if empty
            let media_dir = self.cache_dir.join(PathBuf::from(media_id.to_string()));
            if let Ok(mut entries) = fs::read_dir(&media_dir).await {
                if entries.next_entry().await?.is_none() {
                    let _ = fs::remove_dir(media_dir).await;
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct CleanupResult {
    pub removed_count: usize,
    pub removed_size_mb: f64,
}

/// Background cache cleaner
pub struct CacheCleaner {
    manager: Arc<CacheManager>,
    interval: Duration,
}

impl CacheCleaner {
    pub fn new(manager: Arc<CacheManager>, interval: Duration) -> Self {
        Self { manager, interval }
    }

    /// Start the background cleanup task
    pub fn start(self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(self.interval);

            loop {
                interval.tick().await;

                if let Err(e) = self.manager.cleanup().await {
                    warn!("Cache cleanup failed: {}", e);
                }
            }
        })
    }
}
