use std::{any::type_name, fmt, path::PathBuf, sync::Arc};

use async_trait::async_trait;
use chrono::Utc;
use serde_json::Value;
use tracing::{debug, debug_span, warn};
use uuid::Uuid;

use crate::{
    error::{MediaError, Result},
    orchestration::{
        actors::{
            folder::{FolderScanActor, FolderScanCommand, FolderScanContext},
            messages::ParentDescriptors,
            pipeline::{
                ImageFetchActor, ImageFetchCommand, IndexCommand, IndexerActor, MediaAnalyzeActor,
                MediaAnalyzeCommand, MediaAnalyzed, MediaReadyForIndex, MetadataActor,
                MetadataCommand,
            },
        },
        correlation::CorrelationCache,
        events::{JobEvent, JobEventPayload, ScanEvent, ScanEventBus, stable_path_key},
        job::{
            EnqueueRequest, FolderScanJob, ImageFetchJob, IndexUpsertJob, JobHandle, JobPayload,
            JobPriority, MediaAnalyzeJob, MediaFingerprint, MetadataEnrichJob, ScanReason,
        },
        lease::JobLease,
        queue::QueueService,
        scan_cursor::{ScanCursor, ScanCursorId, ScanCursorRepository},
    },
};

fn priority_for_reason(reason: &ScanReason) -> JobPriority {
    match reason {
        ScanReason::HotChange | ScanReason::WatcherOverflow => JobPriority::P0,
        ScanReason::UserRequested | ScanReason::BulkSeed => JobPriority::P1,
        ScanReason::MaintenanceSweep => JobPriority::P2,
    }
}

/// Outcome of dispatcher execution for a single job.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DispatchStatus {
    Success,
    Retry { error: String },
    DeadLetter { error: String },
}

impl DispatchStatus {
    pub fn needs_retry(&self) -> bool {
        matches!(self, DispatchStatus::Retry { .. })
    }
}

/// Primary contract exposed to worker loops for executing leased jobs.
#[async_trait]
pub trait JobDispatcher: Send + Sync {
    async fn dispatch(&self, lease: &JobLease) -> DispatchStatus;
}

#[derive(Clone)]
pub struct DispatcherActors {
    pub folder: Arc<dyn FolderScanActor>,
    pub analyze: Arc<dyn MediaAnalyzeActor>,
    pub metadata: Arc<dyn MetadataActor>,
    pub indexer: Arc<dyn IndexerActor>,
    pub image: Arc<dyn ImageFetchActor>,
}

impl DispatcherActors {
    pub fn new(
        folder: Arc<dyn FolderScanActor>,
        analyze: Arc<dyn MediaAnalyzeActor>,
        metadata: Arc<dyn MetadataActor>,
        indexer: Arc<dyn IndexerActor>,
        image: Arc<dyn ImageFetchActor>,
    ) -> Self {
        Self {
            folder,
            analyze,
            metadata,
            indexer,
            image,
        }
    }
}

impl fmt::Debug for DispatcherActors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DispatcherActors")
            .field("folder", &"FolderScanActor")
            .field("analyze", &"MediaAnalyzeActor")
            .field("metadata", &"MetadataActor")
            .field("indexer", &"IndexerActor")
            .field("image", &"ImageFetchActor")
            .finish()
    }
}

pub struct DefaultJobDispatcher<Q, E, C>
where
    Q: QueueService + Send + Sync + 'static,
    E: ScanEventBus + Send + Sync + 'static,
    C: ScanCursorRepository + Send + Sync + 'static,
{
    queue: Arc<Q>,
    events: Arc<E>,
    cursors: Arc<C>,
    actors: DispatcherActors,
    correlations: CorrelationCache,
}

impl<Q, E, C> fmt::Debug for DefaultJobDispatcher<Q, E, C>
where
    Q: QueueService + Send + Sync + 'static,
    E: ScanEventBus + Send + Sync + 'static,
    C: ScanCursorRepository + Send + Sync + 'static,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DefaultJobDispatcher")
            .field("queue", &type_name::<Q>())
            .field("events", &type_name::<E>())
            .field("cursors", &type_name::<C>())
            .field("actors", &self.actors)
            .field("correlations", &self.correlations)
            .finish()
    }
}

impl<Q, E, C> DefaultJobDispatcher<Q, E, C>
where
    Q: QueueService + Send + Sync + 'static,
    E: ScanEventBus + Send + Sync + 'static,
    C: ScanCursorRepository + Send + Sync + 'static,
{
    pub fn new(
        queue: Arc<Q>,
        events: Arc<E>,
        cursors: Arc<C>,
        actors: DispatcherActors,
        correlations: CorrelationCache,
    ) -> Self {
        Self {
            queue,
            events,
            cursors,
            actors,
            correlations,
        }
    }

    fn handle_media_error(&self, err: MediaError) -> DispatchStatus {
        match err {
            MediaError::InvalidMedia(msg)
            | MediaError::NotFound(msg)
            | MediaError::Conflict(msg)
            | MediaError::Cancelled(msg) => {
                warn!(error = %msg, "dead-lettering job due to terminal data/intent error");
                DispatchStatus::DeadLetter { error: msg }
            }
            MediaError::Serialization(err) => {
                let msg = err.to_string();
                warn!(error = %msg, "dead-lettering job due to serialization error");
                DispatchStatus::DeadLetter { error: msg }
            }
            MediaError::Io(err) => {
                let msg = err.to_string();
                // Treat filesystem errors as terminal by default to avoid endless retries
                // on bad paths/permissions. Admins can resolve and rescan manually.
                warn!(error = %msg, "dead-lettering job due to filesystem error");
                DispatchStatus::DeadLetter { error: msg }
            }
            #[cfg(feature = "database")]
            MediaError::Database(err) => {
                let msg = err.to_string();
                warn!(error = %msg, "retrying job due to database error");
                DispatchStatus::Retry { error: msg }
            }
            MediaError::Internal(msg) => {
                let lower = msg.to_lowercase();
                let is_transient = lower.contains("timeout")
                    || lower.contains("timed out")
                    || lower.contains("temporar")
                    || lower.contains("connection")
                    || lower.contains("connect")
                    || lower.contains("too many requests")
                    || lower.contains("rate limit")
                    || lower.contains("503")
                    || lower.contains("unavailable");
                if is_transient {
                    warn!(error = %msg, "retrying job due to transient internal error");
                    DispatchStatus::Retry { error: msg }
                } else {
                    warn!(error = %msg, "dead-lettering job due to internal error");
                    DispatchStatus::DeadLetter { error: msg }
                }
            }
            other => {
                let msg = other.to_string();
                warn!(error = %msg, "dead-lettering job due to non-retryable error");
                DispatchStatus::DeadLetter { error: msg }
            }
        }
    }

    async fn publish_enqueue_event(
        &self,
        handle: &JobHandle,
        payload: &JobPayload,
        correlation_hint: Option<Uuid>,
    ) -> Result<()> {
        let path_key = stable_path_key(payload);

        if handle.accepted {
            let event = JobEvent::from_handle(
                handle,
                correlation_hint,
                JobEventPayload::Enqueued {
                    job_id: handle.job_id,
                    kind: handle.kind,
                    priority: handle.priority,
                },
                path_key,
            );
            self.correlations
                .remember(handle.job_id, event.meta.correlation_id)
                .await;
            self.events.publish(event).await
        } else if let Some(existing) = handle.merged_into {
            let existing_correlation = self.correlations.fetch(&existing).await;
            let event = JobEvent::from_handle(
                handle,
                existing_correlation.or(correlation_hint),
                JobEventPayload::Merged {
                    existing_job_id: existing,
                    merged_job_id: handle.job_id,
                    kind: handle.kind,
                    priority: handle.priority,
                },
                path_key,
            );
            self.correlations
                .remember_if_absent(handle.job_id, event.meta.correlation_id)
                .await;
            self.events.publish(event).await
        } else {
            Ok(())
        }
    }

    async fn enqueue_follow_up(&self, request: EnqueueRequest) -> DispatchStatus {
        let correlation_hint = request.correlation_id;

        match self.queue.enqueue(request.clone()).await {
            Ok(handle) => match self
                .publish_enqueue_event(&handle, &request.payload, correlation_hint)
                .await
            {
                Ok(()) => DispatchStatus::Success,
                Err(err) => self.handle_media_error(err),
            },
            Err(err) => self.handle_media_error(err),
        }
    }

    async fn enqueue_follow_up_many(&self, requests: Vec<EnqueueRequest>) -> DispatchStatus {
        if requests.is_empty() {
            return DispatchStatus::Success;
        }

        let cloned_requests = requests.clone();

        match self.queue.enqueue_many(cloned_requests).await {
            Ok(handles) => {
                for (handle, request) in handles.into_iter().zip(requests.into_iter()) {
                    if let Err(err) = self
                        .publish_enqueue_event(&handle, &request.payload, request.correlation_id)
                        .await
                    {
                        return self.handle_media_error(err);
                    }
                }
                DispatchStatus::Success
            }
            Err(err) => self.handle_media_error(err),
        }
    }

    fn parse_parent_context(raw: &Option<String>) -> ParentDescriptors {
        raw.as_ref()
            .and_then(|raw| serde_json::from_str::<ParentDescriptors>(raw).ok())
            .unwrap_or_default()
    }

    async fn handle_folder_scan(&self, lease: &JobLease, job: &FolderScanJob) -> DispatchStatus {
        let parent = Self::parse_parent_context(&job.parent_context);
        let context = FolderScanContext {
            library_id: job.library_id,
            folder_path_norm: job.folder_path_norm.clone(),
            parent,
            reason: job.scan_reason.clone(),
        };
        let command = FolderScanCommand {
            job: job.clone(),
            context: context.clone(),
        };

        let span = debug_span!(
            "\nfolder_scan",
            job_id = %lease.job.id.0,
            library = %job.library_id,
            path = %job.folder_path_norm
        );
        let _enter = span.enter();

        let plan = match self.actors.folder.plan_listing(&command).await {
            Ok(plan) => plan,
            Err(err) => return self.handle_media_error(err),
        };

        // Check cursor to short-circuit unchanged listings
        let cursor_id = ScanCursorId::new(
            job.library_id,
            &vec![PathBuf::from(job.folder_path_norm.clone())],
        );
        match self.cursors.get(&cursor_id).await {
            Ok(Some(existing)) if existing.listing_hash == plan.generated_listing_hash => {
                debug!("listing hash unchanged, refreshing cursor only");
                let cursor = ScanCursor {
                    id: cursor_id,
                    folder_path_norm: job.folder_path_norm.clone(),
                    listing_hash: plan.generated_listing_hash.clone(),
                    entry_count: plan.directories.len()
                        + plan.media_files.len()
                        + plan.ancillary_files.len(),
                    last_scan_at: Utc::now(),
                    last_modified_at: existing.last_modified_at,
                    device_id: job.device_id.clone(),
                };
                if let Err(err) = self.cursors.upsert(cursor).await {
                    return self.handle_media_error(err);
                }
                return DispatchStatus::Success;
            }
            Ok(_) => {}
            Err(err) => return self.handle_media_error(err),
        }

        let discovered = match self.actors.folder.discover_media(&plan, &context).await {
            Ok(files) => files,
            Err(err) => return self.handle_media_error(err),
        };
        let children = match self
            .actors
            .folder
            .derive_child_contexts(&plan, &context)
            .await
        {
            Ok(children) => children,
            Err(err) => return self.handle_media_error(err),
        };

        let summary = match self
            .actors
            .folder
            .finalize(&context, &plan, &discovered, &children)
        {
            Ok(summary) => summary,
            Err(err) => return self.handle_media_error(err),
        };

        let mut discovered_events = Vec::with_capacity(discovered.len());
        let mut followup_errors: Vec<String> = Vec::new();
        for media in &discovered {
            if let Err(err) = self
                .events
                .publish_scan_event(ScanEvent::MediaFileDiscovered(media.clone()))
                .await
            {
                // Continue discovering other items; collect error for admin visibility.
                tracing::warn!(
                    target: "scan::dispatch",
                    error = %err,
                    path = %media.path_norm,
                    "failed to publish MediaFileDiscovered; continuing"
                );
                followup_errors.push(format!("discover_event_failed:{}", media.path_norm));
                continue;
            }
            discovered_events.push(media.clone());

            let analyze_context = serde_json::json!({
                "library_type": media.context.parent.resolved_type,
                "media_kind": media.classified_as.as_str(),
                "parent": media.context.parent,
                "scan_reason": media.context.reason,
            });
            // Elevate analyze priority so per-item pipelines advance ahead of more scans.
            // This prevents breadth-first scanning from starving downstream stages.
            let analyze_priority = priority_for_reason(&media.context.reason)
                .elevate(crate::orchestration::job::JobPriority::P0);
            let analyze = MediaAnalyzeJob {
                library_id: media.library_id,
                path_norm: media.path_norm.clone(),
                fingerprint: media.fingerprint.clone(),
                discovered_at: Utc::now(),
                context: analyze_context,
                scan_reason: media.context.reason.clone(),
            };
            let req = EnqueueRequest::new(analyze_priority, JobPayload::MediaAnalyze(analyze));
            match self.enqueue_follow_up(req).await {
                DispatchStatus::Success => {}
                DispatchStatus::Retry { error } => {
                    tracing::warn!(
                        target: "scan::dispatch",
                        error = %error,
                        path = %media.path_norm,
                        "enqueue MediaAnalyze scheduled for retry; continuing"
                    );
                    followup_errors.push(format!("analyze_enqueue_retry:{}", media.path_norm));
                }
                DispatchStatus::DeadLetter { error } => {
                    tracing::warn!(
                        target: "scan::dispatch",
                        error = %error,
                        path = %media.path_norm,
                        "enqueue MediaAnalyze dead-lettered; continuing"
                    );
                    followup_errors.push(format!("analyze_enqueue_deadletter:{}", media.path_norm));
                }
            }
        }

        if let Err(err) = self
            .events
            .publish_scan_event(ScanEvent::FolderScanCompleted(summary.clone()))
            .await
        {
            return self.handle_media_error(err);
        }

        // Emit FolderDiscovered for each child; orchestrator enqueues from events.
        for child in &children {
            if let Err(err) = self
                .events
                .publish_scan_event(ScanEvent::FolderDiscovered {
                    library_id: child.library_id,
                    folder_path: child.folder_path_norm.clone(),
                    parent: child.parent.clone(),
                    reason: child.reason.clone(),
                })
                .await
            {
                tracing::warn!(
                    target: "scan::dispatch",
                    error = %err,
                    path = %child.folder_path_norm,
                    "failed to publish FolderDiscovered; continuing"
                );
                followup_errors.push(format!(
                    "folder_discovered_publish_failed:{}",
                    child.folder_path_norm
                ));
            }
        }

        let cursor = ScanCursor {
            id: cursor_id,
            folder_path_norm: job.folder_path_norm.clone(),
            listing_hash: plan.generated_listing_hash.clone(),
            entry_count: plan.directories.len()
                + plan.media_files.len()
                + plan.ancillary_files.len(),
            last_scan_at: Utc::now(),
            last_modified_at: None,
            device_id: job.device_id.clone(),
        };
        if let Err(err) = self.cursors.upsert(cursor).await {
            return self.handle_media_error(err);
        }

        if !followup_errors.is_empty() {
            // We intentionally succeed the folder scan while logging the issues.
            // Downstream jobs for other items/children continue to process.
            tracing::warn!(
                target: "scan::dispatch",
                count = followup_errors.len(),
                "folder scan encountered follow-up errors; marked success to continue"
            );
        }

        DispatchStatus::Success
    }

    async fn handle_media_analyze(&self, job: &MediaAnalyzeJob) -> DispatchStatus {
        let analyzed = match self
            .actors
            .analyze
            .analyze(MediaAnalyzeCommand { job: job.clone() })
            .await
        {
            Ok(result) => result,
            Err(err) => return self.handle_media_error(err),
        };

        if let Err(err) = self
            .events
            .publish_scan_event(ScanEvent::MediaAnalyzed(analyzed.clone()))
            .await
        {
            return self.handle_media_error(err);
        }

        let meta_job = MetadataEnrichJob {
            library_id: job.library_id,
            logical_candidate_id: job.path_norm.clone(),
            parse_fields: serde_json::json!({
                "path": job.path_norm,
                "context": analyzed.context,
                "fingerprint": analyzed.fingerprint,
            }),
            external_ids: None,
        };

        let priority = priority_for_reason(&job.scan_reason);

        // Prefer advancing metadata for already-discovered items over additional scans.
        let priority = priority.elevate(crate::orchestration::job::JobPriority::P0);
        let req = EnqueueRequest::new(priority, JobPayload::MetadataEnrich(meta_job));
        self.enqueue_follow_up(req).await
    }

    fn extract_fingerprint(value: &Value) -> MediaFingerprint {
        serde_json::from_value(value.clone()).unwrap_or(MediaFingerprint {
            device_id: None,
            inode: None,
            size: 0,
            mtime: 0,
            weak_hash: None,
        })
    }

    async fn handle_metadata_enrich(&self, job: &MetadataEnrichJob) -> DispatchStatus {
        let (path_norm, fingerprint, context) = match job.parse_fields.get("path") {
            Some(Value::String(path)) => {
                let fp = job
                    .parse_fields
                    .get("fingerprint")
                    .map(Self::extract_fingerprint)
                    .unwrap_or(MediaFingerprint {
                        device_id: None,
                        inode: None,
                        size: 0,
                        mtime: 0,
                        weak_hash: None,
                    });
                let ctx = job
                    .parse_fields
                    .get("context")
                    .cloned()
                    .unwrap_or(Value::Null);
                (path.clone(), fp, ctx)
            }
            _ => (
                job.logical_candidate_id.clone(),
                MediaFingerprint {
                    device_id: None,
                    inode: None,
                    size: 0,
                    mtime: 0,
                    weak_hash: None,
                },
                Value::Null,
            ),
        };

        let analyzed = MediaAnalyzed {
            library_id: job.library_id,
            path_norm: path_norm.clone(),
            fingerprint,
            analyzed_at: Utc::now(),
            streams_json: serde_json::json!({"placeholder": true}),
            thumbnails: vec![],
            context,
        };

        let ready = match self
            .actors
            .metadata
            .enrich(MetadataCommand {
                job: job.clone(),
                analyzed: analyzed.clone(),
            })
            .await
        {
            Ok(result) => result,
            Err(err) => return self.handle_media_error(err),
        };

        if let Err(err) = self
            .events
            .publish_scan_event(ScanEvent::MediaReadyForIndex(ready.clone()))
            .await
        {
            return self.handle_media_error(err);
        }

        if !ready.image_jobs.is_empty() {
            let image_requests: Vec<EnqueueRequest> = ready
                .image_jobs
                .iter()
                .map(|fetch_job| {
                    EnqueueRequest::new(
                        fetch_job.priority_hint.job_priority(),
                        JobPayload::ImageFetch(fetch_job.clone()),
                    )
                })
                .collect();

            match self.enqueue_follow_up_many(image_requests).await {
                DispatchStatus::Success => {}
                status => return status,
            }
        }

        let index_job = IndexUpsertJob {
            library_id: job.library_id,
            logical_entity: serde_json::json!({ "logical_id": ready.logical_id }),
            media_attrs: serde_json::json!({ "path": ready.analyzed.path_norm }),
            relations: serde_json::json!({}),
            path_norm: ready.analyzed.path_norm.clone(),
            idempotency_key: format!("index:{}:{}", job.library_id, ready.analyzed.path_norm),
        };

        // Bias index upserts to complete the item flow promptly.
        let req = EnqueueRequest::new(JobPriority::P0, JobPayload::IndexUpsert(index_job));
        self.enqueue_follow_up(req).await
    }

    async fn handle_index_upsert(&self, job: &IndexUpsertJob) -> DispatchStatus {
        let ready = MediaReadyForIndex {
            library_id: job.library_id,
            logical_id: None,
            normalized_title: None,
            analyzed: MediaAnalyzed {
                library_id: job.library_id,
                path_norm: job.path_norm.clone(),
                fingerprint: MediaFingerprint {
                    device_id: None,
                    inode: None,
                    size: 0,
                    mtime: 0,
                    weak_hash: None,
                },
                analyzed_at: Utc::now(),
                streams_json: serde_json::json!({"placeholder": true}),
                thumbnails: vec![],
                context: Value::Null,
            },
            prepared_at: Utc::now(),
            image_jobs: Vec::new(),
        };

        let outcome = match self
            .actors
            .indexer
            .index(IndexCommand {
                job: job.clone(),
                ready: ready.clone(),
            })
            .await
        {
            Ok(result) => result,
            Err(err) => return self.handle_media_error(err),
        };

        if let Err(err) = self
            .events
            .publish_scan_event(ScanEvent::Indexed(outcome))
            .await
        {
            return self.handle_media_error(err);
        }

        DispatchStatus::Success
    }

    async fn handle_image_fetch(&self, job: &ImageFetchJob) -> DispatchStatus {
        match self
            .actors
            .image
            .fetch(ImageFetchCommand { job: job.clone() })
            .await
        {
            Ok(_) => DispatchStatus::Success,
            Err(err) => self.handle_media_error(err),
        }
    }
}

#[async_trait]
impl<Q, E, C> JobDispatcher for DefaultJobDispatcher<Q, E, C>
where
    Q: QueueService + Send + Sync + 'static,
    E: ScanEventBus + Send + Sync + 'static,
    C: ScanCursorRepository + Send + Sync + 'static,
{
    async fn dispatch(&self, lease: &JobLease) -> DispatchStatus {
        match &lease.job.payload {
            JobPayload::FolderScan(job) => self.handle_folder_scan(lease, job).await,
            JobPayload::MediaAnalyze(job) => self.handle_media_analyze(job).await,
            JobPayload::MetadataEnrich(job) => self.handle_metadata_enrich(job).await,
            JobPayload::IndexUpsert(job) => self.handle_index_upsert(job).await,
            JobPayload::ImageFetch(job) => self.handle_image_fetch(job).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestration::actors::folder::{FolderListingPlan, FolderScanCommand};
    use crate::orchestration::actors::messages::{
        FolderScanSummary, MediaFileDiscovered, MediaKindHint,
    };
    use crate::orchestration::actors::pipeline::{IndexingChange, IndexingOutcome};
    use crate::orchestration::persistence::{PostgresCursorRepository, PostgresQueueService};
    use crate::orchestration::runtime::InProcJobEventBus;
    use crate::orchestration::{
        job::*,
        lease::{DequeueRequest, LeaseId},
    };
    use crate::types::ids::LibraryID;
    use crate::types::library::LibraryType;
    use sqlx::PgPool;
    use tokio::time::Duration;
    use uuid::Uuid;

    const FIXTURE_LIB_A: LibraryID = LibraryID(Uuid::from_u128(0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa));
    const FIXTURE_LIB_B: LibraryID = LibraryID(Uuid::from_u128(0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb));

    async fn upsert_library(
        pool: &PgPool,
        library_id: LibraryID,
        name: &str,
        library_type: LibraryType,
        paths: Vec<String>,
    ) -> sqlx::Result<()> {
        let library_type = match library_type {
            LibraryType::Movies => "movies",
            LibraryType::Series => "tvshows",
        };

        sqlx::query(
            r#"
            INSERT INTO libraries (
                id,
                name,
                library_type,
                paths,
                scan_interval_minutes,
                enabled,
                auto_scan,
                watch_for_changes,
                analyze_on_scan,
                max_retry_attempts
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            ON CONFLICT (id) DO UPDATE SET
                name = EXCLUDED.name,
                paths = EXCLUDED.paths,
                scan_interval_minutes = EXCLUDED.scan_interval_minutes,
                enabled = EXCLUDED.enabled,
                auto_scan = EXCLUDED.auto_scan,
                watch_for_changes = EXCLUDED.watch_for_changes,
                analyze_on_scan = EXCLUDED.analyze_on_scan,
                max_retry_attempts = EXCLUDED.max_retry_attempts,
                updated_at = NOW()
            "#,
        )
        .bind(library_id.as_uuid())
        .bind(name)
        .bind(library_type)
        .bind(paths)
        .bind(60_i32)
        .bind(true)
        .bind(true)
        .bind(true)
        .bind(false)
        .bind(3_i32)
        .execute(pool)
        .await?;

        Ok(())
    }

    struct StubFolderActor {
        plan: FolderListingPlan,
        discovered: Vec<MediaFileDiscovered>,
        children: Vec<FolderScanContext>,
        summary: FolderScanSummary,
    }

    #[async_trait]
    impl FolderScanActor for StubFolderActor {
        async fn plan_listing(&self, _command: &FolderScanCommand) -> Result<FolderListingPlan> {
            Ok(self.plan.clone())
        }

        async fn discover_media(
            &self,
            _plan: &FolderListingPlan,
            _context: &FolderScanContext,
        ) -> Result<Vec<MediaFileDiscovered>> {
            Ok(self.discovered.clone())
        }

        async fn derive_child_contexts(
            &self,
            _plan: &FolderListingPlan,
            _parent: &FolderScanContext,
        ) -> Result<Vec<FolderScanContext>> {
            Ok(self.children.clone())
        }

        fn finalize(
            &self,
            _context: &FolderScanContext,
            _plan: &FolderListingPlan,
            _discovered: &[MediaFileDiscovered],
            _children: &[FolderScanContext],
        ) -> Result<FolderScanSummary> {
            Ok(self.summary.clone())
        }
    }

    struct StubAnalyzeActor;

    #[async_trait]
    impl MediaAnalyzeActor for StubAnalyzeActor {
        async fn analyze(&self, command: MediaAnalyzeCommand) -> Result<MediaAnalyzed> {
            Ok(MediaAnalyzed {
                library_id: command.job.library_id,
                path_norm: command.job.path_norm,
                fingerprint: command.job.fingerprint,
                analyzed_at: Utc::now(),
                streams_json: serde_json::json!({"ok": true}),
                thumbnails: vec![],
                context: command.job.context,
            })
        }
    }

    struct StubMetadataActor;

    #[async_trait]
    impl MetadataActor for StubMetadataActor {
        async fn enrich(&self, command: MetadataCommand) -> Result<MediaReadyForIndex> {
            Ok(MediaReadyForIndex {
                library_id: command.job.library_id,
                logical_id: Some(command.job.logical_candidate_id.clone()),
                normalized_title: None,
                analyzed: command.analyzed,
                prepared_at: Utc::now(),
                image_jobs: Vec::new(),
            })
        }
    }

    struct StubIndexActor;

    #[async_trait]
    impl IndexerActor for StubIndexActor {
        async fn index(&self, command: IndexCommand) -> Result<IndexingOutcome> {
            Ok(IndexingOutcome {
                library_id: command.job.library_id,
                path_norm: command.job.path_norm,
                indexed_at: Utc::now(),
                upserted: true,
                media: None,
                media_id: None,
                change: IndexingChange::Created,
            })
        }
    }

    struct StubImageActor;

    #[async_trait]
    impl ImageFetchActor for StubImageActor {
        async fn fetch(&self, _command: ImageFetchCommand) -> Result<()> {
            Ok(())
        }
    }

    async fn dispatcher_fixture(
        pool: &PgPool,
    ) -> (
        DefaultJobDispatcher<PostgresQueueService, InProcJobEventBus, PostgresCursorRepository>,
        Arc<PostgresQueueService>,
        Arc<InProcJobEventBus>,
        Arc<PostgresCursorRepository>,
        CorrelationCache,
    ) {
        let queue = Arc::new(
            PostgresQueueService::new(pool.clone())
                .await
                .expect("queue init"),
        );
        let events = Arc::new(InProcJobEventBus::new(64));
        let cursors = Arc::new(PostgresCursorRepository::new(pool.clone()));
        let library_id = FIXTURE_LIB_A;

        upsert_library(
            pool,
            library_id,
            "Dispatcher Fixture A",
            LibraryType::Movies,
            vec!["/library".into()],
        )
        .await
        .expect("seed library row");

        let folder_actor = Arc::new(StubFolderActor {
            plan: FolderListingPlan {
                directories: vec![PathBuf::from("/library/child")],
                media_files: vec![PathBuf::from("/library/movie.mkv")],
                ancillary_files: vec![],
                generated_listing_hash: "abc123".into(),
            },
            discovered: vec![MediaFileDiscovered {
                library_id,
                path_norm: "/library/movie.mkv".into(),
                fingerprint: MediaFingerprint {
                    device_id: None,
                    inode: None,
                    size: 1,
                    mtime: 1,
                    weak_hash: None,
                },
                classified_as: MediaKindHint::Movie,
                context: FolderScanContext {
                    library_id,
                    folder_path_norm: "/library".into(),
                    parent: ParentDescriptors::default(),
                    reason: ScanReason::BulkSeed,
                },
            }],
            children: vec![FolderScanContext {
                library_id,
                folder_path_norm: "/library/child".into(),
                parent: ParentDescriptors::default(),
                reason: ScanReason::BulkSeed,
            }],
            summary: FolderScanSummary {
                context: FolderScanContext {
                    library_id,
                    folder_path_norm: "/library".into(),
                    parent: ParentDescriptors::default(),
                    reason: ScanReason::BulkSeed,
                },
                discovered_files: 1,
                enqueued_subfolders: 1,
                listing_hash: "abc123".into(),
                completed_at: Utc::now(),
            },
        }) as Arc<dyn FolderScanActor>;

        let actors = DispatcherActors::new(
            folder_actor,
            Arc::new(StubAnalyzeActor) as Arc<dyn MediaAnalyzeActor>,
            Arc::new(StubMetadataActor) as Arc<dyn MetadataActor>,
            Arc::new(StubIndexActor) as Arc<dyn IndexerActor>,
            Arc::new(StubImageActor) as Arc<dyn ImageFetchActor>,
        );

        let correlations = CorrelationCache::default();

        (
            DefaultJobDispatcher::new(
                Arc::clone(&queue),
                Arc::clone(&events),
                Arc::clone(&cursors),
                actors,
                correlations.clone(),
            ),
            queue,
            events,
            cursors,
            correlations,
        )
    }

    fn lease_for_payload(payload: JobPayload) -> JobLease {
        let record = JobRecord::new(payload, JobPriority::P1);
        JobLease::new(record, "test-worker".into(), chrono::Duration::seconds(30))
    }

    #[sqlx::test(migrator = "crate::MIGRATOR")]
    async fn folder_scan_dispatch_enqueues_follow_up_work(pool: PgPool) {
        let (dispatcher, queue, events, cursors, _correlations) = dispatcher_fixture(&pool).await;
        let mut job_rx = events.subscribe();
        let mut domain_rx = events.subscribe_scan();

        let lease = lease_for_payload(JobPayload::FolderScan(FolderScanJob {
            library_id: FIXTURE_LIB_A,
            folder_path_norm: "/library".into(),
            parent_context: None,
            scan_reason: ScanReason::BulkSeed,
            enqueue_time: Utc::now(),
            device_id: None,
        }));

        let status = dispatcher.dispatch(&lease).await;
        assert!(matches!(status, DispatchStatus::Success));

        // Media analyze job should be enqueued
        let dequeue = DequeueRequest {
            kind: JobKind::MediaAnalyze,
            worker_id: "test".into(),
            lease_ttl: chrono::Duration::seconds(30),
            selector: None,
        };
        let analyze = queue.dequeue(dequeue).await.expect("dequeue ok");
        assert!(analyze.is_some(), "expected media analyze job to be queued");

        // Verify cursor written
        let cursor_id = ScanCursorId::new(
            lease.job.payload.library_id(),
            &vec![PathBuf::from("/library")],
        );
        let cursor = cursors.get(&cursor_id).await.expect("cursor read");
        assert!(
            cursor.is_some(),
            "cursor should be written for scanned folder"
        );

        // Ensure enqueue and domain events emitted
        tokio::time::timeout(Duration::from_millis(100), async {
            let mut saw_enqueue = false;
            let mut saw_discovered = false;
            while let Ok(event) = job_rx.try_recv() {
                if matches!(event.payload, JobEventPayload::Enqueued { .. }) {
                    saw_enqueue = true;
                }
            }
            while let Ok(event) = domain_rx.try_recv() {
                if matches!(event, ScanEvent::MediaFileDiscovered(_)) {
                    saw_discovered = true;
                }
            }
            assert!(saw_enqueue, "expected JobEnqueued event");
            assert!(saw_discovered, "expected MediaFileDiscovered event");
        })
        .await
        .ok();
    }

    #[sqlx::test(migrator = "crate::MIGRATOR")]
    async fn media_analyze_dispatch_enqueues_metadata(pool: PgPool) {
        let (dispatcher, queue, events, _, _correlations) = dispatcher_fixture(&pool).await;
        let mut job_rx = events.subscribe();

        let job = MediaAnalyzeJob {
            library_id: FIXTURE_LIB_A,
            path_norm: "/library/movie.mkv".into(),
            fingerprint: MediaFingerprint {
                device_id: None,
                inode: None,
                size: 100,
                mtime: 1,
                weak_hash: None,
            },
            discovered_at: Utc::now(),
            context: serde_json::Value::Null,
            scan_reason: ScanReason::BulkSeed,
        };
        let lease = lease_for_payload(JobPayload::MediaAnalyze(job));

        let status = dispatcher.dispatch(&lease).await;
        assert!(matches!(status, DispatchStatus::Success));

        let dequeue = DequeueRequest {
            kind: JobKind::MetadataEnrich,
            worker_id: "test".into(),
            lease_ttl: chrono::Duration::seconds(30),
            selector: None,
        };
        let metadata_job = queue.dequeue(dequeue).await.expect("dequeue ok");
        assert!(metadata_job.is_some(), "expected metadata job");

        tokio::time::timeout(Duration::from_millis(100), async {
            let mut saw_enqueue = false;
            while let Ok(event) = job_rx.try_recv() {
                if matches!(event.payload, JobEventPayload::Enqueued { .. }) {
                    saw_enqueue = true;
                }
            }
            assert!(saw_enqueue, "expected JobEnqueued event");
        })
        .await
        .ok();
    }

    #[sqlx::test(migrator = "crate::MIGRATOR")]
    async fn metadata_enrich_transient_error_requests_retry(pool: PgPool) {
        struct TransientMetadataActor;

        #[async_trait]
        impl MetadataActor for TransientMetadataActor {
            async fn enrich(&self, _command: MetadataCommand) -> Result<MediaReadyForIndex> {
                Err(MediaError::Internal("tmdb timeout".into()))
            }
        }

        let queue = Arc::new(
            PostgresQueueService::new(pool.clone())
                .await
                .expect("queue init"),
        );
        let events = Arc::new(InProcJobEventBus::new(8));
        let cursors = Arc::new(PostgresCursorRepository::new(pool.clone()));

        upsert_library(
            &pool,
            FIXTURE_LIB_A,
            "Dispatcher Fixture A",
            LibraryType::Movies,
            vec!["/".into()],
        )
        .await
        .expect("seed fixture library A");

        let actors = DispatcherActors::new(
            Arc::new(StubFolderActor {
                plan: FolderListingPlan::default(),
                discovered: vec![],
                children: vec![],
                summary: FolderScanSummary {
                    context: FolderScanContext {
                        library_id: FIXTURE_LIB_A,
                        folder_path_norm: "/".into(),
                        parent: ParentDescriptors::default(),
                        reason: ScanReason::BulkSeed,
                    },
                    discovered_files: 0,
                    enqueued_subfolders: 0,
                    listing_hash: "".into(),
                    completed_at: Utc::now(),
                },
            }) as Arc<dyn FolderScanActor>,
            Arc::new(StubAnalyzeActor) as Arc<dyn MediaAnalyzeActor>,
            Arc::new(TransientMetadataActor) as Arc<dyn MetadataActor>,
            Arc::new(StubIndexActor) as Arc<dyn IndexerActor>,
            Arc::new(StubImageActor) as Arc<dyn ImageFetchActor>,
        );

        let correlations = CorrelationCache::default();

        let dispatcher = DefaultJobDispatcher::new(
            Arc::clone(&queue),
            Arc::clone(&events),
            Arc::clone(&cursors),
            actors,
            correlations,
        );

        let job = MetadataEnrichJob {
            library_id: FIXTURE_LIB_A,
            logical_candidate_id: "cand".into(),
            parse_fields: serde_json::json!({"path": "/library/movie.mkv"}),
            external_ids: None,
        };
        let lease = lease_for_payload(JobPayload::MetadataEnrich(job));

        let status = dispatcher.dispatch(&lease).await;
        match status {
            DispatchStatus::Retry { error } => {
                assert!(error.contains("tmdb timeout"));
            }
            other => panic!("expected retry status, got {other:?}"),
        }
    }

    #[sqlx::test(migrator = "crate::MIGRATOR")]
    async fn media_error_invalid_marks_dead_letter(pool: PgPool) {
        struct FailingMetadataActor;

        #[async_trait]
        impl MetadataActor for FailingMetadataActor {
            async fn enrich(&self, _command: MetadataCommand) -> Result<MediaReadyForIndex> {
                Err(MediaError::InvalidMedia("bad metadata".into()))
            }
        }

        let queue = Arc::new(
            PostgresQueueService::new(pool.clone())
                .await
                .expect("queue init"),
        );
        let events = Arc::new(InProcJobEventBus::new(8));
        let cursors = Arc::new(PostgresCursorRepository::new(pool.clone()));

        upsert_library(
            &pool,
            FIXTURE_LIB_B,
            "Dispatcher Fixture B",
            LibraryType::Movies,
            vec!["/".into()],
        )
        .await
        .expect("seed fixture library B");

        let actors = DispatcherActors::new(
            Arc::new(StubFolderActor {
                plan: FolderListingPlan::default(),
                discovered: vec![],
                children: vec![],
                summary: FolderScanSummary {
                    context: FolderScanContext {
                        library_id: FIXTURE_LIB_B,
                        folder_path_norm: "/".into(),
                        parent: ParentDescriptors::default(),
                        reason: ScanReason::BulkSeed,
                    },
                    discovered_files: 0,
                    enqueued_subfolders: 0,
                    listing_hash: "".into(),
                    completed_at: Utc::now(),
                },
            }) as Arc<dyn FolderScanActor>,
            Arc::new(StubAnalyzeActor) as Arc<dyn MediaAnalyzeActor>,
            Arc::new(FailingMetadataActor) as Arc<dyn MetadataActor>,
            Arc::new(StubIndexActor) as Arc<dyn IndexerActor>,
            Arc::new(StubImageActor) as Arc<dyn ImageFetchActor>,
        );

        let correlations = CorrelationCache::default();

        let dispatcher = DefaultJobDispatcher::new(
            Arc::clone(&queue),
            Arc::clone(&events),
            Arc::clone(&cursors),
            actors,
            correlations,
        );

        let job = MetadataEnrichJob {
            library_id: FIXTURE_LIB_B,
            logical_candidate_id: "cand".into(),
            parse_fields: serde_json::json!({"path": "/library/movie.mkv"}),
            external_ids: None,
        };
        let lease = lease_for_payload(JobPayload::MetadataEnrich(job));

        let status = dispatcher.dispatch(&lease).await;
        assert!(matches!(status, DispatchStatus::DeadLetter { .. }));
    }

    #[tokio::test]
    async fn correlation_id_propagates_when_provided() {
        let correlations = CorrelationCache::default();

        let library_id = LibraryID(uuid::Uuid::now_v7());
        let payload = JobPayload::FolderScan(FolderScanJob {
            library_id,
            folder_path_norm: "/folder".into(),
            parent_context: None,
            scan_reason: ScanReason::UserRequested,
            enqueue_time: Utc::now(),
            device_id: None,
        });
        let priority = JobPriority::P1;
        let handle = JobHandle::accepted(JobId::new(), &payload, priority);

        let provided = uuid::Uuid::now_v7();
        let enqueued_event = JobEvent::from_handle(
            &handle,
            Some(provided),
            JobEventPayload::Enqueued {
                job_id: handle.job_id,
                kind: payload.kind(),
                priority,
            },
            None,
        );

        correlations
            .remember(handle.job_id, enqueued_event.meta.correlation_id)
            .await;

        assert_eq!(enqueued_event.meta.correlation_id, provided);

        let dequeue_event = JobEvent::from_job(
            Some(correlations.fetch_or_generate(handle.job_id).await),
            payload.library_id(),
            handle.dedupe_key.clone(),
            None,
            JobEventPayload::Dequeued {
                job_id: handle.job_id,
                kind: payload.kind(),
                priority,
                lease_id: LeaseId::new(),
            },
        );

        assert_eq!(dequeue_event.meta.correlation_id, provided);

        let completed_event = JobEvent::from_job(
            Some(correlations.take_or_generate(handle.job_id).await),
            payload.library_id(),
            handle.dedupe_key.clone(),
            None,
            JobEventPayload::Completed {
                job_id: handle.job_id,
                kind: payload.kind(),
                priority,
            },
        );

        assert_eq!(completed_event.meta.correlation_id, provided);
        assert!(correlations.fetch(&handle.job_id).await.is_none());
    }

    #[tokio::test]
    async fn correlation_id_generated_when_missing() {
        let correlations = CorrelationCache::default();

        let library_id = LibraryID(uuid::Uuid::now_v7());
        let payload = JobPayload::FolderScan(FolderScanJob {
            library_id,
            folder_path_norm: "/missing".into(),
            parent_context: None,
            scan_reason: ScanReason::BulkSeed,
            enqueue_time: Utc::now(),
            device_id: None,
        });
        let priority = JobPriority::P2;
        let handle = JobHandle::accepted(JobId::new(), &payload, priority);

        let enqueued_event = JobEvent::from_handle(
            &handle,
            None,
            JobEventPayload::Enqueued {
                job_id: handle.job_id,
                kind: payload.kind(),
                priority,
            },
            None,
        );

        let generated = enqueued_event.meta.correlation_id;
        assert_ne!(generated, uuid::Uuid::nil());

        correlations.remember(handle.job_id, generated).await;

        let dequeue_event = JobEvent::from_job(
            Some(correlations.fetch_or_generate(handle.job_id).await),
            payload.library_id(),
            handle.dedupe_key.clone(),
            None,
            JobEventPayload::Dequeued {
                job_id: handle.job_id,
                kind: payload.kind(),
                priority,
                lease_id: LeaseId::new(),
            },
        );
        assert_eq!(dequeue_event.meta.correlation_id, generated);

        let failed_event = JobEvent::from_job(
            Some(correlations.fetch_or_generate(handle.job_id).await),
            payload.library_id(),
            handle.dedupe_key.clone(),
            None,
            JobEventPayload::Failed {
                job_id: handle.job_id,
                kind: payload.kind(),
                priority,
                retryable: true,
            },
        );
        assert_eq!(failed_event.meta.correlation_id, generated);

        let dead_letter_event = JobEvent::from_job(
            Some(correlations.take_or_generate(handle.job_id).await),
            payload.library_id(),
            handle.dedupe_key.clone(),
            None,
            JobEventPayload::DeadLettered {
                job_id: handle.job_id,
                kind: payload.kind(),
                priority,
            },
        );
        assert_eq!(dead_letter_event.meta.correlation_id, generated);
        assert!(correlations.fetch(&handle.job_id).await.is_none());
    }
}
