use std::collections::HashMap;
use std::fmt;

use async_trait::async_trait;
use sqlx::PgPool;
use uuid::Uuid;

use crate::database::ports::watch_metrics::{ProgressEntry, WatchMetricsReadPort};
use crate::error::{MediaError, Result};

#[derive(Clone)]
pub struct PostgresWatchMetricsRepository {
    pool: PgPool,
}

impl PostgresWatchMetricsRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    fn pool(&self) -> &PgPool {
        &self.pool
    }
}

impl fmt::Debug for PostgresWatchMetricsRepository {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PostgresWatchMetricsRepository")
            .field("pool_size", &self.pool.size())
            .field("idle_connections", &self.pool.num_idle())
            .finish()
    }
}

#[async_trait]
impl WatchMetricsReadPort for PostgresWatchMetricsRepository {
    async fn load_progress_map(&self, user_id: Uuid) -> Result<HashMap<Uuid, ProgressEntry>> {
        let rows = sqlx::query!(
            r#"
            SELECT media_uuid, position, duration, last_watched
            FROM user_watch_progress
            WHERE user_id = $1
            "#,
            user_id
        )
        .fetch_all(self.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to load watch progress: {}", e)))?;

        let mut map = HashMap::with_capacity(rows.len());
        for row in rows {
            let duration = row.duration.max(1.0);
            let ratio = (row.position / duration).clamp(0.0, 1.0) as f32;
            map.insert(
                row.media_uuid,
                ProgressEntry {
                    ratio,
                    last_watched: row.last_watched,
                },
            );
        }

        Ok(map)
    }

    async fn load_completed_map(&self, user_id: Uuid) -> Result<HashMap<Uuid, i64>> {
        let rows = sqlx::query!(
            r#"
            SELECT media_uuid, completed_at
            FROM user_completed_media
            WHERE user_id = $1
            "#,
            user_id
        )
        .fetch_all(self.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to load completed media: {}", e)))?;

        let mut map = HashMap::with_capacity(rows.len());
        for row in rows {
            map.insert(row.media_uuid, row.completed_at);
        }

        Ok(map)
    }
}
