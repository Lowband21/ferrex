use crate::{
    database::repository_ports::watch_status::WatchStatusRepository,
    domain::watch::{
        ContinueWatchingActionHint, ContinueWatchingItem, InProgressItem,
        UpdateProgressRequest, UserWatchState,
    },
    error::{MediaError, Result},
    types::watch::{
        EpisodeKey, EpisodeStatus, NextEpisode, NextReason, SeasonKey,
        SeasonWatchStatus, SeriesWatchStatus,
    },
};

use async_trait::async_trait;
use chrono::Utc;
use ferrex_model::VideoMediaType;
use sqlx::{PgPool, Row};
use std::collections::{HashMap, HashSet};
use tracing::info;
use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct PostgresWatchStatusRepository {
    pool: PgPool,
}

impl PostgresWatchStatusRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    fn pool(&self) -> &PgPool {
        &self.pool
    }

    fn parse_watch_env(name: &str, default: f32) -> f32 {
        std::env::var(name)
            .ok()
            .and_then(|value| value.parse::<f32>().ok())
            .filter(|value| value.is_finite() && *value >= 0.0)
            .unwrap_or(default)
    }

    fn completion_threshold() -> f32 {
        Self::parse_watch_env("FERREX_WATCH_COMPLETION_THRESHOLD", 0.95)
            .clamp(0.0, 1.0)
    }

    fn resume_min_position_seconds() -> f32 {
        Self::parse_watch_env("FERREX_WATCH_RESUME_MIN_POSITION_SECONDS", 30.0)
    }

    fn resume_min_progress_ratio() -> f32 {
        Self::parse_watch_env("FERREX_WATCH_RESUME_MIN_PROGRESS_RATIO", 0.02)
            .clamp(0.0, 1.0)
    }

    fn resume_min_remaining_seconds() -> f32 {
        Self::parse_watch_env("FERREX_WATCH_RESUME_MIN_REMAINING_SECONDS", 60.0)
    }

    fn is_completed_progress(position: f32, duration: f32) -> bool {
        duration > 0.0 && (position / duration) >= Self::completion_threshold()
    }

    fn is_resume_eligible(position: f32, duration: f32) -> bool {
        if duration <= 0.0 || position < Self::resume_min_position_seconds() {
            return false;
        }

        if Self::is_completed_progress(position, duration) {
            return false;
        }

        let progress = position / duration;
        let remaining = (duration - position).max(0.0);

        progress >= Self::resume_min_progress_ratio()
            && remaining >= Self::resume_min_remaining_seconds()
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

    async fn load_movie_continue_watching_metadata(
        &self,
        media_id: Uuid,
    ) -> Result<Option<(String, Option<Uuid>)>> {
        let row = sqlx::query(
            r#"
            SELECT
                mr.title AS title,
                mm.primary_poster_image_id AS poster_iid
            FROM movie_references mr
            LEFT JOIN movie_metadata mm
                ON mm.movie_id = mr.id
            WHERE mr.id = $1
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

        Ok(row.and_then(|row| {
            row.try_get::<String, _>("title")
                .ok()
                .map(|title| (title, row.try_get::<Uuid, _>("poster_iid").ok()))
        }))
    }

    async fn build_series_continue_watching_item(
        &self,
        user_id: Uuid,
        tmdb_series_id: u64,
        last_watched: i64,
    ) -> Result<Option<ContinueWatchingItem>> {
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
        .bind(Self::resume_min_position_seconds())
        .bind(Self::resume_min_progress_ratio())
        .bind(Self::resume_min_remaining_seconds())
        .bind(Self::completion_threshold())
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
            let poster_iid = row.try_get::<Uuid, _>("poster_iid").ok();
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
                card_media_id: Some(card_media_id),
                media_type: VideoMediaType::Series,
                position,
                duration,
                last_watched,
                title: Some(title),
                subtitle,
                action_hint: Some(ContinueWatchingActionHint::Resume),
                poster_iid,
            }));
        }

        let completed_row = sqlx::query(
            r#"
            SELECT EXISTS(
                SELECT 1
                FROM user_episode_state
                WHERE user_id = $1
                  AND tmdb_series_id = $2
                  AND is_completed = true
            ) AS has_completed
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

        let has_completed =
            match completed_row.try_get::<bool, _>("has_completed") {
                Ok(value) => value,
                Err(e) => {
                    return Err(MediaError::Internal(format!(
                        "Failed to decode completed-series flag: {}",
                        e
                    )));
                }
            };

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
        let poster_iid = row.try_get::<Uuid, _>("poster_iid").ok();
        let key = EpisodeKey {
            tmdb_series_id,
            season_number,
            episode_number,
        };

        Ok(Some(ContinueWatchingItem {
            media_id,
            card_media_id: Some(card_media_id),
            media_type: VideoMediaType::Series,
            position: 0.0,
            duration: 0.0,
            last_watched,
            title: Some(title),
            subtitle: Some(format!(
                "Next up: {}",
                Self::format_episode_label(&key)
            )),
            action_hint: Some(ContinueWatchingActionHint::NextEpisode),
            poster_iid,
        }))
    }
}

#[async_trait]
impl WatchStatusRepository for PostgresWatchStatusRepository {
    async fn update_watch_progress(
        &self,
        user_id: Uuid,
        progress: &UpdateProgressRequest,
    ) -> Result<()> {
        let now = Utc::now().timestamp_millis();

        let mut tx = self.pool().begin().await.map_err(|e| {
            MediaError::Internal(format!("Failed to start transaction: {}", e))
        })?;

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

        // Check if we should mark as completed (>95% watched)
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

            // Remove from in-progress
            sqlx::query!(
                r#"
                DELETE FROM user_watch_progress
                WHERE user_id = $1 AND media_uuid = $2
                "#,
                user_id,
                progress.media_id
            )
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                MediaError::Internal(format!(
                    "Failed to remove from in-progress: {}",
                    e
                ))
            })?;
        }

        // For episodes, also upsert identity-based state
        if matches!(progress.media_type, VideoMediaType::Episode) {
            // Prefer provided identity; otherwise resolve from episode_references
            let key = if let Some(k) = &progress.episode {
                Some(*k)
            } else {
                // Resolve identity from episode_references
                let row = sqlx::query!(
                    r#"
                    SELECT tmdb_series_id, season_number, episode_number
                    FROM episode_references
                    WHERE id = $1
                    "#,
                    progress.media_id
                )
                .fetch_optional(&mut *tx)
                .await
                .map_err(|e| {
                    MediaError::Internal(format!(
                        "Failed to resolve episode identity: {}",
                        e
                    ))
                })?;

                row.map(|r| EpisodeKey {
                    tmdb_series_id: r.tmdb_series_id as u64,
                    season_number: r.season_number as u16,
                    episode_number: r.episode_number as u16,
                })
            };

            if let Some(key) = key {
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

            let Some((title, poster_iid)) =
                self.load_movie_continue_watching_metadata(media_id).await?
            else {
                continue;
            };

            let subtitle = Self::format_remaining_label(duration - position)
                .map(|remaining| format!("Resume • {remaining} left"));

            items.push(ContinueWatchingItem {
                media_id,
                card_media_id: Some(media_id),
                media_type: VideoMediaType::Movie,
                position,
                duration,
                last_watched,
                title: Some(title),
                subtitle,
                action_hint: Some(ContinueWatchingActionHint::Resume),
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

    async fn clear_watch_progress(
        &self,
        user_id: Uuid,
        media_id: &Uuid,
    ) -> Result<()> {
        let mut tx = self.pool().begin().await.map_err(|e| {
            MediaError::Internal(format!("Failed to start transaction: {}", e))
        })?;

        // Remove from progress
        let progress_result = sqlx::query!(
            r#"
            DELETE FROM user_watch_progress
            WHERE user_id = $1 AND media_uuid = $2
            "#,
            user_id,
            media_id
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to clear watch progress: {}",
                e
            ))
        })?;

        // Remove from completed
        let completed_result = sqlx::query!(
            r#"
            DELETE FROM user_completed_media
            WHERE user_id = $1 AND media_uuid = $2
            "#,
            user_id,
            media_id
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to clear completed status: {}",
                e
            ))
        })?;

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
        let now = Utc::now().timestamp_millis();
        let mut tx = self.pool().begin().await.map_err(|e| {
            MediaError::Internal(format!("Failed to start transaction: {}", e))
        })?;

        sqlx::query(
            r#"
            DELETE FROM user_watch_progress
            WHERE user_id = $1 AND media_uuid = $2
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

        if matches!(media_type, VideoMediaType::Episode) {
            let key = self
                .resolve_episode_key_for_media_id(media_id)
                .await?
                .ok_or_else(|| {
                    MediaError::Internal(format!(
                        "Failed to resolve episode identity for {}",
                        media_id
                    ))
                })?;

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
                    last_media_uuid = COALESCE(user_episode_state.last_media_uuid, EXCLUDED.last_media_uuid)
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
        let mut tx = self.pool().begin().await.map_err(|e| {
            MediaError::Internal(format!("Failed to start transaction: {}", e))
        })?;

        sqlx::query(
            r#"
            DELETE FROM user_watch_progress
            WHERE user_id = $1 AND media_uuid = $2
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
            WHERE user_id = $1 AND media_uuid = $2
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

        if matches!(media_type, VideoMediaType::Episode) {
            if let Some(key) =
                self.resolve_episode_key_for_media_id(media_id).await?
            {
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
              AND media_uuid IN (
                    SELECT id FROM episode_references WHERE tmdb_series_id = $2
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
                last_media_uuid = COALESCE(user_episode_state.last_media_uuid, EXCLUDED.last_media_uuid)
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
              AND media_uuid IN (
                    SELECT id FROM episode_references WHERE tmdb_series_id = $2
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
              AND media_uuid IN (
                    SELECT id FROM episode_references WHERE tmdb_series_id = $2
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
        .bind(Self::resume_min_position_seconds())
        .bind(Self::resume_min_progress_ratio())
        .bind(Self::resume_min_remaining_seconds())
        .bind(Self::completion_threshold())
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
            let playable_media_id = if let Some(id) =
                row.try_get::<Uuid, _>("last_media_uuid").ok()
            {
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

        match row {
            Some(row) => {
                let tmdb_series_id =
                    row.try_get::<i64, _>("tmdb_series_id").map_err(|e| {
                        MediaError::Internal(format!(
                            "Failed to decode tmdb_series_id for {}: {}",
                            media_id, e
                        ))
                    })? as u64;
                let season_number =
                    row.try_get::<i16, _>("season_number").map_err(|e| {
                        MediaError::Internal(format!(
                            "Failed to decode season_number for {}: {}",
                            media_id, e
                        ))
                    })? as u16;
                let episode_number =
                    row.try_get::<i16, _>("episode_number").map_err(|e| {
                        MediaError::Internal(format!(
                            "Failed to decode episode_number for {}: {}",
                            media_id, e
                        ))
                    })? as u16;

                Ok(Some(EpisodeKey {
                    tmdb_series_id,
                    season_number,
                    episode_number,
                }))
            }
            None => Ok(None),
        }
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
