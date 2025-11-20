use std::sync::Arc;
use std::path::PathBuf;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use rusty_media_core::{
    MediaFile,
    MetadataProvider, TmdbProvider, ProviderError,
    providers::traits::{MediaQuery, DetailedMediaInfo}
};
use anyhow::Result;

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
        let parsed_info = media_file.metadata.as_ref()
            .and_then(|m| m.parsed_info.as_ref())
            .ok_or_else(|| anyhow::anyhow!("No parsed info available"))?;
        
        tracing::info!("Fetching metadata for: {} (type: {:?})", 
            parsed_info.title, parsed_info.media_type);
        
        // For TV shows, use show_name for search, not the full title
        let search_title = match parsed_info.media_type {
            rusty_media_core::MediaType::TvEpisode => {
                parsed_info.show_name.clone()
                    .unwrap_or_else(|| parsed_info.title.clone())
            }
            _ => parsed_info.title.clone()
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
        let result = results.into_iter().next()
            .ok_or(ProviderError::NotFound)?;
        
        // Fetch detailed metadata
        provider.get_metadata(&result.id, query.media_type.clone()).await
    }
    
    /// Download and cache a poster image
    pub async fn cache_poster(&self, poster_path: &str, media_id: &str) -> Result<PathBuf> {
        let poster_filename = format!("{}_poster.jpg", media_id);
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
                return Err(anyhow::anyhow!("Failed to download poster: {}", response.status()));
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
        let poster_filename = format!("{}_poster.jpg", media_id);
        let cache_path = self.cache_dir.join("posters").join(&poster_filename);
        
        if cache_path.exists() {
            Some(cache_path)
        } else {
            None
        }
    }
}