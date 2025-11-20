use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaFile {
    pub id: Uuid,
    pub path: PathBuf,
    pub filename: String,
    pub size: u64,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub metadata: Option<MediaMetadata>,
    pub library_id: Option<Uuid>,
    pub parent_media_id: Option<Uuid>, // For extras: link to parent movie/episode
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaMetadata {
    // Technical metadata from FFmpeg
    pub duration: Option<f64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub bitrate: Option<u64>,
    pub framerate: Option<f64>,
    pub file_size: u64,

    // HDR metadata
    pub color_primaries: Option<String>,
    pub color_transfer: Option<String>,
    pub color_space: Option<String>,
    pub bit_depth: Option<u32>,

    // Parsed from filename
    pub parsed_info: Option<ParsedMediaInfo>,

    // Future: External database info
    pub external_info: Option<ExternalMediaInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedMediaInfo {
    pub media_type: MediaType,
    pub title: String,
    pub year: Option<u32>,

    // TV Show specific
    pub show_name: Option<String>,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    pub episode_title: Option<String>,

    // Extra specific
    pub extra_type: Option<ExtraType>,
    pub parent_title: Option<String>, // Title of the parent movie/show

    // Quality/release info
    pub resolution: Option<String>,
    pub source: Option<String>,
    pub release_group: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub enum MediaType {
    Movie,
    TvEpisode,
    Extra,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub enum ExtraType {
    BehindTheScenes,
    DeletedScenes,
    Featurette,
    Interview,
    Scene,
    Short,
    Trailer,
    Other,
}

impl std::fmt::Display for MediaType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MediaType::Movie => write!(f, "Movie"),
            MediaType::TvEpisode => write!(f, "TvEpisode"),
            MediaType::Extra => write!(f, "Extra"),
            MediaType::Unknown => write!(f, "Unknown"),
        }
    }
}

impl std::fmt::Display for ExtraType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExtraType::BehindTheScenes => write!(f, "Behind the Scenes"),
            ExtraType::DeletedScenes => write!(f, "Deleted Scenes"),
            ExtraType::Featurette => write!(f, "Featurette"),
            ExtraType::Interview => write!(f, "Interview"),
            ExtraType::Scene => write!(f, "Scene"),
            ExtraType::Short => write!(f, "Short"),
            ExtraType::Trailer => write!(f, "Trailer"),
            ExtraType::Other => write!(f, "Other"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalMediaInfo {
    // TMDB/TVDB/IMDB IDs for future lookups
    pub tmdb_id: Option<u32>,
    pub tvdb_id: Option<u32>,
    pub imdb_id: Option<String>,

    // External metadata
    pub description: Option<String>,
    pub poster_url: Option<String>,
    pub backdrop_url: Option<String>,
    pub genres: Vec<String>,
    pub rating: Option<f32>,
    pub release_date: Option<chrono::NaiveDate>,

    // TV Show specific external info
    pub show_description: Option<String>,
    pub show_poster_url: Option<String>,
    pub season_poster_url: Option<String>,
    pub episode_still_url: Option<String>,
}

// TV Show aggregation structures for API responses
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TvShowDetails {
    pub name: String,
    pub tmdb_id: Option<u32>,
    pub description: Option<String>,
    pub poster_url: Option<String>,
    pub backdrop_url: Option<String>,
    pub genres: Vec<String>,
    pub rating: Option<f32>,
    pub seasons: Vec<SeasonSummary>,
    pub total_episodes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeasonSummary {
    pub number: u32,
    pub name: Option<String>,
    pub episode_count: usize,
    pub poster_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeasonDetails {
    pub show_name: String,
    pub number: u32,
    pub name: Option<String>,
    pub poster_url: Option<String>,
    pub episodes: Vec<EpisodeSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpisodeSummary {
    pub id: Uuid,
    pub number: u32,
    pub title: Option<String>,
    pub description: Option<String>,
    pub still_url: Option<String>,
    pub duration: Option<f64>,
    pub air_date: Option<chrono::NaiveDate>,
}

impl MediaFile {
    pub fn new(path: PathBuf) -> crate::Result<Self> {
        let filename = path
            .file_name()
            .ok_or_else(|| crate::MediaError::InvalidMedia("Invalid file path".to_string()))?
            .to_string_lossy()
            .to_string();

        let metadata = path.metadata()?;

        Ok(Self {
            id: Uuid::new_v4(),
            path,
            filename,
            size: metadata.len(),
            created_at: chrono::Utc::now(),
            metadata: None,
            library_id: None,
            parent_media_id: None,
        })
    }

    pub fn new_with_library(path: PathBuf, library_id: Uuid) -> crate::Result<Self> {
        let mut media_file = Self::new(path)?;
        media_file.library_id = Some(library_id);
        Ok(media_file)
    }

    /// Extract full metadata for this media file
    #[cfg(feature = "ffmpeg")]
    pub fn extract_metadata(&mut self) -> crate::Result<()> {
        let mut extractor = crate::MetadataExtractor::new();
        let metadata = extractor.extract_metadata(&self.path)?;
        self.metadata = Some(metadata);
        Ok(())
    }

    pub fn is_video_file(&self) -> bool {
        let video_extensions = ["mp4", "mkv", "avi", "mov", "webm", "flv", "wmv"];

        if let Some(extension) = self.path.extension() {
            if let Some(ext_str) = extension.to_str() {
                return video_extensions.contains(&ext_str.to_lowercase().as_str());
            }
        }

        false
    }
}
