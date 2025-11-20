use crate::media_library::MediaFile;
use serde::{Deserialize, Serialize};
use serde_json;
use std::collections::HashMap;

/// Represents a TV show with all its seasons and episodes
#[derive(Debug, Clone)]
pub struct TvShow {
    pub name: String,
    pub tmdb_id: Option<u32>,
    pub poster_url: Option<String>,
    pub backdrop_url: Option<String>,
    pub description: Option<String>,
    pub genres: Vec<String>,
    pub rating: Option<f32>,
    pub seasons: HashMap<u32, Season>,
    pub total_episodes: usize,
}

impl TvShow {
    /// Create a new TV show from the first episode
    pub fn from_episode(episode: &MediaFile) -> Option<Self> {
        let parsed = episode.metadata.as_ref()?.parsed_info.as_ref()?;
        let show_name = parsed.show_name.clone()?;
        let season_num = parsed.season?;

        let mut show = Self {
            name: show_name,
            tmdb_id: None,
            poster_url: None,
            backdrop_url: None,
            description: None,
            genres: Vec::new(),
            rating: None,
            seasons: HashMap::new(),
            total_episodes: 0,
        };

        // Extract show-level metadata from external info
        if let Some(external) = &episode.metadata.as_ref()?.external_info {
            show.tmdb_id = external.tmdb_id;
            show.poster_url = external
                .show_poster_url
                .clone()
                .or(external.poster_url.clone());
            show.backdrop_url = external.backdrop_url.clone();
            show.description = external
                .show_description
                .clone()
                .or(external.description.clone());
            show.genres = external.genres.clone();
            show.rating = external.rating;
        }

        // Create the season and add the episode
        let mut season = Season::new(season_num);
        season.add_episode(episode.clone());
        show.seasons.insert(season_num, season);
        show.total_episodes = 1;

        Some(show)
    }

    /// Add an episode to the show
    pub fn add_episode(&mut self, episode: MediaFile) -> bool {
        if let Some(parsed) = &episode
            .metadata
            .as_ref()
            .and_then(|m| m.parsed_info.as_ref())
        {
            if let Some(season_num) = parsed.season {
                // Update show metadata if this episode has better info
                if self.poster_url.is_none() {
                    if let Some(external) = &episode
                        .metadata
                        .as_ref()
                        .and_then(|m| m.external_info.as_ref())
                    {
                        self.poster_url = external
                            .show_poster_url
                            .clone()
                            .or(external.poster_url.clone());
                        if self.backdrop_url.is_none() {
                            self.backdrop_url = external.backdrop_url.clone();
                        }
                        if self.description.is_none() {
                            self.description = external.show_description.clone();
                        }
                    }
                }

                // Get or create season
                let season = self
                    .seasons
                    .entry(season_num)
                    .or_insert_with(|| Season::new(season_num));

                // Add episode to season
                if season.add_episode(episode) {
                    self.total_episodes += 1;
                    return true;
                }
            }
        }
        false
    }

    /// Get a sorted list of seasons
    pub fn sorted_seasons(&self) -> Vec<&Season> {
        let mut seasons: Vec<_> = self.seasons.values().collect();
        seasons.sort_by_key(|s| s.number);
        seasons
    }

    /// Get the latest season
    pub fn latest_season(&self) -> Option<&Season> {
        self.seasons.values().max_by_key(|s| s.number)
    }

    pub fn next_episode(&self) -> Option<&MediaFile> {
        self.seasons
            .values()
            .min_by_key(|s| s.number)
            .and_then(|s| s.first_episode())
    }

    /// Get the poster ID for the show (first episode with metadata)
    pub fn get_poster_id(&self) -> Option<String> {
        for season in self.sorted_seasons() {
            for episode in season.sorted_episodes() {
                return Some(episode.id.clone());
            }
        }
        None
    }

    /// Get total episodes count
    pub fn total_episodes(&self) -> usize {
        self.total_episodes
    }

    /// Check if this is a single season show
    pub fn is_single_season(&self) -> bool {
        self.seasons.len() == 1
    }
}

/// Represents a season of a TV show
#[derive(Debug, Clone)]
pub struct Season {
    pub number: u32,
    pub episodes: HashMap<u32, MediaFile>,
    pub poster_url: Option<String>,
    pub episode_count: usize,
}

impl Season {
    /// Create a new season
    pub fn new(number: u32) -> Self {
        Self {
            number,
            episodes: HashMap::new(),
            poster_url: None,
            episode_count: 0,
        }
    }

    /// Add an episode to the season
    pub fn add_episode(&mut self, episode: MediaFile) -> bool {
        if let Some(parsed) = &episode
            .metadata
            .as_ref()
            .and_then(|m| m.parsed_info.as_ref())
        {
            if let Some(episode_num) = parsed.episode {
                // Update season poster if available and not set
                if self.poster_url.is_none() {
                    if let Some(external) = &episode
                        .metadata
                        .as_ref()
                        .and_then(|m| m.external_info.as_ref())
                    {
                        self.poster_url = external.season_poster_url.clone();
                    }
                }

                self.episodes.insert(episode_num, episode);
                self.episode_count = self.episodes.len();
                return true;
            }
        }
        false
    }

    /// Get a sorted list of episodes
    pub fn sorted_episodes(&self) -> Vec<&MediaFile> {
        let mut episodes: Vec<_> = self.episodes.values().collect();
        episodes.sort_by_key(|e| {
            e.metadata
                .as_ref()
                .and_then(|m| m.parsed_info.as_ref())
                .and_then(|p| p.episode)
                .unwrap_or(0)
        });
        episodes
    }

    /// Get the first episode
    pub fn first_episode(&self) -> Option<&MediaFile> {
        self.episodes.values().min_by_key(|e| {
            e.metadata
                .as_ref()
                .and_then(|m| m.parsed_info.as_ref())
                .and_then(|p| p.episode)
                .unwrap_or(u32::MAX)
        })
    }

    /// Get display name for the season
    pub fn display_name(&self) -> String {
        if self.number == 0 {
            "Specials".to_string()
        } else {
            format!("Season {}", self.number)
        }
    }
}

/// Group media files into organized structures
pub struct MediaOrganizer;

impl MediaOrganizer {
    /// Organize only movie references
    pub fn organize_movie_references(references: &[MediaReference]) -> Vec<MovieReference> {
        let mut movies = Vec::new();
        log::info!(
            "MediaOrganizer: Processing {} references for movies only",
            references.len()
        );

        for reference in references {
            if let MediaReference::Movie(movie) = reference {
                movies.push(movie.clone());
            }
        }

        log::info!(
            "Found {} movies out of {} references",
            movies.len(),
            references.len()
        );
        movies
    }

    /// Organize only movie files (legacy)
    pub fn organize_movies(files: &[MediaFile]) -> Vec<MediaFile> {
        let mut movies = Vec::new();
        log::info!(
            "MediaOrganizer: Processing {} files for movies only",
            files.len()
        );

        for file in files {
            if let Some(metadata) = &file.metadata {
                if let Some(parsed) = &metadata.parsed_info {
                    // Only process movies
                    if parsed.media_type == "Movie" {
                        movies.push(file.clone());
                    }
                }
            }
        }

        log::info!("Found {} movies out of {} files", movies.len(), files.len());
        movies
    }

    /// Organize TV show references (series, seasons, episodes)
    pub fn organize_tv_show_references(references: &[MediaReference]) -> HashMap<String, TvShow> {
        let mut tv_shows: HashMap<String, TvShow> = HashMap::new();
        log::info!(
            "MediaOrganizer: Processing {} references for TV shows",
            references.len()
        );

        // First collect all references by type
        let (_, organized) = ReferenceOrganizer::organize_references(references.to_vec());

        // Convert to TvShow structures
        for (_series_id, (series_ref, seasons, episodes)) in organized {
            let show = TvShow::from_series_reference(&series_ref, seasons, episodes);
            tv_shows.insert(show.name.clone(), show);
        }

        log::info!(
            "Found {} TV shows out of {} references",
            tv_shows.len(),
            references.len()
        );
        tv_shows
    }

    /// Organize only TV show files (legacy)
    pub fn organize_tv_shows(files: &[MediaFile]) -> HashMap<String, TvShow> {
        let mut tv_shows: HashMap<String, TvShow> = HashMap::new();
        log::info!(
            "MediaOrganizer: Processing {} files for TV shows only",
            files.len()
        );

        for file in files {
            if let Some(metadata) = &file.metadata {
                if let Some(parsed) = &metadata.parsed_info {
                    // Only process TV episodes
                    if parsed.media_type == "TvEpisode" {
                        if let Some(show_name) = &parsed.show_name {
                            // Get or create TV show
                            if let Some(show) = tv_shows.get_mut(show_name) {
                                show.add_episode(file.clone());
                            } else if let Some(new_show) = TvShow::from_episode(file) {
                                tv_shows.insert(show_name.clone(), new_show);
                            }
                        } else {
                            log::warn!("TV episode without show name: {}", file.filename);
                        }
                    }
                }
            }
        }

        log::info!(
            "Found {} TV shows out of {} files",
            tv_shows.len(),
            files.len()
        );
        tv_shows
    }

    /// Organize media references into movies and TV shows (for All view)
    pub fn organize_media_references(
        references: &[MediaReference],
    ) -> (Vec<MovieReference>, HashMap<String, TvShow>) {
        let movies = Self::organize_movie_references(references);
        let tv_shows = Self::organize_tv_show_references(references);

        log::info!("MediaOrganizer summary:");
        log::info!("  Total references: {}", references.len());
        log::info!("  Movies found: {}", movies.len());
        log::info!("  TV shows found: {}", tv_shows.len());

        (movies, tv_shows)
    }

    /// Organize media files into movies and TV shows (for All view) - legacy
    pub fn organize_media(files: &[MediaFile]) -> (Vec<MediaFile>, HashMap<String, TvShow>) {
        let mut movies = Vec::new();
        let mut tv_shows: HashMap<String, TvShow> = HashMap::new();

        log::info!("MediaOrganizer: Processing {} files", files.len());

        // Debug: Count media types
        let mut type_counts: HashMap<String, usize> = HashMap::new();
        let mut no_metadata_count = 0;
        let mut no_parsed_info_count = 0;

        for file in files {
            // Debug: Log the raw JSON structure for first few files
            if files.len() < 10
                || file.filename.to_lowercase().contains("s0")
                || file.filename.to_lowercase().contains("episode")
            {
                log::debug!(
                    "File {}: metadata = {:?}",
                    file.filename,
                    serde_json::to_string(&file.metadata)
                        .unwrap_or_else(|_| "serialization failed".to_string())
                );
            }

            if let Some(metadata) = &file.metadata {
                if let Some(parsed) = &metadata.parsed_info {
                    // Count media types for debugging
                    *type_counts.entry(parsed.media_type.clone()).or_insert(0) += 1;

                    // Log the actual media type value for debugging (first 10 files)
                    if movies.len() + tv_shows.len() < 10 {
                        log::debug!(
                            "File '{}' has media_type: '{}' (raw)",
                            file.filename,
                            parsed.media_type
                        );
                    }

                    // MediaType enum gets serialized as PascalCase strings
                    // Match against the actual PascalCase values
                    match parsed.media_type.as_str() {
                        "Movie" => {
                            movies.push(file.clone());
                        }
                        "TvEpisode" => {
                            log::debug!(
                                "Found TV episode: {} - show_name: {:?}, S{:?}E{:?}",
                                file.filename,
                                parsed.show_name,
                                parsed.season,
                                parsed.episode
                            );

                            if let Some(show_name) = &parsed.show_name {
                                // Get or create TV show
                                if let Some(show) = tv_shows.get_mut(show_name) {
                                    show.add_episode(file.clone());
                                } else if let Some(new_show) = TvShow::from_episode(file) {
                                    tv_shows.insert(show_name.clone(), new_show);
                                }
                            } else {
                                log::warn!("TV episode without show name: {}", file.filename);
                            }
                        }
                        "Extra" => {
                            // Handle extras separately if needed
                            log::debug!("Found extra content: {}", file.filename);
                        }
                        "Unknown" => {
                            log::warn!("Unknown media type for file: {}", file.filename);
                        }
                        other => {
                            log::warn!(
                                "Unrecognized media type '{}' for file: {}",
                                other,
                                file.filename
                            );
                        }
                    }
                } else {
                    no_parsed_info_count += 1;
                    if no_parsed_info_count <= 5 {
                        log::warn!("File without parsed info: {}", file.filename);
                    }
                }
            } else {
                no_metadata_count += 1;
                if no_metadata_count <= 5 {
                    log::warn!("File without metadata: {}", file.filename);
                }
            }
        }

        // Log summary
        log::info!("MediaOrganizer summary:");
        log::info!("  Total files: {}", files.len());
        log::info!("  Files without metadata: {}", no_metadata_count);
        log::info!("  Files without parsed info: {}", no_parsed_info_count);
        log::info!("  Media type distribution: {:?}", type_counts);
        log::info!("  Movies found: {}", movies.len());
        log::info!("  TV shows found: {}", tv_shows.len());

        // Sort movies by title
        movies.sort_by(|a, b| {
            let title_a = a
                .metadata
                .as_ref()
                .and_then(|m| m.parsed_info.as_ref())
                .map(|p| &p.title)
                .unwrap_or(&a.filename);
            let title_b = b
                .metadata
                .as_ref()
                .and_then(|m| m.parsed_info.as_ref())
                .map(|p| &p.title)
                .unwrap_or(&b.filename);
            title_a.cmp(title_b)
        });

        (movies, tv_shows)
    }

    /// Get all TV shows sorted by name
    pub fn sorted_tv_shows(tv_shows: &HashMap<String, TvShow>) -> Vec<&TvShow> {
        let mut shows: Vec<_> = tv_shows.values().collect();
        shows.sort_by_key(|s| &s.name);
        shows
    }

    /// Find the best poster URL for a show (prioritize show poster over episode posters)
    pub fn get_show_poster_url(show: &TvShow) -> Option<String> {
        // First try show poster
        if let Some(url) = &show.poster_url {
            return Some(url.clone());
        }

        // Then try first season poster
        if let Some(season) = show.sorted_seasons().first() {
            if let Some(url) = &season.poster_url {
                return Some(url.clone());
            }

            // Finally try first episode poster
            if let Some(episode) = season.first_episode() {
                if let Some(external) = &episode.metadata.as_ref()?.external_info {
                    return external.poster_url.clone();
                }
            }
        }

        None
    }
}

/// Media collection statistics
#[derive(Debug, Default)]
pub struct MediaStats {
    pub movie_count: usize,
    pub show_count: usize,
    pub episode_count: usize,
    pub total_duration: f64,
    pub total_size: u64,
}

impl MediaStats {
    pub fn from_organized_media(movies: &[MediaFile], tv_shows: &HashMap<String, TvShow>) -> Self {
        let mut stats = Self::default();

        // Count movies and calculate stats
        stats.movie_count = movies.len();
        for movie in movies {
            if let Some(metadata) = &movie.metadata {
                if let Some(duration) = metadata.duration {
                    stats.total_duration += duration;
                }
                stats.total_size += metadata.file_size;
            }
        }

        // Count shows and episodes
        stats.show_count = tv_shows.len();
        for show in tv_shows.values() {
            stats.episode_count += show.total_episodes;

            for season in show.seasons.values() {
                for episode in season.episodes.values() {
                    if let Some(metadata) = &episode.metadata {
                        if let Some(duration) = metadata.duration {
                            stats.total_duration += duration;
                        }
                        stats.total_size += metadata.file_size;
                    }
                }
            }
        }

        stats
    }

    /// Format total duration as human readable string
    pub fn format_duration(&self) -> String {
        let total_hours = self.total_duration / 3600.0;
        let days = (total_hours / 24.0) as u32;
        let hours = (total_hours % 24.0) as u32;

        if days > 0 {
            format!("{} days, {} hours", days, hours)
        } else {
            format!("{} hours", hours)
        }
    }

    /// Format total size as human readable string
    pub fn format_size(&self) -> String {
        const TB: u64 = 1024 * 1024 * 1024 * 1024;
        const GB: u64 = 1024 * 1024 * 1024;
        const MB: u64 = 1024 * 1024;

        if self.total_size >= TB {
            format!("{:.2} TB", self.total_size as f64 / TB as f64)
        } else if self.total_size >= GB {
            format!("{:.2} GB", self.total_size as f64 / GB as f64)
        } else {
            format!("{:.2} MB", self.total_size as f64 / MB as f64)
        }
    }
}

// Re-export types from ferrex-core for backward compatibility
// Re-export types from ferrex-core for backward compatibility
pub use ferrex_core::media::SeasonDetails;

// Legacy types that are still used in the player
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TvShowDetails {
    pub name: String,
    pub tmdb_id: Option<u64>,
    pub poster_url: Option<String>,
    pub backdrop_url: Option<String>,
    pub description: Option<String>,
    pub seasons: Vec<SeasonSummary>,
    pub genres: Vec<String>,
    pub rating: Option<f32>,
    pub total_episodes: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeasonSummary {
    pub number: u32,
    pub episode_count: usize,
    pub poster_url: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpisodeSummary {
    pub id: String,
    pub filename: String,
    pub episode_number: u32,
    pub number: u32, // Alias for episode_number
    pub title: Option<String>,
    pub air_date: Option<chrono::NaiveDate>,
    pub duration: Option<f64>,
    pub still_url: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MovieDetails {
    pub id: String,
    pub title: String,
    pub year: Option<u16>,
    pub tmdb_id: Option<u32>,
    pub imdb_id: Option<String>,
    pub description: Option<String>,
    pub poster_url: Option<String>,
    pub backdrop_url: Option<String>,
    pub rating: Option<f32>,
    pub release_date: Option<String>,
    pub genres: Vec<String>,
    pub duration: Option<f64>,
    pub file_path: String,
    pub file_size: u64,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub poster_path: Option<String>,
    pub stream_url: Option<String>,
    pub transcode_url: Option<String>,
}

// ===== CONVERSION METHODS FOR NEW REFERENCE TYPES =====

use crate::api_types::{
    EpisodeReference, MediaDetailsOption, MediaReference, MovieReference, SeasonReference,
    SeriesReference, TmdbDetails,
};

impl TvShow {
    /// Create a TvShow from a SeriesReference and its seasons/episodes
    pub fn from_series_reference(
        series: &SeriesReference,
        seasons: Vec<SeasonReference>,
        episodes: HashMap<String, Vec<EpisodeReference>>, // key is season_id
    ) -> Self {
        let mut show = Self {
            name: series.title.as_str().to_string(),
            tmdb_id: Some(series.tmdb_id as u32),
            poster_url: None,
            backdrop_url: None,
            description: None,
            genres: Vec::new(),
            rating: None,
            seasons: HashMap::new(),
            total_episodes: 0,
        };

        // Extract series details if available
        if let MediaDetailsOption::Details(TmdbDetails::Series(details)) = &series.details {
            show.poster_url = details.poster_path.clone();
            show.backdrop_url = details.backdrop_path.clone();
            show.description = details.overview.clone();
            show.genres = details.genres.clone();
            show.rating = details.vote_average;
        }

        // Add seasons and episodes
        for season_ref in seasons {
            let mut season = Season::new(season_ref.season_number.value() as u32);

            // Extract season details if available
            if let MediaDetailsOption::Details(TmdbDetails::Season(details)) = &season_ref.details {
                season.poster_url = details.poster_path.clone();
            }

            // Add episodes for this season
            if let Some(season_episodes) = episodes.get(season_ref.id.as_str()) {
                for episode_ref in season_episodes {
                    // Convert episode reference to legacy MediaFile
                    let legacy_file = crate::api_types::episode_reference_to_legacy(
                        episode_ref,
                        match &series.details {
                            MediaDetailsOption::Details(TmdbDetails::Series(details)) => {
                                Some(details)
                            }
                            _ => None,
                        },
                    );
                    season.add_episode(legacy_file);
                }
            }

            show.total_episodes += season.episode_count;
            show.seasons.insert(season.number, season);
        }

        show
    }
}

impl MovieDetails {
    /// Create MovieDetails from a MovieReference
    pub fn from_movie_reference(movie: &MovieReference, server_url: &str) -> Self {
        let file = &movie.file;
        let metadata = file.media_file_metadata.as_ref();

        let mut details = Self {
            id: movie.id.as_str().to_string(),
            title: movie.title.as_str().to_string(),
            year: None,
            tmdb_id: Some(movie.tmdb_id as u32),
            imdb_id: None,
            description: None,
            poster_url: None,
            backdrop_url: None,
            rating: None,
            release_date: None,
            genres: Vec::new(),
            duration: metadata.and_then(|m| m.duration),
            file_path: file.path.to_string_lossy().to_string(),
            file_size: file.size,
            video_codec: metadata.and_then(|m| m.video_codec.clone()),
            audio_codec: metadata.and_then(|m| m.audio_codec.clone()),
            width: metadata.and_then(|m| m.width),
            height: metadata.and_then(|m| m.height),
            poster_path: None,
            stream_url: Some(format!("{}/stream/{}", server_url, movie.id.as_str())),
            transcode_url: Some(format!(
                "{}/stream/{}/transcode",
                server_url,
                movie.id.as_str()
            )),
        };

        // Extract movie details if available
        if let MediaDetailsOption::Details(TmdbDetails::Movie(movie_details)) = &movie.details {
            details.year = movie_details
                .release_date
                .as_ref()
                .and_then(|date| date.split('-').next())
                .and_then(|year| year.parse().ok());
            details.imdb_id = movie_details.external_ids.imdb_id.clone();
            details.description = movie_details.overview.clone();
            details.poster_url = movie_details.poster_path.clone();
            details.backdrop_url = movie_details.backdrop_path.clone();
            details.rating = movie_details.vote_average;
            details.release_date = movie_details.release_date.clone();
            details.genres = movie_details.genres.clone();
            details.poster_path = movie_details.poster_path.clone();
        }

        details
    }
}

/// Helper to organize MediaReferences into movies and TV shows
pub struct ReferenceOrganizer;

impl ReferenceOrganizer {
    /// Organize media references into movies and TV shows
    pub fn organize_references(
        references: Vec<MediaReference>,
    ) -> (
        Vec<MovieReference>,
        HashMap<
            String,
            (
                SeriesReference,
                Vec<SeasonReference>,
                HashMap<String, Vec<EpisodeReference>>,
            ),
        >,
    ) {
        let mut movies = Vec::new();
        let mut tv_shows: HashMap<
            String,
            (
                SeriesReference,
                Vec<SeasonReference>,
                HashMap<String, Vec<EpisodeReference>>,
            ),
        > = HashMap::new();

        for reference in references {
            match reference {
                MediaReference::Movie(movie) => {
                    movies.push(movie);
                }
                MediaReference::Series(series) => {
                    let series_id = series.id.as_str().to_string();
                    tv_shows.insert(series_id, (series, Vec::new(), HashMap::new()));
                }
                MediaReference::Season(season) => {
                    let series_id = season.series_id.as_str().to_string();
                    if let Some((_, seasons, _)) = tv_shows.get_mut(&series_id) {
                        seasons.push(season);
                    }
                }
                MediaReference::Episode(episode) => {
                    let series_id = episode.series_id.as_str().to_string();
                    let season_id = episode.season_id.as_str().to_string();
                    if let Some((_, _, episodes_map)) = tv_shows.get_mut(&series_id) {
                        episodes_map
                            .entry(season_id)
                            .or_insert_with(Vec::new)
                            .push(episode);
                    }
                }
            }
        }

        // Sort seasons by number
        for (_, (_, seasons, _)) in tv_shows.iter_mut() {
            seasons.sort_by_key(|s| s.season_number.value());
        }

        (movies, tv_shows)
    }
}
