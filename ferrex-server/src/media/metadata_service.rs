use anyhow::Result;
use ferrex_core::{MediaFile, TmdbApiProvider};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

// TODO: This service is part of the old architecture and should be refactored
// to work with the new reference-based types when the client is updated

pub struct MetadataService {
    pub tmdb_provider: Option<Arc<TmdbApiProvider>>,
    cache_dir: PathBuf,
}

impl MetadataService {
    pub fn new(_tmdb_api_key: Option<String>, cache_dir: PathBuf) -> Self {
        // For now, always create TmdbApiProvider which gets key from env
        let tmdb_provider = Some(Arc::new(TmdbApiProvider::new()));

        Self {
            tmdb_provider,
            cache_dir,
        }
    }

    /// Get the cache directory path
    pub fn cache_dir(&self) -> &PathBuf {
        &self.cache_dir
    }

    /// Fetch metadata for a media file - DEPRECATED
    pub async fn fetch_metadata(&self, _media_file: &MediaFile) -> Result<()> {
        // Return empty metadata for now - this is deprecated functionality
        Err(anyhow::anyhow!(
            "Metadata fetching not supported in transition period"
        ))
    }

    /// Fetch TV show metadata directly by TMDB ID - DEPRECATED
    pub async fn fetch_tv_show_metadata(&self, _tmdb_id: u32) -> Result<()> {
        Err(anyhow::anyhow!(
            "TV metadata fetching not supported in transition period"
        ))
    }

    /// Search for a TV show by name and fetch its metadata - DEPRECATED
    pub async fn search_and_fetch_show_metadata(&self, _show_name: &str) -> Result<()> {
        Err(anyhow::anyhow!(
            "TV metadata fetching not supported in transition period"
        ))
    }

    /// Download and cache a TV show poster
    pub async fn download_show_poster(&self, show_name: &str, poster_url: &str) -> Result<PathBuf> {
        let cache_key = format!("show_{}", show_name.replace(' ', "_"));
        let cache_filename = format!("{}.png", cache_key);
        let cache_path = self.cache_dir.join("posters").join(&cache_filename);

        // Check if already cached
        if cache_path.exists() {
            return Ok(cache_path);
        }

        // Ensure directory exists
        fs::create_dir_all(cache_path.parent().unwrap()).await?;

        // Download image
        let response = reqwest::get(poster_url).await?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "Failed to download show poster: {}",
                response.status()
            ));
        }

        let bytes = response.bytes().await?;
        let mut file = fs::File::create(&cache_path).await?;
        file.write_all(&bytes).await?;

        tracing::info!(
            "Downloaded show poster for '{}' to {:?}",
            show_name,
            cache_path
        );

        Ok(cache_path)
    }

    /// Download and cache a poster image
    pub async fn cache_poster(&self, poster_path: &str, media_id: &str) -> Result<PathBuf> {
        let poster_filename = format!("{}_poster.png", media_id);
        let cache_path = self.cache_dir.join("posters").join(&poster_filename);

        // Check if already cached
        if cache_path.exists() {
            return Ok(cache_path);
        }

        // Ensure directory exists
        fs::create_dir_all(cache_path.parent().unwrap()).await?;

        // Download from TMDB
        if let Some(_tmdb) = &self.tmdb_provider {
            let url = if poster_path.starts_with("http") {
                poster_path.to_string()
            } else {
                format!("https://image.tmdb.org/t/p/w500{}", poster_path)
            };

            let response = reqwest::get(&url).await?;

            if !response.status().is_success() {
                return Err(anyhow::anyhow!(
                    "Failed to download poster: {}",
                    response.status()
                ));
            }

            let bytes = response.bytes().await?;
            let mut file = fs::File::create(&cache_path).await?;
            file.write_all(&bytes).await?;

            Ok(cache_path)
        } else {
            Err(anyhow::anyhow!("No TMDB provider configured"))
        }
    }

    /// Get a cached poster path if it exists
    pub fn get_cached_poster(&self, media_id: &str) -> Option<PathBuf> {
        // Try PNG first
        let png_filename = format!("{}_poster.png", media_id);
        let png_path = self.cache_dir.join("posters").join(&png_filename);

        if png_path.exists() {
            return Some(png_path);
        }

        // Fall back to JPG for backwards compatibility
        let jpg_filename = format!("{}_poster.jpg", media_id);
        let jpg_path = self.cache_dir.join("posters").join(&jpg_filename);

        if jpg_path.exists() {
            Some(jpg_path)
        } else {
            None
        }
    }

    /// Convert a TMDB poster path to a full URL
    pub fn get_tmdb_image_url(&self, image_path: &str) -> Option<String> {
        if image_path.starts_with("http://") || image_path.starts_with("https://") {
            // Already a full URL
            Some(image_path.to_string())
        } else {
            // Build full TMDB URL
            Some(format!("https://image.tmdb.org/t/p/w500{}", image_path))
        }
    }

    /// Cache an image from a URL with a specific cache key
    pub async fn cache_image_from_url(&self, url: &str, cache_key: &str) -> Result<PathBuf> {
        let cache_filename = format!("{}.png", cache_key);
        let cache_path = self.cache_dir.join("posters").join(&cache_filename);

        // Check if already cached
        if cache_path.exists() {
            return Ok(cache_path);
        }

        // Ensure directory exists
        fs::create_dir_all(cache_path.parent().unwrap()).await?;

        // Download image
        let response = reqwest::get(url).await?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "Failed to download image: {}",
                response.status()
            ));
        }

        let bytes = response.bytes().await?;
        let mut file = fs::File::create(&cache_path).await?;
        file.write_all(&bytes).await?;

        Ok(cache_path)
    }

    /// Get cached poster for show or season
    pub fn get_cached_show_poster(&self, show_name: &str) -> Option<PathBuf> {
        let cache_key = format!("show_{}", show_name.replace(' ', "_"));

        // Try PNG first
        let png_path = self
            .cache_dir
            .join("posters")
            .join(format!("{}.png", cache_key));

        if png_path.exists() {
            return Some(png_path);
        }

        // Fall back to JPG
        let jpg_path = self
            .cache_dir
            .join("posters")
            .join(format!("{}.jpg", cache_key));

        if jpg_path.exists() {
            Some(jpg_path)
        } else {
            None
        }
    }

    /// Get cached poster for season
    pub fn get_cached_season_poster(&self, show_name: &str, season_num: u32) -> Option<PathBuf> {
        let cache_key = format!("season_{}_{}", show_name.replace(' ', "_"), season_num);

        // Try PNG first
        let png_path = self
            .cache_dir
            .join("posters")
            .join(format!("{}.png", cache_key));

        if png_path.exists() {
            return Some(png_path);
        }

        // Fall back to JPG
        let jpg_path = self
            .cache_dir
            .join("posters")
            .join(format!("{}.jpg", cache_key));

        if jpg_path.exists() {
            Some(jpg_path)
        } else {
            None
        }
    }

    /// Get poster path for a media item
    pub fn get_poster_path(&self, media_id: &Uuid) -> PathBuf {
        self.cache_dir
            .join("posters")
            .join(format!("{}.png", media_id))
    }

    /// Cache a season poster
    pub async fn cache_season_poster(
        &self,
        poster_path: &str,
        show_name: &str,
        season_num: u32,
    ) -> Result<PathBuf> {
        let cache_key = format!("season_{}_{}", show_name.replace(' ', "_"), season_num);
        let cache_filename = format!("{}.png", cache_key);
        let cache_path = self.cache_dir.join("posters").join(&cache_filename);

        // Check if already cached
        if cache_path.exists() {
            return Ok(cache_path);
        }

        // Ensure directory exists
        fs::create_dir_all(cache_path.parent().unwrap()).await?;

        // Download from TMDB
        let url = if poster_path.starts_with("http") {
            poster_path.to_string()
        } else {
            format!("https://image.tmdb.org/t/p/w500{}", poster_path)
        };

        let response = reqwest::get(&url).await?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "Failed to download season poster: {}",
                response.status()
            ));
        }

        let bytes = response.bytes().await?;
        let mut file = fs::File::create(&cache_path).await?;
        file.write_all(&bytes).await?;

        Ok(cache_path)
    }
}

// Temporary stub for DetailedMediaInfo - part of old architecture
#[derive(Debug, Clone)]
pub struct DetailedMediaInfo {
    pub external_info: ExternalMediaInfo,
}

#[derive(Debug, Clone)]
pub struct ExternalMediaInfo {
    pub poster_url: Option<String>,
}
