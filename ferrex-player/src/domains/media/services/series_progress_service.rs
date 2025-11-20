//! Series progress tracking service
//!
//! This service handles determining which episode to play when a series is selected,
//! tracking overall series progress, and managing episode ordering.

use crate::domains::media::store::MediaStore;
use crate::infrastructure::api_types::{EpisodeReference, MediaId, MediaReference};
use ferrex_core::{watch_status::UserWatchState, SeriesID};
use std::sync::{Arc, RwLock as StdRwLock};

/// Service for managing series playback progress
pub struct SeriesProgressService {
    media_store: Arc<StdRwLock<MediaStore>>,
}

impl SeriesProgressService {
    /// Create a new series progress service
    pub fn new(media_store: Arc<StdRwLock<MediaStore>>) -> Self {
        Self { media_store }
    }

    /// Get the next episode to play for a series
    ///
    /// Returns the first in-progress episode, or the first unwatched episode,
    /// or None if all episodes are watched
    pub fn get_next_episode_for_series(
        &self,
        series_id: &SeriesID,
        watch_state: Option<&UserWatchState>,
    ) -> Option<(EpisodeReference, Option<f32>)> {
        let store = self.media_store.read().ok()?;

        // Get all seasons for the series
        let seasons = store.get_seasons(series_id.as_ref());
        if seasons.is_empty() {
            return None;
        }

        // Sort seasons by season number
        let mut sorted_seasons: Vec<_> = seasons.into_iter().collect();
        sorted_seasons.sort_by_key(|s| s.season_number.value());

        // Collect all episodes across all seasons with their ordering info
        let mut all_episodes: Vec<(EpisodeReference, u8, u8)> = Vec::new();
        for season in sorted_seasons {
            let episodes = store.get_episodes(season.id.as_ref());
            for episode in episodes {
                all_episodes.push((
                    episode.clone(),
                    season.season_number.value(),
                    episode.episode_number.value(),
                ));
            }
        }

        // Sort episodes by season and episode number
        all_episodes.sort_by_key(|(_, season_num, episode_num)| (*season_num, *episode_num));

        if let Some(watch_state) = watch_state {
            // First, look for in-progress episodes
            for (episode, _, _) in &all_episodes {
                let media_id = MediaId::Episode(episode.id.clone());
                if let Some(in_progress) = watch_state.get_by_media_id(&media_id) {
                    log::info!(
                        "Found in-progress episode: S{:02}E{:02} at {:.1}s/{:.1}s",
                        episode.season_number.value(),
                        episode.episode_number.value(),
                        in_progress.position,
                        in_progress.duration
                    );
                    return Some((episode.clone(), Some(in_progress.position)));
                }
            }

            // Next, find the first unwatched episode
            for (episode, _, _) in &all_episodes {
                let media_id = MediaId::Episode(episode.id.clone());
                if !watch_state.is_completed(&media_id) {
                    log::info!(
                        "Found unwatched episode: S{:02}E{:02}",
                        episode.season_number.value(),
                        episode.episode_number.value()
                    );
                    return Some((episode.clone(), None));
                }
            }
        } else {
            // No watch state available, return first episode
            if let Some((episode, _, _)) = all_episodes.first() {
                log::info!(
                    "No watch state available, returning first episode: S{:02}E{:02}",
                    episode.season_number.value(),
                    episode.episode_number.value()
                );
                return Some((episode.clone(), None));
            }
        }

        // All episodes are watched
        log::info!("All episodes in series {} are watched", series_id.as_str());
        None
    }

    /// Get the next episode after the given episode
    pub fn get_next_episode(
        &self,
        current_episode_id: &ferrex_core::EpisodeID,
    ) -> Option<EpisodeReference> {
        let store = self.media_store.read().ok()?;

        // Find the current episode to get its series and position
        let current_episode = store
            .get(&MediaId::Episode(current_episode_id.clone()))
            .and_then(|media| media.as_episode().cloned())?;

        // Get all episodes for the series
        let seasons = store.get_seasons(current_episode.series_id.as_ref());

        // Sort seasons and collect episodes
        let mut sorted_seasons: Vec<_> = seasons.into_iter().collect();
        sorted_seasons.sort_by_key(|s| s.season_number.value());

        let mut all_episodes: Vec<EpisodeReference> = Vec::new();
        for season in sorted_seasons {
            let mut episodes: Vec<EpisodeReference> = store
                .get_episodes(season.id.as_ref())
                .into_iter()
                .cloned()
                .collect();
            episodes.sort_by_key(|e| e.episode_number.value());
            all_episodes.extend(episodes);
        }

        // Find current episode index
        let current_index = all_episodes
            .iter()
            .position(|e| e.id == *current_episode_id)?;

        // Return next episode if it exists
        if current_index + 1 < all_episodes.len() {
            Some(all_episodes[current_index + 1].clone())
        } else {
            None
        }
    }

    /// Calculate the overall progress for a series
    /// Returns a percentage (0.0 to 1.0) of episodes watched
    pub fn get_series_progress(
        &self,
        series_id: &SeriesID,
        watch_state: Option<&UserWatchState>,
    ) -> f32 {
        let Some(watch_state) = watch_state else {
            return 0.0;
        };

        let Ok(store) = self.media_store.read() else {
            return 0.0;
        };

        // Count total and watched episodes
        let seasons = store.get_seasons(series_id.as_ref());
        let mut total_episodes = 0;
        let mut watched_episodes = 0;

        for season in seasons {
            let episodes = store.get_episodes(season.id.as_ref());
            total_episodes += episodes.len();

            for episode in episodes {
                let media_id = MediaId::Episode(episode.id.clone());
                if watch_state.is_completed(&media_id) {
                    watched_episodes += 1;
                }
            }
        }

        if total_episodes == 0 {
            0.0
        } else {
            (watched_episodes as f32) / (total_episodes as f32)
        }
    }

    /// Get the number of unwatched episodes in a series
    pub fn get_unwatched_count(
        &self,
        series_id: &SeriesID,
        watch_state: Option<&UserWatchState>,
    ) -> usize {
        let Some(watch_state) = watch_state else {
            // If no watch state, all episodes are unwatched
            let Ok(store) = self.media_store.read() else {
                return 0;
            };

            let seasons = store.get_seasons(series_id.as_ref());
            return seasons
                .iter()
                .map(|s| store.get_episodes(s.id.as_ref()).len())
                .sum();
        };

        let Ok(store) = self.media_store.read() else {
            return 0;
        };

        // Count unwatched episodes
        let seasons = store.get_seasons(series_id.as_ref());
        let mut unwatched_count = 0;

        for season in seasons {
            let episodes = store.get_episodes(season.id.as_ref());
            for episode in episodes {
                let media_id = MediaId::Episode(episode.id.clone());
                if !watch_state.is_completed(&media_id) {
                    unwatched_count += 1;
                }
            }
        }

        unwatched_count
    }
}
