use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use crate::media::{MediaType, ExternalMediaInfo};

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("API error: {0}")]
    ApiError(String),
    
    #[error("Not found")]
    NotFound,
    
    #[error("Rate limited")]
    RateLimited,
    
    #[error("Invalid API key")]
    InvalidApiKey,
    
    #[error("Network error: {0}")]
    NetworkError(#[from] reqwest::Error),
    
    #[error("Parse error: {0}")]
    ParseError(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub id: String,
    pub title: String,
    pub year: Option<u32>,
    pub media_type: MediaType,
    pub poster_path: Option<String>,
    pub overview: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MediaQuery {
    pub title: String,
    pub year: Option<u32>,
    pub media_type: MediaType,
    pub show_name: Option<String>,
    pub season: Option<u32>,
    pub episode: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CastMember {
    pub id: u32,
    pub name: String,
    pub character: Option<String>,
    pub profile_path: Option<String>,
    pub order: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrewMember {
    pub id: u32,
    pub name: String,
    pub job: String,
    pub department: String,
    pub profile_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetailedMediaInfo {
    pub external_info: ExternalMediaInfo,
    pub cast: Vec<CastMember>,
    pub crew: Vec<CrewMember>,
    pub images: MediaImages,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaImages {
    pub posters: Vec<String>,
    pub backdrops: Vec<String>,
    pub logos: Vec<String>,
}

#[async_trait]
pub trait MetadataProvider: Send + Sync {
    /// Search for media matching the query
    async fn search(&self, query: &MediaQuery) -> Result<Vec<SearchResult>, ProviderError>;
    
    /// Get detailed metadata for a specific media item
    async fn get_metadata(&self, provider_id: &str, media_type: MediaType) -> Result<DetailedMediaInfo, ProviderError>;
    
    /// Get the provider name
    fn name(&self) -> &'static str;
    
    /// Get the base URL for images
    fn image_base_url(&self) -> &str;
}