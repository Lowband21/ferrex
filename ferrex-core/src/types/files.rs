use crate::error::{MediaError, Result};
#[cfg(feature = "ffmpeg")]
use crate::metadata::MetadataExtractor;
use rkyv::{
    Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize,
};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;
use uuid::Uuid;

use super::LibraryID;

#[derive(
    Clone,
    Serialize,
    Deserialize,
    PartialEq,
    Archive,
    RkyvSerialize,
    RkyvDeserialize,
)]
#[rkyv(derive(Debug, PartialEq, Eq))]
pub struct MediaFile {
    pub id: Uuid,
    #[rkyv(with = crate::rkyv_wrappers::PathBufWrapper)]
    pub path: PathBuf,
    pub filename: String,
    pub size: u64,
    #[rkyv(with = crate::rkyv_wrappers::DateTimeWrapper)]
    pub discovered_at: chrono::DateTime<chrono::Utc>,
    #[rkyv(with = crate::rkyv_wrappers::DateTimeWrapper)]
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub media_file_metadata: Option<MediaFileMetadata>,
    pub library_id: LibraryID,
}

impl Default for MediaFile {
    fn default() -> Self {
        Self {
            id: Uuid::now_v7(),
            path: PathBuf::new(),
            filename: String::new(),
            size: 0,
            discovered_at: chrono::Utc::now(),
            created_at: chrono::Utc::now(),
            media_file_metadata: None,
            library_id: LibraryID(Uuid::nil()), // Use nil UUID for default
        }
    }
}

#[derive(
    Clone,
    Serialize,
    Deserialize,
    PartialEq,
    Archive,
    RkyvSerialize,
    RkyvDeserialize,
)]
#[rkyv(derive(Debug, PartialEq, Eq))]
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

impl fmt::Debug for MediaFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MediaFile")
            .field("id", &self.id)
            .field("filename", &self.filename)
            .field("path", &self.path)
            .field("size", &self.size)
            .field("discovered_at", &self.discovered_at)
            .field("created_at", &self.created_at)
            .field("has_metadata", &self.media_file_metadata.is_some())
            .field("metadata", &self.media_file_metadata.as_ref())
            .field("library_id", &self.library_id)
            .finish()
    }
}

impl fmt::Debug for MediaFileMetadata {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let resolution = self.width.zip(self.height);
        let parsed_kind = self.parsed_info.as_ref().map(|info| match info {
            ParsedMediaInfo::Movie(_) => "Movie",
            ParsedMediaInfo::Episode(_) => "Episode",
        });

        f.debug_struct("MediaFileMetadata")
            .field("duration", &self.duration)
            .field("resolution", &resolution)
            .field("video_codec", &self.video_codec)
            .field("audio_codec", &self.audio_codec)
            .field("bitrate", &self.bitrate)
            .field("framerate", &self.framerate)
            .field("file_size", &self.file_size)
            .field(
                "hdr",
                &(
                    &self.color_primaries,
                    &self.color_transfer,
                    &self.color_space,
                    &self.bit_depth,
                ),
            )
            .field("parsed_info_kind", &parsed_kind)
            .finish()
    }
}
#[derive(
    Debug,
    Clone,
    Serialize,
    Deserialize,
    PartialEq,
    Archive,
    RkyvSerialize,
    RkyvDeserialize,
)]
#[rkyv(derive(Debug, PartialEq, Eq))]
pub enum ParsedMediaInfo {
    Movie(ParsedMovieInfo),
    Episode(ParsedEpisodeInfo),
}

#[derive(
    Debug,
    Clone,
    Serialize,
    Deserialize,
    PartialEq,
    Archive,
    RkyvSerialize,
    RkyvDeserialize,
)]
#[rkyv(derive(Debug, PartialEq, Eq))]
pub struct ParsedMovieInfo {
    pub title: String,
    pub year: Option<u16>,
    pub resolution: Option<String>,
    pub source: Option<String>,
    pub release_group: Option<String>,
}

#[derive(
    Debug,
    Clone,
    Serialize,
    Deserialize,
    PartialEq,
    Archive,
    RkyvSerialize,
    RkyvDeserialize,
)]
#[rkyv(derive(Debug, PartialEq, Eq))]
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

#[derive(
    Debug,
    Clone,
    Serialize,
    Deserialize,
    PartialEq,
    Archive,
    RkyvSerialize,
    RkyvDeserialize,
)]
#[serde(rename_all = "PascalCase")]
#[rkyv(derive(Debug, PartialEq, Eq))]
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
    pub fn new(path: PathBuf, library_id: LibraryID) -> Result<Self> {
        let filename = path
            .file_name()
            .ok_or_else(|| {
                MediaError::InvalidMedia("Invalid file path".to_string())
            })?
            .to_string_lossy()
            .to_string();

        let metadata = path.metadata().map_err(MediaError::Io)?;

        // Get actual file creation time from filesystem metadata
        let created_at = metadata
            .created()
            .ok()
            .and_then(|time| {
                // Convert SystemTime to chrono DateTime
                let duration =
                    time.duration_since(std::time::UNIX_EPOCH).ok()?;
                chrono::DateTime::<chrono::Utc>::from_timestamp(
                    duration.as_secs() as i64,
                    duration.subsec_nanos(),
                )
            })
            .unwrap_or_else(|| {
                // Fallback to modified time if creation time is not available
                metadata
                    .modified()
                    .ok()
                    .and_then(|time| {
                        let duration =
                            time.duration_since(std::time::UNIX_EPOCH).ok()?;
                        chrono::DateTime::<chrono::Utc>::from_timestamp(
                            duration.as_secs() as i64,
                            duration.subsec_nanos(),
                        )
                    })
                    .unwrap_or_else(chrono::Utc::now)
            });

        let size = metadata.len();

        #[cfg(feature = "demo")]
        let allow_zero_length = crate::demo::allow_zero_length_for(&library_id);
        #[cfg(not(feature = "demo"))]
        let allow_zero_length = false;

        if size == 0 && !allow_zero_length {
            return Err(MediaError::InvalidMedia(
                "Zero-length media files are not supported".to_string(),
            ));
        }

        Ok(Self {
            id: Uuid::now_v7(),
            path,
            filename,
            size,
            // discovered_at represents when we discovered the file in the library (row creation time)
            // DB provides a default NOW(); set it here for in-memory consistency
            discovered_at: chrono::Utc::now(),
            created_at,
            media_file_metadata: None,
            library_id,
        })
    }

    /// Extract full metadata for this media file
    #[cfg(feature = "ffmpeg")]
    pub fn extract_metadata(&mut self) -> Result<()> {
        let mut extractor = MetadataExtractor::new();
        let metadata = extractor.extract_metadata(&self.path)?;
        self.media_file_metadata = Some(metadata);
        Ok(())
    }

    pub fn is_video_file(&self) -> bool {
        let video_extensions =
            ["mp4", "mkv", "avi", "mov", "webm", "flv", "wmv"];

        if let Some(extension) = self.path.extension()
            && let Some(ext_str) = extension.to_str()
        {
            return video_extensions.contains(&ext_str.to_lowercase().as_str());
        }

        false
    }
}
