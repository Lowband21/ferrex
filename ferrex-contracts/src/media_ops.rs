use super::id::MediaIDLike;
use ferrex_model::details::{
    MediaDetailsOption, MediaDetailsOptionLike, TmdbDetails,
};
use ferrex_model::files::MediaFile;
use ferrex_model::ids::{EpisodeID, LibraryID, MovieID, SeasonID, SeriesID};
use ferrex_model::media::{
    EpisodeReference, Media, MovieReference, SeasonReference, SeriesReference,
};
use ferrex_model::media_id::MediaID;
use ferrex_model::urls::UrlLike;
use std::time::Duration;

// ===== Media Trait System =====
//
// This trait system provides a clean interface for working with media references
// without the need for repetitive pattern matching. It maintains backward compatibility
// while offering better ergonomics for common operations.

/// Common interface for all media reference types
pub trait MediaOps: Send + Sync {
    type Id: MediaIDLike;

    fn id(&self) -> Self::Id;

    /// Get the unique media ID
    fn media_id(&self) -> MediaID;

    /// Get theme color if available
    fn theme_color(&self) -> Option<&str>;

    /// Get the API endpoint
    fn endpoint(&self) -> String;
}

/// Specialized trait for media that can be played
pub trait Playable: MediaOps {
    /// Get the media file
    fn file(&self) -> &MediaFile;

    /// Get duration if available from metadata
    fn duration(&self) -> Option<Duration> {
        self.file()
            .media_file_metadata
            .as_ref()
            .and_then(|meta| meta.duration)
            .map(Duration::from_secs_f64)
    }

    /// Check if the media can be transcoded
    fn can_transcode(&self) -> bool {
        // Default implementation - can be overridden
        self.file().media_file_metadata.is_some()
    }
}

pub trait Details: MediaOps {
    /// Get the media details
    fn details(&self) -> &impl MediaDetailsOptionLike;

    /// Get release/air year if available
    fn year(&self) -> Option<u16>;

    /// Get rating if available
    fn rating(&self) -> Option<f32>;

    /// Get genres if available
    fn genres(&self) -> Option<&Vec<String>>;
}

/// Specialized trait for media that contains other media
pub trait Browsable: MediaOps {
    /// Get the number of child items if known
    fn child_count(&self) -> Option<usize>;

    /// Get the library ID this media belongs to
    fn library_id(&self) -> LibraryID;
}

// ===== MediaOps Implementations =====

impl MediaOps for Media {
    type Id = MediaID;

    fn id(&self) -> Self::Id {
        match &self {
            Media::Movie(movie) => MediaID::Movie(movie.id),
            Media::Series(series) => MediaID::Series(series.id),
            Media::Season(season) => MediaID::Season(season.id),
            Media::Episode(episode) => MediaID::Episode(episode.id),
        }
    }

    fn media_id(&self) -> MediaID {
        match &self {
            Media::Movie(movie) => MediaID::Movie(movie.id),
            Media::Series(series) => MediaID::Series(series.id),
            Media::Season(season) => MediaID::Season(season.id),
            Media::Episode(episode) => MediaID::Episode(episode.id),
        }
    }

    fn theme_color(&self) -> Option<&str> {
        match &self {
            Media::Movie(movie) => movie.theme_color.as_deref(),
            Media::Series(series) => series.theme_color.as_deref(),
            Media::Season(season) => season.theme_color.as_deref(),
            Media::Episode(_) => unimplemented!(),
        }
    }

    fn endpoint(&self) -> String {
        match &self {
            Media::Movie(movie) => movie.endpoint.as_str().to_string(),
            Media::Series(series) => series.endpoint.as_str().to_string(),
            Media::Season(season) => season.endpoint.as_str().to_string(),
            Media::Episode(episode) => episode.endpoint.as_str().to_string(),
        }
    }
}

impl MediaOps for MovieReference {
    type Id = MovieID;

    fn id(&self) -> Self::Id {
        self.id
    }

    fn media_id(&self) -> MediaID {
        MediaID::Movie(self.id)
    }

    fn theme_color(&self) -> Option<&str> {
        self.theme_color.as_deref()
    }

    fn endpoint(&self) -> String {
        self.endpoint.as_str().to_string()
    }
}

impl MediaOps for SeriesReference {
    type Id = SeriesID;

    fn id(&self) -> Self::Id {
        self.id
    }

    fn media_id(&self) -> MediaID {
        MediaID::Series(self.id)
    }

    fn theme_color(&self) -> Option<&str> {
        self.theme_color.as_deref()
    }

    fn endpoint(&self) -> String {
        self.endpoint.as_str().to_string()
    }
}

impl MediaOps for SeasonReference {
    type Id = SeasonID;

    fn id(&self) -> Self::Id {
        self.id
    }

    fn media_id(&self) -> MediaID {
        MediaID::Season(self.id)
    }

    fn theme_color(&self) -> Option<&str> {
        self.theme_color.as_deref()
    }

    fn endpoint(&self) -> String {
        self.endpoint.as_str().to_string()
    }
}

impl MediaOps for EpisodeReference {
    type Id = EpisodeID;

    fn id(&self) -> Self::Id {
        self.id
    }

    fn media_id(&self) -> MediaID {
        MediaID::Episode(self.id)
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
            MediaDetailsOption::Details(details) => match details.as_ref() {
                TmdbDetails::Series(details) => {
                    details.number_of_episodes.map(|n| n as usize)
                }
                _ => None,
            },
            _ => None,
        }
    }

    fn library_id(&self) -> LibraryID {
        self.library_id
    }
}

impl Browsable for SeasonReference {
    fn child_count(&self) -> Option<usize> {
        match &self.details {
            MediaDetailsOption::Details(details) => match details.as_ref() {
                TmdbDetails::Season(details) => {
                    Some(details.episode_count as usize)
                }
                _ => None,
            },
            _ => None,
        }
    }

    fn library_id(&self) -> LibraryID {
        // Seasons now have library_id directly
        self.library_id
    }
}

#[cfg(feature = "rkyv")]
mod archived {
    use super::*;
    use ferrex_model::media::{
        ArchivedEpisodeReference, ArchivedMedia, ArchivedMovieReference,
        ArchivedSeasonReference, ArchivedSeriesReference,
    };
    use ferrex_model::media_id::ArchivedMediaID;

    impl MediaOps for ArchivedMedia {
        type Id = ArchivedMediaID;

        fn id(&self) -> Self::Id {
            match &self {
                ArchivedMedia::Movie(movie) => ArchivedMediaID::Movie(movie.id),
                ArchivedMedia::Series(series) => {
                    ArchivedMediaID::Series(series.id)
                }
                ArchivedMedia::Season(season) => {
                    ArchivedMediaID::Season(season.id)
                }
                ArchivedMedia::Episode(episode) => {
                    ArchivedMediaID::Episode(episode.id)
                }
            }
        }

        fn media_id(&self) -> MediaID {
            match &self {
                ArchivedMedia::Movie(movie) => {
                    MediaID::Movie(MovieID(movie.id.0))
                }
                ArchivedMedia::Series(series) => {
                    MediaID::Series(SeriesID(series.id.0))
                }
                ArchivedMedia::Season(season) => {
                    MediaID::Season(SeasonID(season.id.0))
                }
                ArchivedMedia::Episode(episode) => {
                    MediaID::Episode(EpisodeID(episode.id.0))
                }
            }
        }

        fn theme_color(&self) -> Option<&str> {
            match &self {
                ArchivedMedia::Movie(movie) => movie.theme_color.as_deref(),
                ArchivedMedia::Series(series) => series.theme_color.as_deref(),
                ArchivedMedia::Season(season) => season.theme_color.as_deref(),
                ArchivedMedia::Episode(_) => unimplemented!(),
            }
        }

        fn endpoint(&self) -> String {
            match &self {
                ArchivedMedia::Movie(movie) => movie.endpoint(),
                ArchivedMedia::Series(series) => series.endpoint(),
                ArchivedMedia::Season(season) => season.endpoint(),
                ArchivedMedia::Episode(episode) => episode.endpoint(),
            }
        }
    }

    impl MediaOps for ArchivedMovieReference {
        type Id = MovieID;

        fn id(&self) -> Self::Id {
            MovieID(self.id.0)
        }

        fn media_id(&self) -> MediaID {
            MediaID::from(self.id)
        }

        fn theme_color(&self) -> Option<&str> {
            self.theme_color.as_ref().map(|color| color.as_str())
        }

        fn endpoint(&self) -> String {
            self.endpoint.to_string()
        }
    }

    impl MediaOps for ArchivedSeriesReference {
        type Id = SeriesID;

        fn id(&self) -> Self::Id {
            SeriesID(self.id.0)
        }

        fn media_id(&self) -> MediaID {
            MediaID::from(self.id)
        }

        fn theme_color(&self) -> Option<&str> {
            self.theme_color.as_ref().map(|color| color.as_str())
        }

        fn endpoint(&self) -> String {
            self.endpoint.to_string()
        }
    }

    impl MediaOps for ArchivedSeasonReference {
        type Id = SeasonID;

        fn id(&self) -> Self::Id {
            SeasonID(self.id.0)
        }

        fn media_id(&self) -> MediaID {
            MediaID::from(self.id)
        }

        fn theme_color(&self) -> Option<&str> {
            self.theme_color.as_ref().map(|color| color.as_str())
        }

        fn endpoint(&self) -> String {
            self.endpoint.to_string()
        }
    }

    impl MediaOps for ArchivedEpisodeReference {
        type Id = EpisodeID;

        fn id(&self) -> Self::Id {
            EpisodeID(self.id.0)
        }

        fn media_id(&self) -> MediaID {
            MediaID::from(self.id)
        }

        fn theme_color(&self) -> Option<&str> {
            log::warn!("Theme color not implemented for episode reference");
            Option::None
        }

        fn endpoint(&self) -> String {
            self.endpoint.to_string()
        }
    }
}
