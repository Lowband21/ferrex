use std::{any::type_name, fmt, path::PathBuf, sync::Arc};

use async_trait::async_trait;
use chrono::Utc;
use ferrex_model::VideoMediaType;
use tracing::{Instrument, debug, debug_span, warn};
use uuid::Uuid;

use crate::domain::scan::actors::image_fetch::ImageFetchActor;
use crate::domain::scan::actors::index::{IndexCommand, IndexerActor};
use crate::domain::scan::actors::metadata::{
    MediaReadyForIndex, MetadataActor, MetadataCommand,
};
use crate::domain::scan::actors::{
    analyze::{AnalysisContext, MediaAnalyzeActor, MediaAnalyzed},
    folder::FolderScanActor,
};
use crate::domain::scan::orchestration::{
    context::{FolderScanContext, SeriesLink, SeriesRef},
    correlation::CorrelationCache,
    events::{
        JobEvent, JobEventPayload, ScanEvent, ScanEventBus, stable_path_key,
    },
    job::{
        AnalyzeScanHierarchy, DependencyKey, EnqueueRequest, EpisodeMatchJob,
        FolderScanJob, ImageFetchJob, IndexUpsertJob, JobHandle, JobPayload,
        JobPriority, MediaAnalyzeJob, MediaFingerprint, MetadataEnrichJob,
        ScanReason, SeriesResolveJob,
    },
    lease::JobLease,
    queue::QueueService,
    scan_cursor::{ScanCursor, ScanCursorId, ScanCursorRepository},
    series::SeriesResolverPort,
    series_state::{SeriesScanStateRepository, SeriesScanStatus},
};
use crate::error::{MediaError, Result};

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
    series_states: Arc<Box<dyn SeriesScanStateRepository>>,
    series_resolver: Arc<dyn SeriesResolverPort>,
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
            .field("series_states", &"SeriesScanStateRepository")
            .field("series_resolver", &"SeriesResolverPort")
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
        series_states: Arc<Box<dyn SeriesScanStateRepository>>,
        series_resolver: Arc<dyn SeriesResolverPort>,
        actors: DispatcherActors,
        correlations: CorrelationCache,
    ) -> Self {
        Self {
            queue,
            events,
            cursors,
            actors,
            correlations,
            series_states,
            series_resolver,
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
            MediaError::Http(err) => {
                // Network/transport errors are usually transient (DNS hiccups, socket
                // resets, timeouts, etc.). Prefer retrying and let lease/backoff do
                // the throttling rather than dead-lettering permanently.
                let msg = err.to_string();
                warn!(error = %msg, "retrying job due to HTTP client error");
                DispatchStatus::Retry { error: msg }
            }
            MediaError::HttpStatus { status, url } => {
                let msg = format!("HTTP {status} ({url})");
                if status.as_u16() == 404 {
                    warn!(error = %msg, "dead-lettering job due to missing remote resource");
                    DispatchStatus::DeadLetter { error: msg }
                } else if status.as_u16() == 429 || status.is_server_error() {
                    warn!(error = %msg, "retrying job due to transient remote status");
                    DispatchStatus::Retry { error: msg }
                } else {
                    warn!(error = %msg, "dead-lettering job due to remote status");
                    DispatchStatus::DeadLetter { error: msg }
                }
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

    async fn enqueue_follow_up(
        &self,
        request: EnqueueRequest,
    ) -> DispatchStatus {
        let correlation_hint = request.correlation_id;

        match self.queue.enqueue(request.clone()).await {
            Ok(handle) => match self
                .publish_enqueue_event(
                    &handle,
                    &request.payload,
                    correlation_hint,
                )
                .await
            {
                Ok(()) => DispatchStatus::Success,
                Err(err) => self.handle_media_error(err),
            },
            Err(err) => self.handle_media_error(err),
        }
    }

    async fn enqueue_follow_up_many(
        &self,
        requests: Vec<EnqueueRequest>,
    ) -> DispatchStatus {
        if requests.is_empty() {
            return DispatchStatus::Success;
        }

        let cloned_requests = requests.clone();

        match self.queue.enqueue_many(cloned_requests).await {
            Ok(handles) => {
                for (handle, request) in
                    handles.into_iter().zip(requests.into_iter())
                {
                    if let Err(err) = self
                        .publish_enqueue_event(
                            &handle,
                            &request.payload,
                            request.correlation_id,
                        )
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

    async fn handle_folder_scan(
        &self,
        lease: &JobLease,
        job: &FolderScanJob,
    ) -> DispatchStatus {
        let context = job.context.clone();
        let span = debug_span!(
            "\nfolder_scan",
            job_id = %lease.job.id.0,
            library = %context.library_id(),
            path = %context.folder_path_norm()
        );
        async {
            let plan = match self.actors.folder.plan_listing(job).await {
                Ok(plan) => plan,
                Err(err) => return self.handle_media_error(err),
            };

            // Check cursor to short-circuit unchanged listings
            let cursor_id = ScanCursorId::new(
                context.library_id(),
                &vec![PathBuf::from(context.folder_path_norm())],
            );
            let mut listing_unchanged = false;
            let mut last_modified_at = None;
            match self.cursors.get(&cursor_id).await {
                Ok(Some(existing))
                    if existing.listing_hash == plan.generated_listing_hash =>
                {
                    listing_unchanged = true;
                    last_modified_at = existing.last_modified_at;
                }
                Ok(_) => {}
                Err(err) => return self.handle_media_error(err),
            }

            if listing_unchanged {
                debug!("listing hash unchanged, refreshing cursor + emitting completion");

                // Even when we short-circuit, we still emit `FolderScanCompleted` so
                // downstream consumers (scan progress, bundle finalization trackers, etc.)
                // can treat this folder scan as a completed unit of work.
                //
                // We intentionally do *not* publish MediaFileDiscovered / FolderDiscovered
                // here to preserve the short-circuit behavior (no downstream pipeline
                // fan-out when nothing changed).
                let discovered = match self.actors.folder.discover_media(&plan, job).await {
                    Ok(files) => files,
                    Err(err) => return self.handle_media_error(err),
                };

                let summary = match self.actors.folder.finalize(
                    &context,
                    &plan,
                    &discovered,
                    &[],
                ) {
                    Ok(summary) => summary,
                    Err(err) => return self.handle_media_error(err),
                };

                if let FolderScanContext::Series(series_ctx) = &context {
                    let folder_name = std::path::Path::new(
                        series_ctx.series_root_path.as_str(),
                    )
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(|name| name.to_string())
                    .unwrap_or_else(|| {
                        series_ctx.series_root_path.as_str().to_string()
                    });

                    let state = match self
                        .series_states
                        .mark_discovered(
                            series_ctx.library_id,
                            series_ctx.series_root_path.clone(),
                            None,
                        )
                        .await
                    {
                        Ok(state) => state,
                        Err(err) => return self.handle_media_error(err),
                    };

                    if !matches!(state.status, SeriesScanStatus::Resolved) {
                        let series_job = SeriesResolveJob {
                            library_id: series_ctx.library_id,
                            series_root_path: series_ctx.series_root_path.clone(),
                            hint: None,
                            folder_name,
                            scan_reason: job.scan_reason,
                        };
                        let priority = priority_for_reason(&job.scan_reason)
                            .elevate(JobPriority::P0);
                        let req = EnqueueRequest::new(
                            priority,
                            JobPayload::SeriesResolve(series_job),
                        );
                        match self.enqueue_follow_up(req).await {
                            DispatchStatus::Success => {}
                            status => return status,
                        }
                    }
                }

                if let Err(err) = self
                    .events
                    .publish_scan_event(ScanEvent::FolderScanCompleted(
                        summary.clone(),
                    ))
                    .await
                {
                    return self.handle_media_error(err);
                }

                let cursor = ScanCursor {
                    id: cursor_id,
                    folder_path_norm: context.folder_path_norm().to_string(),
                    listing_hash: plan.generated_listing_hash.clone(),
                    entry_count: plan.directories.len()
                        + plan.media_files.len()
                        + plan.ancillary_files.len(),
                    last_scan_at: Utc::now(),
                    last_modified_at,
                    device_id: job.device_id.clone(),
                };
                if let Err(err) = self.cursors.upsert(cursor).await {
                    return self.handle_media_error(err);
                }

                return DispatchStatus::Success;
            }

            let discovered =
                match self.actors.folder.discover_media(&plan, job).await {
                    Ok(files) => files,
                    Err(err) => return self.handle_media_error(err),
                };
            let children = match self
                .actors
                .folder
                .derive_child_contexts(&plan, job)
                .await
            {
                Ok(children) => children,
                Err(err) => return self.handle_media_error(err),
            };

            let summary = match self.actors.folder.finalize(
                &context,
                &plan,
                &discovered,
                &children,
            ) {
                Ok(summary) => summary,
                Err(err) => return self.handle_media_error(err),
            };

            if let FolderScanContext::Series(series_ctx) = &context {
                let folder_name = std::path::Path::new(
                    series_ctx.series_root_path.as_str(),
                )
                .file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.to_string())
                .unwrap_or_else(|| {
                    series_ctx.series_root_path.as_str().to_string()
                });

                let state = match self
                    .series_states
                    .mark_discovered(
                        series_ctx.library_id,
                        series_ctx.series_root_path.clone(),
                        None,
                    )
                    .await
                {
                    Ok(state) => state,
                    Err(err) => return self.handle_media_error(err),
                };

                if !matches!(state.status, SeriesScanStatus::Resolved) {
                    let series_job = SeriesResolveJob {
                        library_id: series_ctx.library_id,
                        series_root_path: series_ctx.series_root_path.clone(),
                        hint: None,
                        folder_name,
                        scan_reason: job.scan_reason,
                    };
                    let priority = priority_for_reason(&job.scan_reason)
                        .elevate(JobPriority::P0);
                    let req = EnqueueRequest::new(
                        priority,
                        JobPayload::SeriesResolve(series_job),
                    );
                    match self.enqueue_follow_up(req).await {
                        DispatchStatus::Success => {}
                        status => return status,
                    }
                }
            }

            let mut discovered_events =
                Vec::with_capacity(discovered.len());
            let mut followup_errors: Vec<String> = Vec::new();
            for media in &discovered {
                if let Err(err) = self
                    .events
                    .publish_scan_event(ScanEvent::MediaFileDiscovered(
                        Box::new(media.clone()),
                    ))
                    .await
                {
                    // Continue discovering other items; collect error for admin visibility.
                    tracing::warn!(
                        target: "scan::dispatch",
                        error = %err,
                        path = %media.path_norm,
                        "failed to publish MediaFileDiscovered; continuing"
                    );
                    followup_errors.push(format!(
                        "discover_event_failed:{}",
                        media.path_norm
                    ));
                    continue;
                }
                discovered_events.push(media.clone());

                // Elevate analyze priority so per-item pipelines advance ahead of more scans.
                // This prevents breadth-first scanning from starving downstream stages.
                let analyze_priority = priority_for_reason(&media.scan_reason)
                    .elevate(JobPriority::P0);

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
                    analyze_priority,
                    JobPayload::MediaAnalyze(analyze),
                );
                match self.enqueue_follow_up(req).await {
                    DispatchStatus::Success => {}
                    DispatchStatus::Retry { error } => {
                        tracing::warn!(
                            target: "scan::dispatch",
                            error = %error,
                            path = %media.path_norm,
                            "enqueue MediaAnalyze scheduled for retry; continuing"
                        );
                        followup_errors.push(format!(
                            "analyze_enqueue_retry:{}",
                            media.path_norm
                        ));
                    }
                    DispatchStatus::DeadLetter { error } => {
                        tracing::warn!(
                            target: "scan::dispatch",
                            error = %error,
                            path = %media.path_norm,
                            "enqueue MediaAnalyze dead-lettered; continuing"
                        );
                        followup_errors.push(format!(
                            "analyze_enqueue_deadletter:{}",
                            media.path_norm
                        ));
                    }
                }
            }

            if let Err(err) = self
                .events
                .publish_scan_event(ScanEvent::FolderScanCompleted(
                    summary.clone(),
                ))
                .await
            {
                return self.handle_media_error(err);
            }

            // Emit FolderDiscovered for each child; orchestrator enqueues from events.
            for child in &children {
                if let Err(err) = self
                    .events
                    .publish_scan_event(ScanEvent::FolderDiscovered {
                        context: Box::new(child.clone()),
                        reason: job.scan_reason,
                    })
                    .await
                {
                    tracing::warn!(
                        target: "scan::dispatch",
                        error = %err,
                        path = %child.folder_path_norm(),
                        "failed to publish FolderDiscovered; continuing"
                    );
                    followup_errors.push(format!(
                        "folder_discovered_publish_failed:{}",
                        child.folder_path_norm()
                    ));
                }
            }

            let cursor = ScanCursor {
                id: cursor_id,
                folder_path_norm: context.folder_path_norm().to_string(),
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
        .instrument(span)
        .await
    }

    async fn handle_media_analyze(
        &self,
        job: &MediaAnalyzeJob,
    ) -> DispatchStatus {
        // TODO: Refactor clone
        let analyzed = match self.actors.analyze.analyze(job.clone()).await {
            Ok(result) => result,
            Err(err) => return self.handle_media_error(err),
        };

        if let Err(err) = self
            .events
            .publish_scan_event(ScanEvent::MediaAnalyzed(Box::new(
                analyzed.clone(),
            )))
            .await
        {
            return self.handle_media_error(err);
        }

        if analyzed.variant == VideoMediaType::Episode {
            use crate::domain::scan::orchestration::context::WithSeriesHierarchy;

            let AnalyzeScanHierarchy::Episode(episode_hierarchy) =
                &analyzed.hierarchy
            else {
                return DispatchStatus::DeadLetter {
                    error: "episode analysis missing episode hierarchy".into(),
                };
            };

            if episode_hierarchy.series_id().is_none() {
                let series_root = episode_hierarchy.series_root_path.clone();
                let hint = episode_hierarchy.series_hint().cloned();
                if let Err(err) = self
                    .series_states
                    .mark_discovered(job.library_id, series_root.clone(), hint)
                    .await
                {
                    return self.handle_media_error(err);
                }

                let state = match self
                    .series_resolver
                    .get_state(job.library_id, &series_root)
                    .await
                {
                    Ok(state) => state,
                    Err(err) => return self.handle_media_error(err),
                };

                if let Some(state) = state
                    && let Some(series_id) = state.series_id
                    && matches!(state.status, SeriesScanStatus::Resolved)
                {
                    let mut hierarchy = episode_hierarchy.clone();
                    hierarchy.series = SeriesLink::Resolved(SeriesRef {
                        id: series_id,
                        slug: state
                            .hint
                            .as_ref()
                            .and_then(|hint| hint.slug.clone()),
                        title: state
                            .hint
                            .as_ref()
                            .map(|hint| hint.title.clone()),
                    });

                    let meta_job = MetadataEnrichJob {
                        library_id: job.library_id,
                        media_id: analyzed.media_id,
                        variant: analyzed.variant,
                        hierarchy: AnalyzeScanHierarchy::Episode(hierarchy),
                        node: analyzed.node.clone(),
                        path_norm: job.path_norm.clone(),
                        fingerprint: analyzed.fingerprint.clone(),
                        scan_reason: job.scan_reason,
                    };

                    let priority = priority_for_reason(&job.scan_reason)
                        .elevate(JobPriority::P0);
                    let req = EnqueueRequest::new(
                        priority,
                        JobPayload::MetadataEnrich(meta_job),
                    );
                    return self.enqueue_follow_up(req).await;
                }

                let match_job = EpisodeMatchJob {
                    library_id: job.library_id,
                    media_id: analyzed.media_id,
                    path_norm: job.path_norm.clone(),
                    fingerprint: analyzed.fingerprint.clone(),
                    hierarchy: episode_hierarchy.clone(),
                    node: analyzed.node.clone(),
                    scan_reason: job.scan_reason,
                };

                let priority = priority_for_reason(&job.scan_reason)
                    .elevate(JobPriority::P0);
                let req = EnqueueRequest::new(
                    priority,
                    JobPayload::EpisodeMatch(match_job),
                )
                .with_dependency(DependencyKey::series_root(&series_root));
                return self.enqueue_follow_up(req).await;
            }
        }

        let meta_job = MetadataEnrichJob {
            library_id: job.library_id,
            media_id: analyzed.media_id,
            variant: analyzed.variant,
            hierarchy: analyzed.hierarchy.clone(),
            node: analyzed.node.clone(),
            path_norm: job.path_norm.clone(),
            fingerprint: analyzed.fingerprint.clone(),
            scan_reason: job.scan_reason,
        };

        let priority = priority_for_reason(&job.scan_reason);

        // Prefer advancing metadata for already-discovered items over additional scans.
        let priority = priority.elevate(JobPriority::P0);
        let req =
            EnqueueRequest::new(priority, JobPayload::MetadataEnrich(meta_job));
        self.enqueue_follow_up(req).await
    }

    async fn handle_series_resolve(
        &self,
        job: &SeriesResolveJob,
    ) -> DispatchStatus {
        let resolution = match self.series_resolver.resolve(job).await {
            Ok(result) => result,
            Err(err) => {
                let status = self.handle_media_error(err);
                if let DispatchStatus::DeadLetter { error } = &status {
                    let _ = self
                        .series_resolver
                        .mark_failed(
                            job.library_id,
                            job.series_root_path.clone(),
                            error.clone(),
                        )
                        .await;
                    if let Err(err) = self
                        .queue
                        .release_dependency(
                            job.library_id,
                            &DependencyKey::series_root(&job.series_root_path),
                        )
                        .await
                    {
                        tracing::warn!(
                            target: "scan::dispatch",
                            error = %err,
                            series_root = %job.series_root_path.as_str(),
                            "failed to release dependency after series resolve dead-letter"
                        );
                    }
                }
                return status;
            }
        };

        let ready = resolution.ready;
        if let Err(err) = self
            .events
            .publish_scan_event(ScanEvent::MediaReadyForIndex(Box::new(
                ready.clone(),
            )))
            .await
        {
            return self.handle_media_error(err);
        }

        if let Err(err) = self
            .queue
            .release_dependency(
                job.library_id,
                &DependencyKey::series_root(&job.series_root_path),
            )
            .await
        {
            return self.handle_media_error(err);
        }

        let index_job = IndexUpsertJob {
            library_id: ready.library_id,
            media_id: ready.media_id,
            variant: ready.variant,
            hierarchy: ready.hierarchy.clone(),
            node: ready.node.clone(),
            path_norm: ready.analyzed.path_norm.clone(),
            idempotency_key: format!(
                "index:{}:{}",
                job.library_id, ready.analyzed.path_norm
            ),
        };

        // Bias index upserts to complete the item flow promptly.
        let req = EnqueueRequest::new(
            JobPriority::P0,
            JobPayload::IndexUpsert(index_job),
        );
        self.enqueue_follow_up(req).await
    }

    async fn handle_metadata_enrich(
        &self,
        job: &MetadataEnrichJob,
    ) -> DispatchStatus {
        let analyzed = MediaAnalyzed {
            library_id: job.library_id,
            variant: job.variant,
            media_id: job.media_id,
            hierarchy: job.hierarchy.clone(),
            node: job.node.clone(),
            path_norm: job.path_norm.clone(),
            fingerprint: job.fingerprint.clone(),
            analyzed_at: Utc::now(),
            analysis: AnalysisContext {
                technical: None,
                demo_note: None,
                tmdb_id_hint: None,
            },
            thumbnails: vec![],
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
            .publish_scan_event(ScanEvent::MediaReadyForIndex(Box::new(
                ready.clone(),
            )))
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
            library_id: ready.library_id,
            media_id: ready.media_id,
            variant: ready.variant,
            hierarchy: ready.hierarchy.clone(),
            node: ready.node.clone(),
            path_norm: ready.analyzed.path_norm.clone(),
            idempotency_key: format!(
                "index:{}:{}",
                job.library_id, ready.analyzed.path_norm
            ),
        };

        // Bias index upserts to complete the item flow promptly.
        let req = EnqueueRequest::new(
            JobPriority::P0,
            JobPayload::IndexUpsert(index_job),
        );
        self.enqueue_follow_up(req).await
    }

    async fn handle_index_upsert(
        &self,
        job: &IndexUpsertJob,
    ) -> DispatchStatus {
        let ready = MediaReadyForIndex {
            library_id: job.library_id,
            media_id: job.media_id,
            variant: job.variant,
            hierarchy: job.hierarchy.clone(),
            node: job.node.clone(),
            normalized_title: None,
            analyzed: MediaAnalyzed {
                library_id: job.library_id,
                media_id: job.media_id,
                variant: job.variant,
                hierarchy: job.hierarchy.clone(),
                node: job.node.clone(),
                path_norm: job.path_norm.clone(),
                fingerprint: MediaFingerprint {
                    device_id: None,
                    inode: None,
                    size: 0,
                    mtime: 0,
                    weak_hash: None,
                },
                analyzed_at: Utc::now(),
                analysis: AnalysisContext {
                    technical: None,
                    demo_note: None,
                    tmdb_id_hint: None,
                },
                thumbnails: vec![],
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
            .publish_scan_event(ScanEvent::Indexed(Box::new(outcome)))
            .await
        {
            return self.handle_media_error(err);
        }

        DispatchStatus::Success
    }

    async fn handle_image_fetch(&self, job: &ImageFetchJob) -> DispatchStatus {
        match self.actors.image.fetch(job).await {
            Ok(_) => DispatchStatus::Success,
            Err(err) => self.handle_media_error(err),
        }
    }

    async fn handle_episode_match(
        &self,
        job: &EpisodeMatchJob,
    ) -> DispatchStatus {
        let series_root = job.hierarchy.series_root_path.clone();

        let state = match self
            .series_resolver
            .get_state(job.library_id, &series_root)
            .await
        {
            Ok(state) => state,
            Err(err) => return self.handle_media_error(err),
        };

        let Some(state) = state else {
            return DispatchStatus::DeadLetter {
                error: "episode match missing series state".into(),
            };
        };

        let Some(series_id) = state.series_id else {
            return DispatchStatus::DeadLetter {
                error: "episode match missing resolved series id".into(),
            };
        };
        if !matches!(state.status, SeriesScanStatus::Resolved) {
            return DispatchStatus::DeadLetter {
                error: "episode match executed before series resolved".into(),
            };
        }

        let mut hierarchy = job.hierarchy.clone();
        hierarchy.series = SeriesLink::Resolved(SeriesRef {
            id: series_id,
            slug: state.hint.as_ref().and_then(|hint| hint.slug.clone()),
            title: state.hint.as_ref().map(|hint| hint.title.clone()),
        });

        let meta_job = MetadataEnrichJob {
            library_id: job.library_id,
            media_id: job.media_id,
            variant: VideoMediaType::Episode,
            hierarchy: AnalyzeScanHierarchy::Episode(hierarchy),
            node: job.node.clone(),
            path_norm: job.path_norm.clone(),
            fingerprint: job.fingerprint.clone(),
            scan_reason: job.scan_reason,
        };

        let priority =
            priority_for_reason(&job.scan_reason).elevate(JobPriority::P0);
        let req =
            EnqueueRequest::new(priority, JobPayload::MetadataEnrich(meta_job));
        self.enqueue_follow_up(req).await
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
            JobPayload::FolderScan(job) => {
                self.handle_folder_scan(lease, job).await
            }
            JobPayload::SeriesResolve(job) => {
                self.handle_series_resolve(job).await
            }
            JobPayload::MediaAnalyze(job) => {
                self.handle_media_analyze(job).await
            }
            JobPayload::MetadataEnrich(job) => {
                self.handle_metadata_enrich(job).await
            }
            JobPayload::IndexUpsert(job) => self.handle_index_upsert(job).await,
            JobPayload::ImageFetch(job) => self.handle_image_fetch(job).await,
            JobPayload::EpisodeMatch(job) => {
                self.handle_episode_match(job).await
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::scan::actors::folder::FolderListingPlan;
    use crate::domain::scan::actors::index::{IndexingChange, IndexingOutcome};
    use crate::domain::scan::actors::messages::{
        FolderScanSummary, MediaFileDiscovered, MediaKindHint,
    };
    use crate::domain::scan::context::{
        MovieFolderScanContext, MovieRootPath, MovieScanHierarchy,
        ScanNodeKind, SeriesHint,
    };
    use crate::domain::scan::orchestration::context::SeriesRootPath;
    use crate::domain::scan::orchestration::context::SeriesScanHierarchy;
    use crate::domain::scan::orchestration::persistence::{
        PostgresCursorRepository, PostgresQueueService,
    };
    use crate::domain::scan::orchestration::runtime::InProcJobEventBus;
    use crate::domain::scan::orchestration::series::SeriesResolution;
    use crate::domain::scan::orchestration::series_state::{
        InMemorySeriesScanStateRepository, SeriesScanState,
    };
    use crate::domain::scan::orchestration::{job::*, lease::DequeueRequest};
    use crate::types::ids::{LibraryId, SeriesID};
    use crate::types::library::LibraryType;
    use ferrex_model::{MediaID, VideoMediaType};
    use sqlx::PgPool;
    use tokio::time::Duration;
    use uuid::Uuid;

    const FIXTURE_LIB_A: LibraryId =
        LibraryId(Uuid::from_u128(0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa));
    // const FIXTURE_LIB_B: LibraryId =
    //     LibraryId(Uuid::from_u128(0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb));

    async fn upsert_library(
        pool: &PgPool,
        library_id: LibraryId,
        name: &str,
        library_type: LibraryType,
        paths: Vec<String>,
    ) -> sqlx::Result<()> {
        let library_type = match library_type {
            LibraryType::Movies => "movies",
            LibraryType::Series => "tvshows",
        };

        sqlx::query!(
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
            library_id.as_uuid(),
            name,
            library_type,
            &paths,
            60_i32,
            true,
            true,
            true,
            false,
            3_i32
        )
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
        async fn plan_listing(
            &self,
            _command: &FolderScanJob,
        ) -> Result<FolderListingPlan> {
            Ok(self.plan.clone())
        }

        async fn discover_media(
            &self,
            _plan: &FolderListingPlan,
            _context: &FolderScanJob,
        ) -> Result<Vec<MediaFileDiscovered>> {
            Ok(self.discovered.clone())
        }

        async fn derive_child_contexts(
            &self,
            _plan: &FolderListingPlan,
            _parent: &FolderScanJob,
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
        async fn analyze(
            &self,
            command: MediaAnalyzeJob,
        ) -> Result<MediaAnalyzed> {
            Ok(MediaAnalyzed {
                library_id: command.library_id,
                media_id: command.media_id,
                variant: command.variant,
                hierarchy: AnalyzeScanHierarchy::Series(SeriesScanHierarchy {
                    series_root_path: SeriesRootPath::try_new(
                        "/library/series",
                    )
                    .unwrap(),
                    series: SeriesLink::Hint(SeriesHint {
                        title: "series".to_string(),
                        slug: None,
                        year: None,
                        region: None,
                    }),
                }),
                node: ScanNodeKind::default(),
                path_norm: command.path_norm,
                fingerprint: command.fingerprint,
                analyzed_at: Utc::now(),
                analysis: AnalysisContext {
                    technical: None,
                    demo_note: None,
                    tmdb_id_hint: None,
                },
                thumbnails: vec![],
            })
        }
    }

    struct StubMetadataActor;

    #[async_trait]
    impl MetadataActor for StubMetadataActor {
        async fn enrich(
            &self,
            command: MetadataCommand,
        ) -> Result<MediaReadyForIndex> {
            Ok(MediaReadyForIndex {
                library_id: command.job.library_id,
                media_id: command.job.media_id,
                variant: command.job.variant,
                hierarchy: command.job.hierarchy.clone(),
                node: command.job.node.clone(),
                normalized_title: None,
                analyzed: command.analyzed,
                prepared_at: Utc::now(),
                image_jobs: Vec::new(),
            })
        }
    }

    #[derive(Clone)]
    struct StubSeriesResolver {
        states: Arc<Box<dyn SeriesScanStateRepository>>,
    }

    impl StubSeriesResolver {
        fn new(states: Arc<Box<dyn SeriesScanStateRepository>>) -> Self {
            Self { states }
        }
    }

    #[async_trait]
    impl SeriesResolverPort for StubSeriesResolver {
        async fn resolve(
            &self,
            job: &SeriesResolveJob,
        ) -> Result<SeriesResolution> {
            let series_id = SeriesID(Uuid::now_v7());
            let series_ref = SeriesRef {
                id: series_id,
                slug: job.hint.as_ref().and_then(|h| h.slug.clone()),
                title: job.hint.as_ref().map(|h| h.title.clone()),
            };
            let hierarchy = AnalyzeScanHierarchy::Series(SeriesScanHierarchy {
                series: SeriesLink::Resolved(series_ref.clone()),
                series_root_path: job.series_root_path.clone(),
            });
            let analyzed = MediaAnalyzed {
                library_id: job.library_id,
                media_id: MediaID::Series(series_id),
                variant: VideoMediaType::Series,
                hierarchy: hierarchy.clone(),
                node: ScanNodeKind::SeriesRoot,
                path_norm: job.series_root_path.as_str().to_string(),
                fingerprint: MediaFingerprint::default(),
                analyzed_at: Utc::now(),
                analysis: AnalysisContext {
                    technical: None,
                    demo_note: None,
                    tmdb_id_hint: None,
                },
                thumbnails: vec![],
            };
            let ready = MediaReadyForIndex {
                library_id: job.library_id,
                media_id: analyzed.media_id,
                variant: analyzed.variant,
                hierarchy: hierarchy.clone(),
                node: analyzed.node.clone(),
                normalized_title: series_ref.title.clone(),
                analyzed,
                prepared_at: Utc::now(),
                image_jobs: vec![],
            };

            let _ = self
                .states
                .mark_resolved(
                    job.library_id,
                    job.series_root_path.clone(),
                    series_ref.clone(),
                )
                .await?;

            Ok(SeriesResolution { series_ref, ready })
        }

        async fn mark_failed(
            &self,
            library_id: LibraryId,
            series_root_path: SeriesRootPath,
            reason: String,
        ) -> Result<()> {
            let _ = self
                .states
                .mark_failed(library_id, series_root_path, reason)
                .await?;
            Ok(())
        }

        async fn get_state(
            &self,
            library_id: LibraryId,
            series_root_path: &SeriesRootPath,
        ) -> Result<Option<SeriesScanState>> {
            self.states.get(library_id, series_root_path).await
        }
    }

    struct StubIndexActor;

    #[async_trait]
    impl IndexerActor for StubIndexActor {
        async fn index(
            &self,
            command: IndexCommand,
        ) -> Result<IndexingOutcome> {
            Ok(IndexingOutcome {
                library_id: command.job.library_id,
                path_norm: command.job.path_norm,
                media_id: command.ready.media_id,
                hierarchy: command.job.hierarchy,
                indexed_at: Utc::now(),
                upserted: true,
                media: None,
                change: IndexingChange::Created,
            })
        }
    }

    struct StubImageActor;

    #[async_trait]
    impl ImageFetchActor for StubImageActor {
        async fn fetch(&self, _job: &ImageFetchJob) -> Result<()> {
            Ok(())
        }
    }

    async fn dispatcher_fixture(
        pool: &PgPool,
    ) -> (
        DefaultJobDispatcher<
            PostgresQueueService,
            InProcJobEventBus,
            PostgresCursorRepository,
        >,
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

        sqlx::query(
            r#"
            DELETE FROM orchestrator_jobs
            WHERE library_id = $1
            "#,
        )
        .bind(library_id.as_uuid())
        .execute(pool)
        .await
        .expect("clear fixture jobs");

        let movie_root_path = MovieRootPath::try_new_under_library_root(
            "/library",
            "/library/movie",
        )
        .unwrap();

        let hierarchy = AnalyzeScanHierarchy::Movie(MovieScanHierarchy {
            movie_root_path: movie_root_path.clone(),
            movie_id: None,
            extra_tag: None,
        });

        let context = FolderScanContext::Movie(MovieFolderScanContext {
            library_id,
            movie_root_path,
        });

        let unique_hash = format!("test-{}", Uuid::now_v7());
        let folder_actor = Arc::new(StubFolderActor {
            plan: FolderListingPlan {
                directories: vec![PathBuf::from("/library/movie/child")],
                media_files: vec![PathBuf::from("/library/movie/movie.mkv")],
                ancillary_files: vec![],
                generated_listing_hash: unique_hash.clone(),
            },
            discovered: vec![MediaFileDiscovered {
                library_id,
                path_norm: "/library/movie/movie.mkv".into(),
                fingerprint: MediaFingerprint {
                    device_id: None,
                    inode: None,
                    size: 1,
                    mtime: 1,
                    weak_hash: None,
                },
                classified_as: MediaKindHint::Movie,
                media_id: MediaID::new(VideoMediaType::Movie),
                variant: VideoMediaType::Movie,
                node: ScanNodeKind::MovieFolder,
                hierarchy,
                context: context.clone(),
                scan_reason: ScanReason::BulkSeed,
            }],
            children: vec![],
            summary: FolderScanSummary {
                context,
                discovered_files: 1,
                enqueued_subfolders: 1,
                listing_hash: unique_hash,
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
        let series_states: Arc<Box<dyn SeriesScanStateRepository>> =
            Arc::new(Box::new(InMemorySeriesScanStateRepository::default()));
        let series_resolver =
            Arc::new(StubSeriesResolver::new(Arc::clone(&series_states)));

        (
            DefaultJobDispatcher::new(
                Arc::clone(&queue),
                Arc::clone(&events),
                Arc::clone(&cursors),
                Arc::clone(&series_states),
                series_resolver,
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
        JobLease::new(
            record,
            "test-worker".into(),
            chrono::Duration::seconds(30),
        )
    }

    #[tokio::test]
    async fn folder_scan_dispatch_enqueues_follow_up_work() {
        let database_url = match std::env::var("DATABASE_URL") {
            Ok(url) => url,
            Err(_) => {
                eprintln!("skipping: DATABASE_URL not set");
                return;
            }
        };

        let pool = match PgPool::connect(&database_url).await {
            Ok(pool) => pool,
            Err(err) => {
                eprintln!(
                    "skipping: failed to connect to DATABASE_URL ({err})"
                );
                return;
            }
        };

        if let Err(err) = crate::MIGRATOR.run(&pool).await {
            eprintln!("skipping: migrations failed ({err})");
            return;
        }

        let (dispatcher, queue, events, cursors, _correlations) =
            dispatcher_fixture(&pool).await;
        let mut job_rx = events.subscribe();
        let mut domain_rx = events.subscribe_scan();

        let lease = lease_for_payload(JobPayload::FolderScan(FolderScanJob {
            context: FolderScanContext::Movie(MovieFolderScanContext {
                library_id: FIXTURE_LIB_A,
                movie_root_path: MovieRootPath::try_new_under_library_root(
                    "/library",
                    "/library/movie",
                )
                .unwrap(),
            }),
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
        let folder_path = match &lease.job.payload {
            JobPayload::FolderScan(job) => job.context.folder_path_norm(),
            _ => panic!("expected folder scan payload"),
        };
        let cursor_id = ScanCursorId::new(
            lease.job.payload.library_id(),
            &vec![PathBuf::from(folder_path)],
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

    // #[sqlx::test(migrator = "crate::MIGRATOR")]
    // async fn media_analyze_dispatch_enqueues_metadata(pool: PgPool) {
    //     let (dispatcher, queue, events, _, _correlations) =
    //         dispatcher_fixture(&pool).await;
    //     let mut job_rx = events.subscribe();

    //     let job = MediaAnalyzeJob {
    //         library_id: FIXTURE_LIB_A,
    //         media_id: MediaID::new(VideoMediaType::Movie),
    //         variant: VideoMediaType::Movie,
    //         hierarchy: SeriesScanHierarchy::default(),
    //         node: ScanNodeKind::MovieFolder,
    //         path_norm: "/library/movie.mkv".into(),
    //         fingerprint: MediaFingerprint {
    //             device_id: None,
    //             inode: None,
    //             size: 100,
    //             mtime: 1,
    //             weak_hash: None,
    //         },
    //         discovered_at: Utc::now(),
    //         scan_reason: ScanReason::BulkSeed,
    //     };
    //     let lease = lease_for_payload(JobPayload::MediaAnalyze(job));

    //     let status = dispatcher.dispatch(&lease).await;
    //     assert!(matches!(status, DispatchStatus::Success));

    //     let dequeue = DequeueRequest {
    //         kind: JobKind::MetadataEnrich,
    //         worker_id: "test".into(),
    //         lease_ttl: chrono::Duration::seconds(30),
    //         selector: None,
    //     };
    //     let metadata_job = queue.dequeue(dequeue).await.expect("dequeue ok");
    //     assert!(metadata_job.is_some(), "expected metadata job");

    //     tokio::time::timeout(Duration::from_millis(100), async {
    //         let mut saw_enqueue = false;
    //         while let Ok(event) = job_rx.try_recv() {
    //             if matches!(event.payload, JobEventPayload::Enqueued { .. }) {
    //                 saw_enqueue = true;
    //             }
    //         }
    //         assert!(saw_enqueue, "expected JobEnqueued event");
    //     })
    //     .await
    //     .ok();
    // }

    // #[sqlx::test(migrator = "crate::MIGRATOR")]
    // async fn metadata_enrich_transient_error_requests_retry(pool: PgPool) {
    //     struct TransientMetadataActor;

    //     #[async_trait]
    //     impl MetadataActor for TransientMetadataActor {
    //         async fn enrich(
    //             &self,
    //             _command: MetadataCommand,
    //         ) -> Result<MediaReadyForIndex> {
    //             Err(MediaError::Internal("tmdb timeout".into()))
    //         }
    //     }

    //     let queue = Arc::new(
    //         PostgresQueueService::new(pool.clone())
    //             .await
    //             .expect("queue init"),
    //     );
    //     let events = Arc::new(InProcJobEventBus::new(8));
    //     let cursors = Arc::new(PostgresCursorRepository::new(pool.clone()));

    //     upsert_library(
    //         &pool,
    //         FIXTURE_LIB_A,
    //         "Dispatcher Fixture A",
    //         LibraryType::Movies,
    //         vec!["/".into()],
    //     )
    //     .await
    //     .expect("seed fixture library A");

    //     let actors = DispatcherActors::new(
    //         Arc::new(StubFolderActor {
    //             plan: FolderListingPlan::default(),
    //             discovered: vec![],
    //             children: vec![],
    //             summary: FolderScanSummary {
    //                 context: FolderScanContext {
    //                     library_id: FIXTURE_LIB_A,
    //                     folder_path_norm: "/".into(),
    //                     node: ScanNodeKind::default(),
    //                     hierarchy: SeriesScanHierarchy::default(),
    //                     reason: ScanReason::BulkSeed,
    //                 },
    //                 discovered_files: 0,
    //                 enqueued_subfolders: 0,
    //                 listing_hash: "".into(),
    //                 completed_at: Utc::now(),
    //             },
    //         }) as Arc<dyn FolderScanActor>,
    //         Arc::new(StubAnalyzeActor) as Arc<dyn MediaAnalyzeActor>,
    //         Arc::new(TransientMetadataActor) as Arc<dyn MetadataActor>,
    //         Arc::new(StubIndexActor) as Arc<dyn IndexerActor>,
    //         Arc::new(StubImageActor) as Arc<dyn ImageFetchActor>,
    //     );

    //     let correlations = CorrelationCache::default();
    //     let series_states: Arc<Box<dyn SeriesScanStateRepository>> =
    //         Arc::new(Box::new(InMemorySeriesScanStateRepository::default()));
    //     let series_resolver =
    //         Arc::new(StubSeriesResolver::new(Arc::clone(&series_states)));

    //     let dispatcher = DefaultJobDispatcher::new(
    //         Arc::clone(&queue),
    //         Arc::clone(&events),
    //         Arc::clone(&cursors),
    //         Arc::clone(&series_states),
    //         series_resolver,
    //         actors,
    //         correlations,
    //     );

    //     let job = MetadataEnrichJob {
    //         library_id: FIXTURE_LIB_A,
    //         media_id: MediaID::new(VideoMediaType::Movie),
    //         variant: VideoMediaType::Movie,
    //         hierarchy: SeriesScanHierarchy::default(),
    //         node: ScanNodeKind::MovieFolder,
    //         path_norm: "/library/movie.mkv".into(),
    //         fingerprint: MediaFingerprint::default(),
    //         scan_reason: ScanReason::BulkSeed,
    //     };
    //     let lease = lease_for_payload(JobPayload::MetadataEnrich(job));

    //     let status = dispatcher.dispatch(&lease).await;
    //     match status {
    //         DispatchStatus::Retry { error } => {
    //             assert!(error.contains("tmdb timeout"));
    //         }
    //         other => panic!("expected retry status, got {other:?}"),
    //     }
    // }

    // #[sqlx::test(migrator = "crate::MIGRATOR")]
    // async fn metadata_enrich_uses_ready_media_id_for_index_job(pool: PgPool) {
    //     struct CanonicalizingMetadataActor;

    //     #[async_trait]
    //     impl MetadataActor for CanonicalizingMetadataActor {
    //         async fn enrich(
    //             &self,
    //             command: MetadataCommand,
    //         ) -> Result<MediaReadyForIndex> {
    //             let canonical = MediaID::from((
    //                 Uuid::from_u128(0xcccccccccccccccccccccccccccccccc),
    //                 command.job.variant,
    //             ));

    //             Ok(MediaReadyForIndex {
    //                 library_id: command.job.library_id,
    //                 media_id: canonical,
    //                 variant: command.job.variant,
    //                 hierarchy: command.job.hierarchy.clone(),
    //                 node: command.job.node.clone(),
    //                 normalized_title: None,
    //                 analyzed: command.analyzed,
    //                 prepared_at: Utc::now(),
    //                 image_jobs: Vec::new(),
    //             })
    //         }
    //     }

    //     let queue = Arc::new(
    //         PostgresQueueService::new(pool.clone())
    //             .await
    //             .expect("queue init"),
    //     );
    //     let events = Arc::new(InProcJobEventBus::new(8));
    //     let cursors = Arc::new(PostgresCursorRepository::new(pool.clone()));

    //     upsert_library(
    //         &pool,
    //         FIXTURE_LIB_A,
    //         "Dispatcher Fixture A",
    //         LibraryType::Movies,
    //         vec!["/".into()],
    //     )
    //     .await
    //     .expect("seed fixture library A");

    //     let actors = DispatcherActors::new(
    //         Arc::new(StubFolderActor {
    //             plan: FolderListingPlan::default(),
    //             discovered: vec![],
    //             children: vec![],
    //             summary: FolderScanSummary {
    //                 context: FolderScanContext {
    //                     library_id: FIXTURE_LIB_A,
    //                     folder_path_norm: "/".into(),
    //                     node: ScanNodeKind::Unknown,
    //                     hierarchy: SeriesScanHierarchy::default(),
    //                     reason: ScanReason::BulkSeed,
    //                 },
    //                 discovered_files: 0,
    //                 enqueued_subfolders: 0,
    //                 listing_hash: "".into(),
    //                 completed_at: Utc::now(),
    //             },
    //         }) as Arc<dyn FolderScanActor>,
    //         Arc::new(StubAnalyzeActor) as Arc<dyn MediaAnalyzeActor>,
    //         Arc::new(CanonicalizingMetadataActor) as Arc<dyn MetadataActor>,
    //         Arc::new(StubIndexActor) as Arc<dyn IndexerActor>,
    //         Arc::new(StubImageActor) as Arc<dyn ImageFetchActor>,
    //     );

    //     let correlations = CorrelationCache::default();
    //     let series_states: Arc<Box<dyn SeriesScanStateRepository>> =
    //         Arc::new(Box::new(InMemorySeriesScanStateRepository::default()));
    //     let series_resolver =
    //         Arc::new(StubSeriesResolver::new(Arc::clone(&series_states)));

    //     let dispatcher = DefaultJobDispatcher::new(
    //         Arc::clone(&queue),
    //         Arc::clone(&events),
    //         Arc::clone(&cursors),
    //         Arc::clone(&series_states),
    //         series_resolver,
    //         actors,
    //         correlations,
    //     );

    //     let original = MediaID::from((
    //         Uuid::from_u128(0xdddddddddddddddddddddddddddddddd),
    //         VideoMediaType::Movie,
    //     ));
    //     let job = MetadataEnrichJob {
    //         library_id: FIXTURE_LIB_A,
    //         media_id: original,
    //         variant: VideoMediaType::Movie,
    //         hierarchy: SeriesScanHierarchy::default(),
    //         node: ScanNodeKind::MovieFolder,
    //         path_norm: "/library/movie.mkv".into(),
    //         fingerprint: MediaFingerprint::default(),
    //         scan_reason: ScanReason::BulkSeed,
    //     };
    //     let lease = lease_for_payload(JobPayload::MetadataEnrich(job));

    //     let status = dispatcher.dispatch(&lease).await;
    //     assert!(matches!(status, DispatchStatus::Success));

    //     let dequeue = DequeueRequest {
    //         kind: JobKind::IndexUpsert,
    //         worker_id: "test".into(),
    //         lease_ttl: chrono::Duration::seconds(30),
    //         selector: None,
    //     };
    //     let index_lease = queue.dequeue(dequeue).await.expect("dequeue ok");
    //     let index_lease = index_lease.expect("expected index upsert job");

    //     let JobPayload::IndexUpsert(index_job) = index_lease.job.payload else {
    //         panic!("expected IndexUpsert payload");
    //     };
    //     let expected = MediaID::from((
    //         Uuid::from_u128(0xcccccccccccccccccccccccccccccccc),
    //         VideoMediaType::Movie,
    //     ));
    //     assert_eq!(index_job.media_id, expected);
    // }

    // #[sqlx::test(migrator = "crate::MIGRATOR")]
    // async fn media_error_invalid_marks_dead_letter(pool: PgPool) {
    //     struct FailingMetadataActor;

    //     #[async_trait]
    //     impl MetadataActor for FailingMetadataActor {
    //         async fn enrich(
    //             &self,
    //             _command: MetadataCommand,
    //         ) -> Result<MediaReadyForIndex> {
    //             Err(MediaError::InvalidMedia("bad metadata".into()))
    //         }
    //     }

    //     let queue = Arc::new(
    //         PostgresQueueService::new(pool.clone())
    //             .await
    //             .expect("queue init"),
    //     );
    //     let events = Arc::new(InProcJobEventBus::new(8));
    //     let cursors = Arc::new(PostgresCursorRepository::new(pool.clone()));

    //     upsert_library(
    //         &pool,
    //         FIXTURE_LIB_B,
    //         "Dispatcher Fixture B",
    //         LibraryType::Movies,
    //         vec!["/".into()],
    //     )
    //     .await
    //     .expect("seed fixture library B");

    //     let actors = DispatcherActors::new(
    //         Arc::new(StubFolderActor {
    //             plan: FolderListingPlan::default(),
    //             discovered: vec![],
    //             children: vec![],
    //             summary: FolderScanSummary {
    //                 context: FolderScanContext {
    //                     library_id: FIXTURE_LIB_B,
    //                     folder_path_norm: "/".into(),
    //                     node: ScanNodeKind::Unknown,
    //                     hierarchy: SeriesScanHierarchy::default(),
    //                     reason: ScanReason::BulkSeed,
    //                 },
    //                 discovered_files: 0,
    //                 enqueued_subfolders: 0,
    //                 listing_hash: "".into(),
    //                 completed_at: Utc::now(),
    //             },
    //         }) as Arc<dyn FolderScanActor>,
    //         Arc::new(StubAnalyzeActor) as Arc<dyn MediaAnalyzeActor>,
    //         Arc::new(FailingMetadataActor) as Arc<dyn MetadataActor>,
    //         Arc::new(StubIndexActor) as Arc<dyn IndexerActor>,
    //         Arc::new(StubImageActor) as Arc<dyn ImageFetchActor>,
    //     );

    //     let correlations = CorrelationCache::default();
    //     let series_states: Arc<Box<dyn SeriesScanStateRepository>> =
    //         Arc::new(Box::new(InMemorySeriesScanStateRepository::default()));
    //     let series_resolver =
    //         Arc::new(StubSeriesResolver::new(Arc::clone(&series_states)));

    //     let dispatcher = DefaultJobDispatcher::new(
    //         Arc::clone(&queue),
    //         Arc::clone(&events),
    //         Arc::clone(&cursors),
    //         Arc::clone(&series_states),
    //         series_resolver,
    //         actors,
    //         correlations,
    //     );

    //     let job = MetadataEnrichJob {
    //         library_id: FIXTURE_LIB_B,
    //         media_id: MediaID::new(VideoMediaType::Movie),
    //         variant: VideoMediaType::Movie,
    //         hierarchy: SeriesScanHierarchy::default(),
    //         node: ScanNodeKind::MovieFolder,
    //         path_norm: "/library/movie.mkv".into(),
    //         fingerprint: MediaFingerprint::default(),
    //         scan_reason: ScanReason::BulkSeed,
    //     };
    //     let lease = lease_for_payload(JobPayload::MetadataEnrich(job));

    //     let status = dispatcher.dispatch(&lease).await;
    //     assert!(matches!(status, DispatchStatus::DeadLetter { .. }));
    // }

    // #[tokio::test]
    // async fn correlation_id_propagates_when_provided() {
    //     let correlations = CorrelationCache::default();

    //     let library_id = LibraryId(uuid::Uuid::now_v7());
    //     let payload = JobPayload::FolderScan(FolderScanJob {
    //         library_id,
    //         folder_path_norm: "/folder".into(),
    //         hierarchy: SeriesScanHierarchy::default(),
    //         scan_reason: ScanReason::UserRequested,
    //         enqueue_time: Utc::now(),
    //         device_id: None,
    //     });
    //     let priority = JobPriority::P1;
    //     let handle = JobHandle::accepted(JobId::new(), &payload, priority);

    //     let provided = uuid::Uuid::now_v7();
    //     let enqueued_event = JobEvent::from_handle(
    //         &handle,
    //         Some(provided),
    //         JobEventPayload::Enqueued {
    //             job_id: handle.job_id,
    //             kind: payload.kind(),
    //             priority,
    //         },
    //         None,
    //     );

    //     correlations
    //         .remember(handle.job_id, enqueued_event.meta.correlation_id)
    //         .await;

    //     assert_eq!(enqueued_event.meta.correlation_id, provided);

    //     let dequeue_event = JobEvent::from_job(
    //         Some(correlations.fetch_or_generate(handle.job_id).await),
    //         payload.library_id(),
    //         handle.dedupe_key.clone(),
    //         None,
    //         JobEventPayload::Dequeued {
    //             job_id: handle.job_id,
    //             kind: payload.kind(),
    //             priority,
    //             lease_id: LeaseId::new(),
    //         },
    //     );

    //     assert_eq!(dequeue_event.meta.correlation_id, provided);

    //     let completed_event = JobEvent::from_job(
    //         Some(correlations.take_or_generate(handle.job_id).await),
    //         payload.library_id(),
    //         handle.dedupe_key.clone(),
    //         None,
    //         JobEventPayload::Completed {
    //             job_id: handle.job_id,
    //             kind: payload.kind(),
    //             priority,
    //         },
    //     );

    //     assert_eq!(completed_event.meta.correlation_id, provided);
    //     assert!(correlations.fetch(&handle.job_id).await.is_none());
    // }

    // #[tokio::test]
    // async fn correlation_id_generated_when_missing() {
    //     let correlations = CorrelationCache::default();

    //     let library_id = LibraryId(uuid::Uuid::now_v7());
    //     let payload = JobPayload::FolderScan(FolderScanJob {
    //         library_id,
    //         folder_path_norm: "/missing".into(),
    //         hierarchy: SeriesScanHierarchy::default(),
    //         scan_reason: ScanReason::BulkSeed,
    //         enqueue_time: Utc::now(),
    //         device_id: None,
    //     });
    //     let priority = JobPriority::P2;
    //     let handle = JobHandle::accepted(JobId::new(), &payload, priority);

    //     let enqueued_event = JobEvent::from_handle(
    //         &handle,
    //         None,
    //         JobEventPayload::Enqueued {
    //             job_id: handle.job_id,
    //             kind: payload.kind(),
    //             priority,
    //         },
    //         None,
    //     );

    //     let generated = enqueued_event.meta.correlation_id;
    //     assert_ne!(generated, uuid::Uuid::nil());

    //     correlations.remember(handle.job_id, generated).await;

    //     let dequeue_event = JobEvent::from_job(
    //         Some(correlations.fetch_or_generate(handle.job_id).await),
    //         payload.library_id(),
    //         handle.dedupe_key.clone(),
    //         None,
    //         JobEventPayload::Dequeued {
    //             job_id: handle.job_id,
    //             kind: payload.kind(),
    //             priority,
    //             lease_id: LeaseId::new(),
    //         },
    //     );
    //     assert_eq!(dequeue_event.meta.correlation_id, generated);

    //     let failed_event = JobEvent::from_job(
    //         Some(correlations.fetch_or_generate(handle.job_id).await),
    //         payload.library_id(),
    //         handle.dedupe_key.clone(),
    //         None,
    //         JobEventPayload::Failed {
    //             job_id: handle.job_id,
    //             kind: payload.kind(),
    //             priority,
    //             retryable: true,
    //         },
    //     );
    //     assert_eq!(failed_event.meta.correlation_id, generated);

    //     let dead_letter_event = JobEvent::from_job(
    //         Some(correlations.take_or_generate(handle.job_id).await),
    //         payload.library_id(),
    //         handle.dedupe_key.clone(),
    //         None,
    //         JobEventPayload::DeadLettered {
    //             job_id: handle.job_id,
    //             kind: payload.kind(),
    //             priority,
    //         },
    //     );
    //     assert_eq!(dead_letter_event.meta.correlation_id, generated);
    //     assert!(correlations.fetch(&handle.job_id).await.is_none());
    // }
}
