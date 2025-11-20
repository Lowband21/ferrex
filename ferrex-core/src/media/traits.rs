use crate::api_types::MediaId;
use super::{
    MovieReference, SeriesReference, SeasonReference, EpisodeReference,
    MediaDetailsOption, TmdbDetails, MediaFile,
    MovieID, SeriesID, SeasonID, EpisodeID,
};
use std::time::Duration;
use uuid::Uuid;


// ===== MediaRef Trait System =====
// 
// This trait system provides a clean interface for working with media references
// without the need for repetitive pattern matching. It maintains backward compatibility
// while offering better ergonomics for common operations.

/// Common interface for all media reference types
pub trait MediaRef: Send + Sync {
    /// Get the unique media ID
    fn id(&self) -> MediaId;
    
    /// Get the display title
    fn title(&self) -> &str;
    
    /// Get media details if available
    fn details(&self) -> &MediaDetailsOption;
    
    /// Get theme color if available
    fn theme_color(&self) -> Option<&str>;
    
    /// Get the API endpoint
    fn endpoint(&self) -> String;
    
    /// Get release/air year if available
    fn year(&self) -> Option<u16> {
        match self.details() {
            MediaDetailsOption::Details(details) => match details {
                TmdbDetails::Movie(movie) => movie.release_date
                    .as_ref()
                    .and_then(|date| date.split('-').next())
                    .and_then(|year| year.parse().ok()),
                TmdbDetails::Series(series) => series.first_air_date
                    .as_ref()
                    .and_then(|date| date.split('-').next())
                    .and_then(|year| year.parse().ok()),
                TmdbDetails::Season(season) => season.air_date
                    .as_ref()
                    .and_then(|date| date.split('-').next())
                    .and_then(|year| year.parse().ok()),
                TmdbDetails::Episode(episode) => episode.air_date
                    .as_ref()
                    .and_then(|date| date.split('-').next())
                    .and_then(|year| year.parse().ok()),
            },
            _ => None,
        }
    }
    
    /// Get rating if available
    fn rating(&self) -> Option<f32> {
        match self.details() {
            MediaDetailsOption::Details(details) => match details {
                TmdbDetails::Movie(movie) => movie.vote_average,
                TmdbDetails::Series(series) => series.vote_average,
                TmdbDetails::Episode(episode) => episode.vote_average,
                TmdbDetails::Season(_) => None, // Seasons don't have ratings
            },
            _ => None,
        }
    }
    
    /// Get genres if available
    fn genres(&self) -> Vec<&str> {
        match self.details() {
            MediaDetailsOption::Details(details) => match details {
                TmdbDetails::Movie(movie) => movie.genres.iter().map(|s| s.as_str()).collect(),
                TmdbDetails::Series(series) => series.genres.iter().map(|s| s.as_str()).collect(),
                _ => Vec::new(), // Episodes and seasons don't have their own genres
            },
            _ => Vec::new(),
        }
    }
}

/// Specialized trait for media that can be played
pub trait Playable: MediaRef {
    /// Get the media file
    fn file(&self) -> &MediaFile;
    
    /// Get duration if available from metadata
    fn duration(&self) -> Option<Duration> {
        self.file()
            .media_file_metadata
            .as_ref()
            .and_then(|meta| meta.duration)
            .map(|secs| Duration::from_secs_f64(secs))
    }
    
    /// Check if the media can be transcoded
    fn can_transcode(&self) -> bool {
        // Default implementation - can be overridden
        self.file().media_file_metadata.is_some()
    }
}

/// Specialized trait for media that contains other media
pub trait Browsable: MediaRef {
    /// Get the number of child items if known
    fn child_count(&self) -> Option<usize>;
    
    /// Get the library ID this media belongs to
    fn library_id(&self) -> Uuid;
}

// ===== MediaRef Implementations =====

impl MediaRef for MovieReference {
    fn id(&self) -> MediaId {
        MediaId::Movie(self.id.clone())
    }
    
    fn title(&self) -> &str {
        self.title.as_str()
    }
    
    fn details(&self) -> &MediaDetailsOption {
        &self.details
    }
    
    fn theme_color(&self) -> Option<&str> {
        self.theme_color.as_deref()
    }
    
    fn endpoint(&self) -> String {
        self.endpoint.as_str().to_string()
    }
}

impl MediaRef for SeriesReference {
    fn id(&self) -> MediaId {
        MediaId::Series(self.id.clone())
    }
    
    fn title(&self) -> &str {
        self.title.as_str()
    }
    
    fn details(&self) -> &MediaDetailsOption {
        &self.details
    }
    
    fn theme_color(&self) -> Option<&str> {
        self.theme_color.as_deref()
    }
    
    fn endpoint(&self) -> String {
        self.endpoint.as_str().to_string()
    }
}

impl MediaRef for SeasonReference {
    fn id(&self) -> MediaId {
        MediaId::Season(self.id.clone())
    }
    
    fn title(&self) -> &str {
        // For seasons, extract the name from details if available
        match &self.details {
            MediaDetailsOption::Details(TmdbDetails::Season(details)) => &details.name,
            _ => "Unknown Season",
        }
    }
    
    fn details(&self) -> &MediaDetailsOption {
        &self.details
    }
    
    fn theme_color(&self) -> Option<&str> {
        self.theme_color.as_deref()
    }
    
    fn endpoint(&self) -> String {
        self.endpoint.as_str().to_string()
    }
}

impl MediaRef for EpisodeReference {
    fn id(&self) -> MediaId {
        MediaId::Episode(self.id.clone())
    }
    
    fn title(&self) -> &str {
        // For episodes, extract the name from details if available
        match &self.details {
            MediaDetailsOption::Details(TmdbDetails::Episode(details)) => &details.name,
            _ => "Unknown Episode",
        }
    }
    
    fn details(&self) -> &MediaDetailsOption {
        &self.details
    }
    
    fn theme_color(&self) -> Option<&str> {
        None // Episodes don't have theme colors in the current schema
    }
    
    fn endpoint(&self) -> String {
        self.endpoint.as_str().to_string()
    }
}

// ===== Playable Implementations =====

impl Playable for MovieReference {
    fn file(&self) -> &MediaFile {
        &self.file
    }
}

impl Playable for EpisodeReference {
    fn file(&self) -> &MediaFile {
        &self.file
    }
}

// ===== Browsable Implementations =====

impl Browsable for SeriesReference {
    fn child_count(&self) -> Option<usize> {
        match &self.details {
            MediaDetailsOption::Details(TmdbDetails::Series(details)) => {
                details.number_of_episodes.map(|n| n as usize)
            }
            _ => None,
        }
    }
    
    fn library_id(&self) -> Uuid {
        self.library_id
    }
}

impl Browsable for SeasonReference {
    fn child_count(&self) -> Option<usize> {
        match &self.details {
            MediaDetailsOption::Details(TmdbDetails::Season(details)) => {
                Some(details.episode_count as usize)
            }
            _ => None,
        }
    }
    
    fn library_id(&self) -> Uuid {
        // Seasons now have library_id directly
        self.library_id
    }
}

