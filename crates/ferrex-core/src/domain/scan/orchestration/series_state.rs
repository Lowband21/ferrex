use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::{
    error::{MediaError, Result},
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

    /// Record an observational episode discovery. This inserts a missing root
    /// but never mutates an existing row; only root enrollment may begin a
    /// fresh resolution generation.
    async fn mark_discovered(
        &self,
        library_id: LibraryId,
        series_root_path: SeriesRootPath,
        hint: Option<SeriesHint>,
    ) -> Result<SeriesScanState>;

    /// Enroll a root for a fresh SeriesResolve generation before its job can be
    /// enqueued. Unlike observational episode discovery, this explicitly marks
    /// every non-resolved root `Discovered`. That state is the durable freshness
    /// signal terminalizing workers use to preserve a newer generation.
    async fn enroll_resolution_generation(
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

    async fn enroll_resolution_generation(
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
            entry.series_id = None;
            entry.resolved_at = None;
            entry.failed_at = None;
            entry.failure_reason = None;
        }
        entry.updated_at = now;
        Ok(entry.clone())
    }

    async fn mark_discovered(
        &self,
        library_id: LibraryId,
        series_root_path: SeriesRootPath,
        hint: Option<SeriesHint>,
    ) -> Result<SeriesScanState> {
        let mut guard = self.states.lock().await;
        if let Some(existing) =
            guard.get(&(library_id, series_root_path.clone()))
        {
            return Ok(existing.clone());
        }

        let now = Utc::now();
        let state = SeriesScanState {
            library_id,
            series_root_path: series_root_path.clone(),
            status: SeriesScanStatus::Discovered,
            series_id: None,
            hint,
            seeded_at: None,
            last_attempt_at: None,
            attempts: 0,
            resolved_at: None,
            failed_at: None,
            failure_reason: None,
            created_at: now,
            updated_at: now,
        };
        guard.insert((library_id, series_root_path), state.clone());
        Ok(state)
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
            entry.failed_at = None;
            entry.failure_reason = None;
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
    async fn episode_observation_inserts_missing_root_then_preserves_it() {
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
        assert_eq!(first.status, SeriesScanStatus::Discovered);
        assert_eq!(first.hint.as_ref(), Some(&hint));

        let replacement_hint = SeriesHint {
            title: "Replacement".into(),
            slug: Some("replacement".into()),
            year: Some(2025),
            region: Some("CA".into()),
        };
        let second = repo
            .mark_discovered(library_id, root.clone(), Some(replacement_hint))
            .await
            .expect("mark discovered again");
        assert_eq!(second.status, first.status);
        assert_eq!(second.hint, first.hint);
        assert_eq!(second.created_at, first.created_at);
        assert_eq!(second.updated_at, first.updated_at);
    }

    #[tokio::test]
    async fn episode_observation_preserves_seeded_state() {
        let repo = InMemorySeriesScanStateRepository::default();
        let library_id = lib(10);
        let root = series_root("/demo/Shows/Seeded Observation");
        let original_hint = SeriesHint {
            title: "Seeded Observation".into(),
            slug: Some("seeded-observation".into()),
            year: Some(2020),
            region: Some("US".into()),
        };
        let seeded = repo
            .mark_seeded(library_id, root.clone(), Some(original_hint.clone()))
            .await
            .expect("series seeded");

        let observed = repo
            .mark_discovered(
                library_id,
                root,
                Some(SeriesHint {
                    title: "Spoofed Enrollment".into(),
                    slug: None,
                    year: Some(2026),
                    region: None,
                }),
            )
            .await
            .expect("episode observes seeded series");

        assert_eq!(observed.status, SeriesScanStatus::Seeded);
        assert_eq!(observed.hint.as_ref(), Some(&original_hint));
        assert_eq!(observed.updated_at, seeded.updated_at);
        assert_eq!(observed.seeded_at, seeded.seeded_at);
        assert_eq!(observed.last_attempt_at, seeded.last_attempt_at);
        assert_eq!(observed.attempts, seeded.attempts);
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

    #[tokio::test]
    async fn mark_discovered_does_not_demote_resolved_state() {
        let repo = InMemorySeriesScanStateRepository::default();
        let library_id = lib(4);
        let root = series_root("/demo/Shows/Rediscovered");

        let series_id = SeriesID(Uuid::from_u128(5));
        let resolved = repo
            .mark_resolved(
                library_id,
                root.clone(),
                SeriesRef {
                    id: series_id,
                    slug: Some("rediscovered".into()),
                    title: Some("Rediscovered".into()),
                },
            )
            .await
            .expect("mark resolved");

        let after = repo
            .mark_discovered(
                library_id,
                root.clone(),
                Some(SeriesHint {
                    title: "Rediscovered".into(),
                    slug: Some("rediscovered".into()),
                    year: Some(2024),
                    region: None,
                }),
            )
            .await
            .expect("mark discovered");

        assert_eq!(after.series_id, Some(series_id));
        assert_eq!(after.status, SeriesScanStatus::Resolved);
        assert!(after.is_resolved());
        assert_eq!(after.hint, resolved.hint);
        assert_eq!(after.updated_at, resolved.updated_at);
        assert_eq!(
            repo.get(library_id, &root)
                .await
                .expect("state lookup")
                .expect("state exists")
                .status,
            SeriesScanStatus::Resolved
        );
    }

    #[tokio::test]
    async fn root_enrollment_reopens_failure_but_episode_discovery_does_not() {
        let repo = InMemorySeriesScanStateRepository::default();
        let library_id = lib(6);
        let root = series_root("/demo/Shows/Failed Then Rediscovered");

        let failed = repo
            .mark_failed(
                library_id,
                root.clone(),
                "provider returned 404".into(),
            )
            .await
            .expect("mark failed");

        let rediscovered = repo
            .mark_discovered(
                library_id,
                root.clone(),
                Some(SeriesHint {
                    title: "Failed Then Rediscovered".into(),
                    slug: None,
                    year: None,
                    region: None,
                }),
            )
            .await
            .expect("mark discovered after failure");
        assert_eq!(rediscovered.status, SeriesScanStatus::Failed);
        assert_eq!(
            rediscovered.failure_reason.as_deref(),
            Some("provider returned 404")
        );
        assert_eq!(rediscovered.hint, failed.hint);
        assert_eq!(rediscovered.updated_at, failed.updated_at);

        let enrolled = repo
            .enroll_resolution_generation(library_id, root.clone(), None)
            .await
            .expect("root enrollment begins a fresh generation");
        assert_eq!(enrolled.status, SeriesScanStatus::Discovered);
        assert!(enrolled.series_id.is_none());
        assert!(enrolled.resolved_at.is_none());
        assert!(enrolled.failed_at.is_none());
        assert!(enrolled.failure_reason.is_none());

        let reseeded = repo
            .mark_seeded(library_id, root, None)
            .await
            .expect("enrolled resolver marks its attempt seeded");
        assert_eq!(reseeded.status, SeriesScanStatus::Seeded);
        assert!(reseeded.failed_at.is_none());
        assert!(reseeded.failure_reason.is_none());
    }

    #[tokio::test]
    async fn root_enrollment_marks_seeded_generation_discovered() {
        let repo = InMemorySeriesScanStateRepository::default();
        let library_id = lib(7);
        let root = series_root("/demo/Shows/Seeded Then Reenrolled");

        let seeded = repo
            .mark_seeded(library_id, root.clone(), None)
            .await
            .expect("attempt seeded");
        assert_eq!(seeded.status, SeriesScanStatus::Seeded);

        let enrolled = repo
            .enroll_resolution_generation(library_id, root, None)
            .await
            .expect("fresh generation enrolled");
        assert_eq!(enrolled.status, SeriesScanStatus::Discovered);
        assert_eq!(enrolled.attempts, seeded.attempts);
        assert_eq!(enrolled.seeded_at, seeded.seeded_at);
        assert_eq!(enrolled.last_attempt_at, seeded.last_attempt_at);
        assert!(enrolled.series_id.is_none());
        assert!(enrolled.resolved_at.is_none());
        assert!(enrolled.failed_at.is_none());
        assert!(enrolled.failure_reason.is_none());
    }

    #[tokio::test]
    async fn root_enrollment_preserves_resolved_generation() {
        let repo = InMemorySeriesScanStateRepository::default();
        let library_id = lib(8);
        let root = series_root("/demo/Shows/Resolved Then Reenrolled");
        let series_id = SeriesID(Uuid::from_u128(9));
        repo.mark_resolved(
            library_id,
            root.clone(),
            SeriesRef {
                id: series_id,
                slug: Some("resolved-then-reenrolled".into()),
                title: Some("Resolved Then Reenrolled".into()),
            },
        )
        .await
        .expect("series resolved");

        let enrolled = repo
            .enroll_resolution_generation(library_id, root, None)
            .await
            .expect("resolved root observed by enrollment");
        assert_eq!(enrolled.status, SeriesScanStatus::Resolved);
        assert_eq!(enrolled.series_id, Some(series_id));
        assert!(enrolled.resolved_at.is_some());
        assert!(enrolled.is_resolved());
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

    async fn enroll_resolution_generation(
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
                    hint.year.map(|value| value as i16),
                    hint.region.clone(),
                )
            })
            .unwrap_or((None, None, None, None));

        sqlx::query!(
            r#"
            INSERT INTO series_scan_state (
                library_id, series_root_path, status,
                series_title, series_slug, series_year, series_region,
                attempts, created_at, updated_at
            )
            VALUES (
                $1, $2, 'discovered'::series_scan_status,
                $3, $4, $5, $6, 0, NOW(), NOW()
            )
            ON CONFLICT (library_id, series_root_path)
            DO UPDATE SET
                series_title = COALESCE(
                    EXCLUDED.series_title,
                    series_scan_state.series_title
                ),
                series_slug = COALESCE(
                    EXCLUDED.series_slug,
                    series_scan_state.series_slug
                ),
                series_year = COALESCE(
                    EXCLUDED.series_year,
                    series_scan_state.series_year
                ),
                series_region = COALESCE(
                    EXCLUDED.series_region,
                    series_scan_state.series_region
                ),
                status = CASE
                    WHEN series_scan_state.status <> 'resolved'
                        THEN 'discovered'::series_scan_status
                    ELSE series_scan_state.status
                END,
                series_id = CASE
                    WHEN series_scan_state.status <> 'resolved' THEN NULL
                    ELSE series_scan_state.series_id
                END,
                resolved_at = CASE
                    WHEN series_scan_state.status <> 'resolved' THEN NULL
                    ELSE series_scan_state.resolved_at
                END,
                failed_at = CASE
                    WHEN series_scan_state.status <> 'resolved' THEN NULL
                    ELSE series_scan_state.failed_at
                END,
                failure_reason = CASE
                    WHEN series_scan_state.status <> 'resolved' THEN NULL
                    ELSE series_scan_state.failure_reason
                END,
                updated_at = NOW()
            "#,
            library_id.0,
            series_root_path.as_str(),
            title,
            slug,
            year,
            region,
        )
        .execute(&self.pool)
        .await?;

        self.get(library_id, &series_root_path)
            .await?
            .ok_or_else(|| {
                MediaError::Internal(format!(
                    "series resolution enrollment disappeared for {}",
                    series_root_path.as_str()
                ))
            })
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

        sqlx::query!(
            r#"
            INSERT INTO series_scan_state (
                library_id, series_root_path, status,
                series_title, series_slug, series_year, series_region,
                attempts, created_at, updated_at
            )
            VALUES (
                $1, $2, 'discovered'::series_scan_status,
                $3, $4, $5, $6, 0, NOW(), NOW()
            )
            ON CONFLICT (library_id, series_root_path)
            DO NOTHING
            "#,
            library_id.0,
            series_root_path.as_str(),
            title,
            slug,
            year,
            region,
        )
        .execute(&self.pool)
        .await?;

        self.get(library_id, &series_root_path)
            .await?
            .ok_or_else(|| {
                MediaError::Internal(format!(
                    "observed series state disappeared for {}",
                    series_root_path.as_str()
                ))
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
                failed_at = CASE
                    WHEN series_scan_state.status = 'resolved'
                        THEN series_scan_state.failed_at
                    ELSE NULL
                END,
                failure_reason = CASE
                    WHEN series_scan_state.status = 'resolved'
                        THEN series_scan_state.failure_reason
                    ELSE NULL
                END,
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
