use crate::{MediaFile, MediaMetadata, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone, Default)]
pub struct MediaFilters {
    pub media_type: Option<String>,
    pub show_name: Option<String>,
    pub season: Option<u32>,
    pub order_by: Option<String>,
    pub limit: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MediaStats {
    pub total_files: u64,
    pub total_size: u64,
    pub by_type: HashMap<String, u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TvShowInfo {
    pub id: Uuid,
    pub tmdb_id: String,
    pub name: String,
    pub overview: Option<String>,
    pub poster_path: Option<String>,
    pub backdrop_path: Option<String>,
    pub seasons: Vec<SeasonInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeasonInfo {
    pub id: Uuid,
    pub season_number: i32,
    pub name: Option<String>,
    pub episode_count: i32,
    pub poster_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpisodeInfo {
    pub id: Uuid,
    pub episode_number: i32,
    pub name: Option<String>,
    pub overview: Option<String>,
    pub air_date: Option<chrono::NaiveDate>,
    pub still_path: Option<String>,
    pub media_file_id: Option<Uuid>,
}

#[async_trait]
pub trait MediaDatabaseTrait: Send + Sync {
    async fn initialize_schema(&self) -> Result<()>;
    async fn store_media(&self, media_file: MediaFile) -> Result<String>;
    async fn get_media(&self, id: &str) -> Result<Option<MediaFile>>;
    async fn get_media_by_path(&self, path: &str) -> Result<Option<MediaFile>>;
    async fn list_media(&self, filters: MediaFilters) -> Result<Vec<MediaFile>>;
    async fn get_stats(&self) -> Result<MediaStats>;
    async fn file_exists(&self, path: &str) -> Result<bool>;
    async fn delete_media(&self, id: &str) -> Result<()>;
    async fn get_all_media(&self) -> Result<Vec<MediaFile>>;

    async fn store_external_metadata(&self, media_id: &str, metadata: &MediaMetadata)
        -> Result<()>;
    async fn store_tv_show(&self, show_info: &TvShowInfo) -> Result<String>;
    async fn get_tv_show(&self, tmdb_id: &str) -> Result<Option<TvShowInfo>>;
    async fn link_episode_to_file(
        &self,
        media_file_id: &str,
        show_tmdb_id: &str,
        season: i32,
        episode: i32,
    ) -> Result<()>;
}

pub enum DatabaseBackend {
    Postgres,
    SurrealDB,
}
