use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::{
    error::Result,
    types::{LibraryId, ids::SeriesID},
};

use super::context::{SeriesHint, SeriesRef, SeriesRootPath};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "database", derive(sqlx::Type))]
#[cfg_attr(
    feature = "database",
    sqlx(type_name = "series_scan_status", rename_all = "lowercase")
)]
pub enum SeriesScanStatus {
    Discovered,
    Seeded,
    Resolved,
    Failed,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SeriesScanState {
    pub library_id: LibraryId,
    pub series_root_path: SeriesRootPath,
    pub status: SeriesScanStatus,
    pub series_id: Option<SeriesID>,
    pub hint: Option<SeriesHint>,
    pub seeded_at: Option<DateTime<Utc>>,
    pub last_attempt_at: Option<DateTime<Utc>>,
    pub attempts: u32,
    pub resolved_at: Option<DateTime<Utc>>,
    pub failed_at: Option<DateTime<Utc>>,
    pub failure_reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl SeriesScanState {
    pub fn is_resolved(&self) -> bool {
        self.series_id.is_some()
            && matches!(self.status, SeriesScanStatus::Resolved)
    }
}

#[async_trait]
pub trait SeriesScanStateRepository: Send + Sync {
    async fn get(
        &self,
        library_id: LibraryId,
        series_root_path: &SeriesRootPath,
    ) -> Result<Option<SeriesScanState>>;

    async fn mark_discovered(
        &self,
        library_id: LibraryId,
        series_root_path: SeriesRootPath,
        hint: Option<SeriesHint>,
    ) -> Result<SeriesScanState>;

    async fn mark_seeded(
        &self,
        library_id: LibraryId,
        series_root_path: SeriesRootPath,
        hint: Option<SeriesHint>,
    ) -> Result<SeriesScanState>;

    async fn mark_resolved(
        &self,
        library_id: LibraryId,
        series_root_path: SeriesRootPath,
        series_ref: SeriesRef,
    ) -> Result<SeriesScanState>;

    async fn mark_failed(
        &self,
        library_id: LibraryId,
        series_root_path: SeriesRootPath,
        reason: String,
    ) -> Result<SeriesScanState>;
}

#[derive(Clone, Debug, Default)]
pub struct InMemorySeriesScanStateRepository {
    states: Arc<Mutex<HashMap<(LibraryId, SeriesRootPath), SeriesScanState>>>,
}

#[async_trait]
impl SeriesScanStateRepository for InMemorySeriesScanStateRepository {
    async fn get(
        &self,
        library_id: LibraryId,
        series_root_path: &SeriesRootPath,
    ) -> Result<Option<SeriesScanState>> {
        let guard = self.states.lock().await;
        Ok(guard.get(&(library_id, series_root_path.clone())).cloned())
    }

    async fn mark_discovered(
        &self,
        library_id: LibraryId,
        series_root_path: SeriesRootPath,
        hint: Option<SeriesHint>,
    ) -> Result<SeriesScanState> {
        let mut guard = self.states.lock().await;
        let now = Utc::now();
        let entry = guard
            .entry((library_id, series_root_path.clone()))
            .or_insert_with(|| SeriesScanState {
                library_id,
                series_root_path: series_root_path.clone(),
                status: SeriesScanStatus::Discovered,
                series_id: None,
                hint: hint.clone(),
                seeded_at: None,
                last_attempt_at: None,
                attempts: 0,
                resolved_at: None,
                failed_at: None,
                failure_reason: None,
                created_at: now,
                updated_at: now,
            });

        if hint.is_some() {
            entry.hint = hint;
        }
        if !matches!(entry.status, SeriesScanStatus::Resolved) {
            entry.status = SeriesScanStatus::Discovered;
        }
        entry.updated_at = now;
        Ok(entry.clone())
    }

    async fn mark_seeded(
        &self,
        library_id: LibraryId,
        series_root_path: SeriesRootPath,
        hint: Option<SeriesHint>,
    ) -> Result<SeriesScanState> {
        let mut guard = self.states.lock().await;
        let now = Utc::now();
        let entry = guard
            .entry((library_id, series_root_path.clone()))
            .or_insert_with(|| SeriesScanState {
                library_id,
                series_root_path: series_root_path.clone(),
                status: SeriesScanStatus::Seeded,
                series_id: None,
                hint: hint.clone(),
                seeded_at: Some(now),
                last_attempt_at: Some(now),
                attempts: 1,
                resolved_at: None,
                failed_at: None,
                failure_reason: None,
                created_at: now,
                updated_at: now,
            });

        if hint.is_some() {
            entry.hint = hint;
        }
        if !matches!(entry.status, SeriesScanStatus::Resolved) {
            entry.status = SeriesScanStatus::Seeded;
        }
        entry.last_attempt_at = Some(now);
        entry.attempts = entry.attempts.saturating_add(1);
        entry.seeded_at.get_or_insert(now);
        entry.updated_at = now;

        Ok(entry.clone())
    }

    async fn mark_resolved(
        &self,
        library_id: LibraryId,
        series_root_path: SeriesRootPath,
        series_ref: SeriesRef,
    ) -> Result<SeriesScanState> {
        let mut guard = self.states.lock().await;
        let now = Utc::now();
        let entry = guard
            .entry((library_id, series_root_path.clone()))
            .or_insert_with(|| SeriesScanState {
                library_id,
                series_root_path: series_root_path.clone(),
                status: SeriesScanStatus::Resolved,
                series_id: Some(series_ref.id),
                hint: None,
                seeded_at: Some(now),
                last_attempt_at: Some(now),
                attempts: 1,
                resolved_at: Some(now),
                failed_at: None,
                failure_reason: None,
                created_at: now,
                updated_at: now,
            });

        entry.series_id = Some(series_ref.id);
        entry.status = SeriesScanStatus::Resolved;
        entry.resolved_at = Some(now);
        entry.updated_at = now;
        entry.failed_at = None;
        entry.failure_reason = None;
        if entry.hint.is_none()
            && (series_ref.title.is_some() || series_ref.slug.is_some())
        {
            entry.hint = Some(SeriesHint {
                title: series_ref.title.clone().unwrap_or_default(),
                slug: series_ref.slug,
                year: None,
                region: None,
            });
        }

        Ok(entry.clone())
    }

    async fn mark_failed(
        &self,
        library_id: LibraryId,
        series_root_path: SeriesRootPath,
        reason: String,
    ) -> Result<SeriesScanState> {
        let mut guard = self.states.lock().await;
        let now = Utc::now();
        let entry = guard
            .entry((library_id, series_root_path.clone()))
            .or_insert_with(|| SeriesScanState {
                library_id,
                series_root_path: series_root_path.clone(),
                status: SeriesScanStatus::Failed,
                series_id: None,
                hint: None,
                seeded_at: None,
                last_attempt_at: None,
                attempts: 0,
                resolved_at: None,
                failed_at: Some(now),
                failure_reason: Some(reason.clone()),
                created_at: now,
                updated_at: now,
            });

        entry.status = SeriesScanStatus::Failed;
        entry.failed_at = Some(now);
        entry.failure_reason = Some(reason);
        entry.updated_at = now;

        Ok(entry.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn lib(id: u128) -> LibraryId {
        LibraryId(Uuid::from_u128(id))
    }

    fn series_root(path: &str) -> SeriesRootPath {
        SeriesRootPath::try_new(path).expect("valid series root path")
    }

    #[tokio::test]
    async fn discovered_hint_is_not_overwritten_by_none() {
        let repo = InMemorySeriesScanStateRepository::default();
        let library_id = lib(1);
        let root = series_root("/demo/Shows/Example");

        let hint = SeriesHint {
            title: "Example".into(),
            slug: Some("example".into()),
            year: Some(2001),
            region: Some("US".into()),
        };

        let first = repo
            .mark_discovered(library_id, root.clone(), Some(hint.clone()))
            .await
            .expect("mark discovered");
        assert_eq!(first.hint.as_ref().map(|h| &h.title), Some(&hint.title));

        let second = repo
            .mark_discovered(library_id, root.clone(), None)
            .await
            .expect("mark discovered again");
        assert_eq!(second.hint.as_ref().map(|h| &h.title), Some(&hint.title));
    }

    #[tokio::test]
    async fn mark_seeded_does_not_demote_resolved_state() {
        let repo = InMemorySeriesScanStateRepository::default();
        let library_id = lib(2);
        let root = series_root("/demo/Shows/Resolved");

        let series_id = SeriesID(Uuid::from_u128(3));
        repo.mark_resolved(
            library_id,
            root.clone(),
            SeriesRef {
                id: series_id,
                slug: Some("resolved".into()),
                title: Some("Resolved".into()),
            },
        )
        .await
        .expect("mark resolved");

        let after = repo
            .mark_seeded(library_id, root.clone(), None)
            .await
            .expect("mark seeded");

        assert_eq!(after.series_id, Some(series_id));
        assert_eq!(after.status, SeriesScanStatus::Resolved);
        assert!(after.is_resolved());
    }
}

#[cfg(feature = "database")]
#[derive(Clone)]
pub struct PostgresSeriesScanStateRepository {
    pool: sqlx::PgPool,
}

#[cfg(feature = "database")]
impl std::fmt::Debug for PostgresSeriesScanStateRepository {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PostgresSeriesScanStateRepository")
            .field("pool_size", &self.pool.size())
            .field("idle_connections", &self.pool.num_idle())
            .finish()
    }
}

#[cfg(feature = "database")]
impl PostgresSeriesScanStateRepository {
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self { pool }
    }
}

#[cfg(feature = "database")]
#[async_trait]
impl SeriesScanStateRepository for PostgresSeriesScanStateRepository {
    async fn get(
        &self,
        library_id: LibraryId,
        series_root_path: &SeriesRootPath,
    ) -> Result<Option<SeriesScanState>> {
        let row: Option<_> = sqlx::query!(
            r#"
            SELECT library_id, series_root_path, status as "status!: SeriesScanStatus",
                   series_id, series_title, series_slug, series_year, series_region,
                   seeded_at, last_attempt_at, attempts,
                   resolved_at, failed_at, failure_reason, created_at, updated_at
            FROM series_scan_state
            WHERE library_id = $1 AND series_root_path = $2
            "#,
            library_id.0,
            series_root_path.as_str()
        )
        .fetch_optional(&self.pool)
        .await?;

        let Some(row) = row else {
            return Ok(None);
        };

        let series_root_path = SeriesRootPath::try_new(row.series_root_path)?;

        Ok(Some(SeriesScanState {
            library_id,
            series_root_path,
            status: row.status as SeriesScanStatus,
            series_id: row.series_id.map(SeriesID),
            hint: if row.series_title.is_some()
                || row.series_slug.is_some()
                || row.series_year.is_some()
                || row.series_region.is_some()
            {
                Some(SeriesHint {
                    title: row.series_title.unwrap_or_default(),
                    slug: row.series_slug,
                    year: row.series_year.map(|v| v as u16),
                    region: row.series_region,
                })
            } else {
                None
            },
            seeded_at: row.seeded_at,
            last_attempt_at: row.last_attempt_at,
            attempts: row.attempts as u32,
            resolved_at: row.resolved_at,
            failed_at: row.failed_at,
            failure_reason: row.failure_reason,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }))
    }

    async fn mark_discovered(
        &self,
        library_id: LibraryId,
        series_root_path: SeriesRootPath,
        hint: Option<SeriesHint>,
    ) -> Result<SeriesScanState> {
        let (title, slug, year, region) = hint
            .as_ref()
            .map(|hint| {
                (
                    Some(hint.title.clone()),
                    hint.slug.clone(),
                    hint.year.map(|v| v as i16),
                    hint.region.clone(),
                )
            })
            .unwrap_or((None, None, None, None));

        let row = sqlx::query!(
            r#"
            INSERT INTO series_scan_state (
                library_id, series_root_path, status,
                series_title, series_slug, series_year, series_region,
                attempts, created_at, updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, 0, NOW(), NOW())
            ON CONFLICT (library_id, series_root_path)
            DO UPDATE SET
                series_title = COALESCE(EXCLUDED.series_title, series_scan_state.series_title),
                series_slug = COALESCE(EXCLUDED.series_slug, series_scan_state.series_slug),
                series_year = COALESCE(EXCLUDED.series_year, series_scan_state.series_year),
                series_region = COALESCE(EXCLUDED.series_region, series_scan_state.series_region),
                status = CASE
                    WHEN series_scan_state.status = 'resolved' THEN series_scan_state.status
                    ELSE 'discovered'
                END,
                updated_at = NOW()
            RETURNING library_id, series_root_path, status as "status: SeriesScanStatus",
                      series_id, series_title, series_slug, series_year, series_region,
                      seeded_at, last_attempt_at, attempts,
                      resolved_at, failed_at, failure_reason, created_at, updated_at
            "#,
            library_id.0,
            series_root_path.as_str(),
            SeriesScanStatus::Discovered as SeriesScanStatus,
            title,
            slug,
            year,
            region
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(SeriesScanState {
            library_id,
            series_root_path: SeriesRootPath::try_new(row.series_root_path)?,
            status: row.status,
            series_id: row.series_id.map(SeriesID),
            hint: if row.series_title.is_some()
                || row.series_slug.is_some()
                || row.series_year.is_some()
                || row.series_region.is_some()
            {
                Some(SeriesHint {
                    title: row.series_title.unwrap_or_default(),
                    slug: row.series_slug,
                    year: row.series_year.map(|v| v as u16),
                    region: row.series_region,
                })
            } else {
                None
            },
            seeded_at: row.seeded_at,
            last_attempt_at: row.last_attempt_at,
            attempts: row.attempts as u32,
            resolved_at: row.resolved_at,
            failed_at: row.failed_at,
            failure_reason: row.failure_reason,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }

    async fn mark_seeded(
        &self,
        library_id: LibraryId,
        series_root_path: SeriesRootPath,
        hint: Option<SeriesHint>,
    ) -> Result<SeriesScanState> {
        let (title, slug, year, region) = hint
            .as_ref()
            .map(|hint| {
                (
                    Some(hint.title.clone()),
                    hint.slug.clone(),
                    hint.year.map(|v| v as i16),
                    hint.region.clone(),
                )
            })
            .unwrap_or((None, None, None, None));

        let row = sqlx::query!(
            r#"
            INSERT INTO series_scan_state (
                library_id, series_root_path, status, series_title, series_slug, series_year, series_region,
                seeded_at, last_attempt_at, attempts, created_at, updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, NOW(), NOW(), 1, NOW(), NOW())
            ON CONFLICT (library_id, series_root_path)
            DO UPDATE SET
                status = CASE
                    WHEN series_scan_state.status = 'resolved' THEN series_scan_state.status
                    ELSE 'seeded'
                END,
                series_title = COALESCE(EXCLUDED.series_title, series_scan_state.series_title),
                series_slug = COALESCE(EXCLUDED.series_slug, series_scan_state.series_slug),
                series_year = COALESCE(EXCLUDED.series_year, series_scan_state.series_year),
                series_region = COALESCE(EXCLUDED.series_region, series_scan_state.series_region),
                seeded_at = COALESCE(series_scan_state.seeded_at, NOW()),
                last_attempt_at = NOW(),
                attempts = series_scan_state.attempts + 1,
                updated_at = NOW()
            RETURNING library_id, series_root_path, status as "status: SeriesScanStatus",
                      series_id, series_title, series_slug, series_year, series_region,
                      seeded_at, last_attempt_at, attempts,
                      resolved_at, failed_at, failure_reason, created_at, updated_at
            "#,
            library_id.0,
            series_root_path.as_str(),
            SeriesScanStatus::Seeded as SeriesScanStatus,
            title,
            slug,
            year,
            region
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(SeriesScanState {
            library_id,
            series_root_path: SeriesRootPath::try_new(row.series_root_path)?,
            status: row.status as SeriesScanStatus,
            series_id: row.series_id.map(SeriesID),
            hint: if row.series_title.is_some()
                || row.series_slug.is_some()
                || row.series_year.is_some()
                || row.series_region.is_some()
            {
                Some(SeriesHint {
                    title: row.series_title.unwrap_or_default(),
                    slug: row.series_slug,
                    year: row.series_year.map(|v| v as u16),
                    region: row.series_region,
                })
            } else {
                None
            },
            seeded_at: row.seeded_at,
            last_attempt_at: row.last_attempt_at,
            attempts: row.attempts as u32,
            resolved_at: row.resolved_at,
            failed_at: row.failed_at,
            failure_reason: row.failure_reason,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }

    async fn mark_resolved(
        &self,
        library_id: LibraryId,
        series_root_path: SeriesRootPath,
        series_ref: SeriesRef,
    ) -> Result<SeriesScanState> {
        let row = sqlx::query!(
            r#"
            INSERT INTO series_scan_state (
                library_id, series_root_path, status, series_id,
                series_title, series_slug, resolved_at, created_at, updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, NOW(), NOW(), NOW())
            ON CONFLICT (library_id, series_root_path)
            DO UPDATE SET
                status = 'resolved',
                series_id = EXCLUDED.series_id,
                series_title = COALESCE(EXCLUDED.series_title, series_scan_state.series_title),
                series_slug = COALESCE(EXCLUDED.series_slug, series_scan_state.series_slug),
                resolved_at = NOW(),
                failed_at = NULL,
                failure_reason = NULL,
                updated_at = NOW()
            RETURNING library_id, series_root_path, status as "status: SeriesScanStatus",
                      series_id, series_title, series_slug, series_year, series_region,
                      seeded_at, last_attempt_at, attempts,
                      resolved_at, failed_at, failure_reason, created_at, updated_at
            "#,
            library_id.0,
            series_root_path.as_str(),
            SeriesScanStatus::Resolved as SeriesScanStatus,
            series_ref.id.0,
            series_ref.title.clone(),
            series_ref.slug.clone(),
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(SeriesScanState {
            library_id,
            series_root_path: SeriesRootPath::try_new(row.series_root_path)?,
            status: row.status,
            series_id: row.series_id.map(SeriesID),
            hint: if row.series_title.is_some()
                || row.series_slug.is_some()
                || row.series_year.is_some()
                || row.series_region.is_some()
            {
                Some(SeriesHint {
                    title: row.series_title.unwrap_or_default(),
                    slug: row.series_slug,
                    year: row.series_year.map(|v| v as u16),
                    region: row.series_region,
                })
            } else {
                None
            },
            seeded_at: row.seeded_at,
            last_attempt_at: row.last_attempt_at,
            attempts: row.attempts as u32,
            resolved_at: row.resolved_at,
            failed_at: row.failed_at,
            failure_reason: row.failure_reason,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }

    async fn mark_failed(
        &self,
        library_id: LibraryId,
        series_root_path: SeriesRootPath,
        reason: String,
    ) -> Result<SeriesScanState> {
        let row = sqlx::query!(
            r#"
            INSERT INTO series_scan_state (
                library_id, series_root_path, status, failed_at, failure_reason, created_at, updated_at
            )
            VALUES ($1, $2, $3, NOW(), $4, NOW(), NOW())
            ON CONFLICT (library_id, series_root_path)
            DO UPDATE SET
                status = 'failed',
                failed_at = NOW(),
                failure_reason = EXCLUDED.failure_reason,
                updated_at = NOW()
            RETURNING library_id, series_root_path, status as "status: SeriesScanStatus",
                      series_id, series_title, series_slug, series_year, series_region,
                      seeded_at, last_attempt_at, attempts,
                      resolved_at, failed_at, failure_reason, created_at, updated_at
            "#,
            library_id.0,
            series_root_path.as_str(),
            SeriesScanStatus::Failed as SeriesScanStatus,
            reason
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(SeriesScanState {
            library_id,
            series_root_path: SeriesRootPath::try_new(row.series_root_path)?,
            status: row.status,
            series_id: row.series_id.map(SeriesID),
            hint: if row.series_title.is_some()
                || row.series_slug.is_some()
                || row.series_year.is_some()
                || row.series_region.is_some()
            {
                Some(SeriesHint {
                    title: row.series_title.unwrap_or_default(),
                    slug: row.series_slug,
                    year: row.series_year.map(|v| v as u16),
                    region: row.series_region,
                })
            } else {
                None
            },
            seeded_at: row.seeded_at,
            last_attempt_at: row.last_attempt_at,
            attempts: row.attempts as u32,
            resolved_at: row.resolved_at,
            failed_at: row.failed_at,
            failure_reason: row.failure_reason,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}
