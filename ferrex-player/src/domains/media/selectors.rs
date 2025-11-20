//! Episode selection helpers for series and seasons
//!
//! Centralizes logic for choosing the next episode to play so views and
//! update handlers can stay simple and consistent.

use crate::state::State;
use crate::infra::repository::accessor::{Accessor, ReadOnly};
use ferrex_core::player_prelude::{
    EpisodeID, EpisodeLike, MediaIDLike, SeasonID, SeasonLike, SeriesID,
    SeriesLike,
};

/// For a series: choose the first in-progress episode, else the first
/// unwatched episode. If all are completed (or no watch state), fallback to
/// the very first episode (S01E01 in sorted order) if available.
pub fn select_next_episode_for_series(
    state: &State,
    series_id: SeriesID,
) -> Option<EpisodeID> {
    // Gather all episodes for the series in canonical order (season, episode)
    let seasons = state
        .domains
        .ui
        .state
        .repo_accessor
        .get_series_seasons(&series_id)
        .unwrap_or_default();

    // Early exit if no seasons
    if seasons.is_empty() {
        return None;
    }

    let mut episodes: Vec<ferrex_core::player_prelude::EpisodeReference> =
        Vec::new();
    for season in &seasons {
        let mut eps = state
            .domains
            .ui
            .state
            .repo_accessor
            .get_season_episodes(&season.id)
            .unwrap_or_default();
        // Ensure ascending episode order (repository already sorts, but be defensive)
        eps.sort_by_key(|e| e.episode_number.value());
        episodes.extend(eps);
    }

    // Defensive sort by (season, episode)
    episodes
        .sort_by_key(|e| (e.season_number.value(), e.episode_number.value()));

    // Nothing to play
    if episodes.is_empty() {
        return None;
    }

    let watch_state_opt = state.domains.media.state.get_watch_state();

    // 1) First in-progress episode in canonical order
    if let Some(watch_state) = watch_state_opt.as_ref() {
        if let Some(in_prog) = episodes
            .iter()
            .find(|e| watch_state.in_progress.contains_key(&e.id.to_uuid()))
        {
            return Some(in_prog.id);
        }
    }

    // 2) First unwatched (neither in_progress nor completed)
    if let Some(watch_state) = watch_state_opt.as_ref() {
        if let Some(unwatched) = episodes.iter().find(|e| {
            let id = e.id.to_uuid();
            !watch_state.in_progress.contains_key(&id)
                && !watch_state.completed.contains(&id)
        }) {
            return Some(unwatched.id);
        }
    } else {
        // No watch state loaded, treat first as unwatched
        return Some(episodes[0].id);
    }

    // 3) Fallback: all watched -> return the first episode of the series
    Some(episodes[0].id)
}

/// For a season: choose the first in-progress episode, else the first
/// unwatched episode. If all are completed (or no watch state), fallback to
/// the first episode in the season if available.
pub fn select_next_episode_for_season(
    state: &State,
    season_id: SeasonID,
) -> Option<EpisodeID> {
    // Load episodes for this season
    let mut episodes = state
        .domains
        .ui
        .state
        .repo_accessor
        .get_season_episodes(&season_id)
        .unwrap_or_default();

    // Ensure ascending order
    episodes.sort_by_key(|e| e.episode_number.value());

    if episodes.is_empty() {
        return None;
    }

    let watch_state_opt = state.domains.media.state.get_watch_state();

    // 1) First in-progress
    if let Some(watch_state) = watch_state_opt.as_ref() {
        if let Some(in_prog) = episodes
            .iter()
            .find(|e| watch_state.in_progress.contains_key(&e.id.to_uuid()))
        {
            return Some(in_prog.id);
        }
    }

    // 2) First unwatched
    if let Some(watch_state) = watch_state_opt.as_ref() {
        if let Some(unwatched) = episodes.iter().find(|e| {
            let id = e.id.to_uuid();
            !watch_state.in_progress.contains_key(&id)
                && !watch_state.completed.contains(&id)
        }) {
            return Some(unwatched.id);
        }
    } else {
        // No watch state -> treat first as unwatched
        return Some(episodes[0].id);
    }

    // 3) Fallback if all completed
    Some(episodes[0].id)
}
