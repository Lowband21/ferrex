use anyhow::Result;
use ferrex_core::{
    providers::traits::{DetailedMediaInfo, MediaQuery},
    MediaFile, MetadataProvider, ProviderError, TmdbProvider,
};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs;
use tokio::io::AsyncWriteExt;

pub struct MetadataService {
    tmdb_provider: Option<Arc<TmdbProvider>>,
    cache_dir: PathBuf,
}

impl MetadataService {
    pub fn new(tmdb_api_key: Option<String>, cache_dir: PathBuf) -> Self {
        let tmdb_provider = tmdb_api_key.map(|key| Arc::new(TmdbProvider::new(key)));

        Self {
            tmdb_provider,
            cache_dir,
        }
    }

    /// Fetch metadata for a media file
    pub async fn fetch_metadata(&self, media_file: &MediaFile) -> Result<DetailedMediaInfo> {
        // Check if we have parsed info to search with
        let parsed_info = media_file
            .metadata
            .as_ref()
            .and_then(|m| m.parsed_info.as_ref())
            .ok_or_else(|| anyhow::anyhow!("No parsed info available"))?;

        tracing::info!(
            "Fetching metadata for: {} (type: {:?})",
            parsed_info.title,
            parsed_info.media_type
        );

        // For TV shows, use show_name for search, not the full title
        let search_title = match parsed_info.media_type {
            ferrex_core::MediaType::TvEpisode => {
                let show_name = parsed_info
                    .show_name
                    .clone()
                    .unwrap_or_else(|| parsed_info.title.clone());

                // Clean show name for TMDB search - remove year in parentheses
                // e.g., "The Americans (2013)" -> "The Americans"
                let cleaned = regex::Regex::new(r"\s*\(\d{4}\)\s*$")
                    .unwrap()
                    .replace(&show_name, "")
                    .to_string();

                tracing::info!(
                    "Cleaned show name for search: '{}' -> '{}'",
                    show_name,
                    cleaned
                );
                cleaned
            }
            _ => parsed_info.title.clone(),
        };

        let query = MediaQuery {
            title: search_title,
            year: parsed_info.year,
            media_type: parsed_info.media_type.clone(),
            show_name: parsed_info.show_name.clone(),
            season: parsed_info.season,
            episode: parsed_info.episode,
        };

        // Try TMDB first
        if let Some(tmdb) = &self.tmdb_provider {
            match self.search_and_fetch_tmdb(&**tmdb, &query).await {
                Ok(info) => return Ok(info),
                Err(e) => {
                    tracing::warn!("TMDB fetch failed: {}", e);
                }
            }
        }

        Err(anyhow::anyhow!("No metadata providers available"))
    }

    async fn search_and_fetch_tmdb(
        &self,
        provider: &TmdbProvider,
        query: &MediaQuery,
    ) -> Result<DetailedMediaInfo, ProviderError> {
        // Search for the media
        let results = provider.search(query).await?;

        // Get the first result (TODO: implement better matching)
        let result = results.into_iter().next().ok_or(ProviderError::NotFound)?;

        // Fetch detailed metadata with season/episode info if available
        provider.get_metadata_with_details(&result.id, query).await
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
        if let Some(tmdb) = &self.tmdb_provider {
            let url = format!("{}/w500{}", tmdb.image_base_url(), poster_path);
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
        } else if let Some(tmdb) = &self.tmdb_provider {
            // Build full TMDB URL
            Some(format!("{}/w500{}", tmdb.image_base_url(), image_path))
        } else {
            None
        }
    }

    /// Cache an image from a URL with a specific cache key
    pub async fn cache_image_from_url(&self, url: &str, cache_key: &str) -> Result<PathBuf> {
        let cache_filename = format!("{}.png", cache_key); // Changed to PNG
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
    pub fn get_poster_path(&self, media_id: &str) -> PathBuf {
        self.cache_dir
            .join("posters")
            .join(format!("{}.png", media_id)) // Changed to PNG
    }

    /// Cache a season poster
    pub async fn cache_season_poster(
        &self,
        poster_path: &str,
        show_name: &str,
        season_num: u32,
    ) -> Result<PathBuf> {
        let cache_key = format!("season_{}_{}", show_name.replace(' ', "_"), season_num);
        let cache_filename = format!("{}.png", cache_key); // Changed to PNG
        let cache_path = self.cache_dir.join("posters").join(&cache_filename);

        // Check if already cached
        if cache_path.exists() {
            return Ok(cache_path);
        }

        // Ensure directory exists
        fs::create_dir_all(cache_path.parent().unwrap()).await?;

        // Download from TMDB
        if let Some(tmdb) = &self.tmdb_provider {
            let url = if poster_path.starts_with("http") {
                poster_path.to_string()
            } else {
                format!("{}/w500{}", tmdb.image_base_url(), poster_path)
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
        } else {
            Err(anyhow::anyhow!("No TMDB provider configured"))
        }
    }
}
