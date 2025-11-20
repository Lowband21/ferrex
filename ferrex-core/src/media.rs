use crate::MediaError;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use url::Url;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaFile {
    pub id: Uuid,
    pub path: PathBuf,
    pub filename: String,
    pub size: u64,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub media_file_metadata: Option<MediaFileMetadata>,
    pub library_id: Uuid,
}

impl Default for MediaFile {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4(),
            path: PathBuf::new(),
            filename: String::new(),
            size: 0,
            created_at: chrono::Utc::now(),
            media_file_metadata: None,
            library_id: Uuid::nil(), // Use nil UUID for default
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaFileMetadata {
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
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ParsedMediaInfo {
    Movie(ParsedMovieInfo),
    Episode(ParsedEpisodeInfo),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedMovieInfo {
    pub title: String,
    pub year: Option<u16>,
    pub resolution: Option<String>,
    pub source: Option<String>,
    pub release_group: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedEpisodeInfo {
    pub show_name: String,
    pub season: u32,
    pub episode: u32,
    pub episode_title: Option<String>,
    pub year: Option<u16>,
    pub resolution: Option<String>,
    pub source: Option<String>,
    pub release_group: Option<String>,
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

impl MediaFile {
    pub fn new(path: PathBuf, library_id: Uuid) -> crate::Result<Self> {
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
            media_file_metadata: None,
            library_id,
        })
    }

    /// Extract full metadata for this media file
    #[cfg(feature = "ffmpeg")]
    pub fn extract_metadata(&mut self) -> crate::Result<()> {
        let mut extractor = crate::MetadataExtractor::new();
        let metadata = extractor.extract_metadata(&self.path)?;
        self.media_file_metadata = Some(metadata);
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
// Old placeholder types removed - see new implementation below

// We should have a library type that contains information about the library and at the very least some kind of index of the media it contains, if not the media types themselves
// We should have a Media wrapper type that can be either a Movie, Series, Season, or Episode, this will be sharable with the player
// Each of the types should have a specific type for metadata, Movie metadata, Series metadata, Season metadata, and Episode metadata, each their own types
// We should have a media endpoint type that is autovalidated and references the playback endpoint url
// Generally all unique types should have wrappers rather than just being strings so that we can have automatic input validation

// ===== NEW STRONGLY TYPED IDS AND REFERENCES =====

/// Strongly typed ID for movies with validation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct MovieID(String);

impl MovieID {
    pub fn new(id: String) -> Result<Self, MediaError> {
        if id.is_empty() {
            return Err(MediaError::InvalidMedia(
                "Movie ID cannot be empty".to_string(),
            ));
        }
        Ok(MovieID(id))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for MovieID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Strongly typed ID for series with validation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct SeriesID(String);

impl SeriesID {
    pub fn new(id: String) -> Result<Self, MediaError> {
        if id.is_empty() {
            return Err(MediaError::InvalidMedia(
                "Series ID cannot be empty".to_string(),
            ));
        }
        Ok(SeriesID(id))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for SeriesID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Strongly typed ID for seasons with validation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct SeasonID(String);

impl SeasonID {
    pub fn new(id: String) -> Result<Self, MediaError> {
        if id.is_empty() {
            return Err(MediaError::InvalidMedia(
                "Season ID cannot be empty".to_string(),
            ));
        }
        Ok(SeasonID(id))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for SeasonID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Strongly typed ID for episodes with validation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct EpisodeID(String);

impl EpisodeID {
    pub fn new(id: String) -> Result<Self, MediaError> {
        if id.is_empty() {
            return Err(MediaError::InvalidMedia(
                "Episode ID cannot be empty".to_string(),
            ));
        }
        Ok(EpisodeID(id))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for EpisodeID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Strongly typed ID for persons with validation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct PersonID(String);

impl PersonID {
    pub fn new(id: String) -> Result<Self, MediaError> {
        if id.is_empty() {
            return Err(MediaError::InvalidMedia(
                "Person ID cannot be empty".to_string(),
            ));
        }
        Ok(PersonID(id))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for PersonID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Strongly typed movie title
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MovieTitle(String);

impl MovieTitle {
    pub fn new(title: String) -> Result<Self, MediaError> {
        if title.is_empty() {
            return Err(MediaError::InvalidMedia(
                "Movie title cannot be empty".to_string(),
            ));
        }
        Ok(MovieTitle(title))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Strongly typed series title
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SeriesTitle(String);

impl SeriesTitle {
    pub fn new(title: String) -> Result<Self, MediaError> {
        if title.is_empty() {
            return Err(MediaError::InvalidMedia(
                "Series title cannot be empty".to_string(),
            ));
        }
        Ok(SeriesTitle(title))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Strongly typed episode title
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EpisodeTitle(String);

impl EpisodeTitle {
    pub fn new(title: String) -> Result<Self, MediaError> {
        if title.is_empty() {
            return Err(MediaError::InvalidMedia(
                "Episode title cannot be empty".to_string(),
            ));
        }
        Ok(EpisodeTitle(title))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Season number with u8 bounds
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct SeasonNumber(u8);

impl SeasonNumber {
    pub fn new(num: u8) -> Self {
        SeasonNumber(num)
    }

    pub fn value(&self) -> u8 {
        self.0
    }
}

/// Episode number with u8 bounds
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct EpisodeNumber(u8);

impl EpisodeNumber {
    pub fn new(num: u8) -> Self {
        EpisodeNumber(num)
    }

    pub fn value(&self) -> u8 {
        self.0
    }
}

/// Movie endpoint URL with validation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MovieURL(String);

impl MovieURL {
    pub fn new(url: Url) -> Self {
        MovieURL(url.to_string())
    }

    pub fn from_string(s: String) -> Self {
        MovieURL(s)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Series endpoint URL with validation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SeriesURL(String);

impl SeriesURL {
    pub fn new(url: Url) -> Self {
        SeriesURL(url.to_string())
    }

    pub fn from_string(s: String) -> Self {
        SeriesURL(s)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Season endpoint URL with validation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SeasonURL(String);

impl SeasonURL {
    pub fn new(url: Url) -> Self {
        SeasonURL(url.to_string())
    }

    pub fn from_string(s: String) -> Self {
        SeasonURL(s)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Episode endpoint URL with validation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EpisodeURL(String);

impl EpisodeURL {
    pub fn new(url: Url) -> Self {
        EpisodeURL(url.to_string())
    }

    pub fn from_string(s: String) -> Self {
        EpisodeURL(s)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Lightweight movie reference for lists/collections
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MediaReference {
    Movie(MovieReference),
    Series(SeriesReference),
    Season(SeasonReference),
    Episode(EpisodeReference),
}

/// Lightweight movie reference for lists/collections
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MovieReference {
    pub id: MovieID,
    pub tmdb_id: u64,
    pub title: MovieTitle,
    pub details: MediaDetailsOption,
    pub endpoint: MovieURL,
    pub file: MediaFile,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme_color: Option<String>, // Hex color e.g. "#2C3E50"
}

/// Lightweight series reference for lists/collections
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeriesReference {
    pub id: SeriesID,
    pub library_id: Uuid,
    pub tmdb_id: u64,
    pub title: SeriesTitle,
    pub details: MediaDetailsOption,
    pub endpoint: SeriesURL,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme_color: Option<String>, // Hex color e.g. "#2C3E50"
}

/// Lightweight season reference
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeasonReference {
    pub id: SeasonID,
    pub season_number: SeasonNumber,
    pub series_id: SeriesID, // Link to parent series
    pub tmdb_series_id: u64,
    pub details: MediaDetailsOption,
    pub endpoint: SeasonURL,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme_color: Option<String>, // Hex color e.g. "#2C3E50"
}

/// Lightweight episode reference
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpisodeReference {
    pub id: EpisodeID,
    pub episode_number: EpisodeNumber,
    pub season_number: SeasonNumber,
    pub season_id: SeasonID, // Link to parent season
    pub series_id: SeriesID, // Link to parent series
    pub tmdb_series_id: u64,
    pub details: MediaDetailsOption,
    pub endpoint: EpisodeURL,
    pub file: MediaFile,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MediaDetailsOption {
    Endpoint(String),
    Details(TmdbDetails),
}

impl MediaDetailsOption {
    /// Extract release year from movie details if available
    pub fn get_release_year(&self) -> Option<u16> {
        match self {
            MediaDetailsOption::Endpoint(_) => None,
            MediaDetailsOption::Details(details) => match details {
                TmdbDetails::Movie(movie) => movie
                    .release_date
                    .as_ref()
                    .and_then(|date| date.split('-').next())
                    .and_then(|year| year.parse().ok()),
                _ => None,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TmdbDetails {
    Movie(EnhancedMovieDetails),
    Series(EnhancedSeriesDetails),
    Season(SeasonDetails),
    Episode(EpisodeDetails),
}

/// Enhanced metadata that includes images, credits, and additional information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnhancedMovieDetails {
    // Basic details
    pub id: u64,
    pub title: String,
    pub overview: Option<String>,
    pub release_date: Option<String>,
    pub runtime: Option<u32>,
    pub vote_average: Option<f32>,
    pub vote_count: Option<u32>,
    pub popularity: Option<f32>,
    pub genres: Vec<String>,
    pub production_companies: Vec<String>,

    // Media assets
    pub poster_path: Option<String>,
    pub backdrop_path: Option<String>,
    pub logo_path: Option<String>,
    pub images: MediaImages,

    // Credits
    pub cast: Vec<CastMember>,
    pub crew: Vec<CrewMember>,

    // Additional
    pub videos: Vec<Video>,
    pub keywords: Vec<String>,
    pub external_ids: ExternalIds,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnhancedSeriesDetails {
    // Basic details
    pub id: u64,
    pub name: String,
    pub overview: Option<String>,
    pub first_air_date: Option<String>,
    pub last_air_date: Option<String>,
    pub number_of_seasons: Option<u32>,
    pub number_of_episodes: Option<u32>,
    pub vote_average: Option<f32>,
    pub vote_count: Option<u32>,
    pub popularity: Option<f32>,
    pub genres: Vec<String>,
    pub networks: Vec<String>,

    // Media assets
    pub poster_path: Option<String>,
    pub backdrop_path: Option<String>,
    pub logo_path: Option<String>,
    pub images: MediaImages,

    // Credits
    pub cast: Vec<CastMember>,
    pub crew: Vec<CrewMember>,

    // Additional
    pub videos: Vec<Video>,
    pub keywords: Vec<String>,
    pub external_ids: ExternalIds,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeasonDetails {
    pub id: u64,
    pub season_number: u8,
    pub name: String,
    pub overview: Option<String>,
    pub air_date: Option<String>,
    pub episode_count: u32,
    pub poster_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpisodeDetails {
    pub id: u64,
    pub episode_number: u8,
    pub season_number: u8,
    pub name: String,
    pub overview: Option<String>,
    pub air_date: Option<String>,
    pub runtime: Option<u32>,
    pub still_path: Option<String>,
    pub vote_average: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageMetadata {
    pub file_path: String,
    pub width: u64,
    pub height: u64,
    pub aspect_ratio: f64,
    pub iso_639_1: Option<String>,
    pub vote_average: f64,
    pub vote_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageWithMetadata {
    pub endpoint: String,
    pub metadata: ImageMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MediaImages {
    pub posters: Vec<ImageWithMetadata>,
    pub backdrops: Vec<ImageWithMetadata>,
    pub logos: Vec<ImageWithMetadata>,
    pub stills: Vec<ImageWithMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CastMember {
    pub id: u64,
    pub name: String,
    pub character: String,
    pub profile_path: Option<String>,
    pub order: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrewMember {
    pub id: u64,
    pub name: String,
    pub job: String,
    pub department: String,
    pub profile_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Video {
    pub key: String,
    pub name: String,
    pub site: String,
    pub video_type: String,
    pub official: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExternalIds {
    pub imdb_id: Option<String>,
    pub tvdb_id: Option<u32>,
    pub facebook_id: Option<String>,
    pub instagram_id: Option<String>,
    pub twitter_id: Option<String>,
}

// Media enum removed - duplicate of definition in tmdb_api_provider.rs

// Library reference type - no media references
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryReference {
    pub id: Uuid,
    pub name: String,
    pub library_type: crate::LibraryType,
    pub paths: Vec<PathBuf>,
}
