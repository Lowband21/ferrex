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
    /// Organize media files into movies and TV shows
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
                            log::warn!("Unrecognized media type '{}' for file: {}", other, file.filename);
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
                stats.total_duration += metadata.duration;
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
                        stats.total_duration += metadata.duration;
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

// API response structures - these match the server's responses
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
    pub id: String,
    pub number: u32,
    pub title: Option<String>,
    pub description: Option<String>,
    pub still_url: Option<String>,
    pub duration: Option<f64>,
    pub air_date: Option<String>,
}
