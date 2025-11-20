// Curated surface from ferrex-core for player-facing code
pub use ferrex_contracts::prelude::{EpisodeLike, SeasonLike};
pub use ferrex_core::player_prelude::{
    AdminUserInfo, ApiResponse, BatchMediaRequest, BatchMediaResponse,
    ConfirmClaimRequest, ConfirmClaimResponse, CreateLibraryRequest,
    CreateUserRequest, DemoLibraryStatus, DemoResetRequest, DemoStatus,
    EnhancedMovieDetails, EnhancedSeriesDetails, EpisodeID, EpisodeReference,
    FetchMediaRequest, ImageData, ImageRequest, ImageSize, ImageType, Library,
    LibraryID, LibraryMediaCache, LibraryMediaResponse, LibraryReference,
    LibraryType, Media, MediaDetailsOption, MediaFile, MediaFileMetadata,
    MediaID, MovieID, MovieReference, ParsedMediaInfo, Priority,
    ScanLifecycleStatus, ScanProgressEvent, ScanSnapshotDto, SeasonID,
    SeasonReference, SeriesID, SeriesReference, SortOrder, StartClaimRequest,
    StartClaimResponse, UpdateLibraryRequest, UpdateUserRequest,
    UserWatchState, WatchProgress,
};

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
        Media::Series(series) => {
            extract_poster_url_from_details(&series.details)
        }
        Media::Season(season) => {
            extract_poster_url_from_details(&season.details)
        }
        Media::Episode(episode) => {
            // Episodes use still images, not posters
            episode.details().and_then(|details| {
                details
                    .still_path
                    .as_ref()
                    .map(|path| get_tmdb_image_url(path))
            })
        }
    }
}

/// Extract poster URL from MediaDetailsOption
/// Returns either a server-cached endpoint or TMDB URL
pub fn extract_poster_url_from_details(
    details: &MediaDetailsOption,
) -> Option<String> {
    if let Some(movie) = details.as_movie() {
        movie.poster_path.as_ref().map(|path| {
            log::debug!("Movie {} has poster path: {}", movie.title, path);
            // Return the path as-is - it could be a server endpoint or TMDB path
            path.clone()
        })
    } else if let Some(series) = details.as_series() {
        series.poster_path.clone()
    } else if let Some(season) = details.as_season() {
        season.poster_path.clone()
    } else {
        None // Episodes don't have posters and endpoints lack paths
    }
}

/// Convert TMDB poster path to full URL
pub fn get_tmdb_image_url(path: &str) -> String {
    // If the server already provides a cached endpoint, return it directly.
    if path.starts_with("/images/") {
        return path.to_string();
    }
    // Otherwise, construct a TMDB URL (w500 is a good balance of quality/perf)
    format!("https://image.tmdb.org/t/p/w500{}", path)
}

// Poster presence and ordering are enforced centrally in core/server.
