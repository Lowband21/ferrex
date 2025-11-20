//! Episode selection helpers for series and seasons
//!
//! Centralizes logic for choosing the next episode to play so views and
//! update handlers can stay simple and consistent.

use crate::{
    infra::repository::accessor::{Accessor, ReadOnly},
    state::State,
};

use ferrex_core::player_prelude::{EpisodeID, MediaIDLike, SeasonID, SeriesID};

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

/// Resolve an episode by id using the provided repository accessor.
fn resolve_episode_using_accessor(
    accessor: &Accessor<ReadOnly>,
    episode_id: &EpisodeID,
) -> Option<ferrex_core::player_prelude::EpisodeReference> {
    accessor.get(episode_id).ok().and_then(|m| match m {
        ferrex_core::player_prelude::Media::Episode(ep) => Some(ep),
        _ => None,
    })
}

/// Gather all episodes in a series in canonical order (season, episode)
/// using the provided repository accessor.
fn ordered_series_episodes(
    accessor: &Accessor<ReadOnly>,
    series_id: &SeriesID,
) -> Vec<ferrex_core::player_prelude::EpisodeReference> {
    let seasons = accessor.get_series_seasons(series_id).unwrap_or_default();

    let mut episodes: Vec<ferrex_core::player_prelude::EpisodeReference> =
        Vec::new();
    for season in &seasons {
        let mut eps =
            accessor.get_season_episodes(&season.id).unwrap_or_default();
        // Ensure ascending episode order within a season
        eps.sort_by_key(|e| e.episode_number.value());
        episodes.extend(eps);
    }

    // Defensive sort by (season, episode)
    episodes
        .sort_by_key(|e| (e.season_number.value(), e.episode_number.value()));
    episodes
}

/// Find the next episode strictly by ordering from the current episode (season, episode).
/// Returns None if the current episode is the last in the series or cannot be resolved.
pub fn next_episode_by_order(
    state: &State,
    current_episode_id: EpisodeID,
) -> Option<EpisodeID> {
    // Resolve the current episode reference to find its series and position
    let current = resolve_episode_using_accessor(
        &state.domains.ui.state.repo_accessor,
        &current_episode_id,
    )?;

    // Gather all episodes of the series sorted by (season, episode)
    let episodes = ordered_series_episodes(
        &state.domains.ui.state.repo_accessor,
        &current.series_id,
    );

    // Find current index and return the next
    if let Some(idx) = episodes.iter().position(|e| e.id == current.id) {
        if idx + 1 < episodes.len() {
            return Some(episodes[idx + 1].id);
        }
    }
    None
}

/// Find the previous episode strictly by ordering from the current episode (season, episode).
/// Returns None if the current episode is the first in the series or cannot be resolved.
pub fn previous_episode_by_order(
    state: &State,
    current_episode_id: EpisodeID,
) -> Option<EpisodeID> {
    // Resolve the current episode to get series and position
    let current = resolve_episode_using_accessor(
        &state.domains.ui.state.repo_accessor,
        &current_episode_id,
    )?;

    let episodes = ordered_series_episodes(
        &state.domains.ui.state.repo_accessor,
        &current.series_id,
    );

    if let Some(idx) = episodes.iter().position(|e| e.id == current.id) {
        if idx > 0 {
            return Some(episodes[idx - 1].id);
        }
    }
    None
}

/// Next episode by ordering using a repository accessor.
pub fn next_episode_by_order_with_repo(
    accessor: &Accessor<ReadOnly>,
    current_episode_id: EpisodeID,
) -> Option<EpisodeID> {
    // Resolve current episode
    let current =
        resolve_episode_using_accessor(accessor, &current_episode_id)?;

    let episodes = ordered_series_episodes(accessor, &current.series_id);

    if let Some(idx) = episodes.iter().position(|e| e.id == current.id) {
        if idx + 1 < episodes.len() {
            return Some(episodes[idx + 1].id);
        }
    }
    None
}

/// Previous episode by ordering using a repository accessor.
pub fn previous_episode_by_order_with_repo(
    accessor: &Accessor<ReadOnly>,
    current_episode_id: EpisodeID,
) -> Option<EpisodeID> {
    // Resolve current episode
    let current =
        resolve_episode_using_accessor(accessor, &current_episode_id)?;

    let episodes = ordered_series_episodes(accessor, &current.series_id);

    if let Some(idx) = episodes.iter().position(|e| e.id == current.id) {
        if idx > 0 {
            return Some(episodes[idx - 1].id);
        }
    }
    None
}
