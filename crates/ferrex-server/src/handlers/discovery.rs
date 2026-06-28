use std::collections::{HashMap, HashSet};

use axum::{
    Extension, Json,
    extract::{Path, Query, State},
};
use ferrex_core::{
    api::{ApiResponse, types::*},
    application::unit_of_work::AppUnitOfWork,
    domain::{
        users::user::User,
        watch::{
            ContinueWatchingActionHint, ItemWatchStatus,
            SeriesContinueWatchingItem,
        },
    },
    player_prelude::{
        MediaFilters, MediaQuery, MediaTypeFilter, MediaWithStatus, Pagination,
        SortBy, SortCriteria, SortOrder,
    },
};
use ferrex_model::{
    LibraryId, LibraryType, MediaID, MovieID, MovieReference, Series, SeriesID,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::infra::{app_state::AppState, demo_mode, errors::AppError};

const DEFAULT_DISCOVERY_SECTION_LIMIT: usize = 20;
const MAX_DISCOVERY_SECTION_LIMIT: usize = 40;
const MAX_EXPLORE_FETCH_LIMIT: usize = 100;

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct ExploreQueryParams {
    /// Optional per-section item limit. Values are clamped to a small shelf size.
    pub limit: Option<usize>,
}

impl ExploreQueryParams {
    fn section_limit(self) -> usize {
        discovery_section_limit(self.limit)
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct ResumeQueryParams {
    /// Optional resume item limit. Values are clamped like Explore shelves.
    pub limit: Option<usize>,
}

impl ResumeQueryParams {
    fn section_limit(self) -> usize {
        discovery_section_limit(self.limit)
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct LibraryDiscoveryQueryParams {
    /// Optional per-section item limit. Values are clamped like Explore shelves.
    pub limit: Option<usize>,
}

impl LibraryDiscoveryQueryParams {
    fn section_limit(self) -> usize {
        discovery_section_limit(self.limit)
    }
}

fn discovery_section_limit(limit: Option<usize>) -> usize {
    limit
        .unwrap_or(DEFAULT_DISCOVERY_SECTION_LIMIT)
        .clamp(1, MAX_DISCOVERY_SECTION_LIMIT)
}

/// Deterministic global Explore shelves.
pub async fn get_explore_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Query(params): Query<ExploreQueryParams>,
) -> Result<Json<ApiResponse<DiscoveryResponse>>, AppError> {
    let limit = params.section_limit();
    let uow = state.unit_of_work();
    let system_definitions = system_global_explore_collections(user.id);
    uow.collections
        .ensure_system_collections(&system_definitions)
        .await?;

    let recently_added = query_movie_shelf(
        uow.as_ref(),
        user.id,
        None,
        SortBy::DateAdded,
        limit,
        recently_added_reason,
    )
    .await?;

    let recently_released = query_movie_shelf(
        uow.as_ref(),
        user.id,
        None,
        SortBy::ReleaseDate,
        limit,
        recently_released_reason,
    )
    .await?;

    let audience_rating_picks = query_movie_shelf(
        uow.as_ref(),
        user.id,
        None,
        SortBy::Rating,
        limit,
        audience_rating_reason,
    )
    .await?;

    let sections = movie_discovery_sections(
        &system_definitions,
        recently_added,
        recently_released,
        audience_rating_picks,
        limit,
    );

    Ok(Json(ApiResponse::success(DiscoveryResponse::new(sections))))
}

/// Deterministic Resume shelf backed by the existing continue-watching read path.
pub async fn get_resume_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Query(params): Query<ResumeQueryParams>,
) -> Result<Json<ApiResponse<DiscoveryResponse>>, AppError> {
    let limit = params.section_limit();
    let uow = state.unit_of_work();
    let system_definition = system_resume_collection(user.id);
    uow.collections
        .ensure_system_collections(std::slice::from_ref(&system_definition))
        .await?;
    let continue_items = uow
        .watch_status
        .get_continue_watching(user.id, limit)
        .await?;
    let items = continue_items
        .iter()
        .map(discovery_item_from_continue_watching)
        .collect();

    let section = system_definition.section(items, limit);

    Ok(Json(ApiResponse::success(DiscoveryResponse::new(vec![
        section,
    ]))))
}

/// Deterministic shelves scoped to one enabled library.
pub async fn get_library_discovery_handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Path(library_uuid): Path<Uuid>,
    Query(params): Query<LibraryDiscoveryQueryParams>,
) -> Result<Json<ApiResponse<DiscoveryResponse>>, AppError> {
    let limit = params.section_limit();
    let library_id = LibraryId(library_uuid);

    if demo_mode::is_demo_mode(&state)
        && !demo_mode::is_demo_library(&library_id)
    {
        return Err(AppError::not_found("Library not found"));
    }

    let uow = state.unit_of_work();
    let Some(library) = uow.libraries.get_library(library_id).await? else {
        return Err(AppError::not_found("Library not found"));
    };

    if !library.enabled {
        return Err(AppError::not_found("Library not found"));
    }

    let system_definitions = system_library_discovery_collections(
        user.id,
        library_id,
        library.library_type,
    );
    uow.collections
        .ensure_system_collections(&system_definitions)
        .await?;

    let sections = match library.library_type {
        LibraryType::Movies => {
            query_movie_library_sections(
                uow.as_ref(),
                user.id,
                library_id,
                &system_definitions,
                limit,
            )
            .await?
        }
        LibraryType::Series => {
            query_series_library_sections(
                uow.as_ref(),
                user.id,
                library_id,
                &system_definitions,
                limit,
            )
            .await?
        }
    };

    Ok(Json(ApiResponse::success(DiscoveryResponse::new(sections))))
}

async fn query_movie_library_sections(
    uow: &AppUnitOfWork,
    user_id: Uuid,
    library_id: LibraryId,
    system_definitions: &[SystemCollectionDefinition],
    limit: usize,
) -> Result<Vec<DiscoverySection>, AppError> {
    let recently_added = query_movie_shelf(
        uow,
        user_id,
        Some(library_id),
        SortBy::DateAdded,
        limit,
        recently_added_reason,
    )
    .await?;

    let recently_released = query_movie_shelf(
        uow,
        user_id,
        Some(library_id),
        SortBy::ReleaseDate,
        limit,
        recently_released_reason,
    )
    .await?;

    let audience_rating_picks = query_movie_shelf(
        uow,
        user_id,
        Some(library_id),
        SortBy::Rating,
        limit,
        audience_rating_reason,
    )
    .await?;

    Ok(movie_discovery_sections(
        system_definitions,
        recently_added,
        recently_released,
        audience_rating_picks,
        limit,
    ))
}

async fn query_series_library_sections(
    uow: &AppUnitOfWork,
    user_id: Uuid,
    library_id: LibraryId,
    system_definitions: &[SystemCollectionDefinition],
    limit: usize,
) -> Result<Vec<DiscoverySection>, AppError> {
    let candidates =
        query_series_library_candidates(uow, user_id, library_id, limit)
            .await?;

    Ok(series_library_discovery_sections(
        &candidates,
        library_id,
        system_definitions,
        limit,
    ))
}

fn series_library_discovery_sections(
    candidates: &[SeriesDiscoveryCandidate],
    library_id: LibraryId,
    system_definitions: &[SystemCollectionDefinition],
    limit: usize,
) -> Vec<DiscoverySection> {
    let continue_series =
        continue_series_shelf_items(candidates, library_id, limit);
    let unwatched_series =
        unwatched_series_shelf_items(candidates, library_id, limit);
    let recently_added =
        recently_added_series_shelf_items(candidates, library_id, limit);
    let mut sections = Vec::new();

    for definition in system_definitions {
        let items = match definition.shelf {
            SystemDiscoveryShelf::ContinueSeries => continue_series.clone(),
            SystemDiscoveryShelf::UnwatchedSeries => unwatched_series.clone(),
            SystemDiscoveryShelf::RecentlyAddedSeries => recently_added.clone(),
            _ => Vec::new(),
        };
        push_section_if_not_empty(
            &mut sections,
            definition.section(items, limit),
        );
    }

    sections
}

fn movie_discovery_sections(
    system_definitions: &[SystemCollectionDefinition],
    recently_added: Vec<DiscoveryItem>,
    recently_released: Vec<DiscoveryItem>,
    audience_rating_picks: Vec<DiscoveryItem>,
    limit: usize,
) -> Vec<DiscoverySection> {
    let mut sections = Vec::new();
    for definition in system_definitions {
        let items = match definition.shelf {
            SystemDiscoveryShelf::RecentlyAddedMovies => recently_added.clone(),
            SystemDiscoveryShelf::RecentlyReleasedMovies => {
                recently_released.clone()
            }
            SystemDiscoveryShelf::AudienceRatingPicks => {
                audience_rating_picks.clone()
            }
            _ => Vec::new(),
        };
        push_section_if_not_empty(
            &mut sections,
            definition.section(items, limit),
        );
    }
    sections
}

async fn query_movie_shelf<F>(
    uow: &AppUnitOfWork,
    user_id: Uuid,
    library_id: Option<LibraryId>,
    sort_by: SortBy,
    limit: usize,
    reason_for: F,
) -> Result<Vec<DiscoveryItem>, AppError>
where
    F: Fn(&MovieReference) -> Option<String>,
{
    let query =
        movie_query(user_id, sort_by, expanded_fetch_limit(limit), library_id);
    let hits = uow.query.query_media(&query).await?;
    let movie_ids = movie_ids_from_hits(&hits);
    let movies = load_movie_references(uow, &movie_ids).await?;

    Ok(build_movie_discovery_items(
        hits, movies, limit, library_id, reason_for,
    ))
}

fn movie_query(
    user_id: Uuid,
    sort_by: SortBy,
    limit: usize,
    library_id: Option<LibraryId>,
) -> MediaQuery {
    MediaQuery {
        filters: MediaFilters {
            media_type: Some(MediaTypeFilter::Movie),
            library_ids: library_id
                .into_iter()
                .map(|id| id.to_uuid())
                .collect(),
            ..Default::default()
        },
        sort: SortCriteria {
            primary: sort_by,
            order: SortOrder::Descending,
            secondary: Some(SortBy::Title),
        },
        search: None,
        pagination: Pagination { offset: 0, limit },
        user_context: Some(user_id),
    }
}

fn expanded_fetch_limit(limit: usize) -> usize {
    limit
        .saturating_mul(4)
        .max(limit)
        .min(MAX_EXPLORE_FETCH_LIMIT)
}

fn movie_ids_from_hits(hits: &[MediaWithStatus]) -> Vec<MovieID> {
    hits.iter()
        .filter_map(|hit| match hit.id {
            MediaID::Movie(id) => Some(id),
            _ => None,
        })
        .collect()
}

async fn load_movie_references(
    uow: &AppUnitOfWork,
    movie_ids: &[MovieID],
) -> Result<Vec<MovieReference>, AppError> {
    let refs: Vec<&MovieID> = movie_ids.iter().collect();
    Ok(uow.media_refs.get_movie_references_bulk(&refs).await?)
}

fn build_movie_discovery_items<F>(
    hits: Vec<MediaWithStatus>,
    movies: Vec<MovieReference>,
    limit: usize,
    library_id: Option<LibraryId>,
    reason_for: F,
) -> Vec<DiscoveryItem>
where
    F: Fn(&MovieReference) -> Option<String>,
{
    let by_id: HashMap<MovieID, MovieReference> =
        movies.into_iter().map(|movie| (movie.id, movie)).collect();
    let mut items = Vec::with_capacity(limit.min(hits.len()));

    for hit in hits {
        if is_completed(hit.watch_status.as_ref()) {
            continue;
        }

        let MediaID::Movie(movie_id) = hit.id else {
            continue;
        };
        let Some(movie) = by_id.get(&movie_id) else {
            continue;
        };
        if library_id.is_some_and(|library_id| movie.library_id != library_id) {
            continue;
        }
        let Some(reason) = reason_for(movie) else {
            continue;
        };

        items.push(movie_to_discovery_item(
            movie,
            hit.watch_status.as_ref(),
            reason,
        ));

        if items.len() >= limit {
            break;
        }
    }

    items
}

/// Internal series discovery primitive used to keep library-series shelves
/// independent from raw hierarchical query rows.
#[derive(Debug, Clone, PartialEq)]
struct SeriesDiscoveryCandidate {
    series_id: SeriesID,
    library_id: LibraryId,
    title: String,
    poster_iid: Option<Uuid>,
    backdrop_iid: Option<Uuid>,
    release_date: Option<String>,
    release_year: Option<u16>,
    subtitle: Option<String>,
    season_count: Option<u16>,
    episode_count: Option<u16>,
    audience_rating: Option<f32>,
    watch: Option<DiscoveryWatchSummary>,
    watch_action_hint: Option<DiscoveryPlaybackHint>,
    playback: Option<SeriesDiscoveryPlaybackTarget>,
    has_meaningful_watch_state: bool,
    last_watch_activity_epoch_seconds: Option<i64>,
    discovered_at_epoch_millis: i64,
    display_reason: String,
}

impl SeriesDiscoveryCandidate {
    fn with_display_reason(mut self, reason: impl Into<String>) -> Self {
        self.display_reason = reason.into();
        self
    }

    fn with_meaningful_watch_state(mut self, value: bool) -> Self {
        self.has_meaningful_watch_state = value;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SeriesDiscoveryPlaybackTarget {
    episode_id: Uuid,
    hint: DiscoveryPlaybackHint,
}

#[derive(Debug, Clone, PartialEq)]
struct SeriesWatchContext {
    summary: DiscoveryWatchSummary,
    action_hint: DiscoveryPlaybackHint,
    subtitle: Option<String>,
    playback: Option<SeriesDiscoveryPlaybackTarget>,
    last_activity_epoch_seconds: i64,
}

async fn query_series_library_candidates(
    uow: &AppUnitOfWork,
    user_id: Uuid,
    library_id: LibraryId,
    continue_limit: usize,
) -> Result<Vec<SeriesDiscoveryCandidate>, AppError> {
    let series_ids = uow
        .media_refs
        .list_library_series_ids_with_episodes(&library_id)
        .await?;
    let refs: Vec<&SeriesID> = series_ids.iter().collect();
    let mut series = uow.media_refs.get_series_bulk(&refs).await?;

    // The ID list is already scoped by the repository, but keep a defensive
    // guard here so a library-scoped endpoint never leaks cross-library rows.
    series.retain(|series| series.library_id == library_id);
    if series.is_empty() {
        return Ok(Vec::new());
    }

    let continue_items = uow
        .watch_status
        .get_library_series_continue_watching(
            user_id,
            library_id,
            continue_limit,
        )
        .await?;
    let watch_by_series = series_watch_contexts_from_scoped_continue_watching(
        &continue_items,
        library_id,
    );
    let watched_series_ids: HashSet<SeriesID> = uow
        .watch_status
        .list_library_series_ids_with_meaningful_watch_state(
            user_id, library_id,
        )
        .await?
        .into_iter()
        .map(SeriesID)
        .collect();

    Ok(series
        .into_iter()
        .map(|series| {
            let has_meaningful_watch_state = watch_by_series
                .contains_key(&series.id)
                || watched_series_ids.contains(&series.id);

            series_discovery_candidate_from_series(
                &series,
                watch_by_series.get(&series.id),
                recently_added_series_reason(),
            )
            .with_meaningful_watch_state(has_meaningful_watch_state)
        })
        .collect())
}

fn series_discovery_candidate_from_series(
    series: &Series,
    watch_context: Option<&SeriesWatchContext>,
    display_reason: String,
) -> SeriesDiscoveryCandidate {
    let season_count = series
        .details
        .available_seasons
        .or(series.details.number_of_seasons)
        .filter(|count| *count > 0);
    let episode_count = series
        .details
        .available_episodes
        .or(series.details.number_of_episodes)
        .filter(|count| *count > 0);
    let release_date = series.details.first_air_date.clone();

    SeriesDiscoveryCandidate {
        series_id: series.id,
        library_id: series.library_id,
        title: series.title.as_ref().to_string(),
        poster_iid: series.details.primary_poster_iid,
        backdrop_iid: series.details.primary_backdrop_iid,
        release_year: release_date.as_deref().and_then(release_year_from_date),
        release_date,
        subtitle: watch_context
            .and_then(|context| context.subtitle.clone())
            .or_else(|| series_count_subtitle(season_count, episode_count)),
        season_count,
        episode_count,
        audience_rating: series
            .details
            .vote_average
            .filter(|rating| rating.is_finite()),
        watch: watch_context.map(|context| context.summary),
        watch_action_hint: watch_context.map(|context| context.action_hint),
        playback: watch_context.and_then(|context| context.playback),
        has_meaningful_watch_state: watch_context.is_some(),
        last_watch_activity_epoch_seconds: watch_context
            .map(|context| context.last_activity_epoch_seconds),
        discovered_at_epoch_millis: series.discovered_at.timestamp_millis(),
        display_reason,
    }
}

fn series_watch_contexts_from_scoped_continue_watching(
    continue_items: &[SeriesContinueWatchingItem],
    library_id: LibraryId,
) -> HashMap<SeriesID, SeriesWatchContext> {
    let mut contexts = HashMap::new();

    for item in continue_items {
        let Some((series_id, context)) =
            series_watch_context_from_scoped_continue_watching(
                item, library_id,
            )
        else {
            continue;
        };

        let should_insert = contexts.get(&series_id).is_none_or(|existing| {
            should_replace_series_watch_context(&context, existing)
        });

        if should_insert {
            contexts.insert(series_id, context);
        }
    }

    contexts
}

fn should_replace_series_watch_context(
    candidate: &SeriesWatchContext,
    existing: &SeriesWatchContext,
) -> bool {
    candidate.last_activity_epoch_seconds > existing.last_activity_epoch_seconds
        || (candidate.last_activity_epoch_seconds
            == existing.last_activity_epoch_seconds
            && candidate.playback.is_some()
            && existing.playback.is_none())
}

fn series_watch_context_from_scoped_continue_watching(
    item: &SeriesContinueWatchingItem,
    library_id: LibraryId,
) -> Option<(SeriesID, SeriesWatchContext)> {
    if item.library_id != library_id {
        return None;
    }

    let action_hint = discovery_hint_from_continue_action(item.action_hint);
    let playback = item.action_episode_id.map(|episode_id| {
        SeriesDiscoveryPlaybackTarget {
            episode_id,
            hint: action_hint,
        }
    });

    Some((
        SeriesID(item.series_id),
        SeriesWatchContext {
            summary: DiscoveryWatchSummary::from_series_continue_watching(item),
            action_hint,
            subtitle: item.subtitle.clone(),
            playback,
            last_activity_epoch_seconds: item.last_watched,
        },
    ))
}

fn discovery_hint_from_continue_action(
    action_hint: ContinueWatchingActionHint,
) -> DiscoveryPlaybackHint {
    match action_hint {
        ContinueWatchingActionHint::NextEpisode => {
            DiscoveryPlaybackHint::NextEpisode
        }
        ContinueWatchingActionHint::Resume => DiscoveryPlaybackHint::Resume,
    }
}

fn continue_series_shelf_items(
    candidates: &[SeriesDiscoveryCandidate],
    library_id: LibraryId,
    limit: usize,
) -> Vec<DiscoveryItem> {
    let mut candidates: Vec<SeriesDiscoveryCandidate> = candidates
        .iter()
        .filter(|candidate| {
            candidate.library_id == library_id
                && candidate.watch.is_some()
                && candidate.last_watch_activity_epoch_seconds.is_some()
        })
        .cloned()
        .map(|candidate| {
            let reason = continue_series_reason(&candidate);
            candidate.with_display_reason(reason)
        })
        .collect();

    sort_continue_series_candidates(&mut candidates);
    candidates
        .into_iter()
        .take(limit)
        .map(|candidate| series_candidate_to_discovery_item(&candidate))
        .collect()
}

fn unwatched_series_shelf_items(
    candidates: &[SeriesDiscoveryCandidate],
    library_id: LibraryId,
    limit: usize,
) -> Vec<DiscoveryItem> {
    let mut candidates: Vec<SeriesDiscoveryCandidate> = candidates
        .iter()
        .filter(|candidate| {
            candidate.library_id == library_id
                && !candidate.has_meaningful_watch_state
                && candidate.watch.is_none()
        })
        .cloned()
        .map(|candidate| {
            candidate.with_display_reason(unwatched_series_reason())
        })
        .collect();

    sort_unwatched_series_candidates(&mut candidates);
    candidates
        .into_iter()
        .take(limit)
        .map(|candidate| series_candidate_to_discovery_item(&candidate))
        .collect()
}

fn recently_added_series_shelf_items(
    candidates: &[SeriesDiscoveryCandidate],
    library_id: LibraryId,
    limit: usize,
) -> Vec<DiscoveryItem> {
    let mut candidates: Vec<SeriesDiscoveryCandidate> = candidates
        .iter()
        .filter(|candidate| candidate.library_id == library_id)
        .cloned()
        .map(|candidate| {
            candidate.with_display_reason(recently_added_series_reason())
        })
        .collect();

    sort_recently_added_series_candidates(&mut candidates);
    candidates
        .into_iter()
        .take(limit)
        .map(|candidate| series_candidate_to_discovery_item(&candidate))
        .collect()
}

fn sort_continue_series_candidates(
    candidates: &mut [SeriesDiscoveryCandidate],
) {
    candidates.sort_by(|a, b| {
        b.last_watch_activity_epoch_seconds
            .unwrap_or(i64::MIN)
            .cmp(&a.last_watch_activity_epoch_seconds.unwrap_or(i64::MIN))
            .then_with(|| a.title.cmp(&b.title))
            .then_with(|| a.series_id.to_uuid().cmp(&b.series_id.to_uuid()))
    });
}

fn sort_unwatched_series_candidates(
    candidates: &mut [SeriesDiscoveryCandidate],
) {
    candidates.sort_by(|a, b| {
        b.discovered_at_epoch_millis
            .cmp(&a.discovered_at_epoch_millis)
            .then_with(|| a.title.cmp(&b.title))
            .then_with(|| a.series_id.to_uuid().cmp(&b.series_id.to_uuid()))
    });
}

fn sort_recently_added_series_candidates(
    candidates: &mut [SeriesDiscoveryCandidate],
) {
    candidates.sort_by(|a, b| {
        b.discovered_at_epoch_millis
            .cmp(&a.discovered_at_epoch_millis)
            .then_with(|| a.title.cmp(&b.title))
            .then_with(|| a.series_id.to_uuid().cmp(&b.series_id.to_uuid()))
    });
}

fn series_candidate_to_discovery_item(
    candidate: &SeriesDiscoveryCandidate,
) -> DiscoveryItem {
    let media_id = MediaID::Series(candidate.series_id);
    let mut item = DiscoveryItem::new(media_id, candidate.title.clone());
    item.subtitle = candidate.subtitle.clone().or_else(|| {
        series_count_subtitle(candidate.season_count, candidate.episode_count)
    });
    item.poster_iid = candidate.poster_iid;
    item.backdrop_iid = candidate.backdrop_iid;
    item.release_date = candidate.release_date.clone();
    item.release_year = candidate.release_year;
    item.ratings = DiscoveryRatingSummary {
        audience: candidate.audience_rating,
        critic: None,
    };
    item.watch = candidate.watch;
    item.playback =
        candidate.playback.map(|playback| DiscoveryPlaybackAction {
            target_media_id: playback.episode_id,
            target_media_type: DiscoveryMediaType::Episode,
            hint: playback.hint,
        });
    item.reason = Some(candidate.display_reason.clone());
    item
}

fn series_count_subtitle(
    season_count: Option<u16>,
    episode_count: Option<u16>,
) -> Option<String> {
    let mut parts = Vec::new();

    if let Some(count) = season_count {
        parts.push(pluralized_count(count, "season", "seasons"));
    }
    if let Some(count) = episode_count {
        parts.push(pluralized_count(count, "episode", "episodes"));
    }

    (!parts.is_empty()).then(|| parts.join(" • "))
}

fn pluralized_count(count: u16, singular: &str, plural: &str) -> String {
    if count == 1 {
        format!("1 {singular}")
    } else {
        format!("{count} {plural}")
    }
}

fn movie_to_discovery_item(
    movie: &MovieReference,
    watch_status: Option<&ItemWatchStatus>,
    reason: String,
) -> DiscoveryItem {
    let media_id = MediaID::Movie(movie.id);
    let mut item =
        DiscoveryItem::new(media_id, movie.title.as_ref().to_string());
    item.poster_iid = movie.details.primary_poster_iid;
    item.backdrop_iid = movie.details.primary_backdrop_iid;
    item.release_date = movie.details.release_date.clone();
    item.release_year = item
        .release_date
        .as_deref()
        .and_then(release_year_from_date);
    item.runtime_minutes = movie.details.runtime;
    item.ratings = DiscoveryRatingSummary {
        audience: movie.details.vote_average,
        critic: None,
    };
    item.watch = watch_status.map(DiscoveryWatchSummary::from_item_status);
    item.playback = Some(DiscoveryPlaybackAction {
        target_media_id: movie.id.to_uuid(),
        target_media_type: DiscoveryMediaType::Movie,
        hint: DiscoveryPlaybackHint::Play,
    });
    item.reason = Some(reason);
    item
}

fn recently_added_reason(_movie: &MovieReference) -> Option<String> {
    Some("Recently added to your library".to_string())
}

fn continue_series_reason(candidate: &SeriesDiscoveryCandidate) -> String {
    match candidate.watch_action_hint {
        Some(DiscoveryPlaybackHint::Resume) => "Ready to resume".to_string(),
        Some(DiscoveryPlaybackHint::NextEpisode) => {
            "Next episode available".to_string()
        }
        Some(DiscoveryPlaybackHint::Play) | None => {
            "Continue this series".to_string()
        }
    }
}

fn recently_added_series_reason() -> String {
    "Recently added to this library".to_string()
}

fn unwatched_series_reason() -> String {
    "New to you".to_string()
}

fn recently_released_reason(movie: &MovieReference) -> Option<String> {
    let release_date = movie.details.release_date.as_deref()?;
    let reason = release_year_from_date(release_date).map_or_else(
        || "Recently released".to_string(),
        |year| format!("Released in {year}"),
    );

    Some(reason)
}

fn audience_rating_reason(movie: &MovieReference) -> Option<String> {
    let rating = movie
        .details
        .vote_average
        .filter(|rating| rating.is_finite())?;
    Some(format!("{rating:.1}/10 audience rating"))
}

fn is_completed(status: Option<&ItemWatchStatus>) -> bool {
    matches!(status, Some(ItemWatchStatus::Completed(_)))
}

fn push_section_if_not_empty(
    sections: &mut Vec<DiscoverySection>,
    section: DiscoverySection,
) {
    if !section.is_empty() {
        sections.push(section);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use chrono::{TimeZone, Utc};
    use ferrex_core::domain::watch::{CompletedItem, InProgressItem};
    use ferrex_model::{
        EnhancedSeriesDetails, details::ExternalIds, image::MediaImages,
        urls::SeriesURL,
    };

    fn test_series(
        id: SeriesID,
        library_id: LibraryId,
        title: &str,
        discovered_at_millis: i64,
        season_count: Option<u16>,
        episode_count: Option<u16>,
    ) -> Series {
        Series {
            id,
            library_id,
            tmdb_id: id.to_uuid().as_u128() as u64,
            title: title.into(),
            details: test_series_details(title, season_count, episode_count),
            endpoint: SeriesURL::from_string(format!("/series/{id}")),
            discovered_at: Utc
                .timestamp_millis_opt(discovered_at_millis)
                .single()
                .expect("valid test timestamp"),
            created_at: Utc
                .timestamp_millis_opt(discovered_at_millis)
                .single()
                .expect("valid test timestamp"),
            theme_color: None,
        }
    }

    fn test_series_details(
        title: &str,
        season_count: Option<u16>,
        episode_count: Option<u16>,
    ) -> EnhancedSeriesDetails {
        EnhancedSeriesDetails {
            id: 42,
            name: title.to_string(),
            original_name: None,
            overview: Some(format!("About {title}")),
            first_air_date: Some("2022-03-04".to_string()),
            last_air_date: None,
            number_of_seasons: season_count,
            number_of_episodes: episode_count,
            available_seasons: season_count,
            available_episodes: episode_count,
            vote_average: Some(8.25),
            vote_count: Some(100),
            popularity: Some(10.0),
            content_rating: Some("TV-14".to_string()),
            content_ratings: Vec::new(),
            release_dates: Vec::new(),
            genres: Vec::new(),
            networks: Vec::new(),
            origin_countries: Vec::new(),
            spoken_languages: Vec::new(),
            production_companies: Vec::new(),
            production_countries: Vec::new(),
            homepage: None,
            status: None,
            tagline: None,
            in_production: None,
            poster_path: None,
            backdrop_path: None,
            logo_path: None,
            primary_poster_iid: Some(Uuid::from_u128(900)),
            primary_backdrop_iid: Some(Uuid::from_u128(901)),
            images: MediaImages::default(),
            cast: Vec::new(),
            crew: Vec::new(),
            videos: Vec::new(),
            keywords: Vec::new(),
            external_ids: ExternalIds::default(),
            alternative_titles: Vec::new(),
            translations: Vec::new(),
            episode_groups: Vec::new(),
            recommendations: Vec::new(),
            similar: Vec::new(),
        }
    }

    fn watch_context(
        library_id: LibraryId,
        series_id: SeriesID,
        episode_id: Uuid,
        last_watched: i64,
        action_hint: ContinueWatchingActionHint,
    ) -> SeriesWatchContext {
        let position = match action_hint {
            ContinueWatchingActionHint::Resume => 120.0,
            ContinueWatchingActionHint::NextEpisode => 0.0,
        };
        let duration = match action_hint {
            ContinueWatchingActionHint::Resume => 1_200.0,
            ContinueWatchingActionHint::NextEpisode => 0.0,
        };
        let subtitle = match action_hint {
            ContinueWatchingActionHint::Resume => "Resume S01E02",
            ContinueWatchingActionHint::NextEpisode => "Next up: S01E03",
        };
        let item = SeriesContinueWatchingItem {
            series_id: series_id.to_uuid(),
            library_id,
            action_episode_id: Some(episode_id),
            action_hint,
            position,
            duration,
            last_watched,
            title: Some("Series card".to_string()),
            subtitle: Some(subtitle.to_string()),
            poster_iid: Some(Uuid::from_u128(902)),
        };

        let (mapped_series_id, context) =
            series_watch_context_from_scoped_continue_watching(
                &item, library_id,
            )
            .expect("series continue item maps to context");
        assert_eq!(mapped_series_id, series_id);
        context
    }

    fn candidate(
        id: SeriesID,
        library_id: LibraryId,
        title: &str,
        discovered_at_millis: i64,
        context: Option<SeriesWatchContext>,
    ) -> SeriesDiscoveryCandidate {
        let series = test_series(
            id,
            library_id,
            title,
            discovered_at_millis,
            Some(2),
            Some(12),
        );
        series_discovery_candidate_from_series(
            &series,
            context.as_ref(),
            "Base reason".to_string(),
        )
    }

    #[test]
    fn discovery_section_limit_clamps_query_param() {
        assert_eq!(discovery_section_limit(None), 20);
        assert_eq!(discovery_section_limit(Some(0)), 1);
        assert_eq!(discovery_section_limit(Some(1)), 1);
        assert_eq!(discovery_section_limit(Some(40)), 40);
        assert_eq!(discovery_section_limit(Some(41)), 40);
    }

    #[test]
    fn expanded_fetch_limit_is_bounded() {
        assert_eq!(expanded_fetch_limit(1), 4);
        assert_eq!(expanded_fetch_limit(20), 80);
        assert_eq!(expanded_fetch_limit(40), 100);
        assert_eq!(expanded_fetch_limit(usize::MAX), 100);
    }

    #[test]
    fn movie_query_uses_user_context_and_bounded_limit() {
        let user_id = Uuid::from_u128(99);
        let query = movie_query(user_id, SortBy::Rating, 16, None);

        assert_eq!(query.user_context, Some(user_id));
        assert_eq!(query.pagination.limit, 16);
        assert_eq!(query.pagination.offset, 0);
        assert_eq!(query.filters.media_type, Some(MediaTypeFilter::Movie));
        assert!(query.filters.library_ids.is_empty());
        assert_eq!(query.sort.primary, SortBy::Rating);
        assert_eq!(query.sort.order, SortOrder::Descending);
        assert_eq!(query.sort.secondary, Some(SortBy::Title));
    }

    #[test]
    fn movie_query_adds_library_scope_when_present() {
        let user_id = Uuid::from_u128(1);
        let library_id = LibraryId(Uuid::from_u128(2));

        let query =
            movie_query(user_id, SortBy::DateAdded, 12, Some(library_id));

        assert_eq!(query.filters.media_type, Some(MediaTypeFilter::Movie));
        assert_eq!(query.filters.library_ids, vec![library_id.to_uuid()]);
        assert_eq!(query.sort.primary, SortBy::DateAdded);
        assert_eq!(query.sort.order, SortOrder::Descending);
        assert_eq!(query.sort.secondary, Some(SortBy::Title));
        assert_eq!(query.pagination.offset, 0);
        assert_eq!(query.pagination.limit, 12);
        assert_eq!(query.user_context, Some(user_id));
    }

    #[test]
    fn movie_sections_follow_system_definition_metadata_and_limits() {
        let user_id = Uuid::from_u128(1_000);
        let added_id = MovieID(Uuid::from_u128(1_001));
        let released_id = MovieID(Uuid::from_u128(1_002));
        let rating_id = MovieID(Uuid::from_u128(1_003));
        let definitions = system_global_explore_collections(user_id);

        let sections = movie_discovery_sections(
            &definitions,
            vec![
                DiscoveryItem::new(MediaID::Movie(added_id), "Added first"),
                DiscoveryItem::new(
                    MediaID::Movie(MovieID(Uuid::from_u128(1_004))),
                    "Added second",
                ),
            ],
            vec![DiscoveryItem::new(
                MediaID::Movie(released_id),
                "Released first",
            )],
            vec![DiscoveryItem::new(MediaID::Movie(rating_id), "Rated first")],
            1,
        );

        assert_eq!(
            sections
                .iter()
                .map(|section| section.id.as_str())
                .collect::<Vec<_>>(),
            vec![
                DISCOVERY_SECTION_RECENTLY_ADDED,
                DISCOVERY_SECTION_RECENTLY_RELEASED,
                DISCOVERY_SECTION_AUDIENCE_RATING_PICKS,
            ]
        );
        assert_eq!(
            sections
                .iter()
                .map(|section| section.title.as_str())
                .collect::<Vec<_>>(),
            vec![
                "Recently added",
                "Recently released",
                "Audience rating picks",
            ]
        );
        assert_eq!(
            sections
                .iter()
                .map(|section| section.items.len())
                .collect::<Vec<_>>(),
            vec![1, 1, 1]
        );
        assert_eq!(sections[0].items[0].media_id, MediaID::Movie(added_id));
        assert_eq!(sections[1].items[0].media_id, MediaID::Movie(released_id));
        assert_eq!(sections[2].items[0].media_id, MediaID::Movie(rating_id));
        assert!(sections.iter().all(|section| {
            section.layout_hint == DiscoveryLayoutHint::PosterRow
        }));
    }

    #[test]
    fn scoped_continue_context_mapping_filters_cross_library_rows() {
        let library_id = LibraryId(Uuid::from_u128(70));
        let other_library_id = LibraryId(Uuid::from_u128(71));
        let in_library_id = SeriesID(Uuid::from_u128(72));
        let off_library_id = SeriesID(Uuid::from_u128(73));
        let episode_id = Uuid::from_u128(74);
        let items = vec![
            SeriesContinueWatchingItem {
                series_id: off_library_id.to_uuid(),
                library_id: other_library_id,
                action_episode_id: Some(Uuid::from_u128(75)),
                action_hint: ContinueWatchingActionHint::Resume,
                position: 180.0,
                duration: 1_800.0,
                last_watched: 2_000,
                title: Some("Off Library".to_string()),
                subtitle: Some("Resume S01E01".to_string()),
                poster_iid: None,
            },
            SeriesContinueWatchingItem {
                series_id: in_library_id.to_uuid(),
                library_id,
                action_episode_id: Some(episode_id),
                action_hint: ContinueWatchingActionHint::NextEpisode,
                position: 0.0,
                duration: 0.0,
                last_watched: 1_000,
                title: Some("In Library".to_string()),
                subtitle: Some("Next up: S01E02".to_string()),
                poster_iid: None,
            },
        ];

        let contexts = series_watch_contexts_from_scoped_continue_watching(
            &items, library_id,
        );

        assert!(!contexts.contains_key(&off_library_id));
        let context = contexts
            .get(&in_library_id)
            .expect("in-library continue row should map");
        assert_eq!(context.action_hint, DiscoveryPlaybackHint::NextEpisode);
        assert_eq!(context.subtitle.as_deref(), Some("Next up: S01E02"));
        assert_eq!(
            context.playback,
            Some(SeriesDiscoveryPlaybackTarget {
                episode_id,
                hint: DiscoveryPlaybackHint::NextEpisode,
            })
        );
        assert_eq!(
            context.summary,
            DiscoveryWatchSummary {
                state: DiscoveryWatchState::Unwatched,
                progress: None,
                position_seconds: Some(0.0),
                duration_seconds: Some(0.0),
                last_watched_epoch_seconds: Some(1_000),
            }
        );
    }

    #[test]
    fn series_candidate_carries_metadata_counts_and_recent_item_mapping() {
        let library_id = LibraryId(Uuid::from_u128(10));
        let series_id = SeriesID(Uuid::from_u128(11));
        let series = test_series(
            series_id,
            library_id,
            "A Show",
            123,
            Some(2),
            Some(12),
        );

        let candidate = series_discovery_candidate_from_series(
            &series,
            None,
            recently_added_series_reason(),
        );
        let item = series_candidate_to_discovery_item(&candidate);

        assert_eq!(candidate.series_id, series_id);
        assert_eq!(candidate.library_id, library_id);
        assert_eq!(candidate.title, "A Show");
        assert_eq!(candidate.release_date.as_deref(), Some("2022-03-04"));
        assert_eq!(candidate.release_year, Some(2022));
        assert_eq!(candidate.season_count, Some(2));
        assert_eq!(candidate.episode_count, Some(12));
        assert_eq!(candidate.audience_rating, Some(8.25));
        assert_eq!(candidate.watch, None);
        assert_eq!(candidate.playback, None);
        assert!(!candidate.has_meaningful_watch_state);

        assert_eq!(item.media_id, MediaID::Series(series_id));
        assert_eq!(item.media_type, DiscoveryMediaType::Series);
        assert_eq!(item.title, "A Show");
        assert_eq!(item.subtitle.as_deref(), Some("2 seasons • 12 episodes"));
        assert_eq!(item.release_year, Some(2022));
        assert_eq!(item.ratings.audience, Some(8.25));
        assert_eq!(
            item.reason.as_deref(),
            Some("Recently added to this library")
        );
    }

    #[test]
    fn continue_series_shelf_filters_orders_and_targets_episode() {
        let library_id = LibraryId(Uuid::from_u128(20));
        let other_library_id = LibraryId(Uuid::from_u128(21));
        let alpha_low_id = SeriesID(Uuid::from_u128(22));
        let alpha_high_id = SeriesID(Uuid::from_u128(23));
        let beta_id = SeriesID(Uuid::from_u128(24));
        let outside_id = SeriesID(Uuid::from_u128(25));
        let no_watch_id = SeriesID(Uuid::from_u128(26));
        let alpha_low_episode = Uuid::from_u128(120);

        let candidates = vec![
            candidate(
                beta_id,
                library_id,
                "Beta",
                100,
                Some(watch_context(
                    library_id,
                    beta_id,
                    Uuid::from_u128(121),
                    100,
                    ContinueWatchingActionHint::Resume,
                )),
            ),
            candidate(
                alpha_high_id,
                library_id,
                "Alpha",
                300,
                Some(watch_context(
                    library_id,
                    alpha_high_id,
                    Uuid::from_u128(122),
                    200,
                    ContinueWatchingActionHint::NextEpisode,
                )),
            ),
            candidate(
                alpha_low_id,
                library_id,
                "Alpha",
                200,
                Some(watch_context(
                    library_id,
                    alpha_low_id,
                    alpha_low_episode,
                    200,
                    ContinueWatchingActionHint::Resume,
                )),
            ),
            candidate(
                outside_id,
                other_library_id,
                "Outside",
                500,
                Some(watch_context(
                    other_library_id,
                    outside_id,
                    Uuid::from_u128(123),
                    500,
                    ContinueWatchingActionHint::Resume,
                )),
            ),
            candidate(no_watch_id, library_id, "Fresh", 400, None),
        ];

        let items = continue_series_shelf_items(&candidates, library_id, 10);

        assert_eq!(
            items.iter().map(|item| item.media_id).collect::<Vec<_>>(),
            vec![
                MediaID::Series(alpha_low_id),
                MediaID::Series(alpha_high_id),
                MediaID::Series(beta_id),
            ]
        );
        assert_eq!(
            items[0].playback,
            Some(DiscoveryPlaybackAction {
                target_media_id: alpha_low_episode,
                target_media_type: DiscoveryMediaType::Episode,
                hint: DiscoveryPlaybackHint::Resume,
            })
        );
        assert_eq!(items[0].reason.as_deref(), Some("Ready to resume"));
        assert_eq!(items[1].reason.as_deref(), Some("Next episode available"));
    }

    #[test]
    fn unwatched_series_shelf_filters_and_orders() {
        let library_id = LibraryId(Uuid::from_u128(60));
        let other_library_id = LibraryId(Uuid::from_u128(61));
        let alpha_low_id = SeriesID(Uuid::from_u128(62));
        let alpha_high_id = SeriesID(Uuid::from_u128(63));
        let beta_id = SeriesID(Uuid::from_u128(64));
        let watched_id = SeriesID(Uuid::from_u128(65));
        let completed_no_next_id = SeriesID(Uuid::from_u128(66));
        let outside_id = SeriesID(Uuid::from_u128(67));
        let older_id = SeriesID(Uuid::from_u128(68));

        let completed_no_next =
            candidate(completed_no_next_id, library_id, "Completed", 300, None)
                .with_meaningful_watch_state(true);

        let candidates = vec![
            candidate(older_id, library_id, "Older", 100, None),
            candidate(beta_id, library_id, "Beta", 300, None),
            candidate(alpha_high_id, library_id, "Alpha", 300, None),
            candidate(alpha_low_id, library_id, "Alpha", 300, None),
            candidate(
                watched_id,
                library_id,
                "Watched",
                400,
                Some(watch_context(
                    library_id,
                    watched_id,
                    Uuid::from_u128(160),
                    400,
                    ContinueWatchingActionHint::Resume,
                )),
            ),
            completed_no_next,
            candidate(outside_id, other_library_id, "Outside", 500, None),
        ];

        let items = unwatched_series_shelf_items(&candidates, library_id, 10);

        assert_eq!(
            items.iter().map(|item| item.media_id).collect::<Vec<_>>(),
            vec![
                MediaID::Series(alpha_low_id),
                MediaID::Series(alpha_high_id),
                MediaID::Series(beta_id),
                MediaID::Series(older_id),
            ]
        );
        assert!(
            items
                .iter()
                .all(|item| item.media_type == DiscoveryMediaType::Series)
        );
        assert!(items.iter().all(|item| item.watch.is_none()));
        assert!(items.iter().all(|item| item.playback.is_none()));
        assert!(
            items
                .iter()
                .all(|item| item.reason.as_deref() == Some("New to you"))
        );
        assert!(
            items.iter().all(|item| item.subtitle.as_deref()
                == Some("2 seasons • 12 episodes"))
        );
    }

    #[test]
    fn recently_added_series_shelf_orders_by_discovery_title_and_id() {
        let library_id = LibraryId(Uuid::from_u128(80));
        let other_library_id = LibraryId(Uuid::from_u128(81));
        let alpha_low_id = SeriesID(Uuid::from_u128(82));
        let alpha_high_id = SeriesID(Uuid::from_u128(83));
        let beta_id = SeriesID(Uuid::from_u128(84));
        let older_id = SeriesID(Uuid::from_u128(85));
        let outside_id = SeriesID(Uuid::from_u128(86));
        let candidates = vec![
            candidate(older_id, library_id, "Older", 100, None),
            candidate(beta_id, library_id, "Beta", 300, None),
            candidate(alpha_high_id, library_id, "Alpha", 300, None),
            candidate(alpha_low_id, library_id, "Alpha", 300, None),
            candidate(outside_id, other_library_id, "Outside", 900, None),
        ];

        let items =
            recently_added_series_shelf_items(&candidates, library_id, 10);

        assert_eq!(
            items.iter().map(|item| item.media_id).collect::<Vec<_>>(),
            vec![
                MediaID::Series(alpha_low_id),
                MediaID::Series(alpha_high_id),
                MediaID::Series(beta_id),
                MediaID::Series(older_id),
            ]
        );
        assert!(items.iter().all(|item| item.reason.as_deref()
            == Some("Recently added to this library")));
    }

    #[test]
    fn series_library_sections_emit_non_empty_rows_in_stable_order() {
        let library_id = LibraryId(Uuid::from_u128(90));
        let continue_id = SeriesID(Uuid::from_u128(91));
        let unwatched_id = SeriesID(Uuid::from_u128(92));
        let candidates = vec![
            candidate(
                continue_id,
                library_id,
                "Continue Show",
                200,
                Some(watch_context(
                    library_id,
                    continue_id,
                    Uuid::from_u128(190),
                    200,
                    ContinueWatchingActionHint::NextEpisode,
                )),
            ),
            candidate(unwatched_id, library_id, "Fresh Show", 100, None),
        ];

        let definitions = system_library_discovery_collections(
            Uuid::from_u128(900),
            library_id,
            LibraryType::Series,
        );
        let sections = series_library_discovery_sections(
            &candidates,
            library_id,
            &definitions,
            10,
        );

        assert_eq!(
            sections
                .iter()
                .map(|section| section.id.as_str())
                .collect::<Vec<_>>(),
            vec![
                DISCOVERY_SECTION_CONTINUE_SERIES,
                DISCOVERY_SECTION_UNWATCHED_SERIES,
                DISCOVERY_SECTION_RECENTLY_ADDED,
            ]
        );
        let continue_section = sections
            .iter()
            .find(|section| section.id == DISCOVERY_SECTION_CONTINUE_SERIES)
            .expect("continue section should be emitted");
        assert_eq!(
            continue_section.layout_hint,
            DiscoveryLayoutHint::ContinueRow
        );
        let unwatched = sections
            .iter()
            .find(|section| section.id == DISCOVERY_SECTION_UNWATCHED_SERIES)
            .expect("unwatched section should be emitted");
        assert_eq!(unwatched.title, "Unwatched series");
        assert_eq!(unwatched.layout_hint, DiscoveryLayoutHint::PosterRow);
        assert_eq!(unwatched.items.len(), 1);
        assert_eq!(unwatched.items[0].media_id, MediaID::Series(unwatched_id));
        assert_eq!(unwatched.items[0].reason.as_deref(), Some("New to you"));
        assert_eq!(unwatched.items[0].playback, None);
    }

    #[test]
    fn series_item_identity_stays_series_while_playback_targets_episode() {
        let library_id = LibraryId(Uuid::from_u128(30));
        let series_id = SeriesID(Uuid::from_u128(31));
        let episode_id = Uuid::from_u128(32);
        let context = watch_context(
            library_id,
            series_id,
            episode_id,
            1_700_000_123,
            ContinueWatchingActionHint::Resume,
        );
        let candidate = candidate(
            series_id,
            library_id,
            "Playback Show",
            123,
            Some(context),
        )
        .with_display_reason("Ready to resume");

        let item = series_candidate_to_discovery_item(&candidate);

        assert_eq!(item.id, format!("series:{series_id}"));
        assert_eq!(item.media_id, MediaID::Series(series_id));
        assert_eq!(item.media_type, DiscoveryMediaType::Series);
        assert_eq!(item.title, "Playback Show");
        assert_eq!(item.subtitle.as_deref(), Some("Resume S01E02"));
        assert_eq!(
            item.watch,
            Some(DiscoveryWatchSummary {
                state: DiscoveryWatchState::InProgress,
                progress: Some(0.1),
                position_seconds: Some(120.0),
                duration_seconds: Some(1_200.0),
                last_watched_epoch_seconds: Some(1_700_000_123),
            })
        );
        assert_eq!(
            item.playback,
            Some(DiscoveryPlaybackAction {
                target_media_id: episode_id,
                target_media_type: DiscoveryMediaType::Episode,
                hint: DiscoveryPlaybackHint::Resume,
            })
        );
        assert_eq!(item.reason.as_deref(), Some("Ready to resume"));
    }

    #[test]
    fn series_watch_mapping_omits_unreliable_playback_targets() {
        let library_id = LibraryId(Uuid::from_u128(40));
        let series_id = SeriesID(Uuid::from_u128(41));
        let continue_item = SeriesContinueWatchingItem {
            series_id: series_id.to_uuid(),
            library_id,
            action_episode_id: None,
            action_hint: ContinueWatchingActionHint::NextEpisode,
            position: 0.0,
            duration: 0.0,
            last_watched: 1_700_000_456,
            title: Some("Next Show".to_string()),
            subtitle: Some("Next up: S01E03".to_string()),
            poster_iid: None,
        };
        let (_, context) = series_watch_context_from_scoped_continue_watching(
            &continue_item,
            library_id,
        )
        .expect("series context should map");
        let candidate =
            candidate(series_id, library_id, "Next Show", 456, Some(context));
        let item = series_candidate_to_discovery_item(&candidate);

        assert_eq!(item.media_id, MediaID::Series(series_id));
        assert_eq!(item.playback, None);
        assert_eq!(
            item.watch.expect("watch summary").state,
            DiscoveryWatchState::Unwatched
        );
    }

    #[test]
    fn completed_watch_status_is_filtered() {
        let movie_id = MovieID(Uuid::from_u128(100));
        let completed = ItemWatchStatus::Completed(CompletedItem {
            media_id: MediaID::Movie(movie_id),
            last_watched: 1_700_000_500,
        });
        let in_progress = ItemWatchStatus::InProgress(InProgressItem {
            media_id: movie_id.to_uuid(),
            position: 12.0,
            duration: 120.0,
            last_watched: 1_700_000_600,
        });

        assert!(is_completed(Some(&completed)));
        assert!(!is_completed(Some(&in_progress)));
        assert!(!is_completed(None));
    }
}
