use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Datelike, NaiveDate, Utc};
use ferrex_model::{EpisodeID, MediaID, MovieID, SeriesID};
use ordered_float::OrderedFloat;
use sqlx::PgPool;
use uuid::Uuid;

use crate::api::types::collections::{
    CollectionLimitPolicy, CollectionLimitWindow,
    CollectionMaterializationState, CollectionMaterializationStatus,
    CollectionMediaKind, CollectionMember, CollectionMemberAvailability,
    CollectionMemberAvailabilityStatus, CollectionMemberKey,
    CollectionPageInfo, CollectionPagination, CollectionPersonRole,
    CollectionRuleField, CollectionRuleOperator, CollectionRulePredicate,
    CollectionRuleValue, CollectionSortDirection, CollectionSortField,
    CollectionSortKey, CollectionSortNulls, CollectionSortPolicy,
    CollectionSortTieBreaker, CollectionWatchStatus, DynamicCollectionRule,
    PreviewCollectionRuleResponse,
};
use crate::database::repository_ports::collections::{
    CollectionReadMode, clamp_collection_page_limit, page_info_for_slice,
    parse_collection_cursor,
};
use crate::error::{MediaError, Result};

#[derive(Debug)]
struct MovieCandidateRow {
    id: Uuid,
    library_id: Uuid,
    title: String,
    overview: Option<String>,
    release_date: Option<NaiveDate>,
    discovered_at: DateTime<Utc>,
    added_at: DateTime<Utc>,
    created_at: Option<DateTime<Utc>>,
    updated_at: DateTime<Utc>,
    runtime_minutes: Option<i32>,
    rating: Option<f64>,
    popularity: Option<f64>,
    content_rating: Option<String>,
    tmdb_id: i64,
    file_size_bytes: i64,
    is_available: bool,
    tombstone_reason: Option<String>,
    genres: Vec<String>,
    keywords: Vec<String>,
    actor_names: Vec<String>,
    actor_tmdb_ids: Vec<i64>,
    director_names: Vec<String>,
    director_tmdb_ids: Vec<i64>,
    watch_position: Option<f64>,
    watch_duration: Option<f64>,
    last_watched: Option<i64>,
    completed_at: Option<i64>,
}

#[derive(Debug)]
struct EpisodeCandidateRow {
    id: Uuid,
    library_id: Uuid,
    title: String,
    subtitle: Option<String>,
    overview: Option<String>,
    release_date: Option<NaiveDate>,
    discovered_at: DateTime<Utc>,
    added_at: DateTime<Utc>,
    created_at: Option<DateTime<Utc>>,
    updated_at: DateTime<Utc>,
    runtime_minutes: Option<i32>,
    rating: Option<f64>,
    popularity: Option<f64>,
    content_rating: Option<String>,
    tmdb_id: Option<i64>,
    file_size_bytes: i64,
    is_available: bool,
    tombstone_reason: Option<String>,
    genres: Vec<String>,
    keywords: Vec<String>,
    actor_names: Vec<String>,
    actor_tmdb_ids: Vec<i64>,
    director_names: Vec<String>,
    director_tmdb_ids: Vec<i64>,
    watch_position: Option<f64>,
    watch_duration: Option<f64>,
    last_watched: Option<i64>,
    completed_at: Option<i64>,
}

#[derive(Debug, sqlx::FromRow)]
struct SeriesCandidateRow {
    id: Uuid,
    library_id: Uuid,
    title: String,
    overview: Option<String>,
    release_date: Option<NaiveDate>,
    discovered_at: DateTime<Utc>,
    added_at: DateTime<Utc>,
    created_at: Option<DateTime<Utc>>,
    updated_at: DateTime<Utc>,
    runtime_minutes: Option<i32>,
    rating: Option<f64>,
    popularity: Option<f64>,
    content_rating: Option<String>,
    tmdb_id: Option<i64>,
    file_size_bytes: Option<i64>,
    is_available: bool,
    tombstone_reason: Option<String>,
    genres: Vec<String>,
    keywords: Vec<String>,
    actor_names: Vec<String>,
    actor_tmdb_ids: Vec<i64>,
    director_names: Vec<String>,
    director_tmdb_ids: Vec<i64>,
    watch_position: Option<f64>,
    watch_duration: Option<f64>,
    last_watched: Option<i64>,
    completed_count: i64,
    in_progress_count: i64,
    available_episode_count: i64,
}

#[derive(Debug, Clone)]
struct DynamicCollectionCandidate {
    media_id: MediaID,
    media_type: CollectionMediaKind,
    item_key: CollectionMemberKey,
    library_id: Uuid,
    title: String,
    subtitle: Option<String>,
    sort_title: String,
    overview: Option<String>,
    genres: Vec<String>,
    keywords: Vec<String>,
    actor_names: Vec<String>,
    actor_tmdb_ids: Vec<i64>,
    director_names: Vec<String>,
    director_tmdb_ids: Vec<i64>,
    release_date: Option<NaiveDate>,
    added_at: DateTime<Utc>,
    discovered_at: DateTime<Utc>,
    created_at: Option<DateTime<Utc>>,
    updated_at: DateTime<Utc>,
    runtime_minutes: Option<i32>,
    rating: Option<f64>,
    popularity: Option<f64>,
    content_rating: Option<String>,
    tmdb_id: Option<i64>,
    file_size_bytes: Option<i64>,
    availability: CollectionMemberAvailability,
    watch_progress_percent: Option<f64>,
    last_watched: Option<i64>,
    watch_status: CollectionWatchStatus,
}

#[derive(Debug, Clone)]
pub(super) struct DynamicCollectionEvaluatedItem {
    pub member: CollectionMember,
    pub visible: bool,
    pub hidden_reason: Option<String>,
    pub order_key: String,
}

#[derive(Debug, Clone)]
pub(super) struct DynamicCollectionEvaluation {
    pub items: Vec<DynamicCollectionEvaluatedItem>,
    pub total_count: u32,
    pub visible_count: u32,
    pub rule_hash_input: String,
    pub rule_hash: String,
}

pub(super) struct DynamicCollectionEvaluator<'a> {
    pool: &'a PgPool,
}

impl<'a> DynamicCollectionEvaluator<'a> {
    pub(super) fn new(pool: &'a PgPool) -> Self {
        Self { pool }
    }

    pub(super) async fn preview(
        &self,
        rule: &DynamicCollectionRule,
        page: CollectionPagination,
        mode: CollectionReadMode,
    ) -> Result<PreviewCollectionRuleResponse> {
        let evaluation = self.evaluate(rule).await?;
        Ok(preview_response(evaluation, page, mode)?)
    }

    pub(super) async fn evaluate(
        &self,
        rule: &DynamicCollectionRule,
    ) -> Result<DynamicCollectionEvaluation> {
        let normalized = rule.normalized();
        let report = normalized.validation_report();
        if !report.valid {
            return Err(MediaError::InvalidMedia(format!(
                "invalid dynamic collection rule: {}",
                format_rule_errors(&report.errors)
            )));
        }
        ensure_evaluator_support(&normalized)?;

        let watch_user_id = report.watch_user_ids.first().copied();
        let candidates = self.load_candidates(watch_user_id).await?;
        let mut matched = candidates
            .into_iter()
            .filter(|candidate| {
                predicate_matches(&normalized.predicate, candidate)
            })
            .collect::<Vec<_>>();

        apply_limit_window(&mut matched, &normalized.limit);
        sort_candidates(&mut matched, &normalized.sort);
        apply_limit(&mut matched, &normalized.limit);

        let mut items = Vec::with_capacity(matched.len());
        let mut visible_count = 0_u32;
        for (position, candidate) in matched.into_iter().enumerate() {
            let position = u32::try_from(position).map_err(|_| {
                MediaError::InvalidMedia(
                    "dynamic collection output exceeds supported item count"
                        .to_string(),
                )
            })?;
            let visible = candidate.availability.status
                == CollectionMemberAvailabilityStatus::Available;
            if visible {
                visible_count = visible_count.saturating_add(1);
            }
            let order_key =
                format!("{position:010}:{}", candidate.item_key.as_str());
            items.push(DynamicCollectionEvaluatedItem {
                visible,
                hidden_reason: (!visible).then(|| {
                    candidate.availability.reason.clone().unwrap_or_else(|| {
                        format!("media is {:?}", candidate.availability.status)
                            .to_lowercase()
                    })
                }),
                order_key: order_key.clone(),
                member: CollectionMember {
                    item_key: candidate.item_key,
                    media_id: candidate.media_id,
                    media_type: candidate.media_type,
                    title: candidate.title,
                    subtitle: candidate.subtitle,
                    position,
                    sort_key: Some(order_key),
                    availability: candidate.availability,
                    added_at: Some(candidate.added_at),
                    added_by: None,
                },
            });
        }

        let total_count = u32::try_from(items.len()).map_err(|_| {
            MediaError::InvalidMedia(
                "dynamic collection output exceeds supported item count"
                    .to_string(),
            )
        })?;

        Ok(DynamicCollectionEvaluation {
            items,
            total_count,
            visible_count,
            rule_hash_input: report.rule_hash_input,
            rule_hash: report.rule_hash.ok_or_else(|| {
                MediaError::Internal(
                    "valid dynamic collection rule did not produce a hash"
                        .to_string(),
                )
            })?,
        })
    }

    async fn load_candidates(
        &self,
        watch_user_id: Option<Uuid>,
    ) -> Result<Vec<DynamicCollectionCandidate>> {
        let mut candidates = Vec::new();
        candidates.extend(self.load_movie_candidates(watch_user_id).await?);
        candidates.extend(self.load_series_candidates(watch_user_id).await?);
        candidates.extend(self.load_episode_candidates(watch_user_id).await?);
        Ok(candidates)
    }

    async fn load_movie_candidates(
        &self,
        watch_user_id: Option<Uuid>,
    ) -> Result<Vec<DynamicCollectionCandidate>> {
        let movie_media_type = CollectionMediaKind::Movie.media_type_code();
        let rows = sqlx::query_as!(
            MovieCandidateRow,
            r#"
            SELECT
                mr.id,
                mr.library_id,
                mr.title::text AS "title!",
                mm.overview AS "overview?",
                mm.release_date AS "release_date?",
                mr.discovered_at AS "discovered_at!",
                mf.discovered_at AS "added_at!",
                mr.created_at AS "created_at?",
                mr.updated_at AS "updated_at!",
                mm.runtime AS "runtime_minutes?",
                mm.vote_average::double precision AS "rating?",
                mm.popularity::double precision AS "popularity?",
                mm.primary_certification AS "content_rating?",
                mr.tmdb_id AS "tmdb_id!",
                mf.file_size AS "file_size_bytes!",
                mf.is_available AS "is_available!",
                mf.tombstone_reason AS "tombstone_reason?",
                ARRAY(
                    SELECT lower(mg.name)
                    FROM movie_genres mg
                    WHERE mg.movie_id = mr.id
                    ORDER BY lower(mg.name), mg.genre_id
                ) AS "genres!",
                ARRAY(
                    SELECT lower(mk.name)
                    FROM movie_keywords mk
                    WHERE mk.movie_id = mr.id
                    ORDER BY lower(mk.name), mk.keyword_id
                ) AS "keywords!",
                ARRAY(
                    SELECT lower(p.name)
                    FROM movie_cast mc
                    JOIN persons p ON p.id = mc.person_id
                    WHERE mc.movie_id = mr.id
                    ORDER BY lower(p.name), mc.person_tmdb_id
                ) AS "actor_names!",
                ARRAY(
                    SELECT mc.person_tmdb_id
                    FROM movie_cast mc
                    WHERE mc.movie_id = mr.id
                    ORDER BY mc.person_tmdb_id
                ) AS "actor_tmdb_ids!",
                ARRAY(
                    SELECT lower(p.name)
                    FROM movie_crew mc
                    JOIN persons p ON p.id = mc.person_id
                    WHERE mc.movie_id = mr.id
                      AND lower(mc.job) = 'director'
                    ORDER BY lower(p.name), mc.person_tmdb_id
                ) AS "director_names!",
                ARRAY(
                    SELECT mc.person_tmdb_id
                    FROM movie_crew mc
                    WHERE mc.movie_id = mr.id
                      AND lower(mc.job) = 'director'
                    ORDER BY mc.person_tmdb_id
                ) AS "director_tmdb_ids!",
                uwp.position::double precision AS "watch_position?",
                uwp.duration::double precision AS "watch_duration?",
                uwp.last_watched AS "last_watched?",
                ucm.completed_at AS "completed_at?"
            FROM movie_references mr
            JOIN media_files mf ON mf.id = mr.file_id
            LEFT JOIN movie_metadata mm ON mm.movie_id = mr.id
            LEFT JOIN user_watch_progress uwp
              ON $1::uuid IS NOT NULL
             AND uwp.user_id = $1
             AND uwp.media_uuid = mr.id
             AND uwp.media_type = $2
            LEFT JOIN user_completed_media ucm
              ON $1::uuid IS NOT NULL
             AND ucm.user_id = $1
             AND ucm.media_uuid = mr.id
             AND ucm.media_type = $2
            ORDER BY mr.id
            "#,
            watch_user_id,
            movie_media_type,
        )
        .fetch_all(self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "failed to load dynamic collection movie candidates: {e}"
            ))
        })?;

        Ok(rows.into_iter().map(movie_candidate_from_row).collect())
    }

    async fn load_series_candidates(
        &self,
        watch_user_id: Option<Uuid>,
    ) -> Result<Vec<DynamicCollectionCandidate>> {
        let episode_media_type = CollectionMediaKind::Episode.media_type_code();
        let rows = sqlx::query_as::<_, SeriesCandidateRow>(
            r#"
            SELECT
                s.id,
                s.library_id,
                s.title::text AS title,
                sm.overview AS overview,
                sm.first_air_date AS release_date,
                s.discovered_at AS discovered_at,
                s.discovered_at AS added_at,
                s.created_at AS created_at,
                s.updated_at AS updated_at,
                NULL::integer AS runtime_minutes,
                sm.vote_average::double precision AS rating,
                sm.popularity::double precision AS popularity,
                sm.primary_content_rating AS content_rating,
                s.tmdb_id AS tmdb_id,
                availability.file_size_bytes AS file_size_bytes,
                (availability.available_episode_count > 0) AS is_available,
                CASE
                    WHEN availability.episode_count = 0 THEN 'series has no episodes'
                    WHEN availability.available_episode_count = 0 THEN 'series has no available episodes'
                    ELSE NULL
                END AS tombstone_reason,
                ARRAY(
                    SELECT lower(sg.name)
                    FROM series_genres sg
                    WHERE sg.series_id = s.id
                    ORDER BY lower(sg.name), sg.genre_id
                ) AS genres,
                ARRAY(
                    SELECT lower(sk.name)
                    FROM series_keywords sk
                    WHERE sk.series_id = s.id
                    ORDER BY lower(sk.name), sk.keyword_id
                ) AS keywords,
                ARRAY(
                    SELECT lower(p.name)
                    FROM series_cast sc
                    JOIN persons p ON p.id = sc.person_id
                    WHERE sc.series_id = s.id
                    ORDER BY lower(p.name), sc.person_tmdb_id
                ) AS actor_names,
                ARRAY(
                    SELECT sc.person_tmdb_id
                    FROM series_cast sc
                    WHERE sc.series_id = s.id
                    ORDER BY sc.person_tmdb_id
                ) AS actor_tmdb_ids,
                ARRAY(
                    SELECT lower(p.name)
                    FROM series_crew sc
                    JOIN persons p ON p.id = sc.person_id
                    WHERE sc.series_id = s.id
                      AND lower(sc.job) = 'director'
                    ORDER BY lower(p.name), sc.person_tmdb_id
                ) AS director_names,
                ARRAY(
                    SELECT sc.person_tmdb_id
                    FROM series_crew sc
                    WHERE sc.series_id = s.id
                      AND lower(sc.job) = 'director'
                    ORDER BY sc.person_tmdb_id
                ) AS director_tmdb_ids,
                progress.position::double precision AS watch_position,
                progress.duration::double precision AS watch_duration,
                activity.last_watched AS last_watched,
                activity.completed_count,
                activity.in_progress_count,
                availability.available_episode_count
            FROM series s
            LEFT JOIN series_metadata sm ON sm.series_id = s.id
            LEFT JOIN LATERAL (
                SELECT
                    COUNT(*) AS episode_count,
                    COUNT(*) FILTER (WHERE mf.is_available) AS available_episode_count,
                    SUM(mf.file_size)::bigint AS file_size_bytes
                FROM episode_references er
                JOIN media_files mf ON mf.id = er.file_id
                WHERE er.series_id = s.id
            ) availability ON TRUE
            LEFT JOIN LATERAL (
                WITH watched AS (
                    SELECT
                        MAX(uwp.last_watched) AS progress_last_watched,
                        MAX(ucm.completed_at) AS completed_last_watched,
                        COUNT(uwp.media_uuid) AS in_progress_count,
                        COUNT(ucm.media_uuid) AS completed_count
                    FROM episode_references er
                    LEFT JOIN user_watch_progress uwp
                      ON $1::uuid IS NOT NULL
                     AND uwp.user_id = $1
                     AND uwp.media_uuid = er.id
                     AND uwp.media_type = $2
                    LEFT JOIN user_completed_media ucm
                      ON $1::uuid IS NOT NULL
                     AND ucm.user_id = $1
                     AND ucm.media_uuid = er.id
                     AND ucm.media_type = $2
                    WHERE er.series_id = s.id
                )
                SELECT
                    CASE
                        WHEN progress_last_watched IS NULL THEN completed_last_watched
                        WHEN completed_last_watched IS NULL THEN progress_last_watched
                        ELSE GREATEST(progress_last_watched, completed_last_watched)
                    END AS last_watched,
                    in_progress_count,
                    completed_count
                FROM watched
            ) activity ON TRUE
            LEFT JOIN LATERAL (
                SELECT uwp.position, uwp.duration
                FROM episode_references er
                JOIN user_watch_progress uwp
                  ON $1::uuid IS NOT NULL
                 AND uwp.user_id = $1
                 AND uwp.media_uuid = er.id
                 AND uwp.media_type = $2
                WHERE er.series_id = s.id
                ORDER BY uwp.last_watched DESC, er.id
                LIMIT 1
            ) progress ON TRUE
            ORDER BY s.id
            "#,
        )
        .bind(watch_user_id)
        .bind(episode_media_type)
        .fetch_all(self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "failed to load dynamic collection series candidates: {e}"
            ))
        })?;

        Ok(rows.into_iter().map(series_candidate_from_row).collect())
    }

    async fn load_episode_candidates(
        &self,
        watch_user_id: Option<Uuid>,
    ) -> Result<Vec<DynamicCollectionCandidate>> {
        let episode_media_type = CollectionMediaKind::Episode.media_type_code();
        let rows = sqlx::query_as!(
            EpisodeCandidateRow,
            r#"
            SELECT
                er.id,
                mf.library_id,
                COALESCE(em.name, 'Episode ' || er.episode_number::text) AS "title!",
                ('S' || er.season_number::text || ' E' || er.episode_number::text) AS "subtitle?",
                em.overview AS "overview?",
                em.air_date AS "release_date?",
                er.discovered_at AS "discovered_at!",
                mf.discovered_at AS "added_at!",
                er.created_at AS "created_at?",
                er.updated_at AS "updated_at!",
                em.runtime AS "runtime_minutes?",
                em.vote_average::double precision AS "rating?",
                NULL::double precision AS "popularity?",
                NULL::text AS "content_rating?",
                em.tmdb_id AS "tmdb_id?",
                mf.file_size AS "file_size_bytes!",
                mf.is_available AS "is_available!",
                mf.tombstone_reason AS "tombstone_reason?",
                ARRAY(
                    SELECT lower(sg.name)
                    FROM series_genres sg
                    WHERE sg.series_id = er.series_id
                    ORDER BY lower(sg.name), sg.genre_id
                ) AS "genres!",
                ARRAY(
                    SELECT lower(ek.name)
                    FROM episode_keywords ek
                    WHERE ek.episode_id = er.id
                    ORDER BY lower(ek.name), ek.keyword_id
                ) AS "keywords!",
                ARRAY(
                    SELECT lower(actor.name)
                    FROM (
                        SELECT p.name, ec.person_tmdb_id
                        FROM episode_cast ec
                        JOIN persons p ON p.id = ec.person_id
                        WHERE ec.episode_id = er.id
                        UNION
                        SELECT p.name, egs.person_tmdb_id
                        FROM episode_guest_stars egs
                        JOIN persons p ON p.id = egs.person_id
                        WHERE egs.episode_id = er.id
                    ) actor
                    ORDER BY lower(actor.name), actor.person_tmdb_id
                ) AS "actor_names!",
                ARRAY(
                    SELECT actor.person_tmdb_id
                    FROM (
                        SELECT ec.person_tmdb_id
                        FROM episode_cast ec
                        WHERE ec.episode_id = er.id
                        UNION
                        SELECT egs.person_tmdb_id
                        FROM episode_guest_stars egs
                        WHERE egs.episode_id = er.id
                    ) actor
                    ORDER BY actor.person_tmdb_id
                ) AS "actor_tmdb_ids!",
                ARRAY(
                    SELECT lower(p.name)
                    FROM episode_crew ec
                    JOIN persons p ON p.id = ec.person_id
                    WHERE ec.episode_id = er.id
                      AND lower(ec.job) = 'director'
                    ORDER BY lower(p.name), ec.person_tmdb_id
                ) AS "director_names!",
                ARRAY(
                    SELECT ec.person_tmdb_id
                    FROM episode_crew ec
                    WHERE ec.episode_id = er.id
                      AND lower(ec.job) = 'director'
                    ORDER BY ec.person_tmdb_id
                ) AS "director_tmdb_ids!",
                uwp.position::double precision AS "watch_position?",
                uwp.duration::double precision AS "watch_duration?",
                uwp.last_watched AS "last_watched?",
                ucm.completed_at AS "completed_at?"
            FROM episode_references er
            JOIN media_files mf ON mf.id = er.file_id
            LEFT JOIN episode_metadata em ON em.episode_id = er.id
            LEFT JOIN user_watch_progress uwp
              ON $1::uuid IS NOT NULL
             AND uwp.user_id = $1
             AND uwp.media_uuid = er.id
             AND uwp.media_type = $2
            LEFT JOIN user_completed_media ucm
              ON $1::uuid IS NOT NULL
             AND ucm.user_id = $1
             AND ucm.media_uuid = er.id
             AND ucm.media_type = $2
            ORDER BY er.id
            "#,
            watch_user_id,
            episode_media_type,
        )
        .fetch_all(self.pool)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "failed to load dynamic collection episode candidates: {e}"
            ))
        })?;

        Ok(rows.into_iter().map(episode_candidate_from_row).collect())
    }
}

trait CollectionMediaKindExt {
    fn media_type_code(self) -> i16;
}

impl CollectionMediaKindExt for CollectionMediaKind {
    fn media_type_code(self) -> i16 {
        match self {
            CollectionMediaKind::Movie => 0,
            CollectionMediaKind::Series => 1,
            CollectionMediaKind::Season => 2,
            CollectionMediaKind::Episode => 3,
        }
    }
}

fn movie_candidate_from_row(
    row: MovieCandidateRow,
) -> DynamicCollectionCandidate {
    let media_id = MediaID::Movie(MovieID(row.id));
    candidate_from_parts(CandidateParts {
        media_id,
        media_type: CollectionMediaKind::Movie,
        library_id: row.library_id,
        title: row.title,
        subtitle: None,
        overview: row.overview,
        release_date: row.release_date,
        discovered_at: row.discovered_at,
        added_at: row.added_at,
        created_at: row.created_at,
        updated_at: row.updated_at,
        runtime_minutes: row.runtime_minutes,
        rating: row.rating,
        popularity: row.popularity,
        content_rating: row.content_rating,
        tmdb_id: Some(row.tmdb_id),
        file_size_bytes: Some(row.file_size_bytes),
        is_available: row.is_available,
        tombstone_reason: row.tombstone_reason,
        genres: row.genres,
        keywords: row.keywords,
        actor_names: row.actor_names,
        actor_tmdb_ids: row.actor_tmdb_ids,
        director_names: row.director_names,
        director_tmdb_ids: row.director_tmdb_ids,
        watch_position: row.watch_position,
        watch_duration: row.watch_duration,
        last_watched: row.last_watched,
        completed_at: row.completed_at,
        watch_status: None,
    })
}

fn series_candidate_from_row(
    row: SeriesCandidateRow,
) -> DynamicCollectionCandidate {
    let media_id = MediaID::Series(SeriesID(row.id));
    let watch_status = if row.available_episode_count > 0
        && row.completed_count >= row.available_episode_count
    {
        CollectionWatchStatus::Completed
    } else if row.completed_count > 0 || row.in_progress_count > 0 {
        CollectionWatchStatus::InProgress
    } else {
        CollectionWatchStatus::Unwatched
    };

    candidate_from_parts(CandidateParts {
        media_id,
        media_type: CollectionMediaKind::Series,
        library_id: row.library_id,
        title: row.title,
        subtitle: None,
        overview: row.overview,
        release_date: row.release_date,
        discovered_at: row.discovered_at,
        added_at: row.added_at,
        created_at: row.created_at,
        updated_at: row.updated_at,
        runtime_minutes: row.runtime_minutes,
        rating: row.rating,
        popularity: row.popularity,
        content_rating: row.content_rating,
        tmdb_id: row.tmdb_id,
        file_size_bytes: row.file_size_bytes,
        is_available: row.is_available,
        tombstone_reason: row.tombstone_reason,
        genres: row.genres,
        keywords: row.keywords,
        actor_names: row.actor_names,
        actor_tmdb_ids: row.actor_tmdb_ids,
        director_names: row.director_names,
        director_tmdb_ids: row.director_tmdb_ids,
        watch_position: row.watch_position,
        watch_duration: row.watch_duration,
        last_watched: row.last_watched,
        completed_at: None,
        watch_status: Some(watch_status),
    })
}

fn episode_candidate_from_row(
    row: EpisodeCandidateRow,
) -> DynamicCollectionCandidate {
    let media_id = MediaID::Episode(EpisodeID(row.id));
    candidate_from_parts(CandidateParts {
        media_id,
        media_type: CollectionMediaKind::Episode,
        library_id: row.library_id,
        title: row.title,
        subtitle: row.subtitle,
        overview: row.overview,
        release_date: row.release_date,
        discovered_at: row.discovered_at,
        added_at: row.added_at,
        created_at: row.created_at,
        updated_at: row.updated_at,
        runtime_minutes: row.runtime_minutes,
        rating: row.rating,
        popularity: row.popularity,
        content_rating: row.content_rating,
        tmdb_id: row.tmdb_id,
        file_size_bytes: Some(row.file_size_bytes),
        is_available: row.is_available,
        tombstone_reason: row.tombstone_reason,
        genres: row.genres,
        keywords: row.keywords,
        actor_names: row.actor_names,
        actor_tmdb_ids: row.actor_tmdb_ids,
        director_names: row.director_names,
        director_tmdb_ids: row.director_tmdb_ids,
        watch_position: row.watch_position,
        watch_duration: row.watch_duration,
        last_watched: row.last_watched,
        completed_at: row.completed_at,
        watch_status: None,
    })
}

struct CandidateParts {
    media_id: MediaID,
    media_type: CollectionMediaKind,
    library_id: Uuid,
    title: String,
    subtitle: Option<String>,
    overview: Option<String>,
    release_date: Option<NaiveDate>,
    discovered_at: DateTime<Utc>,
    added_at: DateTime<Utc>,
    created_at: Option<DateTime<Utc>>,
    updated_at: DateTime<Utc>,
    runtime_minutes: Option<i32>,
    rating: Option<f64>,
    popularity: Option<f64>,
    content_rating: Option<String>,
    tmdb_id: Option<i64>,
    file_size_bytes: Option<i64>,
    is_available: bool,
    tombstone_reason: Option<String>,
    genres: Vec<String>,
    keywords: Vec<String>,
    actor_names: Vec<String>,
    actor_tmdb_ids: Vec<i64>,
    director_names: Vec<String>,
    director_tmdb_ids: Vec<i64>,
    watch_position: Option<f64>,
    watch_duration: Option<f64>,
    last_watched: Option<i64>,
    completed_at: Option<i64>,
    watch_status: Option<CollectionWatchStatus>,
}

fn candidate_from_parts(parts: CandidateParts) -> DynamicCollectionCandidate {
    let item_key = CollectionMemberKey::for_media(&parts.media_id);
    let watch_progress_percent =
        match (parts.watch_position, parts.watch_duration) {
            (Some(position), Some(duration)) if duration > 0.0 => {
                Some(((position / duration) * 100.0).clamp(0.0, 100.0))
            }
            _ => None,
        };
    let inferred_watch_status = if parts.completed_at.is_some() {
        CollectionWatchStatus::Completed
    } else if watch_progress_percent.is_some_and(|percent| percent > 0.0) {
        CollectionWatchStatus::InProgress
    } else {
        CollectionWatchStatus::Unwatched
    };
    let watch_status = parts.watch_status.unwrap_or(inferred_watch_status);
    let availability = if parts.is_available {
        CollectionMemberAvailability {
            status: CollectionMemberAvailabilityStatus::Available,
            reason: None,
            checked_at: Some(Utc::now()),
        }
    } else {
        CollectionMemberAvailability {
            status: CollectionMemberAvailabilityStatus::Tombstoned,
            reason: Some(
                parts
                    .tombstone_reason
                    .unwrap_or_else(|| "media file is tombstoned".to_string()),
            ),
            checked_at: Some(Utc::now()),
        }
    };
    let sort_title = normalize_text(&parts.title);

    DynamicCollectionCandidate {
        media_id: parts.media_id,
        media_type: parts.media_type,
        item_key,
        library_id: parts.library_id,
        title: parts.title,
        subtitle: parts.subtitle,
        sort_title,
        overview: parts.overview,
        genres: normalize_strings(parts.genres),
        keywords: normalize_strings(parts.keywords),
        actor_names: normalize_strings(parts.actor_names),
        actor_tmdb_ids: parts.actor_tmdb_ids,
        director_names: normalize_strings(parts.director_names),
        director_tmdb_ids: parts.director_tmdb_ids,
        release_date: parts.release_date,
        added_at: parts.added_at,
        discovered_at: parts.discovered_at,
        created_at: parts.created_at,
        updated_at: parts.updated_at,
        runtime_minutes: parts.runtime_minutes,
        rating: parts.rating,
        popularity: parts.popularity,
        content_rating: parts
            .content_rating
            .map(|value| normalize_text(&value)),
        tmdb_id: parts.tmdb_id,
        file_size_bytes: parts.file_size_bytes,
        availability,
        watch_progress_percent,
        last_watched: parts.last_watched,
        watch_status,
    }
}

fn normalize_strings(values: Vec<String>) -> Vec<String> {
    let mut values = values
        .into_iter()
        .map(|value| normalize_text(&value))
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    values.sort();
    values.dedup();
    values
}

fn normalize_text(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn ensure_evaluator_support(rule: &DynamicCollectionRule) -> Result<()> {
    let mut unsupported = Vec::new();
    collect_unsupported_predicates(
        &rule.predicate,
        "predicate",
        &mut unsupported,
    );
    collect_unsupported_sorts(&rule.sort, &mut unsupported);
    if !matches!(rule.limit.window, CollectionLimitWindow::All)
        && rule.limit.max_items.is_none()
        && rule.limit.per_media_type.is_none()
    {
        unsupported.push(
            "limit.window requires max_items or per_media_type".to_string(),
        );
    }
    if unsupported.is_empty() {
        Ok(())
    } else {
        Err(MediaError::InvalidMedia(format!(
            "unsupported dynamic collection rule: {}",
            unsupported.join("; ")
        )))
    }
}

fn collect_unsupported_predicates(
    predicate: &CollectionRulePredicate,
    path: &str,
    unsupported: &mut Vec<String>,
) {
    match predicate {
        CollectionRulePredicate::All { clauses }
        | CollectionRulePredicate::Any { clauses } => {
            for (index, clause) in clauses.iter().enumerate() {
                collect_unsupported_predicates(
                    clause,
                    &format!("{path}.clauses[{index}]"),
                    unsupported,
                );
            }
        }
        CollectionRulePredicate::Not { clause } => {
            collect_unsupported_predicates(
                clause,
                &format!("{path}.clause"),
                unsupported,
            );
        }
        CollectionRulePredicate::Field {
            field,
            operator,
            value,
        } => {
            if matches!(
                (*field, *operator),
                (
                    CollectionRuleField::WatchStatus
                        | CollectionRuleField::WatchProgress,
                    CollectionRuleOperator::Exists
                )
            ) {
                unsupported.push(format!(
                    "{path}.operator Exists is not backed for watch-state fields because it has no explicit user scope"
                ));
            }
            if !field_is_supported(*field) {
                unsupported.push(format!(
                    "{path}.field {:?} is not backed by the dynamic evaluator",
                    field
                ));
            }
            if let CollectionRuleValue::MediaType(kind) = value {
                if !media_kind_is_supported(*kind) {
                    unsupported.push(format!(
                        "{path}.value media_type {:?} is not backed by the dynamic evaluator",
                        kind
                    ));
                }
            }
            if let CollectionRuleValue::MediaTypes(kinds) = value {
                for kind in kinds {
                    if !media_kind_is_supported(*kind) {
                        unsupported.push(format!(
                            "{path}.value media_type {:?} is not backed by the dynamic evaluator",
                            kind
                        ));
                    }
                }
            }
            if let CollectionRuleValue::Person(person) = value {
                if !matches!(
                    person.role,
                    CollectionPersonRole::Actor
                        | CollectionPersonRole::Director
                        | CollectionPersonRole::Any
                ) {
                    unsupported.push(format!(
                        "{path}.value person role {:?} is not backed by the dynamic evaluator",
                        person.role
                    ));
                }
            }
            if let CollectionRuleValue::WatchStatus(watch) = value {
                if watch.statuses.iter().any(|status| {
                    matches!(status, CollectionWatchStatus::Abandoned)
                }) {
                    unsupported.push(format!(
                        "{path}.value abandoned watch status is not backed by stored watch-state data"
                    ));
                }
            }
        }
    }
}

fn field_is_supported(field: CollectionRuleField) -> bool {
    matches!(
        field,
        CollectionRuleField::MediaType
            | CollectionRuleField::LibraryId
            | CollectionRuleField::Title
            | CollectionRuleField::SortTitle
            | CollectionRuleField::Overview
            | CollectionRuleField::SearchText
            | CollectionRuleField::Genre
            | CollectionRuleField::Keyword
            | CollectionRuleField::Person
            | CollectionRuleField::ReleaseYear
            | CollectionRuleField::ReleaseDate
            | CollectionRuleField::AddedAt
            | CollectionRuleField::DiscoveredAt
            | CollectionRuleField::CreatedAt
            | CollectionRuleField::UpdatedAt
            | CollectionRuleField::RuntimeMinutes
            | CollectionRuleField::AudienceRating
            | CollectionRuleField::CriticRating
            | CollectionRuleField::UserRating
            | CollectionRuleField::Rating
            | CollectionRuleField::Popularity
            | CollectionRuleField::ContentRating
            | CollectionRuleField::WatchStatus
            | CollectionRuleField::WatchProgress
            | CollectionRuleField::Availability
            | CollectionRuleField::TmdbId
            | CollectionRuleField::ActorName
            | CollectionRuleField::DirectorName
            | CollectionRuleField::FileSizeBytes
    )
}

fn media_kind_is_supported(kind: CollectionMediaKind) -> bool {
    matches!(
        kind,
        CollectionMediaKind::Movie
            | CollectionMediaKind::Series
            | CollectionMediaKind::Episode
    )
}

fn collect_unsupported_sorts(
    sort: &CollectionSortPolicy,
    unsupported: &mut Vec<String>,
) {
    for (index, key) in sort.keys.iter().enumerate() {
        if !sort_field_is_supported(key.field) {
            unsupported.push(format!(
                "sort.keys[{index}].field {:?} is not backed by the dynamic evaluator",
                key.field
            ));
        }
    }
}

fn sort_field_is_supported(field: CollectionSortField) -> bool {
    matches!(
        field,
        CollectionSortField::RecentlyAdded
            | CollectionSortField::RecentlyReleased
            | CollectionSortField::Title
            | CollectionSortField::SortTitle
            | CollectionSortField::ReleaseDate
            | CollectionSortField::AddedAt
            | CollectionSortField::DiscoveredAt
            | CollectionSortField::CreatedAt
            | CollectionSortField::UpdatedAt
            | CollectionSortField::RuntimeMinutes
            | CollectionSortField::AudienceRating
            | CollectionSortField::CriticRating
            | CollectionSortField::UserRating
            | CollectionSortField::Rating
            | CollectionSortField::Popularity
            | CollectionSortField::FileSizeBytes
            | CollectionSortField::LastWatchedAt
            | CollectionSortField::WatchProgress
    )
}

fn predicate_matches(
    predicate: &CollectionRulePredicate,
    candidate: &DynamicCollectionCandidate,
) -> bool {
    match predicate {
        CollectionRulePredicate::All { clauses } => clauses
            .iter()
            .all(|clause| predicate_matches(clause, candidate)),
        CollectionRulePredicate::Any { clauses } => clauses
            .iter()
            .any(|clause| predicate_matches(clause, candidate)),
        CollectionRulePredicate::Not { clause } => {
            !predicate_matches(clause, candidate)
        }
        CollectionRulePredicate::Field {
            field,
            operator,
            value,
        } => field_matches(*field, *operator, value, candidate),
    }
}

fn field_matches(
    field: CollectionRuleField,
    operator: CollectionRuleOperator,
    value: &CollectionRuleValue,
    candidate: &DynamicCollectionCandidate,
) -> bool {
    if matches!(operator, CollectionRuleOperator::Exists) {
        return field_exists(field, value, candidate);
    }

    match field {
        CollectionRuleField::MediaType => {
            media_type_matches(operator, value, candidate.media_type)
        }
        CollectionRuleField::LibraryId => {
            uuid_matches(operator, value, candidate.library_id)
        }
        CollectionRuleField::Title => {
            text_matches(operator, value, Some(&candidate.title))
        }
        CollectionRuleField::SortTitle => {
            text_matches(operator, value, Some(&candidate.sort_title))
        }
        CollectionRuleField::Overview => {
            text_matches(operator, value, candidate.overview.as_deref())
        }
        CollectionRuleField::SearchText => {
            search_text_matches(operator, value, candidate)
        }
        CollectionRuleField::Genre => {
            text_set_matches(operator, value, &candidate.genres)
        }
        CollectionRuleField::Keyword => {
            text_set_matches(operator, value, &candidate.keywords)
        }
        CollectionRuleField::Person => {
            person_matches(operator, value, candidate)
        }
        CollectionRuleField::ReleaseYear => integer_matches(
            operator,
            value,
            candidate.release_date.map(|date| i64::from(date.year())),
        ),
        CollectionRuleField::ReleaseDate => {
            date_matches(operator, value, candidate.release_date)
        }
        CollectionRuleField::AddedAt => {
            datetime_as_date_matches(operator, value, Some(candidate.added_at))
        }
        CollectionRuleField::DiscoveredAt => datetime_as_date_matches(
            operator,
            value,
            Some(candidate.discovered_at),
        ),
        CollectionRuleField::CreatedAt => {
            datetime_as_date_matches(operator, value, candidate.created_at)
        }
        CollectionRuleField::UpdatedAt => datetime_as_date_matches(
            operator,
            value,
            Some(candidate.updated_at),
        ),
        CollectionRuleField::RuntimeMinutes => integer_matches(
            operator,
            value,
            candidate.runtime_minutes.map(i64::from),
        ),
        CollectionRuleField::AudienceRating
        | CollectionRuleField::CriticRating
        | CollectionRuleField::UserRating
        | CollectionRuleField::Rating => {
            decimal_matches(operator, value, candidate.rating)
        }
        CollectionRuleField::Popularity => {
            decimal_matches(operator, value, candidate.popularity)
        }
        CollectionRuleField::ContentRating => text_set_matches(
            operator,
            value,
            &candidate.content_rating.iter().cloned().collect::<Vec<_>>(),
        ),
        CollectionRuleField::WatchStatus => {
            watch_status_matches(operator, value, candidate)
        }
        CollectionRuleField::WatchProgress => {
            watch_progress_matches(operator, value, candidate)
        }
        CollectionRuleField::Availability => {
            availability_matches(operator, value, candidate.availability.status)
        }
        CollectionRuleField::TmdbId => {
            integer_matches(operator, value, candidate.tmdb_id)
        }
        CollectionRuleField::ActorName => {
            text_set_matches(operator, value, &candidate.actor_names)
        }
        CollectionRuleField::DirectorName => {
            text_set_matches(operator, value, &candidate.director_names)
        }
        CollectionRuleField::FileSizeBytes => {
            integer_matches(operator, value, candidate.file_size_bytes)
        }
        CollectionRuleField::BitrateKbps
        | CollectionRuleField::ResolutionWidth
        | CollectionRuleField::ResolutionHeight
        | CollectionRuleField::VideoCodec
        | CollectionRuleField::AudioCodec
        | CollectionRuleField::AudioChannelCount
        | CollectionRuleField::SubtitleLanguage
        | CollectionRuleField::HasSubtitles => false,
    }
}

fn field_exists(
    field: CollectionRuleField,
    value: &CollectionRuleValue,
    candidate: &DynamicCollectionCandidate,
) -> bool {
    let exists = match field {
        CollectionRuleField::MediaType | CollectionRuleField::LibraryId => true,
        CollectionRuleField::Title | CollectionRuleField::SortTitle => {
            !candidate.sort_title.is_empty()
        }
        CollectionRuleField::Overview => candidate
            .overview
            .as_deref()
            .is_some_and(|overview| !normalize_text(overview).is_empty()),
        CollectionRuleField::SearchText => {
            !candidate.sort_title.is_empty()
                || candidate.overview.as_deref().is_some_and(|overview| {
                    !normalize_text(overview).is_empty()
                })
                || !candidate.genres.is_empty()
                || !candidate.keywords.is_empty()
                || !candidate.actor_names.is_empty()
                || !candidate.director_names.is_empty()
        }
        CollectionRuleField::Genre => !candidate.genres.is_empty(),
        CollectionRuleField::Keyword => !candidate.keywords.is_empty(),
        CollectionRuleField::Person => {
            !candidate.actor_names.is_empty()
                || !candidate.actor_tmdb_ids.is_empty()
                || !candidate.director_names.is_empty()
                || !candidate.director_tmdb_ids.is_empty()
        }
        CollectionRuleField::ReleaseYear | CollectionRuleField::ReleaseDate => {
            candidate.release_date.is_some()
        }
        CollectionRuleField::AddedAt
        | CollectionRuleField::DiscoveredAt
        | CollectionRuleField::UpdatedAt => true,
        CollectionRuleField::CreatedAt => candidate.created_at.is_some(),
        CollectionRuleField::RuntimeMinutes => {
            candidate.runtime_minutes.is_some()
        }
        CollectionRuleField::AudienceRating
        | CollectionRuleField::CriticRating
        | CollectionRuleField::UserRating
        | CollectionRuleField::Rating => candidate.rating.is_some(),
        CollectionRuleField::Popularity => candidate.popularity.is_some(),
        CollectionRuleField::ContentRating => {
            candidate.content_rating.is_some()
        }
        CollectionRuleField::WatchStatus => false,
        CollectionRuleField::WatchProgress => {
            candidate.watch_progress_percent.is_some()
        }
        CollectionRuleField::Availability => true,
        CollectionRuleField::TmdbId => candidate.tmdb_id.is_some(),
        CollectionRuleField::ActorName => !candidate.actor_names.is_empty(),
        CollectionRuleField::DirectorName => {
            !candidate.director_names.is_empty()
        }
        CollectionRuleField::FileSizeBytes => {
            candidate.file_size_bytes.is_some()
        }
        CollectionRuleField::BitrateKbps
        | CollectionRuleField::ResolutionWidth
        | CollectionRuleField::ResolutionHeight
        | CollectionRuleField::VideoCodec
        | CollectionRuleField::AudioCodec
        | CollectionRuleField::AudioChannelCount
        | CollectionRuleField::SubtitleLanguage
        | CollectionRuleField::HasSubtitles => false,
    };
    exists_matches(value, exists)
}

fn media_type_matches(
    operator: CollectionRuleOperator,
    value: &CollectionRuleValue,
    candidate: CollectionMediaKind,
) -> bool {
    match (operator, value) {
        (
            CollectionRuleOperator::Equals,
            CollectionRuleValue::MediaType(kind),
        ) => candidate == *kind,
        (
            CollectionRuleOperator::NotEquals,
            CollectionRuleValue::MediaType(kind),
        ) => candidate != *kind,
        (
            CollectionRuleOperator::In,
            CollectionRuleValue::MediaTypes(kinds),
        ) => kinds.contains(&candidate),
        (
            CollectionRuleOperator::NotIn,
            CollectionRuleValue::MediaTypes(kinds),
        ) => !kinds.contains(&candidate),
        _ => false,
    }
}

fn uuid_matches(
    operator: CollectionRuleOperator,
    value: &CollectionRuleValue,
    candidate: Uuid,
) -> bool {
    match (operator, value) {
        (CollectionRuleOperator::Equals, CollectionRuleValue::Uuid(value)) => {
            candidate == *value
        }
        (
            CollectionRuleOperator::NotEquals,
            CollectionRuleValue::Uuid(value),
        ) => candidate != *value,
        (CollectionRuleOperator::In, CollectionRuleValue::Uuids(values)) => {
            values.contains(&candidate)
        }
        (CollectionRuleOperator::NotIn, CollectionRuleValue::Uuids(values)) => {
            !values.contains(&candidate)
        }
        _ => false,
    }
}

fn text_matches(
    operator: CollectionRuleOperator,
    value: &CollectionRuleValue,
    candidate: Option<&str>,
) -> bool {
    if matches!(operator, CollectionRuleOperator::Exists) {
        return exists_matches(
            value,
            candidate.is_some_and(|value| !value.trim().is_empty()),
        );
    }
    let Some(candidate) = candidate
        .map(normalize_text)
        .filter(|value| !value.is_empty())
    else {
        return false;
    };
    match (operator, value) {
        (
            CollectionRuleOperator::Equals,
            CollectionRuleValue::String(value),
        ) => candidate == normalize_text(value),
        (
            CollectionRuleOperator::NotEquals,
            CollectionRuleValue::String(value),
        ) => candidate != normalize_text(value),
        (
            CollectionRuleOperator::Contains,
            CollectionRuleValue::String(value),
        ) => candidate.contains(&normalize_text(value)),
        (
            CollectionRuleOperator::StartsWith,
            CollectionRuleValue::String(value),
        ) => candidate.starts_with(&normalize_text(value)),
        (CollectionRuleOperator::In, CollectionRuleValue::Strings(values)) => {
            normalized_value_set(values).contains(&candidate)
        }
        (
            CollectionRuleOperator::NotIn,
            CollectionRuleValue::Strings(values),
        ) => !normalized_value_set(values).contains(&candidate),
        _ => false,
    }
}

fn search_text_matches(
    operator: CollectionRuleOperator,
    value: &CollectionRuleValue,
    candidate: &DynamicCollectionCandidate,
) -> bool {
    let mut haystack =
        vec![candidate.title.clone(), candidate.sort_title.clone()];
    if let Some(overview) = &candidate.overview {
        haystack.push(overview.clone());
    }
    haystack.extend(candidate.genres.iter().cloned());
    haystack.extend(candidate.keywords.iter().cloned());
    haystack.extend(candidate.actor_names.iter().cloned());
    haystack.extend(candidate.director_names.iter().cloned());
    text_matches(operator, value, Some(&haystack.join(" ")))
}

fn text_set_matches(
    operator: CollectionRuleOperator,
    value: &CollectionRuleValue,
    candidates: &[String],
) -> bool {
    if matches!(operator, CollectionRuleOperator::Exists) {
        return exists_matches(value, !candidates.is_empty());
    }
    let normalized = candidates
        .iter()
        .map(|value| normalize_text(value))
        .collect::<HashSet<_>>();
    match (operator, value) {
        (
            CollectionRuleOperator::Equals,
            CollectionRuleValue::String(value),
        ) => normalized.contains(&normalize_text(value)),
        (
            CollectionRuleOperator::NotEquals,
            CollectionRuleValue::String(value),
        ) => !normalized.contains(&normalize_text(value)),
        (
            CollectionRuleOperator::Contains,
            CollectionRuleValue::String(value),
        ) => {
            let needle = normalize_text(value);
            normalized
                .iter()
                .any(|candidate| candidate.contains(&needle))
        }
        (
            CollectionRuleOperator::ContainsAny | CollectionRuleOperator::In,
            CollectionRuleValue::Strings(values),
        ) => normalized_value_set(values)
            .iter()
            .any(|value| normalized.contains(value)),
        (
            CollectionRuleOperator::ContainsAll,
            CollectionRuleValue::Strings(values),
        ) => normalized_value_set(values)
            .iter()
            .all(|value| normalized.contains(value)),
        (
            CollectionRuleOperator::NotIn,
            CollectionRuleValue::Strings(values),
        ) => normalized_value_set(values)
            .iter()
            .all(|value| !normalized.contains(value)),
        _ => false,
    }
}

fn normalized_value_set(values: &[String]) -> HashSet<String> {
    values
        .iter()
        .map(|value| normalize_text(value))
        .filter(|value| !value.is_empty())
        .collect()
}

fn person_matches(
    operator: CollectionRuleOperator,
    value: &CollectionRuleValue,
    candidate: &DynamicCollectionCandidate,
) -> bool {
    let CollectionRuleValue::Person(person) = value else {
        return false;
    };
    let (names, tmdb_ids): (Vec<&String>, Vec<i64>) = match person.role {
        CollectionPersonRole::Actor => (
            candidate.actor_names.iter().collect(),
            candidate.actor_tmdb_ids.clone(),
        ),
        CollectionPersonRole::Director => (
            candidate.director_names.iter().collect(),
            candidate.director_tmdb_ids.clone(),
        ),
        CollectionPersonRole::Any => {
            let mut names = candidate.actor_names.iter().collect::<Vec<_>>();
            names.extend(candidate.director_names.iter());
            let mut ids = candidate.actor_tmdb_ids.clone();
            ids.extend(candidate.director_tmdb_ids.iter().copied());
            (names, ids)
        }
        _ => return false,
    };

    let matched = person
        .tmdb_id
        .is_some_and(|tmdb_id| tmdb_ids.contains(&tmdb_id))
        || person.name.as_ref().is_some_and(|name| {
            let needle = normalize_text(name);
            names.iter().any(|candidate| candidate.contains(&needle))
        });
    match operator {
        CollectionRuleOperator::Equals | CollectionRuleOperator::Contains => {
            matched
        }
        CollectionRuleOperator::NotEquals => !matched,
        _ => false,
    }
}

fn integer_matches(
    operator: CollectionRuleOperator,
    value: &CollectionRuleValue,
    candidate: Option<i64>,
) -> bool {
    let Some(candidate) = candidate else {
        return false;
    };
    match (operator, value) {
        (
            CollectionRuleOperator::Equals,
            CollectionRuleValue::Integer(value),
        ) => candidate == *value,
        (
            CollectionRuleOperator::NotEquals,
            CollectionRuleValue::Integer(value),
        ) => candidate != *value,
        (
            CollectionRuleOperator::GreaterThan,
            CollectionRuleValue::Integer(value),
        ) => candidate > *value,
        (
            CollectionRuleOperator::GreaterThanOrEqual,
            CollectionRuleValue::Integer(value),
        ) => candidate >= *value,
        (
            CollectionRuleOperator::LessThan,
            CollectionRuleValue::Integer(value),
        ) => candidate < *value,
        (
            CollectionRuleOperator::LessThanOrEqual,
            CollectionRuleValue::Integer(value),
        ) => candidate <= *value,
        (CollectionRuleOperator::In, CollectionRuleValue::Integers(values)) => {
            values.contains(&candidate)
        }
        (
            CollectionRuleOperator::NotIn,
            CollectionRuleValue::Integers(values),
        ) => !values.contains(&candidate),
        (
            CollectionRuleOperator::Between,
            CollectionRuleValue::Integers(values),
        ) if values.len() == 2 => {
            candidate >= values[0] && candidate <= values[1]
        }
        _ => false,
    }
}

fn decimal_matches(
    operator: CollectionRuleOperator,
    value: &CollectionRuleValue,
    candidate: Option<f64>,
) -> bool {
    let Some(candidate) = candidate else {
        return false;
    };
    match (operator, value) {
        (
            CollectionRuleOperator::Equals,
            CollectionRuleValue::Decimal(value),
        ) => parse_decimal(value).is_some_and(|value| candidate == value),
        (
            CollectionRuleOperator::Equals,
            CollectionRuleValue::Integer(value),
        ) => candidate == *value as f64,
        (
            CollectionRuleOperator::NotEquals,
            CollectionRuleValue::Decimal(value),
        ) => parse_decimal(value).is_none_or(|value| candidate != value),
        (
            CollectionRuleOperator::NotEquals,
            CollectionRuleValue::Integer(value),
        ) => candidate != *value as f64,
        (
            CollectionRuleOperator::GreaterThan,
            CollectionRuleValue::Decimal(value),
        ) => parse_decimal(value).is_some_and(|value| candidate > value),
        (
            CollectionRuleOperator::GreaterThan,
            CollectionRuleValue::Integer(value),
        ) => candidate > *value as f64,
        (
            CollectionRuleOperator::GreaterThanOrEqual,
            CollectionRuleValue::Decimal(value),
        ) => parse_decimal(value).is_some_and(|value| candidate >= value),
        (
            CollectionRuleOperator::GreaterThanOrEqual,
            CollectionRuleValue::Integer(value),
        ) => candidate >= *value as f64,
        (
            CollectionRuleOperator::LessThan,
            CollectionRuleValue::Decimal(value),
        ) => parse_decimal(value).is_some_and(|value| candidate < value),
        (
            CollectionRuleOperator::LessThan,
            CollectionRuleValue::Integer(value),
        ) => candidate < *value as f64,
        (
            CollectionRuleOperator::LessThanOrEqual,
            CollectionRuleValue::Decimal(value),
        ) => parse_decimal(value).is_some_and(|value| candidate <= value),
        (
            CollectionRuleOperator::LessThanOrEqual,
            CollectionRuleValue::Integer(value),
        ) => candidate <= *value as f64,
        (CollectionRuleOperator::In, CollectionRuleValue::Decimals(values)) => {
            values
                .iter()
                .filter_map(|value| parse_decimal(value))
                .any(|value| candidate == value)
        }
        (
            CollectionRuleOperator::NotIn,
            CollectionRuleValue::Decimals(values),
        ) => values
            .iter()
            .filter_map(|value| parse_decimal(value))
            .all(|value| candidate != value),
        (
            CollectionRuleOperator::Between,
            CollectionRuleValue::Decimals(values),
        ) if values.len() == 2 => {
            match (parse_decimal(&values[0]), parse_decimal(&values[1])) {
                (Some(min), Some(max)) => candidate >= min && candidate <= max,
                _ => false,
            }
        }
        _ => false,
    }
}

fn parse_decimal(value: &str) -> Option<f64> {
    value
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
}

fn date_matches(
    operator: CollectionRuleOperator,
    value: &CollectionRuleValue,
    candidate: Option<NaiveDate>,
) -> bool {
    let Some(candidate) = candidate else {
        return false;
    };
    match (operator, value) {
        (CollectionRuleOperator::Equals, CollectionRuleValue::Date(value)) => {
            parse_rule_date(value).is_some_and(|value| candidate == value)
        }
        (
            CollectionRuleOperator::NotEquals,
            CollectionRuleValue::Date(value),
        ) => parse_rule_date(value).is_none_or(|value| candidate != value),
        (
            CollectionRuleOperator::GreaterThan,
            CollectionRuleValue::Date(value),
        ) => parse_rule_date(value).is_some_and(|value| candidate > value),
        (
            CollectionRuleOperator::GreaterThanOrEqual,
            CollectionRuleValue::Date(value),
        ) => parse_rule_date(value).is_some_and(|value| candidate >= value),
        (
            CollectionRuleOperator::LessThan,
            CollectionRuleValue::Date(value),
        ) => parse_rule_date(value).is_some_and(|value| candidate < value),
        (
            CollectionRuleOperator::LessThanOrEqual,
            CollectionRuleValue::Date(value),
        ) => parse_rule_date(value).is_some_and(|value| candidate <= value),
        (
            CollectionRuleOperator::Between,
            CollectionRuleValue::Dates(values),
        ) if values.len() == 2 => {
            match (parse_rule_date(&values[0]), parse_rule_date(&values[1])) {
                (Some(min), Some(max)) => candidate >= min && candidate <= max,
                _ => false,
            }
        }
        _ => false,
    }
}

fn datetime_as_date_matches(
    operator: CollectionRuleOperator,
    value: &CollectionRuleValue,
    candidate: Option<DateTime<Utc>>,
) -> bool {
    date_matches(operator, value, candidate.map(|value| value.date_naive()))
}

fn parse_rule_date(value: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d")
        .ok()
        .or_else(|| {
            DateTime::parse_from_rfc3339(value.trim())
                .ok()
                .map(|value| value.with_timezone(&Utc).date_naive())
        })
}

fn watch_status_matches(
    operator: CollectionRuleOperator,
    value: &CollectionRuleValue,
    candidate: &DynamicCollectionCandidate,
) -> bool {
    let CollectionRuleValue::WatchStatus(rule) = value else {
        return false;
    };
    let contains = rule.statuses.iter().any(|status| {
        *status == candidate.watch_status
            || (matches!(status, CollectionWatchStatus::Watched)
                && matches!(
                    candidate.watch_status,
                    CollectionWatchStatus::Completed
                ))
            || (matches!(status, CollectionWatchStatus::Completed)
                && matches!(
                    candidate.watch_status,
                    CollectionWatchStatus::Watched
                ))
    });
    match operator {
        CollectionRuleOperator::Equals | CollectionRuleOperator::In => contains,
        CollectionRuleOperator::NotEquals | CollectionRuleOperator::NotIn => {
            !contains
        }
        _ => false,
    }
}

fn watch_progress_matches(
    _operator: CollectionRuleOperator,
    value: &CollectionRuleValue,
    candidate: &DynamicCollectionCandidate,
) -> bool {
    let CollectionRuleValue::WatchProgress(rule) = value else {
        return false;
    };
    let Some(percent) = candidate.watch_progress_percent else {
        return false;
    };
    rule.min_percent.is_none_or(|min| percent >= f64::from(min))
        && rule.max_percent.is_none_or(|max| percent <= f64::from(max))
}

fn availability_matches(
    operator: CollectionRuleOperator,
    value: &CollectionRuleValue,
    candidate: CollectionMemberAvailabilityStatus,
) -> bool {
    let CollectionRuleValue::Availability(expected) = value else {
        return false;
    };
    match operator {
        CollectionRuleOperator::Equals => candidate == *expected,
        CollectionRuleOperator::NotEquals => candidate != *expected,
        _ => false,
    }
}

fn exists_matches(value: &CollectionRuleValue, exists: bool) -> bool {
    matches!(value, CollectionRuleValue::Boolean(expected) if exists == *expected)
}

fn apply_limit_window(
    candidates: &mut Vec<DynamicCollectionCandidate>,
    limit: &CollectionLimitPolicy,
) {
    if matches!(limit.window, CollectionLimitWindow::All) {
        return;
    }
    sort_for_window(candidates, limit.window);
    apply_limit(candidates, limit);
}

fn apply_limit(
    candidates: &mut Vec<DynamicCollectionCandidate>,
    limit: &CollectionLimitPolicy,
) {
    if limit.max_items.is_none() && limit.per_media_type.is_none() {
        return;
    }
    let max_items = limit.max_items.unwrap_or(u32::MAX);
    let per_media_type = limit.per_media_type.unwrap_or(u32::MAX);
    let mut per_kind: HashMap<CollectionMediaKind, u32> = HashMap::new();
    let mut kept = Vec::with_capacity(candidates.len());
    for candidate in candidates.drain(..) {
        if u32::try_from(kept.len()).unwrap_or(u32::MAX) >= max_items {
            break;
        }
        let count = per_kind.entry(candidate.media_type).or_default();
        if *count >= per_media_type {
            continue;
        }
        *count += 1;
        kept.push(candidate);
    }
    *candidates = kept;
}

fn sort_for_window(
    candidates: &mut [DynamicCollectionCandidate],
    window: CollectionLimitWindow,
) {
    candidates.sort_by(|left, right| {
        let ordering = match window {
            CollectionLimitWindow::All => Ordering::Equal,
            CollectionLimitWindow::Newest => compare_option_datetime(
                left.created_at,
                right.created_at,
                CollectionSortDirection::Desc,
                CollectionSortNulls::Last,
            ),
            CollectionLimitWindow::Oldest => compare_option_datetime(
                left.created_at,
                right.created_at,
                CollectionSortDirection::Asc,
                CollectionSortNulls::Last,
            ),
            CollectionLimitWindow::RecentlyAdded => compare_datetime(
                left.added_at,
                right.added_at,
                CollectionSortDirection::Desc,
            ),
            CollectionLimitWindow::RecentlyReleased => compare_option_date(
                left.release_date,
                right.release_date,
                CollectionSortDirection::Desc,
                CollectionSortNulls::Last,
            ),
            CollectionLimitWindow::RecentlyUpdated => compare_datetime(
                left.updated_at,
                right.updated_at,
                CollectionSortDirection::Desc,
            ),
        };
        ordering.then_with(|| stable_key_cmp(left, right))
    });
}

fn sort_candidates(
    candidates: &mut [DynamicCollectionCandidate],
    sort: &CollectionSortPolicy,
) {
    candidates.sort_by(|left, right| {
        for key in &sort.keys {
            let ordering = compare_sort_key(left, right, key);
            if ordering != Ordering::Equal {
                return ordering;
            }
        }
        compare_tie_breaker(left, right, sort.tie_breaker)
    });
}

fn compare_sort_key(
    left: &DynamicCollectionCandidate,
    right: &DynamicCollectionCandidate,
    key: &CollectionSortKey,
) -> Ordering {
    match key.field {
        CollectionSortField::RecentlyAdded | CollectionSortField::AddedAt => {
            compare_datetime(left.added_at, right.added_at, key.direction)
        }
        CollectionSortField::RecentlyReleased
        | CollectionSortField::ReleaseDate => compare_option_date(
            left.release_date,
            right.release_date,
            key.direction,
            key.nulls,
        ),
        CollectionSortField::Title | CollectionSortField::SortTitle => {
            compare_text(&left.sort_title, &right.sort_title, key.direction)
        }
        CollectionSortField::DiscoveredAt => compare_datetime(
            left.discovered_at,
            right.discovered_at,
            key.direction,
        ),
        CollectionSortField::CreatedAt => compare_option_datetime(
            left.created_at,
            right.created_at,
            key.direction,
            key.nulls,
        ),
        CollectionSortField::UpdatedAt => {
            compare_datetime(left.updated_at, right.updated_at, key.direction)
        }
        CollectionSortField::RuntimeMinutes => compare_option_i64(
            left.runtime_minutes.map(i64::from),
            right.runtime_minutes.map(i64::from),
            key.direction,
            key.nulls,
        ),
        CollectionSortField::AudienceRating
        | CollectionSortField::CriticRating
        | CollectionSortField::UserRating
        | CollectionSortField::Rating => compare_option_f64(
            left.rating,
            right.rating,
            key.direction,
            key.nulls,
        ),
        CollectionSortField::Popularity => compare_option_f64(
            left.popularity,
            right.popularity,
            key.direction,
            key.nulls,
        ),
        CollectionSortField::FileSizeBytes => compare_option_i64(
            left.file_size_bytes,
            right.file_size_bytes,
            key.direction,
            key.nulls,
        ),
        CollectionSortField::LastWatchedAt => compare_option_i64(
            left.last_watched,
            right.last_watched,
            key.direction,
            key.nulls,
        ),
        CollectionSortField::WatchProgress => compare_option_f64(
            left.watch_progress_percent,
            right.watch_progress_percent,
            key.direction,
            key.nulls,
        ),
        CollectionSortField::BitrateKbps
        | CollectionSortField::ResolutionWidth
        | CollectionSortField::ResolutionHeight
        | CollectionSortField::ManualPosition
        | CollectionSortField::RandomStable => Ordering::Equal,
    }
}

fn compare_tie_breaker(
    left: &DynamicCollectionCandidate,
    right: &DynamicCollectionCandidate,
    tie_breaker: CollectionSortTieBreaker,
) -> Ordering {
    match tie_breaker {
        CollectionSortTieBreaker::StableMediaKey => stable_key_cmp(left, right),
        CollectionSortTieBreaker::TitleThenStableKey
        | CollectionSortTieBreaker::ManualPositionThenStableKey => left
            .sort_title
            .cmp(&right.sort_title)
            .then_with(|| stable_key_cmp(left, right)),
    }
}

fn stable_key_cmp(
    left: &DynamicCollectionCandidate,
    right: &DynamicCollectionCandidate,
) -> Ordering {
    left.item_key.as_str().cmp(right.item_key.as_str())
}

fn compare_text(
    left: &str,
    right: &str,
    direction: CollectionSortDirection,
) -> Ordering {
    apply_direction(left.cmp(right), direction)
}

fn compare_datetime(
    left: DateTime<Utc>,
    right: DateTime<Utc>,
    direction: CollectionSortDirection,
) -> Ordering {
    apply_direction(left.cmp(&right), direction)
}

fn compare_option_datetime(
    left: Option<DateTime<Utc>>,
    right: Option<DateTime<Utc>>,
    direction: CollectionSortDirection,
    nulls: CollectionSortNulls,
) -> Ordering {
    compare_option(left, right, direction, nulls, |left, right| left.cmp(right))
}

fn compare_option_date(
    left: Option<NaiveDate>,
    right: Option<NaiveDate>,
    direction: CollectionSortDirection,
    nulls: CollectionSortNulls,
) -> Ordering {
    compare_option(left, right, direction, nulls, |left, right| left.cmp(right))
}

fn compare_option_i64(
    left: Option<i64>,
    right: Option<i64>,
    direction: CollectionSortDirection,
    nulls: CollectionSortNulls,
) -> Ordering {
    compare_option(left, right, direction, nulls, |left, right| left.cmp(right))
}

fn compare_option_f64(
    left: Option<f64>,
    right: Option<f64>,
    direction: CollectionSortDirection,
    nulls: CollectionSortNulls,
) -> Ordering {
    compare_option(
        left.map(OrderedFloat),
        right.map(OrderedFloat),
        direction,
        nulls,
        |left, right| left.cmp(right),
    )
}

fn compare_option<T>(
    left: Option<T>,
    right: Option<T>,
    direction: CollectionSortDirection,
    nulls: CollectionSortNulls,
    compare_some: impl FnOnce(&T, &T) -> Ordering,
) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => {
            apply_direction(compare_some(&left, &right), direction)
        }
        (None, Some(_)) => match nulls {
            CollectionSortNulls::First => Ordering::Less,
            CollectionSortNulls::Last => Ordering::Greater,
        },
        (Some(_), None) => match nulls {
            CollectionSortNulls::First => Ordering::Greater,
            CollectionSortNulls::Last => Ordering::Less,
        },
        (None, None) => Ordering::Equal,
    }
}

fn apply_direction(
    ordering: Ordering,
    direction: CollectionSortDirection,
) -> Ordering {
    match direction {
        CollectionSortDirection::Asc => ordering,
        CollectionSortDirection::Desc => ordering.reverse(),
    }
}

fn preview_response(
    evaluation: DynamicCollectionEvaluation,
    page: CollectionPagination,
    mode: CollectionReadMode,
) -> Result<PreviewCollectionRuleResponse> {
    let offset = parse_collection_cursor(page.cursor.as_deref())?;
    let limit = clamp_collection_page_limit(page.limit);
    let visible_items = filter_items_for_mode(evaluation.items, mode);
    let total = visible_items.len();
    let items = visible_items
        .into_iter()
        .skip(offset)
        .take(limit as usize)
        .map(|item| item.member)
        .collect();
    let now = Utc::now();
    Ok(PreviewCollectionRuleResponse {
        items,
        page: page_info_for_slice(offset, limit, total),
        materialization: CollectionMaterializationStatus {
            state: CollectionMaterializationState::Ready,
            item_count: evaluation.visible_count,
            total_count: evaluation.total_count,
            visible_count: evaluation.visible_count,
            rule_hash: Some(evaluation.rule_hash.clone()),
            generated_at: Some(now),
            ..CollectionMaterializationStatus::default()
        },
        rule_hash_input: evaluation.rule_hash_input,
        rule_hash: Some(evaluation.rule_hash),
    })
}

pub(super) fn filter_items_for_mode(
    items: Vec<DynamicCollectionEvaluatedItem>,
    mode: CollectionReadMode,
) -> Vec<DynamicCollectionEvaluatedItem> {
    items
        .into_iter()
        .filter(|item| mode.exposes_preserved_membership() || item.visible)
        .collect()
}

pub(super) fn page_info_for_materialized_slice(
    offset: usize,
    limit: u16,
    total: usize,
) -> CollectionPageInfo {
    page_info_for_slice(offset, limit, total)
}

fn format_rule_errors(
    errors: &[crate::api::types::collections::CollectionRuleValidationError],
) -> String {
    errors
        .iter()
        .map(|error| format!("{}: {}", error.path, error.message))
        .collect::<Vec<_>>()
        .join("; ")
}
