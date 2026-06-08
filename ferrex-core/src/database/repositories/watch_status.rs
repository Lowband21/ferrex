use crate::{
    database::repository_ports::watch_status::WatchStatusRepository,
    domain::watch::{
        ContinueWatchingActionHint, ContinueWatchingActionTarget,
        ContinueWatchingItem, InProgressItem, SeriesContinueWatchingItem,
        UpdateProgressRequest, UserWatchState, WatchResumePolicy,
    },
    error::{MediaError, Result},
    types::watch::{
        EpisodeKey, EpisodeStatus, NextEpisode, NextReason, SeasonKey,
        SeasonWatchStatus, SeriesWatchStatus,
    },
};

use async_trait::async_trait;
use chrono::Utc;
use ferrex_model::{LibraryId, VideoMediaType};
use sqlx::{PgPool, Row};
use std::collections::{HashMap, HashSet};
use tracing::info;
use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct PostgresWatchStatusRepository {
    pool: PgPool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LibrarySeriesContinueRow {
    series_id: Uuid,
    tmdb_series_id: u64,
    last_watched: i64,
}

impl PostgresWatchStatusRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    fn pool(&self) -> &PgPool {
        &self.pool
    }

    fn watch_policy() -> WatchResumePolicy {
        WatchResumePolicy::from_env()
    }

    fn is_completed_progress(position: f32, duration: f32) -> bool {
        Self::watch_policy().is_completed_progress(position, duration)
    }

    fn is_resume_eligible(position: f32, duration: f32) -> bool {
        Self::watch_policy().is_resume_eligible(position, duration)
    }

    fn parse_media_type_label(label: &str) -> Option<VideoMediaType> {
        match label.to_ascii_lowercase().as_str() {
            "movie" => Some(VideoMediaType::Movie),
            "series" => Some(VideoMediaType::Series),
            "season" => Some(VideoMediaType::Season),
            "episode" => Some(VideoMediaType::Episode),
            _ => None,
        }
    }

    async fn resolve_progress_target(
        &self,
        progress: &UpdateProgressRequest,
    ) -> Result<UpdateProgressRequest> {
        let Some(row) = sqlx::query(
            r#"
            SELECT media_id, media_type::text AS media_type
            FROM media_files
            WHERE id = $1
            "#,
        )
        .bind(progress.media_id)
        .fetch_optional(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to resolve playback media id: {}",
                e
            ))
        })?
        else {
            return Ok(progress.clone());
        };

        let logical_media_id =
            row.try_get::<Uuid, _>("media_id").map_err(|e| {
                MediaError::Internal(format!(
                    "Failed to decode logical watch media id: {}",
                    e
                ))
            })?;
        let resolved_media_type = row
            .try_get::<String, _>("media_type")
            .ok()
            .and_then(|value| Self::parse_media_type_label(&value))
            .ok_or_else(|| {
                MediaError::Internal(
                    "Resolved media file had an unsupported media_type"
                        .to_string(),
                )
            })?;

        if resolved_media_type != progress.media_type {
            return Err(MediaError::InvalidMedia(format!(
                "media_type {:?} did not match resolved playback target {:?}",
                progress.media_type, resolved_media_type
            )));
        }

        let mut resolved = progress.clone();
        resolved.media_id = logical_media_id;
        resolved.media_type = resolved_media_type;
        resolved.last_media_uuid =
            resolved.last_media_uuid.or(Some(progress.media_id));
        Ok(resolved)
    }

    fn format_episode_label(key: &EpisodeKey) -> String {
        format!("S{:02}E{:02}", key.season_number, key.episode_number)
    }

    fn format_remaining_label(seconds: f32) -> Option<String> {
        if seconds <= 0.0 {
            return None;
        }

        let total_seconds = seconds.ceil() as i64;
        if total_seconds >= 3600 {
            let hours = total_seconds / 3600;
            let minutes = (total_seconds % 3600 + 59) / 60;
            if minutes > 0 {
                Some(format!("{hours}h {minutes}m"))
            } else {
                Some(format!("{hours}h"))
            }
        } else if total_seconds >= 60 {
            Some(format!("{}m", (total_seconds + 59) / 60))
        } else {
            Some(format!("{total_seconds}s"))
        }
    }

    fn movie_action(media_id: Uuid) -> ContinueWatchingActionTarget {
        ContinueWatchingActionTarget {
            media_id,
            media_type: VideoMediaType::Movie,
        }
    }

    fn episode_action(media_id: Uuid) -> ContinueWatchingActionTarget {
        ContinueWatchingActionTarget {
            media_id,
            media_type: VideoMediaType::Episode,
        }
    }

    async fn load_movie_continue_watching_metadata(
        &self,
        media_id: Uuid,
    ) -> Result<Option<(Uuid, String, Option<Uuid>)>> {
        let row = sqlx::query(
            r#"
            SELECT
                mr.id AS logical_media_id,
                mr.title AS title,
                mm.primary_poster_image_id AS poster_iid
            FROM movie_references mr
            LEFT JOIN movie_metadata mm
                ON mm.movie_id = mr.id
            WHERE mr.id = $1

            UNION ALL

            SELECT
                mr.id AS logical_media_id,
                mr.title AS title,
                mm.primary_poster_image_id AS poster_iid
            FROM media_files mf
            JOIN movie_references mr
                ON mr.id = mf.media_id
            LEFT JOIN movie_metadata mm
                ON mm.movie_id = mr.id
            WHERE mf.id = $1 AND mf.media_type = 'movie'
            LIMIT 1
            "#,
        )
        .bind(media_id)
        .fetch_optional(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to load movie continue-watching metadata: {}",
                e
            ))
        })?;

        row.map(|row| {
            Ok((
                row.try_get::<Uuid, _>("logical_media_id").map_err(|e| {
                    MediaError::Internal(format!(
                        "Failed to decode movie logical id: {}",
                        e
                    ))
                })?,
                row.try_get::<String, _>("title").map_err(|e| {
                    MediaError::Internal(format!(
                        "Failed to decode movie title: {}",
                        e
                    ))
                })?,
                row.try_get::<Option<Uuid>, _>("poster_iid").map_err(|e| {
                    MediaError::Internal(format!(
                        "Failed to decode movie poster iid: {}",
                        e
                    ))
                })?,
            ))
        })
        .transpose()
    }

    async fn build_series_continue_watching_item(
        &self,
        user_id: Uuid,
        tmdb_series_id: u64,
        last_watched: i64,
    ) -> Result<Option<ContinueWatchingItem>> {
        let policy = Self::watch_policy();
        if let Some(row) = sqlx::query(
            r#"
            SELECT
                er.id AS media_id,
                er.series_id AS card_media_id,
                ues.season_number,
                ues.episode_number,
                ues.position,
                ues.duration,
                COALESCE(sm.name, s.title) AS title,
                sm.primary_poster_image_id AS poster_iid
            FROM user_episode_state ues
            JOIN episode_references er
                ON er.tmdb_series_id = ues.tmdb_series_id
               AND er.season_number = ues.season_number
               AND er.episode_number = ues.episode_number
            JOIN series s
                ON s.id = er.series_id
            LEFT JOIN series_metadata sm
                ON sm.series_id = er.series_id
            WHERE ues.user_id = $1
              AND ues.tmdb_series_id = $2
              AND ues.position >= $3
              AND ues.duration > 0
              AND (ues.position / ues.duration) >= $4
              AND (ues.duration - ues.position) >= $5
              AND ues.is_completed = false
              AND (ues.position / ues.duration) < $6
            ORDER BY ues.last_watched DESC, er.discovered_at ASC, er.id ASC
            LIMIT 1
            "#,
        )
        .bind(user_id)
        .bind(tmdb_series_id as i64)
        .bind(policy.resume_min_position_seconds)
        .bind(policy.resume_min_progress_ratio)
        .bind(policy.resume_min_remaining_seconds)
        .bind(policy.completion_threshold)
        .fetch_optional(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to resolve series resume target: {}",
                e
            ))
        })? {
            let media_id = row.try_get::<Uuid, _>("media_id").map_err(|e| {
                MediaError::Internal(format!(
                    "Failed to decode series resume media id: {}",
                    e
                ))
            })?;
            let card_media_id =
                row.try_get::<Uuid, _>("card_media_id").map_err(|e| {
                    MediaError::Internal(format!(
                        "Failed to decode series card media id: {}",
                        e
                    ))
                })?;
            let season_number =
                row.try_get::<i16, _>("season_number").map_err(|e| {
                    MediaError::Internal(format!(
                        "Failed to decode series resume season: {}",
                        e
                    ))
                })? as u16;
            let episode_number =
                row.try_get::<i16, _>("episode_number").map_err(|e| {
                    MediaError::Internal(format!(
                        "Failed to decode series resume episode: {}",
                        e
                    ))
                })? as u16;
            let position = row.try_get::<f32, _>("position").map_err(|e| {
                MediaError::Internal(format!(
                    "Failed to decode series resume position: {}",
                    e
                ))
            })?;
            let duration = row.try_get::<f32, _>("duration").map_err(|e| {
                MediaError::Internal(format!(
                    "Failed to decode series resume duration: {}",
                    e
                ))
            })?;
            let title = row.try_get::<String, _>("title").map_err(|e| {
                MediaError::Internal(format!(
                    "Failed to decode series resume title: {}",
                    e
                ))
            })?;
            let poster_iid =
                row.try_get::<Option<Uuid>, _>("poster_iid").map_err(|e| {
                    MediaError::Internal(format!(
                        "Failed to decode series poster iid: {}",
                        e
                    ))
                })?;
            let key = EpisodeKey {
                tmdb_series_id,
                season_number,
                episode_number,
            };
            let label = Self::format_episode_label(&key);
            let subtitle = Self::format_remaining_label(duration - position)
                .map(|remaining| format!("Resume {label} • {remaining} left"))
                .or_else(|| Some(format!("Resume {label}")));

            return Ok(Some(ContinueWatchingItem {
                media_id,
                media_type: VideoMediaType::Series,
                card_media_id,
                action_target: Self::episode_action(media_id),
                action_hint: ContinueWatchingActionHint::Resume,
                position,
                duration,
                last_watched,
                title,
                subtitle,
                poster_iid,
            }));
        }

        let has_completed = sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS(
                SELECT 1
                FROM user_episode_state
                WHERE user_id = $1
                  AND tmdb_series_id = $2
                  AND is_completed = true
            )
            "#,
        )
        .bind(user_id)
        .bind(tmdb_series_id as i64)
        .fetch_one(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to inspect completed series history: {}",
                e
            ))
        })?;

        if !has_completed {
            return Ok(None);
        }

        let next_row = sqlx::query(
            r#"
            SELECT
                er.id AS media_id,
                er.series_id AS card_media_id,
                er.season_number,
                er.episode_number,
                COALESCE(sm.name, s.title) AS title,
                sm.primary_poster_image_id AS poster_iid
            FROM episode_references er
            JOIN series s
                ON s.id = er.series_id
            LEFT JOIN series_metadata sm
                ON sm.series_id = er.series_id
            LEFT JOIN user_episode_state ues
                ON ues.user_id = $1
               AND ues.tmdb_series_id = er.tmdb_series_id
               AND ues.season_number = er.season_number
               AND ues.episode_number = er.episode_number
            WHERE er.tmdb_series_id = $2
              AND (ues.is_completed IS NULL OR ues.is_completed = false)
            ORDER BY er.season_number ASC,
                     er.episode_number ASC,
                     er.discovered_at ASC,
                     er.id ASC
            LIMIT 1
            "#,
        )
        .bind(user_id)
        .bind(tmdb_series_id as i64)
        .fetch_optional(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to resolve series next-episode target: {}",
                e
            ))
        })?;

        let Some(row) = next_row else {
            return Ok(None);
        };

        let media_id = row.try_get::<Uuid, _>("media_id").map_err(|e| {
            MediaError::Internal(format!(
                "Failed to decode next-episode media id: {}",
                e
            ))
        })?;
        let card_media_id =
            row.try_get::<Uuid, _>("card_media_id").map_err(|e| {
                MediaError::Internal(format!(
                    "Failed to decode next-episode card media id: {}",
                    e
                ))
            })?;
        let season_number =
            row.try_get::<i16, _>("season_number").map_err(|e| {
                MediaError::Internal(format!(
                    "Failed to decode next-episode season: {}",
                    e
                ))
            })? as u16;
        let episode_number =
            row.try_get::<i16, _>("episode_number").map_err(|e| {
                MediaError::Internal(format!(
                    "Failed to decode next-episode episode: {}",
                    e
                ))
            })? as u16;
        let title = row.try_get::<String, _>("title").map_err(|e| {
            MediaError::Internal(format!(
                "Failed to decode next-episode title: {}",
                e
            ))
        })?;
        let poster_iid =
            row.try_get::<Option<Uuid>, _>("poster_iid").map_err(|e| {
                MediaError::Internal(format!(
                    "Failed to decode next-episode poster iid: {}",
                    e
                ))
            })?;
        let key = EpisodeKey {
            tmdb_series_id,
            season_number,
            episode_number,
        };

        Ok(Some(ContinueWatchingItem {
            media_id,
            media_type: VideoMediaType::Series,
            card_media_id,
            action_target: Self::episode_action(media_id),
            action_hint: ContinueWatchingActionHint::NextEpisode,
            position: 0.0,
            duration: 0.0,
            last_watched,
            title,
            subtitle: Some(format!(
                "Next up: {}",
                Self::format_episode_label(&key)
            )),
            poster_iid,
        }))
    }

    fn sort_series_continue_watching_items(
        items: &mut [SeriesContinueWatchingItem],
    ) {
        items.sort_by(|a, b| {
            b.last_watched
                .cmp(&a.last_watched)
                .then_with(|| compare_series_continue_titles(a, b))
                .then_with(|| a.series_id.cmp(&b.series_id))
        });
    }

    async fn load_library_series_continue_rows(
        &self,
        user_id: Uuid,
        library_id: LibraryId,
    ) -> Result<Vec<LibrarySeriesContinueRow>> {
        let policy = Self::watch_policy();
        let rows = sqlx::query(
            r#"
            SELECT
                s.id AS series_id,
                er.tmdb_series_id AS tmdb_series_id,
                MAX(ues.last_watched) FILTER (
                    WHERE ues.is_completed = true
                       OR (ues.duration > 0 AND (ues.position / ues.duration) >= $6)
                       OR (
                              ues.position >= $3
                          AND ues.duration > 0
                          AND (ues.position / ues.duration) >= $4
                          AND (ues.duration - ues.position) >= $5
                          AND (ues.position / ues.duration) < $6
                       )
                ) AS last_watched,
                COALESCE(sm.name, s.title) AS title
            FROM user_episode_state ues
            JOIN episode_references er
                ON er.tmdb_series_id = ues.tmdb_series_id
               AND er.season_number = ues.season_number
               AND er.episode_number = ues.episode_number
            JOIN series s
                ON s.id = er.series_id
            LEFT JOIN series_metadata sm
                ON sm.series_id = s.id
            WHERE ues.user_id = $1
              AND s.library_id = $2
            GROUP BY s.id, er.tmdb_series_id, COALESCE(sm.name, s.title)
            HAVING MAX(ues.last_watched) FILTER (
                    WHERE ues.is_completed = true
                       OR (ues.duration > 0 AND (ues.position / ues.duration) >= $6)
                       OR (
                              ues.position >= $3
                          AND ues.duration > 0
                          AND (ues.position / ues.duration) >= $4
                          AND (ues.duration - ues.position) >= $5
                          AND (ues.position / ues.duration) < $6
                       )
                ) IS NOT NULL
            ORDER BY last_watched DESC,
                     LOWER(COALESCE(sm.name, s.title)) ASC,
                     s.id ASC
            "#,
        )
        .bind(user_id)
        .bind(library_id.to_uuid())
        .bind(policy.resume_min_position_seconds)
        .bind(policy.resume_min_progress_ratio)
        .bind(policy.resume_min_remaining_seconds)
        .bind(policy.completion_threshold)
        .fetch_all(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to get library series continue rows: {}",
                e
            ))
        })?;

        rows.into_iter()
            .map(|row| {
                let series_id =
                    row.try_get::<Uuid, _>("series_id").map_err(|e| {
                        MediaError::Internal(format!(
                            "Failed to decode library series continue id: {}",
                            e
                        ))
                    })?;
                let tmdb_series_id = row
                    .try_get::<i64, _>("tmdb_series_id")
                    .map_err(|e| {
                        MediaError::Internal(format!(
                            "Failed to decode library series continue TMDB id: {}",
                            e
                        ))
                    })? as u64;
                let last_watched = row
                    .try_get::<i64, _>("last_watched")
                    .map_err(|e| {
                        MediaError::Internal(format!(
                            "Failed to decode library series continue timestamp: {}",
                            e
                        ))
                    })?;

                Ok(LibrarySeriesContinueRow {
                    series_id,
                    tmdb_series_id,
                    last_watched,
                })
            })
            .collect()
    }

    async fn build_library_series_continue_watching_item(
        &self,
        user_id: Uuid,
        library_id: LibraryId,
        candidate: LibrarySeriesContinueRow,
    ) -> Result<Option<SeriesContinueWatchingItem>> {
        let policy = Self::watch_policy();
        if let Some(row) = sqlx::query(
            r#"
            SELECT
                er.id AS media_id,
                er.season_number,
                er.episode_number,
                ues.position,
                ues.duration,
                COALESCE(sm.name, s.title) AS title,
                sm.primary_poster_image_id AS poster_iid
            FROM user_episode_state ues
            JOIN episode_references er
                ON er.tmdb_series_id = ues.tmdb_series_id
               AND er.season_number = ues.season_number
               AND er.episode_number = ues.episode_number
            JOIN series s
                ON s.id = er.series_id
            LEFT JOIN series_metadata sm
                ON sm.series_id = s.id
            WHERE ues.user_id = $1
              AND s.library_id = $2
              AND er.series_id = $3
              AND ues.tmdb_series_id = $4
              AND ues.position >= $5
              AND ues.duration > 0
              AND (ues.position / ues.duration) >= $6
              AND (ues.duration - ues.position) >= $7
              AND ues.is_completed = false
              AND (ues.position / ues.duration) < $8
            ORDER BY ues.last_watched DESC, er.discovered_at ASC, er.id ASC
            LIMIT 1
            "#,
        )
        .bind(user_id)
        .bind(library_id.to_uuid())
        .bind(candidate.series_id)
        .bind(candidate.tmdb_series_id as i64)
        .bind(policy.resume_min_position_seconds)
        .bind(policy.resume_min_progress_ratio)
        .bind(policy.resume_min_remaining_seconds)
        .bind(policy.completion_threshold)
        .fetch_optional(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to resolve library series resume target: {}",
                e
            ))
        })? {
            let media_id = row.try_get::<Uuid, _>("media_id").map_err(|e| {
                MediaError::Internal(format!(
                    "Failed to decode library series resume media id: {}",
                    e
                ))
            })?;
            let season_number =
                row.try_get::<i16, _>("season_number").map_err(|e| {
                    MediaError::Internal(format!(
                        "Failed to decode library series resume season: {}",
                        e
                    ))
                })? as u16;
            let episode_number =
                row.try_get::<i16, _>("episode_number").map_err(|e| {
                    MediaError::Internal(format!(
                        "Failed to decode library series resume episode: {}",
                        e
                    ))
                })? as u16;
            let position = row.try_get::<f32, _>("position").map_err(|e| {
                MediaError::Internal(format!(
                    "Failed to decode library series resume position: {}",
                    e
                ))
            })?;
            let duration = row.try_get::<f32, _>("duration").map_err(|e| {
                MediaError::Internal(format!(
                    "Failed to decode library series resume duration: {}",
                    e
                ))
            })?;
            let title = row.try_get::<String, _>("title").map_err(|e| {
                MediaError::Internal(format!(
                    "Failed to decode library series resume title: {}",
                    e
                ))
            })?;
            let poster_iid =
                row.try_get::<Option<Uuid>, _>("poster_iid").map_err(|e| {
                    MediaError::Internal(format!(
                        "Failed to decode library series poster iid: {}",
                        e
                    ))
                })?;
            let key = EpisodeKey {
                tmdb_series_id: candidate.tmdb_series_id,
                season_number,
                episode_number,
            };
            let label = Self::format_episode_label(&key);
            let subtitle = Self::format_remaining_label(duration - position)
                .map(|remaining| format!("Resume {label} • {remaining} left"))
                .or_else(|| Some(format!("Resume {label}")));

            return Ok(Some(SeriesContinueWatchingItem {
                series_id: candidate.series_id,
                library_id,
                action_episode_id: Some(media_id),
                action_hint: ContinueWatchingActionHint::Resume,
                position,
                duration,
                last_watched: candidate.last_watched,
                title: Some(title),
                subtitle,
                poster_iid,
            }));
        }

        let has_completed = sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS(
                SELECT 1
                FROM user_episode_state ues
                JOIN episode_references er
                    ON er.tmdb_series_id = ues.tmdb_series_id
                   AND er.season_number = ues.season_number
                   AND er.episode_number = ues.episode_number
                JOIN series s
                    ON s.id = er.series_id
                WHERE ues.user_id = $1
                  AND s.library_id = $2
                  AND er.series_id = $3
                  AND ues.tmdb_series_id = $4
                  AND ues.is_completed = true
            )
            "#,
        )
        .bind(user_id)
        .bind(library_id.to_uuid())
        .bind(candidate.series_id)
        .bind(candidate.tmdb_series_id as i64)
        .fetch_one(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to inspect library completed series history: {}",
                e
            ))
        })?;

        if !has_completed {
            return Ok(None);
        }

        let next_row = sqlx::query(
            r#"
            SELECT
                er.id AS media_id,
                er.season_number,
                er.episode_number,
                COALESCE(sm.name, s.title) AS title,
                sm.primary_poster_image_id AS poster_iid
            FROM episode_references er
            JOIN series s
                ON s.id = er.series_id
            LEFT JOIN series_metadata sm
                ON sm.series_id = s.id
            LEFT JOIN user_episode_state ues
                ON ues.user_id = $1
               AND ues.tmdb_series_id = er.tmdb_series_id
               AND ues.season_number = er.season_number
               AND ues.episode_number = er.episode_number
            WHERE s.library_id = $2
              AND er.series_id = $3
              AND er.tmdb_series_id = $4
              AND (ues.is_completed IS NULL OR ues.is_completed = false)
            ORDER BY er.season_number ASC,
                     er.episode_number ASC,
                     er.discovered_at ASC,
                     er.id ASC
            LIMIT 1
            "#,
        )
        .bind(user_id)
        .bind(library_id.to_uuid())
        .bind(candidate.series_id)
        .bind(candidate.tmdb_series_id as i64)
        .fetch_optional(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to resolve library series next-episode target: {}",
                e
            ))
        })?;

        let Some(row) = next_row else {
            return Ok(None);
        };

        let media_id = row.try_get::<Uuid, _>("media_id").map_err(|e| {
            MediaError::Internal(format!(
                "Failed to decode library next-episode media id: {}",
                e
            ))
        })?;
        let season_number =
            row.try_get::<i16, _>("season_number").map_err(|e| {
                MediaError::Internal(format!(
                    "Failed to decode library next-episode season: {}",
                    e
                ))
            })? as u16;
        let episode_number =
            row.try_get::<i16, _>("episode_number").map_err(|e| {
                MediaError::Internal(format!(
                    "Failed to decode library next-episode episode: {}",
                    e
                ))
            })? as u16;
        let title = row.try_get::<String, _>("title").map_err(|e| {
            MediaError::Internal(format!(
                "Failed to decode library next-episode title: {}",
                e
            ))
        })?;
        let poster_iid =
            row.try_get::<Option<Uuid>, _>("poster_iid").map_err(|e| {
                MediaError::Internal(format!(
                    "Failed to decode library next-episode poster iid: {}",
                    e
                ))
            })?;
        let key = EpisodeKey {
            tmdb_series_id: candidate.tmdb_series_id,
            season_number,
            episode_number,
        };

        Ok(Some(SeriesContinueWatchingItem {
            series_id: candidate.series_id,
            library_id,
            action_episode_id: Some(media_id),
            action_hint: ContinueWatchingActionHint::NextEpisode,
            position: 0.0,
            duration: 0.0,
            last_watched: candidate.last_watched,
            title: Some(title),
            subtitle: Some(format!(
                "Next up: {}",
                Self::format_episode_label(&key)
            )),
            poster_iid,
        }))
    }
}

fn compare_series_continue_titles(
    a: &SeriesContinueWatchingItem,
    b: &SeriesContinueWatchingItem,
) -> std::cmp::Ordering {
    let a_title = series_continue_title_key(a);
    let b_title = series_continue_title_key(b);

    a_title
        .to_lowercase()
        .cmp(&b_title.to_lowercase())
        .then_with(|| a_title.cmp(b_title))
}

fn series_continue_title_key(item: &SeriesContinueWatchingItem) -> &str {
    item.title.as_deref().unwrap_or("")
}

#[async_trait]
impl WatchStatusRepository for PostgresWatchStatusRepository {
    async fn update_watch_progress(
        &self,
        user_id: Uuid,
        progress: &UpdateProgressRequest,
    ) -> Result<()> {
        let progress = self.resolve_progress_target(progress).await?;
        let episode_key =
            if matches!(progress.media_type, VideoMediaType::Episode) {
                self.resolve_episode_key_for_media_id(progress.media_id)
                    .await?
                    .or(progress.episode)
            } else {
                None
            };
        let now = Utc::now().timestamp_millis();

        let mut tx = self.pool().begin().await.map_err(|e| {
            MediaError::Internal(format!("Failed to start transaction: {}", e))
        })?;

        if progress.position <= 0.0 {
            sqlx::query(
                r#"
                DELETE FROM user_watch_progress
                WHERE user_id = $1
                  AND (
                      media_uuid = $2
                      OR media_uuid IN (
                          SELECT id FROM media_files WHERE media_id = $2
                      )
                  )
                "#,
            )
            .bind(user_id)
            .bind(progress.media_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                MediaError::Internal(format!(
                    "Failed to clear zero-position progress: {}",
                    e
                ))
            })?;

            tx.commit().await.map_err(|e| {
                MediaError::Internal(format!(
                    "Failed to commit transaction: {}",
                    e
                ))
            })?;

            return Ok(());
        }

        // Update or insert watch progress
        sqlx::query!(
            r#"
            INSERT INTO user_watch_progress (
                user_id, media_uuid, media_type, position, duration, last_watched, updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $6)
            ON CONFLICT (user_id, media_uuid) DO UPDATE SET
                media_type = EXCLUDED.media_type,
                position = EXCLUDED.position,
                duration = EXCLUDED.duration,
                last_watched = EXCLUDED.last_watched,
                updated_at = EXCLUDED.updated_at
            "#,
            user_id,
            progress.media_id,
            progress.media_type as i16,
            progress.position,
            progress.duration,
            now
        )
            .execute(&mut *tx)
            .await
            .map_err(|e| MediaError::Internal(format!("Failed to update watch progress: {}", e)))?;

        // Check if we should mark as completed (default >=95% watched)
        let completion_ratio = progress.position / progress.duration;
        if Self::is_completed_progress(progress.position, progress.duration) {
            info!(
                "Media {} ({}) is {}% complete, marking as completed",
                progress.media_id,
                progress.media_type,
                (completion_ratio * 100.0) as i32
            );

            sqlx::query!(
                r#"
                INSERT INTO user_completed_media (user_id, media_uuid, media_type, completed_at)
                VALUES ($1, $2, $3, $4)
                ON CONFLICT (user_id, media_uuid) DO NOTHING
                "#,
                user_id,
                progress.media_id,
                progress.media_type as i16,
                now
            )
                .execute(&mut *tx)
                .await
                .map_err(|e| MediaError::Internal(format!("Failed to mark as completed: {}", e)))?;

            // Remove from in-progress, including stale legacy playback-id rows.
            sqlx::query(
                r#"
                DELETE FROM user_watch_progress
                WHERE user_id = $1
                  AND (
                      media_uuid = $2
                      OR media_uuid IN (
                          SELECT id FROM media_files WHERE media_id = $2
                      )
                  )
                "#,
            )
            .bind(user_id)
            .bind(progress.media_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                MediaError::Internal(format!(
                    "Failed to remove from in-progress: {}",
                    e
                ))
            })?;
        } else {
            // A below-threshold update represents active progress, so remove any
            // completed state for the same logical media or stale playback ids.
            sqlx::query(
                r#"
                DELETE FROM user_completed_media
                WHERE user_id = $1
                  AND (
                      media_uuid = $2
                      OR media_uuid IN (
                          SELECT id FROM media_files WHERE media_id = $2
                      )
                  )
                "#,
            )
            .bind(user_id)
            .bind(progress.media_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                MediaError::Internal(format!(
                    "Failed to clear completed state for active progress: {}",
                    e
                ))
            })?;
        }

        // For episodes, also upsert identity-based state.
        if matches!(progress.media_type, VideoMediaType::Episode) {
            if let Some(key) = episode_key {
                let is_completed = Self::is_completed_progress(
                    progress.position,
                    progress.duration,
                );
                sqlx::query!(
                    r#"
                    INSERT INTO user_episode_state (
                        user_id, tmdb_series_id, season_number, episode_number,
                        position, duration, last_watched, is_completed, last_media_uuid
                    ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
                    ON CONFLICT (user_id, tmdb_series_id, season_number, episode_number)
                    DO UPDATE SET
                        position = EXCLUDED.position,
                        duration = EXCLUDED.duration,
                        last_watched = EXCLUDED.last_watched,
                        is_completed = EXCLUDED.is_completed,
                        last_media_uuid = COALESCE(EXCLUDED.last_media_uuid, user_episode_state.last_media_uuid)
                    "#,
                    user_id,
                    key.tmdb_series_id as i64,
                    key.season_number as i16,
                    key.episode_number as i16,
                    progress.position,
                    progress.duration,
                    now,
                    is_completed,
                    progress.last_media_uuid.unwrap_or(progress.media_id)
                )
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| MediaError::Internal(format!("Failed to upsert episode identity state: {}", e)))?;
            }
        }

        tx.commit().await.map_err(|e| {
            MediaError::Internal(format!("Failed to commit transaction: {}", e))
        })?;

        Ok(())
    }

    async fn get_user_watch_state(
        &self,
        user_id: Uuid,
    ) -> Result<UserWatchState> {
        // Get in-progress items
        let progress_rows = sqlx::query!(
            r#"
            SELECT media_uuid, position, duration, last_watched
            FROM user_watch_progress
            WHERE user_id = $1
            ORDER BY last_watched DESC
            "#,
            user_id
        )
        .fetch_all(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Failed to get watch progress: {}", e))
        })?;

        let mut in_progress = HashMap::new();
        for row in progress_rows {
            in_progress.insert(
                row.media_uuid,
                InProgressItem {
                    media_id: row.media_uuid,
                    position: row.position,
                    duration: row.duration,
                    last_watched: row.last_watched,
                },
            );
        }

        // Get completed items
        let completed_rows = sqlx::query!(
            r#"
            SELECT media_uuid
            FROM user_completed_media
            WHERE user_id = $1
            "#,
            user_id
        )
        .fetch_all(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to get completed media: {}",
                e
            ))
        })?;

        let mut completed = HashSet::new();
        for row in completed_rows {
            completed.insert(row.media_uuid);
        }

        info!(
            "User {} has {} in-progress and {} completed items",
            user_id,
            in_progress.len(),
            completed.len()
        );

        Ok(UserWatchState {
            in_progress,
            completed,
        })
    }

    async fn get_continue_watching(
        &self,
        user_id: Uuid,
        limit: usize,
    ) -> Result<Vec<ContinueWatchingItem>> {
        let movie_rows = sqlx::query(
            r#"
            SELECT media_uuid, position, duration, last_watched
            FROM user_watch_progress
            WHERE user_id = $1 AND media_type = $2
            ORDER BY last_watched DESC
            "#,
        )
        .bind(user_id)
        .bind(VideoMediaType::Movie as i16)
        .fetch_all(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to get movie continue watching rows: {}",
                e
            ))
        })?;

        let mut items = Vec::new();
        for row in movie_rows {
            let media_id =
                row.try_get::<Uuid, _>("media_uuid").map_err(|e| {
                    MediaError::Internal(format!(
                        "Failed to decode continue-watching movie id: {}",
                        e
                    ))
                })?;
            let position = row.try_get::<f32, _>("position").map_err(|e| {
                MediaError::Internal(format!(
                    "Failed to decode continue-watching movie position: {}",
                    e
                ))
            })?;
            let duration = row.try_get::<f32, _>("duration").map_err(|e| {
                MediaError::Internal(format!(
                    "Failed to decode continue-watching movie duration: {}",
                    e
                ))
            })?;
            let last_watched =
                row.try_get::<i64, _>("last_watched").map_err(|e| {
                    MediaError::Internal(format!(
                        "Failed to decode continue-watching movie timestamp: {}",
                        e
                    ))
                })?;

            if !Self::is_resume_eligible(position, duration) {
                continue;
            }

            let Some((logical_media_id, title, poster_iid)) =
                self.load_movie_continue_watching_metadata(media_id).await?
            else {
                continue;
            };

            let subtitle = Self::format_remaining_label(duration - position)
                .map(|remaining| format!("Resume • {remaining} left"));

            items.push(ContinueWatchingItem {
                media_id: logical_media_id,
                media_type: VideoMediaType::Movie,
                card_media_id: logical_media_id,
                action_target: Self::movie_action(logical_media_id),
                action_hint: ContinueWatchingActionHint::Resume,
                position,
                duration,
                last_watched,
                title,
                subtitle,
                poster_iid,
            });
        }

        let series_rows = sqlx::query(
            r#"
            SELECT tmdb_series_id, MAX(last_watched) AS last_watched
            FROM user_episode_state
            WHERE user_id = $1
            GROUP BY tmdb_series_id
            ORDER BY MAX(last_watched) DESC
            "#,
        )
        .bind(user_id)
        .fetch_all(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to get series continue watching rows: {}",
                e
            ))
        })?;

        for row in series_rows {
            let tmdb_series_id =
                row.try_get::<i64, _>("tmdb_series_id").map_err(|e| {
                    MediaError::Internal(format!(
                        "Failed to decode continue-watching series id: {}",
                        e
                    ))
                })? as u64;
            let last_watched =
                row.try_get::<i64, _>("last_watched").map_err(|e| {
                    MediaError::Internal(format!(
                        "Failed to decode continue-watching series timestamp: {}",
                        e
                    ))
                })?;

            if let Some(item) = self
                .build_series_continue_watching_item(
                    user_id,
                    tmdb_series_id,
                    last_watched,
                )
                .await?
            {
                items.push(item);
            }
        }

        items.sort_by(|a, b| b.last_watched.cmp(&a.last_watched));
        items.truncate(limit);

        Ok(items)
    }

    async fn get_library_series_continue_watching(
        &self,
        user_id: Uuid,
        library_id: LibraryId,
        limit: usize,
    ) -> Result<Vec<SeriesContinueWatchingItem>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let candidates = self
            .load_library_series_continue_rows(user_id, library_id)
            .await?;
        let mut items = Vec::new();

        for candidate in candidates {
            if let Some(item) = self
                .build_library_series_continue_watching_item(
                    user_id, library_id, candidate,
                )
                .await?
            {
                items.push(item);
            }
        }

        Self::sort_series_continue_watching_items(&mut items);
        items.truncate(limit);

        Ok(items)
    }

    async fn list_library_series_ids_with_meaningful_watch_state(
        &self,
        user_id: Uuid,
        library_id: LibraryId,
    ) -> Result<Vec<Uuid>> {
        let policy = Self::watch_policy();
        let rows = sqlx::query(
            r#"
            SELECT DISTINCT watched.series_id
            FROM (
                SELECT s.id AS series_id
                FROM series s
                JOIN episode_references er
                    ON er.series_id = s.id
                JOIN user_episode_state ues
                    ON ues.tmdb_series_id = er.tmdb_series_id
                   AND ues.season_number = er.season_number
                   AND ues.episode_number = er.episode_number
                WHERE ues.user_id = $1
                  AND s.library_id = $2
                  AND (
                        ues.is_completed = true
                     OR (ues.duration > 0 AND (ues.position / ues.duration) >= $3)
                     OR (
                            ues.position >= $4
                        AND ues.duration > 0
                        AND (ues.position / ues.duration) >= $5
                        AND (ues.duration - ues.position) >= $6
                        AND (ues.position / ues.duration) < $3
                     )
                  )

                UNION

                SELECT s.id AS series_id
                FROM series s
                JOIN episode_references er
                    ON er.series_id = s.id
                JOIN user_watch_progress uwp
                    ON uwp.media_uuid = er.id
                WHERE uwp.user_id = $1
                  AND s.library_id = $2
                  AND uwp.media_type = $7
                  AND uwp.duration > 0
                  AND (
                        (uwp.position / uwp.duration) >= $3
                     OR (
                            uwp.position >= $4
                        AND (uwp.position / uwp.duration) >= $5
                        AND (uwp.duration - uwp.position) >= $6
                        AND (uwp.position / uwp.duration) < $3
                     )
                  )

                UNION

                SELECT s.id AS series_id
                FROM series s
                JOIN episode_references er
                    ON er.series_id = s.id
                JOIN user_completed_media ucm
                    ON ucm.media_uuid = er.id
                WHERE ucm.user_id = $1
                  AND s.library_id = $2
                  AND ucm.media_type = $7
            ) watched
            ORDER BY watched.series_id
            "#,
        )
        .bind(user_id)
        .bind(library_id.to_uuid())
        .bind(policy.completion_threshold)
        .bind(policy.resume_min_position_seconds)
        .bind(policy.resume_min_progress_ratio)
        .bind(policy.resume_min_remaining_seconds)
        .bind(VideoMediaType::Episode as i16)
        .fetch_all(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to list library series watch-state ids: {}",
                e
            ))
        })?;

        rows.into_iter()
            .map(|row| {
                row.try_get::<Uuid, _>("series_id").map_err(|e| {
                    MediaError::Internal(format!(
                        "Failed to decode watched series id: {}",
                        e
                    ))
                })
            })
            .collect()
    }

    async fn clear_watch_progress(
        &self,
        user_id: Uuid,
        media_id: &Uuid,
    ) -> Result<()> {
        let episode_key =
            self.resolve_episode_key_for_media_id(*media_id).await?;
        let mut tx = self.pool().begin().await.map_err(|e| {
            MediaError::Internal(format!("Failed to start transaction: {}", e))
        })?;

        // Remove progress rows for the logical media id and stale playback ids.
        let progress_result = sqlx::query(
            r#"
            DELETE FROM user_watch_progress
            WHERE user_id = $1
              AND (
                  media_uuid = $2
                  OR media_uuid IN (
                      SELECT id FROM media_files WHERE media_id = $2
                  )
              )
            "#,
        )
        .bind(user_id)
        .bind(*media_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to clear watch progress: {}",
                e
            ))
        })?;

        // Remove completed rows for the logical media id and stale playback ids.
        let completed_result = sqlx::query(
            r#"
            DELETE FROM user_completed_media
            WHERE user_id = $1
              AND (
                  media_uuid = $2
                  OR media_uuid IN (
                      SELECT id FROM media_files WHERE media_id = $2
                  )
              )
            "#,
        )
        .bind(user_id)
        .bind(*media_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to clear completed status: {}",
                e
            ))
        })?;

        if let Some(key) = episode_key {
            sqlx::query(
                r#"
                DELETE FROM user_episode_state
                WHERE user_id = $1
                  AND tmdb_series_id = $2
                  AND season_number = $3
                  AND episode_number = $4
                "#,
            )
            .bind(user_id)
            .bind(key.tmdb_series_id as i64)
            .bind(key.season_number as i16)
            .bind(key.episode_number as i16)
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                MediaError::Internal(format!(
                    "Failed to clear episode identity state: {}",
                    e
                ))
            })?;
        }

        tx.commit().await.map_err(|e| {
            MediaError::Internal(format!("Failed to commit transaction: {}", e))
        })?;

        info!(
            "Cleared watch progress for user {} media {}: {} progress, {} completed removed",
            user_id,
            media_id,
            progress_result.rows_affected(),
            completed_result.rows_affected()
        );

        Ok(())
    }

    async fn is_media_completed(
        &self,
        user_id: Uuid,
        media_id: &Uuid,
    ) -> Result<bool> {
        let exists = sqlx::query!(
            r#"
            SELECT EXISTS(
                SELECT 1 FROM user_completed_media
                WHERE user_id = $1 AND media_uuid = $2
            ) as "exists!"
            "#,
            user_id,
            media_id
        )
        .fetch_one(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to check completion status: {}",
                e
            ))
        })?;

        Ok(exists.exists)
    }

    async fn mark_media_watched(
        &self,
        user_id: Uuid,
        media_id: Uuid,
        media_type: VideoMediaType,
        last_media_uuid: Option<Uuid>,
    ) -> Result<()> {
        let episode_key = if matches!(media_type, VideoMediaType::Episode) {
            Some(
                self.resolve_episode_key_for_media_id(media_id)
                    .await?
                    .ok_or_else(|| {
                        MediaError::Internal(format!(
                            "Failed to resolve episode identity for {}",
                            media_id
                        ))
                    })?,
            )
        } else {
            None
        };
        let now = Utc::now().timestamp_millis();
        let mut tx = self.pool().begin().await.map_err(|e| {
            MediaError::Internal(format!("Failed to start transaction: {}", e))
        })?;

        sqlx::query(
            r#"
            DELETE FROM user_watch_progress
            WHERE user_id = $1
              AND (
                  media_uuid = $2
                  OR media_uuid IN (
                      SELECT id FROM media_files WHERE media_id = $2
                  )
              )
            "#,
        )
        .bind(user_id)
        .bind(media_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to clear in-progress state before mark watched: {}",
                e
            ))
        })?;

        sqlx::query(
            r#"
            DELETE FROM user_completed_media
            WHERE user_id = $1
              AND media_uuid IN (
                  SELECT id FROM media_files WHERE media_id = $2
              )
            "#,
        )
        .bind(user_id)
        .bind(media_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to clear stale playback completed state: {}",
                e
            ))
        })?;

        sqlx::query(
            r#"
            INSERT INTO user_completed_media (user_id, media_uuid, media_type, completed_at)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (user_id, media_uuid)
            DO UPDATE SET
                media_type = EXCLUDED.media_type,
                completed_at = GREATEST(user_completed_media.completed_at, EXCLUDED.completed_at)
            "#,
        )
        .bind(user_id)
        .bind(media_id)
        .bind(media_type as i16)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to persist completed state: {}",
                e
            ))
        })?;

        if let Some(key) = episode_key {
            sqlx::query(
                r#"
                INSERT INTO user_episode_state (
                    user_id, tmdb_series_id, season_number, episode_number,
                    position, duration, last_watched, is_completed, last_media_uuid
                ) VALUES ($1,$2,$3,$4,1.0,1.0,$5,true,$6)
                ON CONFLICT (user_id, tmdb_series_id, season_number, episode_number)
                DO UPDATE SET
                    position = EXCLUDED.position,
                    duration = EXCLUDED.duration,
                    last_watched = GREATEST(user_episode_state.last_watched, EXCLUDED.last_watched),
                    is_completed = true,
                    last_media_uuid = COALESCE(EXCLUDED.last_media_uuid, user_episode_state.last_media_uuid)
                "#,
            )
            .bind(user_id)
            .bind(key.tmdb_series_id as i64)
            .bind(key.season_number as i16)
            .bind(key.episode_number as i16)
            .bind(now)
            .bind(last_media_uuid.or(Some(media_id)))
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                MediaError::Internal(format!(
                    "Failed to upsert explicit episode completed state: {}",
                    e
                ))
            })?;
        }

        tx.commit().await.map_err(|e| {
            MediaError::Internal(format!("Failed to commit transaction: {}", e))
        })?;

        Ok(())
    }

    async fn mark_media_unwatched(
        &self,
        user_id: Uuid,
        media_id: Uuid,
        media_type: VideoMediaType,
    ) -> Result<()> {
        let episode_key = if matches!(media_type, VideoMediaType::Episode) {
            self.resolve_episode_key_for_media_id(media_id).await?
        } else {
            None
        };
        let mut tx = self.pool().begin().await.map_err(|e| {
            MediaError::Internal(format!("Failed to start transaction: {}", e))
        })?;

        sqlx::query(
            r#"
            DELETE FROM user_watch_progress
            WHERE user_id = $1
              AND (
                  media_uuid = $2
                  OR media_uuid IN (
                      SELECT id FROM media_files WHERE media_id = $2
                  )
              )
            "#,
        )
        .bind(user_id)
        .bind(media_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to clear in-progress state: {}",
                e
            ))
        })?;

        sqlx::query(
            r#"
            DELETE FROM user_completed_media
            WHERE user_id = $1
              AND (
                  media_uuid = $2
                  OR media_uuid IN (
                      SELECT id FROM media_files WHERE media_id = $2
                  )
              )
            "#,
        )
        .bind(user_id)
        .bind(media_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to clear completed state: {}",
                e
            ))
        })?;

        if let Some(key) = episode_key {
            sqlx::query(
                r#"
                DELETE FROM user_episode_state
                WHERE user_id = $1 AND tmdb_series_id = $2 AND season_number = $3 AND episode_number = $4
                "#,
            )
            .bind(user_id)
            .bind(key.tmdb_series_id as i64)
            .bind(key.season_number as i16)
            .bind(key.episode_number as i16)
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                MediaError::Internal(format!(
                    "Failed to clear explicit episode state: {}",
                    e
                ))
            })?;
        }

        tx.commit().await.map_err(|e| {
            MediaError::Internal(format!("Failed to commit transaction: {}", e))
        })?;

        Ok(())
    }

    async fn mark_series_watched(
        &self,
        user_id: Uuid,
        tmdb_series_id: u64,
    ) -> Result<()> {
        let now = Utc::now().timestamp_millis();
        let mut tx = self.pool().begin().await.map_err(|e| {
            MediaError::Internal(format!("Failed to start transaction: {}", e))
        })?;

        sqlx::query(
            r#"
            DELETE FROM user_watch_progress
            WHERE user_id = $1
              AND (
                  media_uuid IN (
                      SELECT id FROM episode_references WHERE tmdb_series_id = $2
                  )
                  OR media_uuid IN (
                      SELECT mf.id
                      FROM media_files mf
                      JOIN episode_references er ON er.id = mf.media_id
                      WHERE er.tmdb_series_id = $2
                  )
              )
            "#,
        )
        .bind(user_id)
        .bind(tmdb_series_id as i64)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to clear series in-progress state: {}",
                e
            ))
        })?;

        sqlx::query(
            r#"
            DELETE FROM user_completed_media
            WHERE user_id = $1
              AND media_uuid IN (
                  SELECT mf.id
                  FROM media_files mf
                  JOIN episode_references er ON er.id = mf.media_id
                  WHERE er.tmdb_series_id = $2
              )
            "#,
        )
        .bind(user_id)
        .bind(tmdb_series_id as i64)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to clear stale series playback completed rows: {}",
                e
            ))
        })?;

        sqlx::query(
            r#"
            INSERT INTO user_completed_media (user_id, media_uuid, media_type, completed_at)
            SELECT $1, er.id, $3, $4
            FROM episode_references er
            WHERE er.tmdb_series_id = $2
            ON CONFLICT (user_id, media_uuid)
            DO UPDATE SET
                media_type = EXCLUDED.media_type,
                completed_at = GREATEST(user_completed_media.completed_at, EXCLUDED.completed_at)
            "#,
        )
        .bind(user_id)
        .bind(tmdb_series_id as i64)
        .bind(VideoMediaType::Episode as i16)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to mark series episodes completed: {}",
                e
            ))
        })?;

        sqlx::query(
            r#"
            INSERT INTO user_episode_state (
                user_id, tmdb_series_id, season_number, episode_number,
                position, duration, last_watched, is_completed, last_media_uuid
            )
            SELECT
                $1,
                er.tmdb_series_id,
                er.season_number,
                er.episode_number,
                1.0,
                1.0,
                $3,
                true,
                er.id
            FROM episode_references er
            WHERE er.tmdb_series_id = $2
            ON CONFLICT (user_id, tmdb_series_id, season_number, episode_number)
            DO UPDATE SET
                position = EXCLUDED.position,
                duration = EXCLUDED.duration,
                last_watched = GREATEST(user_episode_state.last_watched, EXCLUDED.last_watched),
                is_completed = true,
                last_media_uuid = COALESCE(EXCLUDED.last_media_uuid, user_episode_state.last_media_uuid)
            "#,
        )
        .bind(user_id)
        .bind(tmdb_series_id as i64)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to mark series identity state completed: {}",
                e
            ))
        })?;

        tx.commit().await.map_err(|e| {
            MediaError::Internal(format!("Failed to commit transaction: {}", e))
        })?;

        Ok(())
    }

    async fn mark_series_unwatched(
        &self,
        user_id: Uuid,
        tmdb_series_id: u64,
    ) -> Result<()> {
        let mut tx = self.pool().begin().await.map_err(|e| {
            MediaError::Internal(format!("Failed to start transaction: {}", e))
        })?;

        sqlx::query(
            r#"
            DELETE FROM user_episode_state
            WHERE user_id = $1 AND tmdb_series_id = $2
            "#,
        )
        .bind(user_id)
        .bind(tmdb_series_id as i64)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to clear series episode identity state: {}",
                e
            ))
        })?;

        sqlx::query(
            r#"
            DELETE FROM user_watch_progress
            WHERE user_id = $1
              AND (
                  media_uuid IN (
                      SELECT id FROM episode_references WHERE tmdb_series_id = $2
                  )
                  OR media_uuid IN (
                      SELECT mf.id
                      FROM media_files mf
                      JOIN episode_references er ON er.id = mf.media_id
                      WHERE er.tmdb_series_id = $2
                  )
              )
            "#,
        )
        .bind(user_id)
        .bind(tmdb_series_id as i64)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to clear series in-progress rows: {}",
                e
            ))
        })?;

        sqlx::query(
            r#"
            DELETE FROM user_completed_media
            WHERE user_id = $1
              AND (
                  media_uuid IN (
                      SELECT id FROM episode_references WHERE tmdb_series_id = $2
                  )
                  OR media_uuid IN (
                      SELECT mf.id
                      FROM media_files mf
                      JOIN episode_references er ON er.id = mf.media_id
                      WHERE er.tmdb_series_id = $2
                  )
              )
            "#,
        )
        .bind(user_id)
        .bind(tmdb_series_id as i64)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to clear series completed rows: {}",
                e
            ))
        })?;

        tx.commit().await.map_err(|e| {
            MediaError::Internal(format!("Failed to commit transaction: {}", e))
        })?;

        Ok(())
    }

    // ===== Identity-based Episode State =====

    async fn upsert_episode_identity_progress(
        &self,
        user_id: Uuid,
        key: &EpisodeKey,
        position: f32,
        duration: f32,
        last_media_uuid: Option<Uuid>,
    ) -> Result<()> {
        let now = Utc::now().timestamp_millis();
        let is_completed = Self::is_completed_progress(position, duration);
        sqlx::query!(
            r#"
            INSERT INTO user_episode_state (
                user_id, tmdb_series_id, season_number, episode_number,
                position, duration, last_watched, is_completed, last_media_uuid
            ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
            ON CONFLICT (user_id, tmdb_series_id, season_number, episode_number)
            DO UPDATE SET
                position = EXCLUDED.position,
                duration = EXCLUDED.duration,
                last_watched = EXCLUDED.last_watched,
                is_completed = EXCLUDED.is_completed,
                last_media_uuid = COALESCE(EXCLUDED.last_media_uuid, user_episode_state.last_media_uuid)
            "#,
            user_id,
            key.tmdb_series_id as i64,
            key.season_number as i16,
            key.episode_number as i16,
            position,
            duration,
            now,
            is_completed,
            last_media_uuid
        )
            .execute(self.pool())
            .await
            .map_err(|e| MediaError::Internal(format!("Failed to upsert episode identity: {}", e)))?;

        Ok(())
    }

    async fn get_series_watch_status(
        &self,
        user_id: Uuid,
        tmdb_series_id: u64,
    ) -> Result<SeriesWatchStatus> {
        use std::collections::HashMap;

        // Fetch catalog of episodes for this series
        let rows = sqlx::query!(
            r#"
            SELECT season_number, episode_number
            FROM episode_metadata
            WHERE series_tmdb_id = $1
            GROUP BY season_number, episode_number
            ORDER BY season_number, episode_number
            "#,
            tmdb_series_id as i64
        )
        .fetch_all(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!("Failed to list episodes: {}", e))
        })?;

        // Fetch user state for this series
        let state = sqlx::query!(
            r#"
            SELECT season_number, episode_number, position, duration, is_completed, last_watched, last_media_uuid
            FROM user_episode_state
            WHERE user_id = $1 AND tmdb_series_id = $2
            "#,
            user_id,
            tmdb_series_id as i64
        )
            .fetch_all(self.pool())
            .await
            .map_err(|e| MediaError::Internal(format!("Failed to fetch user episode state: {}", e)))?;

        let mut seasons: HashMap<u16, SeasonWatchStatus> = HashMap::new();
        let mut total = 0u32;
        let mut watched = 0u32;
        let mut in_progress = 0u32;

        // Index user state
        let mut state_map: HashMap<
            (i16, i16),
            (f32, f32, bool, i64, Option<Uuid>),
        > = HashMap::new();
        for r in state.into_iter() {
            state_map.insert(
                (r.season_number, r.episode_number),
                (
                    r.position,
                    r.duration,
                    r.is_completed,
                    r.last_watched,
                    r.last_media_uuid,
                ),
            );
        }

        // Determine next episode
        let mut best_in_progress: Option<(i64, EpisodeKey, Option<Uuid>)> =
            None;
        let mut first_unwatched: Option<EpisodeKey> = None;

        for r in rows.into_iter() {
            if let (Some(s), Some(e)) = (r.season_number, r.episode_number) {
                let s = s as u16;
                let e = e as u16;

                let key = EpisodeKey {
                    tmdb_series_id,
                    season_number: s,
                    episode_number: e,
                };
                total += 1;

                let entry =
                    seasons.entry(s).or_insert_with(|| SeasonWatchStatus {
                        key: SeasonKey {
                            tmdb_series_id,
                            season_number: s,
                        },
                        total: 0,
                        watched: 0,
                        in_progress: 0,
                        is_completed: false,
                        episodes: HashMap::new(),
                    });
                entry.total += 1;

                if let Some((pos, dur, done, last, last_media_uuid)) =
                    state_map.get(&(s as i16, e as i16)).copied()
                {
                    if done || Self::is_completed_progress(pos, dur) {
                        entry.episodes.insert(e, EpisodeStatus::Completed);
                        watched += 1;
                        entry.watched += 1;
                    } else if pos > 0.0 && dur > 0.0 {
                        let prog = (pos / dur).clamp(0.0, 1.0);
                        entry.episodes.insert(
                            e,
                            EpisodeStatus::InProgress { progress: prog },
                        );
                        in_progress += 1;
                        entry.in_progress += 1;
                        if Self::is_resume_eligible(pos, dur)
                            && best_in_progress
                                .map(|(best_last, _, _)| best_last)
                                .unwrap_or(0)
                                < last
                        {
                            best_in_progress =
                                Some((last, key, last_media_uuid));
                        }
                    } else {
                        entry.episodes.insert(e, EpisodeStatus::Unwatched);
                        if first_unwatched.is_none() {
                            first_unwatched = Some(key);
                        }
                    }
                } else {
                    entry.episodes.insert(e, EpisodeStatus::Unwatched);
                    if first_unwatched.is_none() {
                        first_unwatched = Some(key);
                    }
                }
            } else {
                continue;
            }
        }

        // mark season completions
        for season in seasons.values_mut() {
            season.is_completed =
                season.watched == season.total && season.total > 0;
        }

        // Decide next_episode
        let next_episode = if let Some((_, key, last_media)) = best_in_progress
        {
            let playable_media_id = if let Some(id) = last_media {
                Some(id)
            } else {
                self.lookup_playable_episode(&key).await?
            };
            Some(NextEpisode {
                key,
                playable_media_id,
                reason: NextReason::ResumeInProgress,
            })
        } else if let Some(key) = first_unwatched {
            let playable_media_id = self.lookup_playable_episode(&key).await?;
            Some(NextEpisode {
                key,
                playable_media_id,
                reason: NextReason::FirstUnwatched,
            })
        } else {
            None
        };

        Ok(SeriesWatchStatus {
            tmdb_series_id,
            total_episodes: total,
            watched,
            in_progress,
            seasons,
            next_episode,
        })
    }

    async fn get_season_watch_status(
        &self,
        user_id: Uuid,
        tmdb_series_id: u64,
        season_number: u16,
    ) -> Result<SeasonWatchStatus> {
        use std::collections::HashMap;

        let rows = sqlx::query!(
            r#"
            SELECT episode_number
            FROM episode_metadata
            WHERE series_tmdb_id = $1 AND season_number = $2
            GROUP BY episode_number
            ORDER BY episode_number
            "#,
            tmdb_series_id as i64,
            season_number as i16
        )
        .fetch_all(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to list episodes for season: {}",
                e
            ))
        })?;

        let state = sqlx::query!(
            r#"
            SELECT episode_number, position, duration, is_completed
            FROM user_episode_state
            WHERE user_id = $1 AND tmdb_series_id = $2 AND season_number = $3
            "#,
            user_id,
            tmdb_series_id as i64,
            season_number as i16
        )
        .fetch_all(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to fetch season user state: {}",
                e
            ))
        })?;

        let mut episodes = HashMap::new();
        let mut total = 0u32;
        let mut watched = 0u32;
        let mut in_prog = 0u32;
        let state_map: HashMap<i16, (f32, f32, bool)> = state
            .into_iter()
            .map(|r| {
                (r.episode_number, (r.position, r.duration, r.is_completed))
            })
            .collect();

        for r in rows.into_iter() {
            total += 1;
            if let Some(ep_no) = r.episode_number {
                let ep_no = ep_no as i16;
                if let Some((pos, dur, done)) = state_map.get(&ep_no).copied() {
                    if done || Self::is_completed_progress(pos, dur) {
                        episodes.insert(ep_no as u16, EpisodeStatus::Completed);
                        watched += 1;
                    } else if pos > 0.0 && dur > 0.0 {
                        let prog = (pos / dur).clamp(0.0, 1.0);
                        episodes.insert(
                            ep_no as u16,
                            EpisodeStatus::InProgress { progress: prog },
                        );
                        in_prog += 1;
                    } else {
                        episodes.insert(ep_no as u16, EpisodeStatus::Unwatched);
                    }
                } else {
                    episodes.insert(ep_no as u16, EpisodeStatus::Unwatched);
                }
            } else {
                log::warn!("Missing episode number");
            }
        }

        Ok(SeasonWatchStatus {
            key: SeasonKey {
                tmdb_series_id,
                season_number,
            },
            total,
            watched,
            in_progress: in_prog,
            is_completed: watched == total && total > 0,
            episodes,
        })
    }

    async fn get_next_episode(
        &self,
        user_id: Uuid,
        tmdb_series_id: u64,
    ) -> Result<Option<NextEpisode>> {
        let policy = Self::watch_policy();

        // Prefer latest resume-eligible in-progress episode.
        if let Some(row) = sqlx::query(
            r#"
            SELECT season_number, episode_number, last_media_uuid
            FROM user_episode_state
            WHERE user_id = $1
              AND tmdb_series_id = $2
              AND position >= $3
              AND duration > 0
              AND (position / duration) >= $4
              AND (duration - position) >= $5
              AND is_completed = false
              AND (position / duration) < $6
            ORDER BY last_watched DESC
            LIMIT 1
            "#,
        )
        .bind(user_id)
        .bind(tmdb_series_id as i64)
        .bind(policy.resume_min_position_seconds)
        .bind(policy.resume_min_progress_ratio)
        .bind(policy.resume_min_remaining_seconds)
        .bind(policy.completion_threshold)
        .fetch_optional(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to query in-progress next episode: {}",
                e
            ))
        })? {
            let key = EpisodeKey {
                tmdb_series_id,
                season_number: row.try_get::<i16, _>("season_number").map_err(
                    |e| {
                        MediaError::Internal(format!(
                            "Failed to decode next-episode season: {}",
                            e
                        ))
                    },
                )? as u16,
                episode_number: row
                    .try_get::<i16, _>("episode_number")
                    .map_err(|e| {
                        MediaError::Internal(format!(
                            "Failed to decode next-episode episode: {}",
                            e
                        ))
                    })? as u16,
            };
            let playable_media_id = if let Some(id) = row
                .try_get::<Option<Uuid>, _>("last_media_uuid")
                .map_err(|e| {
                    MediaError::Internal(format!(
                        "Failed to decode next-episode playback id: {}",
                        e
                    ))
                })? {
                Some(id)
            } else {
                self.lookup_playable_episode(&key).await?
            };
            return Ok(Some(NextEpisode {
                key,
                playable_media_id,
                reason: NextReason::ResumeInProgress,
            }));
        }

        // Else first unwatched from the known playable catalog.
        if let Some(row) = sqlx::query(
            r#"
            SELECT er.season_number, er.episode_number
            FROM episode_references er
            LEFT JOIN user_episode_state ues
                ON ues.user_id = $1
               AND ues.tmdb_series_id = er.tmdb_series_id
               AND ues.season_number = er.season_number
               AND ues.episode_number = er.episode_number
            WHERE er.tmdb_series_id = $2
              AND (ues.is_completed IS NULL OR ues.is_completed = false)
            ORDER BY er.season_number ASC,
                     er.episode_number ASC,
                     er.discovered_at ASC,
                     er.id ASC
            LIMIT 1
            "#,
        )
        .bind(user_id)
        .bind(tmdb_series_id as i64)
        .fetch_optional(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to query first unwatched episode: {}",
                e
            ))
        })? {
            let key = EpisodeKey {
                tmdb_series_id,
                season_number: row.try_get::<i16, _>("season_number").map_err(
                    |e| {
                        MediaError::Internal(format!(
                            "Failed to decode first-unwatched season: {}",
                            e
                        ))
                    },
                )? as u16,
                episode_number: row
                    .try_get::<i16, _>("episode_number")
                    .map_err(|e| {
                        MediaError::Internal(format!(
                            "Failed to decode first-unwatched episode: {}",
                            e
                        ))
                    })? as u16,
            };
            let playable_media_id = self.lookup_playable_episode(&key).await?;
            return Ok(Some(NextEpisode {
                key,
                playable_media_id,
                reason: NextReason::FirstUnwatched,
            }));
        }

        Ok(None)
    }

    async fn mark_episode_completed(
        &self,
        user_id: Uuid,
        key: &EpisodeKey,
    ) -> Result<()> {
        let now = Utc::now().timestamp_millis();
        sqlx::query!(
            r#"
            INSERT INTO user_episode_state (
                user_id, tmdb_series_id, season_number, episode_number,
                position, duration, last_watched, is_completed
            ) VALUES ($1,$2,$3,$4,1.0,1.0,$5,true)
            ON CONFLICT (user_id, tmdb_series_id, season_number, episode_number)
            DO UPDATE SET is_completed = true, last_watched = GREATEST(user_episode_state.last_watched, EXCLUDED.last_watched)
            "#,
            user_id,
            key.tmdb_series_id as i64,
            key.season_number as i16,
            key.episode_number as i16,
            now
        )
            .execute(self.pool())
            .await
            .map_err(|e| MediaError::Internal(format!("Failed to mark episode completed: {}", e)))?;
        Ok(())
    }

    async fn clear_episode_state(
        &self,
        user_id: Uuid,
        key: &EpisodeKey,
    ) -> Result<()> {
        sqlx::query!(
            r#"
            DELETE FROM user_episode_state
            WHERE user_id = $1 AND tmdb_series_id = $2 AND season_number = $3 AND episode_number = $4
            "#,
            user_id,
            key.tmdb_series_id as i64,
            key.season_number as i16,
            key.episode_number as i16
        )
            .execute(self.pool())
            .await
            .map_err(|e| MediaError::Internal(format!("Failed to clear episode state: {}", e)))?;
        Ok(())
    }
}

impl PostgresWatchStatusRepository {
    async fn resolve_episode_key_for_media_id(
        &self,
        media_id: Uuid,
    ) -> Result<Option<EpisodeKey>> {
        let row = sqlx::query(
            r#"
            SELECT tmdb_series_id, season_number, episode_number
            FROM episode_references
            WHERE id = $1
            "#,
        )
        .bind(media_id)
        .fetch_optional(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to resolve episode identity for {}: {}",
                media_id, e
            ))
        })?;

        row.map(|row| {
            Ok(EpisodeKey {
                tmdb_series_id: row
                    .try_get::<i64, _>("tmdb_series_id")
                    .map_err(|e| {
                        MediaError::Internal(format!(
                            "Failed to decode tmdb_series_id for {}: {}",
                            media_id, e
                        ))
                    })? as u64,
                season_number: row.try_get::<i16, _>("season_number").map_err(
                    |e| {
                        MediaError::Internal(format!(
                            "Failed to decode season_number for {}: {}",
                            media_id, e
                        ))
                    },
                )? as u16,
                episode_number: row
                    .try_get::<i16, _>("episode_number")
                    .map_err(|e| {
                        MediaError::Internal(format!(
                            "Failed to decode episode_number for {}: {}",
                            media_id, e
                        ))
                    })? as u16,
            })
        })
        .transpose()
    }

    async fn lookup_playable_episode(
        &self,
        key: &EpisodeKey,
    ) -> Result<Option<Uuid>> {
        let row = sqlx::query(
            r#"
            SELECT id FROM episode_references
            WHERE tmdb_series_id = $1 AND season_number = $2 AND episode_number = $3
            ORDER BY discovered_at ASC, id ASC
            LIMIT 1
            "#,
        )
        .bind(key.tmdb_series_id as i64)
        .bind(key.season_number as i16)
        .bind(key.episode_number as i16)
        .fetch_optional(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to resolve playable episode: {}",
                e
            ))
        })?;

        row.map(|row| row.try_get::<Uuid, _>("id"))
            .transpose()
            .map_err(|e| {
                MediaError::Internal(format!(
                    "Failed to decode playable episode id: {}",
                    e
                ))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn series_continue_item(
        series_id: Uuid,
        title: &str,
        last_watched: i64,
    ) -> SeriesContinueWatchingItem {
        SeriesContinueWatchingItem {
            series_id,
            library_id: LibraryId(Uuid::from_u128(1)),
            action_episode_id: Some(Uuid::from_u128(100)),
            action_hint: ContinueWatchingActionHint::Resume,
            position: 120.0,
            duration: 1_200.0,
            last_watched,
            title: Some(title.to_string()),
            subtitle: Some("Resume S01E01".to_string()),
            poster_iid: None,
        }
    }

    #[test]
    fn library_series_continue_items_sort_by_activity_title_and_series_id() {
        let alpha_late = Uuid::from_u128(30);
        let beta = Uuid::from_u128(20);
        let alpha_tie_low_id = Uuid::from_u128(10);
        let mut items = vec![
            series_continue_item(beta, "Beta", 100),
            series_continue_item(alpha_late, "Alpha", 200),
            series_continue_item(alpha_tie_low_id, "Alpha", 200),
        ];

        PostgresWatchStatusRepository::sort_series_continue_watching_items(
            &mut items,
        );

        assert_eq!(
            items.iter().map(|item| item.series_id).collect::<Vec<_>>(),
            vec![alpha_tie_low_id, alpha_late, beta]
        );
    }
}
