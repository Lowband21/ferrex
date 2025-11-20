use std::collections::{HashMap, HashSet};

use crate::{
    common::messages::CrossDomainEvent,
    domains::{
        metadata::image_service::FirstDisplayHint, ui::types::ViewState,
    },
    infra::api_types::Media,
    state::State,
};

use ferrex_core::player_prelude::{
    ImageRequest, ImageSize, ImageType, LibraryID, MediaIDLike, MediaOps,
    Priority, SeasonID, SeriesID,
};

/// Result of applying media events to the repository
#[derive(Debug, Default)]
pub struct MediaEventApplyOutcome {
    pub touched_libraries: HashSet<LibraryID>,
    pub inline_additions: HashMap<LibraryID, Vec<Media>>, // for Movies/Series only
    pub affected_series: HashSet<SeriesID>,
    pub affected_seasons: HashSet<SeasonID>,
}

/// Apply discovered media (additions during scans). Centralizes repo upserts and
/// tracks which libraries and parent entities are affected.
pub fn apply_media_discovered(
    state: &mut State,
    references: Vec<Media>,
) -> MediaEventApplyOutcome {
    let mut outcome = MediaEventApplyOutcome::default();

    if references.is_empty() {
        return outcome;
    }

    for media in references {
        let Some(library_id) = media_library_id(&media) else {
            continue;
        };

        // Only inline Movies/Series into the library grid
        if matches!(media, Media::Movie(_) | Media::Series(_)) {
            outcome
                .inline_additions
                .entry(library_id)
                .or_default()
                .push(media.clone());

            // Revert to per-item image flip for Movies/Series while in Library view
            let hint =
                if matches!(state.domains.ui.state.view, ViewState::Library) {
                    FirstDisplayHint::FastThenSlow
                } else {
                    FirstDisplayHint::FlipOnce
                };

            if let Some(request) = image_request_for_media(&media) {
                state
                    .domains
                    .metadata
                    .state
                    .image_service
                    .flag_first_display_hint(&request, hint);
            }
        }

        let should_upsert = should_upsert_media(state, &media);
        if should_upsert {
            // Track parent relations only when applying
            match &media {
                Media::Season(season) => {
                    outcome.affected_series.insert(season.series_id);
                }
                Media::Episode(ep) => {
                    outcome.affected_seasons.insert(ep.season_id);
                }
                _ => {}
            }

            let media_uuid = media.media_id().to_uuid();
            match state
                .domains
                .library
                .state
                .repo_accessor
                .upsert(media, &library_id)
            {
                Ok(()) => {
                    outcome.touched_libraries.insert(library_id);
                }
                Err(err) => {
                    log::error!(
                        "Failed to upsert discovered media {} in library {}: {}",
                        media_uuid,
                        library_id,
                        err
                    );
                }
            }
        }
    }

    outcome
}

/// Apply a single updated media reference.
pub fn apply_media_updated(
    state: &mut State,
    media: Media,
) -> MediaEventApplyOutcome {
    let mut outcome = MediaEventApplyOutcome::default();

    let Some(library_id) = media_library_id(&media) else {
        return outcome;
    };

    let should_upsert = should_upsert_media(state, &media);
    if should_upsert {
        // Track parent relations for precise UI refreshes
        match &media {
            Media::Season(season) => {
                outcome.affected_series.insert(season.series_id);
            }
            Media::Episode(ep) => {
                outcome.affected_seasons.insert(ep.season_id);
            }
            _ => {}
        }

        let media_uuid = media.media_id().to_uuid();
        match state
            .domains
            .library
            .state
            .repo_accessor
            .upsert(media, &library_id)
        {
            Ok(()) => {
                outcome.touched_libraries.insert(library_id);
            }
            Err(err) => {
                log::error!(
                    "Failed to apply media update {} in library {}: {}",
                    media_uuid,
                    library_id,
                    err
                );
            }
        }
    }

    outcome
}

// --- helpers ---

fn media_library_id(media: &Media) -> Option<LibraryID> {
    match media {
        Media::Movie(movie) => Some(movie.library_id),
        Media::Series(series) => Some(series.library_id),
        Media::Season(season) => Some(season.library_id),
        Media::Episode(episode) => Some(episode.library_id),
    }
}

fn image_request_for_media(media: &Media) -> Option<ImageRequest> {
    match media {
        Media::Movie(movie) => Some(
            ImageRequest::new(
                movie.id.to_uuid(),
                ImageSize::Poster,
                ImageType::Movie,
            )
            .with_priority(Priority::Visible)
            .with_index(0),
        ),
        Media::Series(series) => Some(
            ImageRequest::new(
                series.id.to_uuid(),
                ImageSize::Poster,
                ImageType::Series,
            )
            .with_priority(Priority::Visible)
            .with_index(0),
        ),
        Media::Season(season) => Some(
            ImageRequest::new(
                season.id.to_uuid(),
                ImageSize::Poster,
                ImageType::Season,
            )
            .with_priority(Priority::Visible)
            .with_index(0),
        ),
        Media::Episode(episode) => Some(
            ImageRequest::new(
                *episode.id.as_uuid(),
                ImageSize::Thumbnail,
                ImageType::Episode,
            )
            .with_priority(Priority::Visible)
            .with_index(0),
        ),
    }
}

/// Build UI cross-domain events for affected series/seasons, coalesced.
pub fn build_children_changed_events(
    affected_series: &HashSet<SeriesID>,
    affected_seasons: &HashSet<SeasonID>,
) -> Vec<CrossDomainEvent> {
    let mut events =
        Vec::with_capacity(affected_series.len() + affected_seasons.len());
    for id in affected_series.iter().copied() {
        events.push(CrossDomainEvent::SeriesChildrenChanged(id));
    }
    for id in affected_seasons.iter().copied() {
        events.push(CrossDomainEvent::SeasonChildrenChanged(id));
    }
    events
}

fn should_upsert_media(state: &State, media: &Media) -> bool {
    match media {
        Media::Movie(_) | Media::Series(_) => true,
        Media::Season(season) => match &state.domains.ui.state.view {
            ViewState::SeriesDetail { series_id, .. } => {
                season.series_id == *series_id
            }
            ViewState::SeasonDetail { series_id, .. } => {
                season.series_id == *series_id
            }
            _ => false,
        },
        Media::Episode(ep) => match &state.domains.ui.state.view {
            ViewState::SeasonDetail { season_id, .. } => {
                ep.season_id == *season_id
            }
            ViewState::EpisodeDetail { episode_id, .. } => ep.id == *episode_id,
            _ => false,
        },
    }
}
