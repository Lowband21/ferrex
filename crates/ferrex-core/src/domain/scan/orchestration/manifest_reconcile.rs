//! Manifest reconciliation into the existing scan pipeline.
//!
//! Manifest walks classify a whole root/partition at once. This module turns a
//! completed manifest scope into the same durable media effects that folder
//! scans already produce: media discoveries enqueue analyze work, unambiguous
//! fingerprint-preserving moves update `media_files` in place, and deletions are
//! represented as availability tombstones plus read-model cleanup.

use std::any::type_name;
use std::collections::BTreeSet;
use std::fmt;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use ferrex_model::{MediaID, VideoMediaType};
use uuid::Uuid;

use crate::database::repository_ports::manifest::{
    ManifestBatchUpsertSummary, ManifestRepository, ManifestRunCompletion,
};
use crate::domain::scan::actors::messages::{
    MediaFileDiscovered, MediaKindHint,
};
use crate::domain::scan::manifest::{
    ManifestEntryBatch, ManifestEntryKind, ManifestFingerprint,
    ManifestLogicalContext, ManifestRun, ManifestRunStatus, ManifestScope,
    ManifestSupportedMedia,
};
use crate::domain::scan::orchestration::context::{
    EpisodeHint, EpisodeLink, EpisodeScanHierarchy, FolderScanContext,
    MovieFolderScanContext, MovieRootPath, MovieScanHierarchy, ScanNodeKind,
    SeasonFolderPath, SeasonFolderScanContext, SeasonLink, SeasonScanHierarchy,
    SeriesFolderScanContext, SeriesHint, SeriesLink, SeriesRootPath,
};
use crate::domain::scan::orchestration::delta::{
    StoredMediaFile, reconcile_direct_media,
};
use crate::domain::scan::orchestration::events::{
    ScanEvent, ScanEventPublisher,
};
use crate::domain::scan::orchestration::job::{
    EnqueueRequest, JobPayload, JobPriority, MediaAnalyzeJob, MediaFingerprint,
    ScanReason,
};
use crate::domain::scan::orchestration::queue::QueueService;
use crate::domain::scan::orchestration::scan_cursor::ScanCursorRepository;
use crate::error::{MediaError, Result};
use crate::types::ids::LibraryId;

/// Repository effects needed to reconcile manifest media against `media_files`.
#[async_trait]
pub trait ManifestMediaRepository: Send + Sync {
    /// Return stored media rows that are relevant for the completed manifest
    /// scope. Root and prefix-scoped runs should return the whole scope so
    /// deletions and moves can be detected. Prefix-less synthetic partitions may
    /// safely return only `observed_paths` to avoid tombstoning paths the run did
    /// not cover.
    async fn list_media_for_manifest_reconciliation(
        &self,
        scope: &ManifestScope,
        observed_paths: &[String],
    ) -> Result<Vec<StoredMediaFile>>;

    async fn move_media_by_path(
        &self,
        library_id: LibraryId,
        old_path_norm: &str,
        new_path_norm: &str,
        fingerprint: &MediaFingerprint,
    ) -> Result<Uuid>;

    async fn mark_available_with_fingerprint(
        &self,
        library_id: LibraryId,
        path_norm: &str,
        fingerprint: &MediaFingerprint,
    ) -> Result<()>;

    async fn mark_unavailable_by_paths(
        &self,
        library_id: LibraryId,
        paths: Vec<String>,
        reason: &str,
    ) -> Result<u64>;

    async fn mark_unavailable_by_prefixes(
        &self,
        library_id: LibraryId,
        prefixes: Vec<String>,
        reason: &str,
    ) -> Result<u64>;

    async fn delete_folder_inventory_by_prefixes(
        &self,
        library_id: LibraryId,
        prefixes: Vec<String>,
    ) -> Result<u64>;
}

/// Input for reconciling one manifest run.
#[derive(Clone, Debug)]
pub struct ManifestReconcileInput {
    pub run_id: Uuid,
    pub scope: ManifestScope,
    pub batches: Vec<ManifestEntryBatch>,
    pub supported_media: Vec<ManifestSupportedMedia>,
    pub status: ManifestRunStatus,
    pub started_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,
    pub scan_reason: ScanReason,
    pub error_message: Option<String>,
}

impl ManifestReconcileInput {
    pub fn successful(
        run_id: Uuid,
        scope: ManifestScope,
        batches: Vec<ManifestEntryBatch>,
        supported_media: Vec<ManifestSupportedMedia>,
        scan_reason: ScanReason,
    ) -> Self {
        let diagnostics_seen = diagnostics_seen(&batches);
        Self {
            run_id,
            scope,
            batches,
            supported_media,
            status: if diagnostics_seen > 0 {
                ManifestRunStatus::CompletedWithDiagnostics
            } else {
                ManifestRunStatus::Completed
            },
            started_at: Utc::now(),
            completed_at: Utc::now(),
            scan_reason,
            error_message: None,
        }
    }
}

/// Aggregate effect counts from a manifest reconciliation pass.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ManifestReconciliationSummary {
    pub entries_upserted: u64,
    pub diagnostics_upserted: u64,
    pub entries_seen: u64,
    pub diagnostics_seen: u64,
    pub supported_media_seen: u64,
    pub unchanged_media: u64,
    pub media_enqueued: u64,
    pub media_moved: u64,
    pub media_tombstoned: u64,
    pub manifest_entries_marked_missing: u64,
    pub stale_cursor_rows_deleted: u64,
    pub stale_folder_inventory_rows_deleted: u64,
    pub tombstone_prefixes: u64,
    pub diagnostic_entries_recorded: u64,
    pub ignored_entries_recorded: u64,
}

/// Service that reconciles completed manifest scopes.
pub struct ManifestReconciler<M, R, Q, E, C>
where
    M: ManifestRepository + ?Sized,
    R: ManifestMediaRepository + ?Sized,
    Q: QueueService + ?Sized,
    E: ScanEventPublisher + ?Sized,
    C: ScanCursorRepository + ?Sized,
{
    manifest: Arc<M>,
    media: Arc<R>,
    queue: Arc<Q>,
    events: Arc<E>,
    cursors: Arc<C>,
}

impl<M, R, Q, E, C> fmt::Debug for ManifestReconciler<M, R, Q, E, C>
where
    M: ManifestRepository + ?Sized,
    R: ManifestMediaRepository + ?Sized,
    Q: QueueService + ?Sized,
    E: ScanEventPublisher + ?Sized,
    C: ScanCursorRepository + ?Sized,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ManifestReconciler")
            .field("manifest", &type_name::<M>())
            .field("media", &type_name::<R>())
            .field("queue", &type_name::<Q>())
            .field("events", &type_name::<E>())
            .field("cursors", &type_name::<C>())
            .finish()
    }
}

impl<M, R, Q, E, C> ManifestReconciler<M, R, Q, E, C>
where
    M: ManifestRepository + ?Sized,
    R: ManifestMediaRepository + ?Sized,
    Q: QueueService + ?Sized,
    E: ScanEventPublisher + ?Sized,
    C: ScanCursorRepository + ?Sized,
{
    pub fn new(
        manifest: Arc<M>,
        media: Arc<R>,
        queue: Arc<Q>,
        events: Arc<E>,
        cursors: Arc<C>,
    ) -> Self {
        Self {
            manifest,
            media,
            queue,
            events,
            cursors,
        }
    }

    /// Persist manifest entries, complete the run, and reconcile resulting
    /// media changes. Tombstones are applied only after the run completes with a
    /// successful status.
    pub async fn reconcile_run(
        &self,
        input: ManifestReconcileInput,
    ) -> Result<ManifestReconciliationSummary> {
        validate_terminal_status(input.status)?;

        let entries_seen = entries_seen(&input.batches);
        let diagnostics_seen = diagnostics_seen(&input.batches);
        let mut summary = ManifestReconciliationSummary {
            entries_seen,
            diagnostics_seen,
            supported_media_seen: input.supported_media.len() as u64,
            diagnostic_entries_recorded: diagnostic_entries(&input.batches),
            ignored_entries_recorded: ignored_entries(&input.batches),
            ..ManifestReconciliationSummary::default()
        };

        self.manifest
            .start_run(ManifestRun {
                run_id: input.run_id,
                scope: input.scope.clone(),
                status: ManifestRunStatus::Running,
                started_at: input.started_at,
                completed_at: None,
                entries_seen: 0,
                diagnostics_seen: 0,
            })
            .await?;

        for batch in &input.batches {
            let batch_summary = self
                .manifest
                .upsert_batch_entries(input.run_id, batch)
                .await?;
            merge_batch_summary(&mut summary, batch_summary);
        }

        let completed = self
            .manifest
            .complete_run(ManifestRunCompletion {
                run_id: input.run_id,
                status: input.status,
                completed_at: input.completed_at,
                entries_seen,
                diagnostics_seen,
                error_message: input.error_message.clone(),
            })
            .await?;

        if !is_successful_manifest_status(completed.status) {
            return Ok(summary);
        }

        self.reconcile_successful_media(&input, &mut summary)
            .await?;
        self.reconcile_successful_missing_entries(&input, &mut summary)
            .await?;

        Ok(summary)
    }

    async fn reconcile_successful_media(
        &self,
        input: &ManifestReconcileInput,
        summary: &mut ManifestReconciliationSummary,
    ) -> Result<()> {
        let current = input
            .supported_media
            .iter()
            .map(|media| {
                manifest_supported_media_to_discovered(
                    media,
                    input.scan_reason,
                    None,
                )
            })
            .collect::<Result<Vec<_>>>()?;
        let observed_paths: Vec<String> = current
            .iter()
            .map(|media| media.path_norm.clone())
            .collect();
        let stored = self
            .media
            .list_media_for_manifest_reconciliation(
                &input.scope,
                &observed_paths,
            )
            .await?;
        let mut delta = reconcile_direct_media(stored, current);

        for move_delta in &delta.moves {
            self.media
                .move_media_by_path(
                    input.scope.library_id(),
                    &move_delta.old_path_norm,
                    &move_delta.new_path_norm,
                    &move_delta.fingerprint,
                )
                .await?;
            summary.media_moved += 1;
        }

        for media in &delta.modifications {
            self.media
                .mark_available_with_fingerprint(
                    media.library_id,
                    &media.path_norm,
                    &media.fingerprint,
                )
                .await?;
        }

        let pipeline_media = delta.media_requiring_pipeline();
        for media in pipeline_media {
            self.publish_and_enqueue_media(media).await?;
            summary.media_enqueued += 1;
        }

        let removed_paths: Vec<String> = delta
            .removals
            .drain(..)
            .filter(|media| media.is_available)
            .map(|media| media.path_norm)
            .collect();
        summary.media_tombstoned += self
            .media
            .mark_unavailable_by_paths(
                input.scope.library_id(),
                dedup_sorted(removed_paths),
                "manifest_scope_file_missing",
            )
            .await?;

        summary.unchanged_media += delta.unchanged.len() as u64;
        Ok(())
    }

    async fn reconcile_successful_missing_entries(
        &self,
        input: &ManifestReconcileInput,
        summary: &mut ManifestReconciliationSummary,
    ) -> Result<()> {
        let missing_entries = self
            .manifest
            .mark_missing_entries_after_successful_run(input.run_id)
            .await?;
        summary.manifest_entries_marked_missing = missing_entries.len() as u64;

        let mut missing_files = Vec::new();
        let mut missing_prefixes = Vec::new();
        for entry in missing_entries {
            match entry.entry_kind {
                ManifestEntryKind::File => missing_files.push(entry.path_norm),
                ManifestEntryKind::Directory => {
                    missing_prefixes.push(entry.path_norm)
                }
            }
        }

        summary.media_tombstoned += self
            .media
            .mark_unavailable_by_paths(
                input.scope.library_id(),
                dedup_sorted(missing_files),
                "manifest_entry_file_missing",
            )
            .await?;

        let missing_prefixes = dedup_sorted(missing_prefixes);
        summary.tombstone_prefixes = missing_prefixes.len() as u64;
        if missing_prefixes.is_empty() {
            return Ok(());
        }

        summary.media_tombstoned += self
            .media
            .mark_unavailable_by_prefixes(
                input.scope.library_id(),
                missing_prefixes.clone(),
                "manifest_entry_folder_missing",
            )
            .await?;
        summary.stale_folder_inventory_rows_deleted = self
            .media
            .delete_folder_inventory_by_prefixes(
                input.scope.library_id(),
                missing_prefixes.clone(),
            )
            .await?;
        summary.stale_cursor_rows_deleted = self
            .cursors
            .delete_by_path_prefixes(input.scope.library_id(), missing_prefixes)
            .await? as u64;

        Ok(())
    }

    async fn publish_and_enqueue_media(
        &self,
        media: MediaFileDiscovered,
    ) -> Result<()> {
        self.events
            .publish_scan_event(ScanEvent::MediaFileDiscovered(Box::new(
                media.clone(),
            )))
            .await?;

        let analyze = MediaAnalyzeJob {
            library_id: media.library_id,
            path_norm: media.path_norm.clone(),
            fingerprint: media.fingerprint.clone(),
            discovered_at: Utc::now(),
            media_id: media.media_id,
            variant: media.variant,
            hierarchy: media.hierarchy.clone(),
            node: media.node.clone(),
            scan_reason: media.scan_reason,
        };
        let req = EnqueueRequest::new(
            priority_for_reason(&media.scan_reason).elevate(JobPriority::P0),
            JobPayload::MediaAnalyze(analyze),
        );
        self.queue.enqueue(req).await?;
        Ok(())
    }
}

/// Returns true for manifest statuses that safely completed scope observation.
pub fn is_successful_manifest_status(status: ManifestRunStatus) -> bool {
    matches!(
        status,
        ManifestRunStatus::Completed
            | ManifestRunStatus::CompletedWithDiagnostics
    )
}

/// Prefix covered by a manifest scope when that scope is safe for recursive
/// reconciliation. Prefix-less synthetic partitions intentionally return None.
pub fn manifest_scope_reconciliation_prefix(
    scope: &ManifestScope,
) -> Option<&str> {
    match scope {
        ManifestScope::Root(root) => Some(root.root_path_norm.as_str()),
        ManifestScope::Partition(partition) => partition.prefix_norm.as_deref(),
    }
}

pub fn manifest_fingerprint_to_media(
    fingerprint: &ManifestFingerprint,
) -> MediaFingerprint {
    MediaFingerprint {
        device_id: fingerprint.device_id.map(|id| id.to_string()),
        inode: fingerprint.inode,
        size: fingerprint.size,
        mtime: fingerprint.mtime_ms.unwrap_or_default(),
        weak_hash: fingerprint.weak_hash.clone(),
    }
}

pub fn manifest_supported_media_to_discovered(
    media: &ManifestSupportedMedia,
    scan_reason: ScanReason,
    existing_media_id: Option<MediaID>,
) -> Result<MediaFileDiscovered> {
    let fingerprint = manifest_fingerprint_to_media(&media.fingerprint);

    match &media.context {
        ManifestLogicalContext::Movie(context) => {
            let movie_root_path = context
                .movie_folder_path_norm
                .as_deref()
                .unwrap_or(media.path_norm.as_str());
            let movie_root_path = MovieRootPath::try_new(movie_root_path)?;
            let hierarchy = MovieScanHierarchy {
                movie_root_path: movie_root_path.clone(),
                movie_id: None,
                extra_tag: None,
            };
            let media_id = existing_media_id
                .unwrap_or_else(|| MediaID::new(VideoMediaType::Movie));
            Ok(MediaFileDiscovered {
                library_id: media.scope.library_id(),
                path_norm: media.path_norm.clone(),
                fingerprint,
                classified_as: MediaKindHint::Movie,
                media_id,
                variant: VideoMediaType::Movie,
                node: ScanNodeKind::MovieFolder,
                hierarchy: crate::domain::scan::AnalyzeScanHierarchy::Movie(
                    hierarchy,
                ),
                context: FolderScanContext::Movie(MovieFolderScanContext {
                    library_id: media.scope.library_id(),
                    movie_root_path,
                }),
                scan_reason,
            })
        }
        ManifestLogicalContext::Episode(context) => {
            let series_root_path =
                SeriesRootPath::try_new(context.series_root_path_norm.clone())?;
            let series_hint = SeriesHint {
                title: context.series_title_hint.clone(),
                slug: None,
                year: None,
                region: None,
            };
            let season_hierarchy = SeasonScanHierarchy {
                series_root_path: series_root_path.clone(),
                series: SeriesLink::Hint(series_hint),
                season: SeasonLink::Number(context.season_number),
            };
            let hierarchy = EpisodeScanHierarchy::from_season_hierarch(
                season_hierarchy,
                EpisodeLink::Hint(EpisodeHint {
                    number: context.episode_number,
                    title: context.episode_title_hint.clone(),
                }),
            );
            let folder_context = if let Some(season_folder_path_norm) =
                &context.season_folder_path_norm
            {
                let (season_folder_path, parsed_season) =
                    SeasonFolderPath::try_new_under_series_root(
                        &series_root_path,
                        season_folder_path_norm.clone(),
                    )?;
                if parsed_season != context.season_number {
                    return Err(MediaError::InvalidMedia(format!(
                        "manifest episode season mismatch for {} (context S{:02}, folder S{:02})",
                        media.path_norm, context.season_number, parsed_season
                    )));
                }
                FolderScanContext::Season(SeasonFolderScanContext {
                    library_id: media.scope.library_id(),
                    series_root_path: series_root_path.clone(),
                    season_folder_path,
                    season_number: context.season_number,
                })
            } else {
                FolderScanContext::Series(SeriesFolderScanContext {
                    library_id: media.scope.library_id(),
                    series_root_path: series_root_path.clone(),
                })
            };
            let media_id = existing_media_id
                .unwrap_or_else(|| MediaID::new(VideoMediaType::Episode));
            Ok(MediaFileDiscovered {
                library_id: media.scope.library_id(),
                path_norm: media.path_norm.clone(),
                fingerprint,
                classified_as: MediaKindHint::Episode,
                media_id,
                variant: VideoMediaType::Episode,
                node: ScanNodeKind::EpisodeFile,
                hierarchy: crate::domain::scan::AnalyzeScanHierarchy::Episode(
                    hierarchy,
                ),
                context: folder_context,
                scan_reason,
            })
        }
    }
}

fn priority_for_reason(reason: &ScanReason) -> JobPriority {
    match reason {
        ScanReason::HotChange | ScanReason::WatcherOverflow => JobPriority::P0,
        ScanReason::UserRequested | ScanReason::BulkSeed => JobPriority::P1,
        ScanReason::MaintenanceSweep => JobPriority::P2,
    }
}

fn validate_terminal_status(status: ManifestRunStatus) -> Result<()> {
    match status {
        ManifestRunStatus::Pending | ManifestRunStatus::Running => {
            Err(MediaError::InvalidMedia(format!(
                "manifest reconciliation requires a terminal run status, got {status:?}"
            )))
        }
        ManifestRunStatus::Completed
        | ManifestRunStatus::CompletedWithDiagnostics
        | ManifestRunStatus::Failed
        | ManifestRunStatus::Canceled
        | ManifestRunStatus::Stalled => Ok(()),
    }
}

fn merge_batch_summary(
    summary: &mut ManifestReconciliationSummary,
    batch_summary: ManifestBatchUpsertSummary,
) {
    summary.entries_upserted += batch_summary.entries_upserted;
    summary.diagnostics_upserted += batch_summary.diagnostics_upserted;
}

fn entries_seen(batches: &[ManifestEntryBatch]) -> u64 {
    batches.iter().map(|batch| batch.entries.len() as u64).sum()
}

fn diagnostics_seen(batches: &[ManifestEntryBatch]) -> u64 {
    batches
        .iter()
        .flat_map(|batch| batch.entries.iter())
        .map(|entry| entry.diagnostics().len() as u64)
        .sum()
}

fn diagnostic_entries(batches: &[ManifestEntryBatch]) -> u64 {
    batches
        .iter()
        .flat_map(|batch| batch.entries.iter())
        .filter(|entry| {
            matches!(
                entry.classification(),
                crate::domain::scan::manifest::ManifestEntryClassification::Unsupported(_)
            )
        })
        .count() as u64
}

fn ignored_entries(batches: &[ManifestEntryBatch]) -> u64 {
    batches
        .iter()
        .flat_map(|batch| batch.entries.iter())
        .filter(|entry| {
            matches!(
                entry.classification(),
                crate::domain::scan::manifest::ManifestEntryClassification::Ignored(_)
            )
        })
        .count() as u64
}

fn dedup_sorted(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::repository_ports::manifest::{
        ManifestBackfillSummary, ManifestDeferredWatchHintFilter,
        ManifestDeferredWatchHintInput, ManifestDeferredWatchHintRecord,
        ManifestDeferredWatchHintStatus, ManifestDiagnosticFilter,
        ManifestDiagnosticRecord, ManifestMissingEntryRecord,
        ManifestPartitionCursorRecord,
    };
    use crate::domain::scan::manifest::{
        ManifestDiagnostic, ManifestDiagnosticReason,
        ManifestEntryClassification, ManifestMediaEntry, ManifestRootId,
        ManifestRootScope, ManifestSupportedClassification,
    };
    use crate::domain::scan::orchestration::delta::fingerprints_equivalent;
    use crate::domain::scan::orchestration::job::{JobHandle, JobId, JobKind};
    use crate::domain::scan::orchestration::lease::{
        DequeueRequest, JobLease, LeaseId, LeaseRenewal,
    };
    use crate::domain::scan::orchestration::queue::QueueService;
    use crate::domain::scan::orchestration::scan_cursor::{
        ScanCursor, ScanCursorId,
    };
    use crate::error::Result;
    use crate::types::library::LibraryType;
    use std::path::Path;
    use std::sync::Mutex;

    #[derive(Default)]
    struct FakeManifestRepository {
        upserted: Mutex<Vec<ManifestEntryBatch>>,
        completed: Mutex<Vec<ManifestRunCompletion>>,
        missing: Mutex<Vec<ManifestMissingEntryRecord>>,
        mark_missing_calls: Mutex<u64>,
    }

    #[async_trait]
    impl ManifestRepository for FakeManifestRepository {
        async fn start_run(&self, run: ManifestRun) -> Result<ManifestRun> {
            Ok(run)
        }

        async fn upsert_batch_entries(
            &self,
            _run_id: Uuid,
            batch: &ManifestEntryBatch,
        ) -> Result<ManifestBatchUpsertSummary> {
            self.upserted.lock().unwrap().push(batch.clone());
            Ok(ManifestBatchUpsertSummary {
                entries_upserted: batch.entries.len() as u64,
                diagnostics_upserted: diagnostics_seen(std::slice::from_ref(
                    batch,
                )),
            })
        }

        async fn complete_run(
            &self,
            completion: ManifestRunCompletion,
        ) -> Result<ManifestRun> {
            self.completed.lock().unwrap().push(completion.clone());
            Ok(ManifestRun {
                run_id: completion.run_id,
                scope: scope(),
                status: completion.status,
                started_at: Utc::now(),
                completed_at: Some(completion.completed_at),
                entries_seen: completion.entries_seen,
                diagnostics_seen: completion.diagnostics_seen,
            })
        }

        async fn mark_missing_entries_after_successful_run(
            &self,
            _run_id: Uuid,
        ) -> Result<Vec<ManifestMissingEntryRecord>> {
            *self.mark_missing_calls.lock().unwrap() += 1;
            Ok(self.missing.lock().unwrap().clone())
        }

        async fn list_stale_partitions(
            &self,
            _library_id: LibraryId,
            _older_than: DateTime<Utc>,
            _limit: u32,
        ) -> Result<Vec<ManifestPartitionCursorRecord>> {
            Ok(Vec::new())
        }

        async fn list_diagnostics(
            &self,
            _filter: ManifestDiagnosticFilter,
        ) -> Result<Vec<ManifestDiagnosticRecord>> {
            Ok(Vec::new())
        }

        async fn upsert_deferred_watch_hint(
            &self,
            _hint: ManifestDeferredWatchHintInput,
        ) -> Result<ManifestDeferredWatchHintRecord> {
            Err(MediaError::Internal("unused fake method".into()))
        }

        async fn list_deferred_watch_hints(
            &self,
            _filter: ManifestDeferredWatchHintFilter,
        ) -> Result<Vec<ManifestDeferredWatchHintRecord>> {
            Ok(Vec::new())
        }

        async fn update_deferred_watch_hint_status(
            &self,
            _id: Uuid,
            _status: ManifestDeferredWatchHintStatus,
            _last_error: Option<String>,
        ) -> Result<Option<ManifestDeferredWatchHintRecord>> {
            Ok(None)
        }

        async fn backfill_legacy_manifest_state(
            &self,
            _library_id: Option<LibraryId>,
        ) -> Result<ManifestBackfillSummary> {
            Ok(ManifestBackfillSummary::default())
        }
    }

    #[derive(Default)]
    struct FakeMediaRepository {
        stored: Mutex<Vec<StoredMediaFile>>,
        moves: Mutex<Vec<(String, String)>>,
        available: Mutex<Vec<String>>,
        unavailable_paths: Mutex<Vec<String>>,
        unavailable_prefixes: Mutex<Vec<String>>,
        folder_inventory_deleted: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl ManifestMediaRepository for FakeMediaRepository {
        async fn list_media_for_manifest_reconciliation(
            &self,
            _scope: &ManifestScope,
            _observed_paths: &[String],
        ) -> Result<Vec<StoredMediaFile>> {
            Ok(self.stored.lock().unwrap().clone())
        }

        async fn move_media_by_path(
            &self,
            _library_id: LibraryId,
            old_path_norm: &str,
            new_path_norm: &str,
            _fingerprint: &MediaFingerprint,
        ) -> Result<Uuid> {
            self.moves
                .lock()
                .unwrap()
                .push((old_path_norm.to_string(), new_path_norm.to_string()));
            Ok(Uuid::now_v7())
        }

        async fn mark_available_with_fingerprint(
            &self,
            _library_id: LibraryId,
            path_norm: &str,
            _fingerprint: &MediaFingerprint,
        ) -> Result<()> {
            self.available.lock().unwrap().push(path_norm.to_string());
            Ok(())
        }

        async fn mark_unavailable_by_paths(
            &self,
            _library_id: LibraryId,
            paths: Vec<String>,
            _reason: &str,
        ) -> Result<u64> {
            let count = paths.len() as u64;
            self.unavailable_paths.lock().unwrap().extend(paths);
            Ok(count)
        }

        async fn mark_unavailable_by_prefixes(
            &self,
            _library_id: LibraryId,
            prefixes: Vec<String>,
            _reason: &str,
        ) -> Result<u64> {
            let count = prefixes.len() as u64;
            self.unavailable_prefixes.lock().unwrap().extend(prefixes);
            Ok(count)
        }

        async fn delete_folder_inventory_by_prefixes(
            &self,
            _library_id: LibraryId,
            prefixes: Vec<String>,
        ) -> Result<u64> {
            let count = prefixes.len() as u64;
            self.folder_inventory_deleted
                .lock()
                .unwrap()
                .extend(prefixes);
            Ok(count)
        }
    }

    #[derive(Default)]
    struct FakeQueue {
        enqueued: Mutex<Vec<EnqueueRequest>>,
    }

    #[async_trait]
    impl QueueService for FakeQueue {
        async fn enqueue(&self, request: EnqueueRequest) -> Result<JobHandle> {
            let handle = JobHandle {
                job_id: JobId::new(),
                kind: request.payload.kind(),
                dedupe_key: request.payload.dedupe_key().to_string(),
                library_id: request.payload.library_id(),
                priority: request.priority,
                accepted: true,
                merged_into: None,
            };
            self.enqueued.lock().unwrap().push(request);
            Ok(handle)
        }

        async fn dequeue(
            &self,
            _request: DequeueRequest,
        ) -> Result<Option<JobLease>> {
            Ok(None)
        }

        async fn renew(&self, _renewal: LeaseRenewal) -> Result<JobLease> {
            Err(MediaError::Internal("unused fake method".into()))
        }

        async fn complete(&self, _lease_id: LeaseId) -> Result<()> {
            Ok(())
        }

        async fn fail(
            &self,
            _lease_id: LeaseId,
            _retryable: bool,
            _error: Option<String>,
        ) -> Result<()> {
            Ok(())
        }

        async fn dead_letter(
            &self,
            _lease_id: LeaseId,
            _error: Option<String>,
        ) -> Result<()> {
            Ok(())
        }

        async fn cancel_job(&self, _job_id: JobId) -> Result<()> {
            Ok(())
        }

        async fn queue_depth(&self, _kind: JobKind) -> Result<usize> {
            Ok(0)
        }

        async fn release_dependency(
            &self,
            _library_id: LibraryId,
            _dependency_key: &crate::domain::scan::orchestration::job::DependencyKey,
        ) -> Result<u64> {
            Ok(0)
        }
    }

    #[derive(Default)]
    struct FakeEvents {
        scan_events: Mutex<Vec<ScanEvent>>,
    }

    #[async_trait]
    impl ScanEventPublisher for FakeEvents {
        async fn publish_scan_event(&self, event: ScanEvent) -> Result<()> {
            self.scan_events.lock().unwrap().push(event);
            Ok(())
        }
    }

    #[derive(Default)]
    struct FakeCursors {
        deleted_prefixes: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl ScanCursorRepository for FakeCursors {
        async fn get(&self, _id: &ScanCursorId) -> Result<Option<ScanCursor>> {
            Ok(None)
        }

        async fn list_by_library(
            &self,
            _library_id: LibraryId,
        ) -> Result<Vec<ScanCursor>> {
            Ok(Vec::new())
        }

        async fn upsert(&self, _cursor: ScanCursor) -> Result<()> {
            Ok(())
        }

        async fn delete_by_library(
            &self,
            _library_id: LibraryId,
        ) -> Result<usize> {
            Ok(0)
        }

        async fn delete_by_path_prefixes(
            &self,
            _library_id: LibraryId,
            prefixes: Vec<String>,
        ) -> Result<usize> {
            let count = prefixes.len();
            self.deleted_prefixes.lock().unwrap().extend(prefixes);
            Ok(count)
        }

        async fn list_stale(
            &self,
            _library_id: LibraryId,
            _older_than: DateTime<Utc>,
        ) -> Result<Vec<ScanCursor>> {
            Ok(Vec::new())
        }
    }

    fn library_id() -> LibraryId {
        LibraryId(Uuid::from_u128(0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa))
    }

    fn scope() -> ManifestScope {
        ManifestScope::Root(ManifestRootScope {
            library_id: library_id(),
            library_type: LibraryType::Movies,
            root_id: ManifestRootId(0),
            root_path_norm: "/library".to_string(),
        })
    }

    fn fp(size: u64, mtime: i64) -> ManifestFingerprint {
        ManifestFingerprint {
            size,
            mtime_ms: Some(mtime),
            ..ManifestFingerprint::default()
        }
    }

    fn media_fp(size: u64, mtime: i64) -> MediaFingerprint {
        MediaFingerprint {
            size,
            mtime,
            ..MediaFingerprint::default()
        }
    }

    fn supported_movie(
        path: &str,
        fingerprint: ManifestFingerprint,
    ) -> ManifestSupportedMedia {
        ManifestSupportedMedia {
            scope: scope(),
            path_norm: path.to_string(),
            relative_path: path.trim_start_matches("/library/").to_string(),
            fingerprint,
            classification: ManifestSupportedClassification::MovieRootMedia,
            context: ManifestLogicalContext::Movie(
                crate::domain::scan::manifest::ManifestMovieContext {
                    title_hint: Path::new(path)
                        .file_stem()
                        .and_then(|name| name.to_str())
                        .unwrap_or("Movie")
                        .to_string(),
                    movie_folder_path_norm: None,
                },
            ),
        }
    }

    fn media_entry(
        path: &str,
        classification: ManifestEntryClassification,
        diagnostics: Vec<ManifestDiagnostic>,
    ) -> crate::domain::scan::manifest::ManifestEntry {
        crate::domain::scan::manifest::ManifestEntry::Media(
            ManifestMediaEntry {
                scope: scope(),
                path_norm: path.to_string(),
                relative_path: path.trim_start_matches("/library/").to_string(),
                fingerprint: fp(1, 1),
                classification,
                diagnostics,
            },
        )
    }

    fn stored(path: &str, fingerprint: MediaFingerprint) -> StoredMediaFile {
        StoredMediaFile {
            id: Uuid::now_v7(),
            media_id: MediaID::new(VideoMediaType::Movie),
            path_norm: path.to_string(),
            fingerprint,
            is_available: true,
        }
    }

    type Fixture = (
        ManifestReconciler<
            FakeManifestRepository,
            FakeMediaRepository,
            FakeQueue,
            FakeEvents,
            FakeCursors,
        >,
        Arc<FakeManifestRepository>,
        Arc<FakeMediaRepository>,
        Arc<FakeQueue>,
        Arc<FakeEvents>,
        Arc<FakeCursors>,
    );

    fn fixture() -> Fixture {
        let manifest = Arc::new(FakeManifestRepository::default());
        let media = Arc::new(FakeMediaRepository::default());
        let queue = Arc::new(FakeQueue::default());
        let events = Arc::new(FakeEvents::default());
        let cursors = Arc::new(FakeCursors::default());
        let reconciler = ManifestReconciler::new(
            Arc::clone(&manifest),
            Arc::clone(&media),
            Arc::clone(&queue),
            Arc::clone(&events),
            Arc::clone(&cursors),
        );
        (reconciler, manifest, media, queue, events, cursors)
    }

    #[tokio::test]
    async fn add_and_modify_manifest_media_enqueue_analyze_pipeline() {
        let (reconciler, _manifest, media_repo, queue, events, _cursors) =
            fixture();
        media_repo
            .stored
            .lock()
            .unwrap()
            .push(stored("/library/Existing.mkv", media_fp(10, 20)));

        let input = ManifestReconcileInput::successful(
            Uuid::now_v7(),
            scope(),
            Vec::new(),
            vec![
                supported_movie("/library/New.mkv", fp(1, 1)),
                supported_movie("/library/Existing.mkv", fp(99, 30)),
            ],
            ScanReason::BulkSeed,
        );
        let summary = reconciler.reconcile_run(input).await.unwrap();

        assert_eq!(summary.media_enqueued, 2);
        assert_eq!(queue.enqueued.lock().unwrap().len(), 2);
        assert_eq!(events.scan_events.lock().unwrap().len(), 2);
        assert_eq!(
            media_repo.available.lock().unwrap().as_slice(),
            &["/library/Existing.mkv".to_string()]
        );
    }

    #[tokio::test]
    async fn fingerprint_preserving_move_updates_path_without_pipeline() {
        let (reconciler, _manifest, media_repo, queue, events, _cursors) =
            fixture();
        media_repo
            .stored
            .lock()
            .unwrap()
            .push(stored("/library/Old.mkv", media_fp(10, 20)));

        let input = ManifestReconcileInput::successful(
            Uuid::now_v7(),
            scope(),
            Vec::new(),
            vec![supported_movie("/library/New.mkv", fp(10, 20))],
            ScanReason::MaintenanceSweep,
        );
        let summary = reconciler.reconcile_run(input).await.unwrap();

        assert_eq!(summary.media_moved, 1);
        assert_eq!(summary.media_enqueued, 0);
        assert_eq!(queue.enqueued.lock().unwrap().len(), 0);
        assert_eq!(events.scan_events.lock().unwrap().len(), 0);
        assert_eq!(
            media_repo.moves.lock().unwrap().as_slice(),
            &[(
                "/library/Old.mkv".to_string(),
                "/library/New.mkv".to_string()
            )]
        );
    }

    #[tokio::test]
    async fn successful_delete_tombstones_media_and_cleans_stale_state() {
        let (reconciler, manifest, media_repo, _queue, _events, cursors) =
            fixture();
        media_repo
            .stored
            .lock()
            .unwrap()
            .push(stored("/library/Deleted/Movie.mkv", media_fp(10, 20)));
        manifest.missing.lock().unwrap().extend([
            ManifestMissingEntryRecord {
                library_id: library_id(),
                root_id: 0,
                partition_id: None,
                path_norm: "/library/Deleted".to_string(),
                entry_kind: ManifestEntryKind::Directory,
            },
            ManifestMissingEntryRecord {
                library_id: library_id(),
                root_id: 0,
                partition_id: None,
                path_norm: "/library/Deleted/Movie.mkv".to_string(),
                entry_kind: ManifestEntryKind::File,
            },
        ]);

        let input = ManifestReconcileInput::successful(
            Uuid::now_v7(),
            scope(),
            Vec::new(),
            Vec::new(),
            ScanReason::MaintenanceSweep,
        );
        let summary = reconciler.reconcile_run(input).await.unwrap();

        assert_eq!(summary.manifest_entries_marked_missing, 2);
        assert_eq!(summary.tombstone_prefixes, 1);
        assert!(
            media_repo
                .unavailable_paths
                .lock()
                .unwrap()
                .contains(&"/library/Deleted/Movie.mkv".to_string())
        );
        assert_eq!(
            media_repo.unavailable_prefixes.lock().unwrap().as_slice(),
            &["/library/Deleted".to_string()]
        );
        assert_eq!(
            media_repo
                .folder_inventory_deleted
                .lock()
                .unwrap()
                .as_slice(),
            &["/library/Deleted".to_string()]
        );
        assert_eq!(
            cursors.deleted_prefixes.lock().unwrap().as_slice(),
            &["/library/Deleted".to_string()]
        );
    }

    #[tokio::test]
    async fn failed_manifest_run_does_not_tombstone_or_cleanup() {
        let (reconciler, manifest, media_repo, _queue, _events, cursors) =
            fixture();
        media_repo
            .stored
            .lock()
            .unwrap()
            .push(stored("/library/Missing.mkv", media_fp(10, 20)));
        manifest
            .missing
            .lock()
            .unwrap()
            .push(ManifestMissingEntryRecord {
                library_id: library_id(),
                root_id: 0,
                partition_id: None,
                path_norm: "/library/Missing.mkv".to_string(),
                entry_kind: ManifestEntryKind::File,
            });

        let input = ManifestReconcileInput {
            run_id: Uuid::now_v7(),
            scope: scope(),
            batches: Vec::new(),
            supported_media: Vec::new(),
            status: ManifestRunStatus::Failed,
            started_at: Utc::now(),
            completed_at: Utc::now(),
            scan_reason: ScanReason::MaintenanceSweep,
            error_message: Some("walk failed".into()),
        };
        let summary = reconciler.reconcile_run(input).await.unwrap();

        assert_eq!(summary.media_tombstoned, 0);
        assert_eq!(*manifest.mark_missing_calls.lock().unwrap(), 0);
        assert!(media_repo.unavailable_paths.lock().unwrap().is_empty());
        assert!(cursors.deleted_prefixes.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn unsupported_diagnostics_persist_without_pipeline_work() {
        let (reconciler, _manifest, _media_repo, queue, events, _cursors) =
            fixture();
        let unsupported_path = "/library/Movie/Extras/Trailer.mkv";
        let batch = ManifestEntryBatch {
            scope: scope(),
            entries: vec![media_entry(
                unsupported_path,
                ManifestEntryClassification::Unsupported(
                    ManifestDiagnosticReason::MovieExtrasUnsupported,
                ),
                vec![ManifestDiagnostic::new(
                    unsupported_path,
                    ManifestDiagnosticReason::MovieExtrasUnsupported,
                )],
            )],
        };

        let input = ManifestReconcileInput::successful(
            Uuid::now_v7(),
            scope(),
            vec![batch],
            Vec::new(),
            ScanReason::BulkSeed,
        );
        let summary = reconciler.reconcile_run(input).await.unwrap();

        assert_eq!(summary.diagnostics_upserted, 1);
        assert_eq!(summary.diagnostic_entries_recorded, 1);
        assert!(queue.enqueued.lock().unwrap().is_empty());
        assert!(events.scan_events.lock().unwrap().is_empty());
    }

    #[test]
    fn manifest_media_conversion_supports_direct_series_root_episode() {
        let series_scope = ManifestScope::Root(ManifestRootScope {
            library_id: library_id(),
            library_type: LibraryType::Series,
            root_id: ManifestRootId(0),
            root_path_norm: "/series".to_string(),
        });
        let media = ManifestSupportedMedia {
            scope: series_scope,
            path_norm: "/series/Fringe/S01E01 - Pilot.mkv".to_string(),
            relative_path: "Fringe/S01E01 - Pilot.mkv".to_string(),
            fingerprint: fp(10, 20),
            classification:
                ManifestSupportedClassification::DirectSeriesRootEpisode {
                    season_number: 1,
                    episode_number: 1,
                    specials: false,
                },
            context: ManifestLogicalContext::Episode(
                crate::domain::scan::manifest::ManifestEpisodeContext {
                    series_title_hint: "Fringe".to_string(),
                    series_root_path_norm: "/series/Fringe".to_string(),
                    season_number: 1,
                    episode_number: 1,
                    specials: false,
                    season_folder_title_hint: None,
                    season_folder_path_norm: None,
                    episode_title_hint: Some("Pilot".to_string()),
                },
            ),
        };

        let discovered = manifest_supported_media_to_discovered(
            &media,
            ScanReason::BulkSeed,
            None,
        )
        .unwrap();
        assert_eq!(discovered.variant, VideoMediaType::Episode);
        assert!(matches!(discovered.context, FolderScanContext::Series(_)));
        assert!(matches!(
            discovered.hierarchy,
            crate::domain::scan::AnalyzeScanHierarchy::Episode(_)
        ));
    }

    #[test]
    fn fingerprint_equivalence_still_matches_manifest_converted_media() {
        let manifest = fp(42, 1234);
        let converted = manifest_fingerprint_to_media(&manifest);
        assert!(fingerprints_equivalent(&converted, &media_fp(42, 1234)));
    }
}
