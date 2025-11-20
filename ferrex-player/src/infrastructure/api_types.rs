// Re-export types from ferrex-core for use in the player
pub use ferrex_core::{
    //CastMember,
    //CrewMember,
    EnhancedMovieDetails,
    EnhancedSeriesDetails,
    //EpisodeDetails,
    EpisodeID,

    //EpisodeNumber,
    EpisodeReference,

    //EpisodeTitle,

    //EpisodeURL,

    //ExternalIds,

    // Library types
    LibraryReference,

    //Video,
    // Supporting types
    //MediaImages,
    // References
    Media,
    // Details
    MediaDetailsOption,
    // File types (still needed for playback)
    MediaFile,
    MediaFileMetadata,
    MediaID,
    // IDs
    MovieID,
    MovieReference,
    // Titles
    //MovieTitle,
    // URLs
    //MovieURL,
    //ParsedEpisodeInfo,
    ParsedMediaInfo,
    //ParsedMovieInfo,
    //SeasonDetails,
    SeasonID,
    // Numbers
    //SeasonNumber,
    SeasonReference,
    //SeasonURL,
    SeriesID,
    SeriesReference,
    //SeriesTitle,
    //SeriesURL,
    TmdbDetails,
};

pub use ferrex_core::api_types::{
    ApiResponse, BatchMediaRequest, BatchMediaResponse, CreateLibraryRequest, FetchMediaRequest,
    LibraryMediaCache, LibraryMediaResponse, UpdateLibraryRequest,
};

pub use ferrex_core::types::library::Library;

pub use ferrex_core::LibraryType;

use ferrex_core::MediaIDLike;
pub use ferrex_core::watch_status::{UserWatchState, WatchProgress};

/// Helper to check if we need to fetch full details
pub fn needs_details_fetch(details: &MediaDetailsOption) -> bool {
    matches!(details, MediaDetailsOption::Endpoint(_))
}

/// Extract endpoint URL from MediaDetailsOption
pub fn get_details_endpoint(details: &MediaDetailsOption) -> Option<&str> {
    match details {
        MediaDetailsOption::Endpoint(url) => Some(url),
        MediaDetailsOption::Details(_) => None,
    }
}

/// Extract poster URL from Media
pub fn extract_poster_url_from_reference(media_ref: &Media) -> Option<String> {
    match media_ref {
        Media::Movie(movie) => extract_poster_url_from_details(&movie.details),
        Media::Series(series) => extract_poster_url_from_details(&series.details),
        Media::Season(season) => extract_poster_url_from_details(&season.details),
        Media::Episode(episode) => {
            // Episodes use still images, not posters
            match &episode.details {
                MediaDetailsOption::Details(TmdbDetails::Episode(details)) => details
                    .still_path
                    .as_ref()
                    .map(|path| get_tmdb_image_url(path)),
                _ => None,
            }
        }
    }
}

/// Extract poster URL from MediaDetailsOption
/// Returns either a server-cached endpoint or TMDB URL
pub fn extract_poster_url_from_details(details: &MediaDetailsOption) -> Option<String> {
    match details {
        MediaDetailsOption::Details(tmdb_details) => match tmdb_details {
            TmdbDetails::Movie(movie) => {
                movie.poster_path.as_ref().map(|path| {
                    log::debug!("Movie {} has poster path: {}", movie.title, path);
                    // Return the path as-is - it could be a server endpoint or TMDB path
                    path.clone()
                })
            }
            TmdbDetails::Series(series) => {
                series.poster_path.as_ref().map(|path| {
                    // Return the path as-is - it could be a server endpoint or TMDB path
                    path.clone()
                })
            }
            TmdbDetails::Season(season) => {
                season.poster_path.as_ref().map(|path| {
                    // Return the path as-is - it could be a server endpoint or TMDB path
                    path.clone()
                })
            }
            TmdbDetails::Episode(_) => None, // Episodes don't have posters
        },
        MediaDetailsOption::Endpoint(_) => None, // No poster URL without details
    }
}

/// Convert TMDB poster path to full URL
pub fn get_tmdb_image_url(path: &str) -> String {
    // Check if this is a server endpoint path (legacy data) or actual TMDB path
    if path.starts_with("/images/") {
        // This is a server endpoint, not a TMDB path
        // Log warning and return empty to trigger fallback
        log::warn!("Invalid TMDB path detected (server endpoint): {}", path);
        return String::new();
    }

    // TMDB image base URL with w500 size (good balance of quality and performance)
    format!("https://image.tmdb.org/t/p/w500{}", path)
}
