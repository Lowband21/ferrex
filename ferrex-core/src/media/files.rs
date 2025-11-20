use crate::{MediaError, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use chrono::{DateTime, Utc};
use uuid::Uuid;


#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ParsedMediaInfo {
    Movie(ParsedMovieInfo),
    Episode(ParsedEpisodeInfo),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ParsedMovieInfo {
    pub title: String,
    pub year: Option<u16>,
    pub resolution: Option<String>,
    pub source: Option<String>,
    pub release_group: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
        
        // Get actual file creation time from filesystem metadata
        let created_at = metadata.created()
            .ok()
            .and_then(|time| {
                // Convert SystemTime to chrono DateTime
                let duration = time.duration_since(std::time::UNIX_EPOCH).ok()?;
                chrono::DateTime::<chrono::Utc>::from_timestamp(
                    duration.as_secs() as i64,
                    duration.subsec_nanos()
                )
            })
            .unwrap_or_else(|| {
                // Fallback to modified time if creation time is not available
                metadata.modified()
                    .ok()
                    .and_then(|time| {
                        let duration = time.duration_since(std::time::UNIX_EPOCH).ok()?;
                        chrono::DateTime::<chrono::Utc>::from_timestamp(
                            duration.as_secs() as i64,
                            duration.subsec_nanos()
                        )
                    })
                    .unwrap_or_else(chrono::Utc::now)
            });

        Ok(Self {
            id: Uuid::new_v4(),
            path,
            filename,
            size: metadata.len(),
            created_at,
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

