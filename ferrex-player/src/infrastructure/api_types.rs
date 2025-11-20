// Re-export types from ferrex-core for use in the player
pub use ferrex_core::media::{
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

    // Details
    MediaDetailsOption,
    // File types (still needed for playback)
    MediaFile,
    MediaFileMetadata,
    //Video,
    // Supporting types
    //MediaImages,
    // References
    MediaReference,
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
    ApiResponse,
    BatchMediaRequest,
    BatchMediaResponse,
    CreateLibraryRequest,
    FetchMediaRequest,
    //LibraryFilters,
    // API types
    LibraryMediaCache,
    LibraryMediaResponse,
    //ManualMatchRequest,
    //MediaEvent,
    MediaId,
    //MediaStats,
    //MetadataRequest,
    ScanProgress,
    //ScanRequest,
    ScanStatus,
    UpdateLibraryRequest,
};

pub use ferrex_core::library::Library;

pub use ferrex_core::LibraryType;

pub use ferrex_core::watch_status::{UserWatchState, WatchProgress};

// Conversion utilities for backward compatibility during migration

use crate::domains::media::library::{
    ExternalInfo as LegacyExternalInfo, MediaFile as LegacyMediaFile,
    MediaMetadata as LegacyMediaMetadata, ParsedInfo as LegacyParsedInfo,
};

/// Convert new MediaFile to legacy MediaFile for backward compatibility
pub fn to_legacy_media_file(file: &MediaFile) -> LegacyMediaFile {
    LegacyMediaFile {
        id: file.id.to_string(),
        filename: file.filename.clone(),
        path: file.path.to_string_lossy().to_string(),
        size: file.size,
        created_at: file.created_at.to_rfc3339(),
        metadata: file.media_file_metadata.as_ref().map(to_legacy_metadata),
        library_id: Some(file.library_id.to_string()),
    }
}

fn to_legacy_metadata(metadata: &MediaFileMetadata) -> LegacyMediaMetadata {
    LegacyMediaMetadata {
        duration: metadata.duration,
        width: metadata.width,
        height: metadata.height,
        video_codec: metadata.video_codec.clone(),
        audio_codec: metadata.audio_codec.clone(),
        bitrate: metadata.bitrate,
        framerate: metadata.framerate,
        file_size: metadata.file_size,
        color_primaries: metadata.color_primaries.clone(),
        color_transfer: metadata.color_transfer.clone(),
        color_space: metadata.color_space.clone(),
        bit_depth: metadata.bit_depth,
        parsed_info: metadata.parsed_info.as_ref().map(to_legacy_parsed_info),
        external_info: None, // Will be populated from reference metadata
    }
}

fn to_legacy_parsed_info(info: &ParsedMediaInfo) -> LegacyParsedInfo {
    match info {
        ParsedMediaInfo::Movie(movie_info) => LegacyParsedInfo {
            media_type: "Movie".to_string(),
            title: movie_info.title.clone(),
            year: movie_info.year,
            show_name: None,
            season: None,
            episode: None,
            episode_title: None,
            resolution: movie_info.resolution.clone(),
            source: movie_info.source.clone(),
            release_group: movie_info.release_group.clone(),
        },
        ParsedMediaInfo::Episode(episode_info) => LegacyParsedInfo {
            media_type: "TvEpisode".to_string(),
            title: episode_info.show_name.clone(),
            year: episode_info.year,
            show_name: Some(episode_info.show_name.clone()),
            season: Some(episode_info.season),
            episode: Some(episode_info.episode),
            episode_title: episode_info.episode_title.clone(),
            resolution: episode_info.resolution.clone(),
            source: episode_info.source.clone(),
            release_group: episode_info.release_group.clone(),
        },
    }
}

/// Convert MovieReference to legacy MediaFile for movie display
pub fn movie_reference_to_legacy(movie: &MovieReference, _server_url: &str) -> LegacyMediaFile {
    let mut legacy = to_legacy_media_file(&movie.file);

    // Add external info from movie details if available
    if let MediaDetailsOption::Details(TmdbDetails::Movie(details)) = &movie.details {
        if let Some(metadata) = &mut legacy.metadata {
            metadata.external_info = Some(movie_details_to_external_info(details));
        }
    }

    legacy
}

/// Convert EpisodeReference to legacy MediaFile for episode display
pub fn episode_reference_to_legacy(
    episode: &EpisodeReference,
    series_details: Option<&EnhancedSeriesDetails>,
) -> LegacyMediaFile {
    let mut legacy = to_legacy_media_file(&episode.file);

    // Add external info from episode/series details if available
    if let Some(metadata) = &mut legacy.metadata {
        metadata.external_info = Some(episode_to_external_info(episode, series_details));
    }

    legacy
}

fn movie_details_to_external_info(details: &EnhancedMovieDetails) -> LegacyExternalInfo {
    LegacyExternalInfo {
        tmdb_id: Some(details.id as u32),
        tvdb_id: None,
        imdb_id: details.external_ids.imdb_id.clone(),
        description: details.overview.clone(),
        poster_url: details.poster_path.clone(),
        backdrop_url: details.backdrop_path.clone(),
        genres: details.genres.clone(),
        rating: details.vote_average,
        release_date: details.release_date.clone(),
        show_description: None,
        show_poster_url: None,
        season_poster_url: None,
        episode_still_url: None,
        extra_type: None,
        parent_title: None,
    }
}

fn episode_to_external_info(
    episode: &EpisodeReference,
    series_details: Option<&EnhancedSeriesDetails>,
) -> LegacyExternalInfo {
    let (show_description, show_poster, genres, rating) = if let Some(series) = series_details {
        (
            series.overview.clone(),
            series.poster_path.clone(),
            series.genres.clone(),
            series.vote_average,
        )
    } else {
        (None, None, Vec::new(), None)
    };

    let (episode_description, episode_still) = match &episode.details {
        MediaDetailsOption::Details(TmdbDetails::Episode(details)) => {
            (details.overview.clone(), details.still_path.clone())
        }
        _ => (None, None),
    };

    LegacyExternalInfo {
        tmdb_id: Some(episode.tmdb_series_id as u32),
        tvdb_id: None,
        imdb_id: None,
        description: episode_description,
        poster_url: episode_still.clone(),
        backdrop_url: None,
        genres,
        rating,
        release_date: None,
        show_description,
        show_poster_url: show_poster,
        season_poster_url: None,
        episode_still_url: episode_still,
        extra_type: None,
        parent_title: None,
    }
}

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

/// Extract poster URL from MediaReference
pub fn extract_poster_url_from_reference(media_ref: &MediaReference) -> Option<String> {
    match media_ref {
        MediaReference::Movie(movie) => extract_poster_url_from_details(&movie.details),
        MediaReference::Series(series) => extract_poster_url_from_details(&series.details),
        MediaReference::Season(season) => extract_poster_url_from_details(&season.details),
        MediaReference::Episode(episode) => {
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

/// Get the media ID from a MediaReference
pub fn get_media_id_from_reference(media_ref: &MediaReference) -> String {
    match media_ref {
        MediaReference::Movie(movie) => movie.id.as_str().to_string(),
        MediaReference::Series(series) => series.id.as_str().to_string(),
        MediaReference::Season(season) => season.id.as_str().to_string(),
        MediaReference::Episode(episode) => episode.id.as_str().to_string(),
    }
}
