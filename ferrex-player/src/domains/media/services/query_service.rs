use crate::domains::media::store::MediaStore;
use crate::infrastructure::api_types::{
    EpisodeReference, MovieReference, SeasonReference, SeriesReference,
};
use crate::infrastructure::api_types::{MediaId, MediaReference};
use ferrex_core::media::{EpisodeID, MovieID, SeasonID, SeriesID};
use std::sync::{Arc, RwLock as StdRwLock};
use uuid::Uuid;

/// Service for querying media data
/// This extracts business logic from UI views to maintain clean architecture
pub struct MediaQueryService {
    media_store: Arc<StdRwLock<MediaStore>>,
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::all_functions
)]
impl MediaQueryService {
    pub fn new(media_store: Arc<StdRwLock<MediaStore>>) -> Self {
        Self { media_store }
    }

    /// Check if the media store has any media
    pub fn has_any_media(&self) -> bool {
        if let Ok(store) = self.media_store.read() {
            !store.get_movies(None).is_empty() || !store.get_series(None).is_empty()
        } else {
            false
        }
    }

    /// Check if a specific library has media
    pub fn has_media_in_library(&self, library_id: Uuid) -> bool {
        if let Ok(store) = self.media_store.read() {
            !store.get_movies(Some(library_id)).is_empty()
                || !store.get_series(Some(library_id)).is_empty()
        } else {
            false
        }
    }

    /// Get a series by ID
    pub fn get_series(&self, series_id: &SeriesID) -> Option<SeriesReference> {
        if let Ok(store) = self.media_store.read() {
            if let Some(MediaReference::Series(series)) =
                store.get(&MediaId::Series(series_id.clone()))
            {
                Some(series.clone())
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Get a movie by ID
    pub fn get_movie(&self, movie_id: &MovieID) -> Option<MovieReference> {
        if let Ok(store) = self.media_store.read() {
            if let Some(MediaReference::Movie(movie)) = store.get(&MediaId::Movie(movie_id.clone()))
            {
                Some(movie.clone())
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Get a season by ID
    pub fn get_season(&self, season_id: &SeasonID) -> Option<SeasonReference> {
        if let Ok(store) = self.media_store.read() {
            if let Some(MediaReference::Season(season)) =
                store.get(&MediaId::Season(season_id.clone()))
            {
                Some(season.clone())
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Get an episode by ID
    pub fn get_episode(&self, episode_id: &EpisodeID) -> Option<EpisodeReference> {
        if let Ok(store) = self.media_store.read() {
            if let Some(MediaReference::Episode(episode)) =
                store.get(&MediaId::Episode(episode_id.clone()))
            {
                Some(episode.clone())
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Get title for a series (used in header view)
    pub fn get_series_title(&self, series_id: &SeriesID) -> String {
        self.get_series(series_id)
            .map(|s| s.title.as_str().to_string())
            .unwrap_or_else(|| "TV Show".to_string())
    }

    /// Get title for any media ID
    pub fn get_media_title(&self, media_id: &MediaId) -> Option<String> {
        if let Ok(store) = self.media_store.read() {
            store.get(media_id).map(|media_ref| match media_ref {
                MediaReference::Movie(m) => m.title.as_str().to_string(),
                MediaReference::Series(s) => s.title.as_str().to_string(),
                MediaReference::Season(s) => format!("Season {}", s.season_number),
                MediaReference::Episode(e) => format!("Episode {}", e.episode_number),
            })
        } else {
            None
        }
    }

    /// Get all seasons for a series
    pub fn get_seasons_for_series(&self, series_id: &SeriesID) -> Vec<SeasonReference> {
        if let Ok(store) = self.media_store.read() {
            store
                .get_seasons(series_id.as_str())
                .into_iter()
                .cloned()
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Get all episodes for a season
    pub fn get_episodes_for_season(&self, season_id: &SeasonID) -> Vec<EpisodeReference> {
        if let Ok(store) = self.media_store.read() {
            store
                .get_episodes(season_id.as_str())
                .into_iter()
                .cloned()
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Get all episodes for a series
    pub fn get_all_episodes_for_series(&self, series_id: &SeriesID) -> Vec<EpisodeReference> {
        // Get all seasons for this series, then get all episodes for each season
        let seasons = self.get_seasons_for_series(series_id);
        let mut all_episodes = Vec::new();

        for season in seasons {
            let episodes = self.get_episodes_for_season(&season.id);
            all_episodes.extend(episodes);
        }

        all_episodes
    }

    /// Get first episode of a series (for preview/continue watching)
    pub fn get_first_episode(&self, series_id: &SeriesID) -> Option<EpisodeReference> {
        self.get_all_episodes_for_series(series_id)
            .into_iter()
            .min_by_key(|e| (e.season_number, e.episode_number))
    }

    /// Get next unwatched episode for a series
    pub fn get_next_unwatched_episode(&self, series_id: &SeriesID) -> Option<EpisodeReference> {
        // TODO: This will need to check watch status once that's implemented
        // For now, just return the first episode
        self.get_first_episode(series_id)
    }

    /// Get media count for a library
    pub fn get_library_media_count(&self, library_id: Uuid) -> (usize, usize) {
        if let Ok(store) = self.media_store.read() {
            let movie_count = store.get_movies(Some(library_id)).len();
            let series_count = store.get_series(Some(library_id)).len();
            (movie_count, series_count)
        } else {
            (0, 0)
        }
    }

    /// Check if media exists
    pub fn media_exists(&self, media_id: &MediaId) -> bool {
        if let Ok(store) = self.media_store.read() {
            store.get(media_id).is_some()
        } else {
            false
        }
    }
}

impl std::fmt::Debug for MediaQueryService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MediaQueryService")
            .field("has_store", &true)
            .finish()
    }
}
