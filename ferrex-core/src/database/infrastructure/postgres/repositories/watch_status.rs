use crate::{
    database::ports::watch_status::WatchStatusRepository,
    domain::watch::{InProgressItem, UpdateProgressRequest, UserWatchState},
    error::{MediaError, Result},
    types::watch::{
        EpisodeKey, EpisodeStatus, NextEpisode, NextReason, SeasonKey,
        SeasonWatchStatus, SeriesWatchStatus,
    },
};

use async_trait::async_trait;
use chrono::Utc;
use ferrex_model::MediaType;
use sqlx::PgPool;
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
        if completion_ratio > 0.95 {
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
        if matches!(progress.media_type, MediaType::Episode) {
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
                let is_completed = completion_ratio > 0.95;
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
    ) -> Result<Vec<InProgressItem>> {
        let rows = sqlx::query!(
            r#"
            SELECT media_uuid, position, duration, last_watched
            FROM user_watch_progress
            WHERE user_id = $1
            ORDER BY last_watched DESC
            LIMIT $2
            "#,
            user_id,
            limit as i64
        )
        .fetch_all(self.pool())
        .await
        .map_err(|e| {
            MediaError::Internal(format!(
                "Failed to get continue watching: {}",
                e
            ))
        })?;

        let mut items = Vec::new();
        for row in rows {
            items.push(InProgressItem {
                media_id: row.media_uuid,
                position: row.position,
                duration: row.duration,
                last_watched: row.last_watched,
            });
        }

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
        let is_completed = position / duration > 0.95;
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
                    if done || (dur > 0.0 && pos / dur > 0.95) {
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
                        if best_in_progress
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
                    if done || (dur > 0.0 && pos / dur > 0.95) {
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
        // Prefer latest in-progress
        if let Some(row) = sqlx::query!(
            r#"
            SELECT season_number, episode_number, last_media_uuid
            FROM user_episode_state
            WHERE user_id = $1 AND tmdb_series_id = $2 AND position > 0 AND duration > 0 AND (position/duration) < 0.95
            ORDER BY last_watched DESC
            LIMIT 1
            "#,
            user_id,
            tmdb_series_id as i64
        )
        .fetch_optional(self.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to query in-progress: {}", e)))? {
            let key = EpisodeKey { tmdb_series_id, season_number: row.season_number as u16, episode_number: row.episode_number as u16 };
            let playable_media_id = if let Some(id) = row.last_media_uuid { Some(id) } else { self.lookup_playable_episode(&key).await? };
            return Ok(Some(NextEpisode { key, playable_media_id, reason: NextReason::ResumeInProgress }));
        }

        // Else first unwatched from catalog
        if let Some(row) = sqlx::query!(
            r#"
            SELECT em.season_number, em.episode_number
            FROM episode_metadata em
            LEFT JOIN user_episode_state ues
                ON ues.user_id = $1 AND ues.tmdb_series_id = em.series_tmdb_id
                AND ues.season_number = em.season_number AND ues.episode_number = em.episode_number
            WHERE em.series_tmdb_id = $2 AND (ues.is_completed IS NULL OR ues.is_completed = false)
            GROUP BY em.season_number, em.episode_number
            ORDER BY em.season_number, em.episode_number
            LIMIT 1
            "#,
            user_id,
            tmdb_series_id as i64
        )
        .fetch_optional(self.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to query first unwatched: {}", e)))? {
            if let (Some(s), Some(e)) = (row.season_number, row.episode_number) {
                let key = EpisodeKey { tmdb_series_id, season_number: s as u16, episode_number: e as u16 };
                let playable_media_id = self.lookup_playable_episode(&key).await?;
                return Ok(Some(NextEpisode { key, playable_media_id, reason: NextReason::FirstUnwatched }));
            } else {
                "Missing season or episode number".to_string();
            }
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
    async fn lookup_playable_episode(
        &self,
        key: &EpisodeKey,
    ) -> Result<Option<Uuid>> {
        let row = sqlx::query!(
            r#"
            SELECT id FROM episode_references
            WHERE tmdb_series_id = $1 AND season_number = $2 AND episode_number = $3
            ORDER BY discovered_at ASC
            LIMIT 1
            "#,
            key.tmdb_series_id as i64,
            key.season_number as i16,
            key.episode_number as i16
        )
        .fetch_optional(self.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to resolve playable episode: {}", e)))?;
        Ok(row.map(|r| r.id))
    }
}
