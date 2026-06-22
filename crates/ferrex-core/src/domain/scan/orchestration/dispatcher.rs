use std::{any::type_name, fmt, path::PathBuf, sync::Arc};

use crate::database::repository_ports::{
    intelligence::IntelligenceRepository, library::LibraryRepository,
    transcripts::TranscriptRepository,
};
use crate::domain::scan::actors::image_fetch::ImageFetchActor;
use crate::domain::scan::actors::index::{
    IndexCommand, IndexerActor, IndexingOutcome,
};
use crate::domain::scan::actors::metadata::{
    MediaReadyForIndex, MetadataActor, MetadataCommand,
};
use crate::domain::scan::actors::{
    analyze::{AnalysisContext, MediaAnalyzeActor, MediaAnalyzed},
    folder::FolderScanActor,
    messages::FolderScanOutcome,
};
use crate::domain::scan::orchestration::{
    context::FolderScanContext,
    correlation::CorrelationCache,
    delta::{
        FolderDeltaRepository, NoopFolderDeltaRepository,
        fingerprints_equivalent, reconcile_direct_media,
        removed_child_prefixes,
    },
    enqueuer::PipelineEnqueuer,
    events::{ScanEvent, ScanEventBus},
    job::{
        AnalyzeScanHierarchy, DependencyKey, EnqueueRequest, EpisodeMatchJob,
        FolderScanJob, ImageFetchJob, IndexUpsertJob, JobPayload, JobPriority,
        MediaAnalyzeJob, MediaFingerprint, MetadataEnrichJob, ScanReason,
        SeriesResolveJob,
    },
    lease::JobLease,
    queue::QueueService,
    scan_cursor::{ScanCursor, ScanCursorId, ScanCursorRepository},
    series::{
        EpisodeDependencyDecision, SeriesCoordinator, SeriesDependencyReleaser,
        SeriesResolverPort,
    },
    series_state::SeriesScanStateRepository,
};
use crate::error::{MediaError, Result};
use crate::infra::media::timed_text::{
    TimedTextExtractionConfig, TimedTextExtractionOutcome,
    TimedTextExtractionRequest, TimedTextExtractor,
};
use async_trait::async_trait;
use chrono::Utc;
use ferrex_model::{Media, MediaFile, MediaID, VideoMediaType};
use tracing::{Instrument, debug, debug_span, warn};

async fn path_exists(path: &str) -> bool {
    tokio::fs::try_exists(path).await.unwrap_or(false)
}

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
    folder_flow: FolderScanFlow<Q, E, C>,
    media_pipeline_flow: MediaPipelineFlow<Q, E>,
}

impl<Q, E, C> fmt::Debug for DefaultJobDispatcher<Q, E, C>
where
    Q: QueueService + Send + Sync + 'static,
    E: ScanEventBus + Send + Sync + 'static,
    C: ScanCursorRepository + Send + Sync + 'static,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DefaultJobDispatcher")
            .field("folder_flow", &self.folder_flow)
            .field("media_pipeline_flow", &self.media_pipeline_flow)
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
        let planner = FollowUpPlanner::new();
        let follow_ups =
            FollowUpEnqueuer::new(queue, events.clone(), correlations);
        let deltas = Arc::new(NoopFolderDeltaRepository);
        let series_coordinator =
            Arc::new(SeriesCoordinator::new(series_states, series_resolver));

        Self {
            folder_flow: FolderScanFlow::new(
                events.clone(),
                cursors,
                actors.clone(),
                series_coordinator.clone(),
                deltas,
                planner,
                follow_ups.clone(),
            ),
            media_pipeline_flow: MediaPipelineFlow::new(
                events,
                actors,
                series_coordinator,
                planner,
                follow_ups,
            ),
        }
    }

    pub fn with_delta_repository(
        mut self,
        deltas: Arc<dyn FolderDeltaRepository>,
    ) -> Self {
        self.folder_flow = self.folder_flow.with_delta_repository(deltas);
        self
    }

    pub fn with_intelligence_repository(
        mut self,
        intelligence: Arc<dyn IntelligenceRepository>,
    ) -> Self {
        self.folder_flow = self
            .folder_flow
            .with_intelligence_repository(Arc::clone(&intelligence));
        self.media_pipeline_flow = self
            .media_pipeline_flow
            .with_intelligence_repository(intelligence);
        self
    }

    pub fn with_timed_text_extraction(
        mut self,
        libraries: Arc<dyn LibraryRepository>,
        transcripts: Arc<dyn TranscriptRepository>,
        config: TimedTextExtractionConfig,
    ) -> Self {
        self.media_pipeline_flow = self
            .media_pipeline_flow
            .with_timed_text_extraction(libraries, transcripts, config);
        self
    }
}

fn classify_media_error(err: MediaError) -> DispatchStatus {
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

struct FollowUpEnqueuer<Q, E>
where
    Q: QueueService + Send + Sync + 'static,
    E: ScanEventBus + Send + Sync + 'static,
{
    enqueuer: PipelineEnqueuer<Q, E>,
}

impl<Q, E> Clone for FollowUpEnqueuer<Q, E>
where
    Q: QueueService + Send + Sync + 'static,
    E: ScanEventBus + Send + Sync + 'static,
{
    fn clone(&self) -> Self {
        Self {
            enqueuer: self.enqueuer.clone(),
        }
    }
}

impl<Q, E> fmt::Debug for FollowUpEnqueuer<Q, E>
where
    Q: QueueService + Send + Sync + 'static,
    E: ScanEventBus + Send + Sync + 'static,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FollowUpEnqueuer")
            .field("enqueuer", &self.enqueuer)
            .finish()
    }
}

impl<Q, E> FollowUpEnqueuer<Q, E>
where
    Q: QueueService + Send + Sync + 'static,
    E: ScanEventBus + Send + Sync + 'static,
{
    fn new(
        queue: Arc<Q>,
        events: Arc<E>,
        correlations: CorrelationCache,
    ) -> Self {
        let enqueuer = PipelineEnqueuer::new(queue, events, correlations);

        Self { enqueuer }
    }

    async fn enqueue(&self, request: EnqueueRequest) -> DispatchStatus {
        match self.enqueuer.enqueue(request).await {
            Ok(_) => DispatchStatus::Success,
            Err(err) => classify_media_error(err),
        }
    }

    async fn enqueue_many(
        &self,
        requests: Vec<EnqueueRequest>,
    ) -> DispatchStatus {
        match self.enqueuer.enqueue_many(requests).await {
            Ok(_) => DispatchStatus::Success,
            Err(err) => classify_media_error(err),
        }
    }

    async fn release_dependency(
        &self,
        library_id: crate::types::ids::LibraryId,
        dependency_key: &DependencyKey,
    ) -> Result<()> {
        self.enqueuer
            .release_dependency(library_id, dependency_key)
            .await
            .map(|_| ())
    }
}

#[async_trait]
impl<Q, E> SeriesDependencyReleaser for FollowUpEnqueuer<Q, E>
where
    Q: QueueService + Send + Sync + 'static,
    E: ScanEventBus + Send + Sync + 'static,
{
    async fn release_series_root_dependency(
        &self,
        library_id: crate::types::ids::LibraryId,
        series_root_path: &crate::domain::scan::orchestration::context::SeriesRootPath,
    ) -> Result<()> {
        self.release_dependency(
            library_id,
            &DependencyKey::series_root(series_root_path),
        )
        .await
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct FollowUpPlanner;

impl FollowUpPlanner {
    fn new() -> Self {
        Self
    }

    fn series_resolve_for_folder(
        &self,
        series_ctx: &crate::domain::scan::orchestration::context::SeriesFolderScanContext,
        folder_name: String,
        scan_reason: ScanReason,
    ) -> EnqueueRequest {
        let series_job = SeriesResolveJob {
            library_id: series_ctx.library_id,
            series_root_path: series_ctx.series_root_path.clone(),
            hint: None,
            folder_name,
            scan_reason,
        };
        let priority =
            priority_for_reason(&scan_reason).elevate(JobPriority::P0);
        EnqueueRequest::new(priority, JobPayload::SeriesResolve(series_job))
    }

    fn media_analyze_for_discovery(
        &self,
        media: &crate::domain::scan::actors::messages::MediaFileDiscovered,
    ) -> EnqueueRequest {
        // Elevate analyze priority so per-item pipelines advance ahead of more scans.
        // This prevents breadth-first scanning from starving downstream stages.
        let analyze_priority =
            priority_for_reason(&media.scan_reason).elevate(JobPriority::P0);

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
        EnqueueRequest::new(analyze_priority, JobPayload::MediaAnalyze(analyze))
    }

    fn metadata_after_analysis(
        &self,
        job: &MediaAnalyzeJob,
        analyzed: &MediaAnalyzed,
    ) -> EnqueueRequest {
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
        EnqueueRequest::new(priority, JobPayload::MetadataEnrich(meta_job))
    }

    fn metadata_after_resolved_episode_analysis(
        &self,
        job: &MediaAnalyzeJob,
        analyzed: &MediaAnalyzed,
        hierarchy: crate::domain::scan::orchestration::context::EpisodeScanHierarchy,
    ) -> EnqueueRequest {
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

        let priority =
            priority_for_reason(&job.scan_reason).elevate(JobPriority::P0);
        EnqueueRequest::new(priority, JobPayload::MetadataEnrich(meta_job))
    }

    fn episode_match_after_analysis(
        &self,
        job: &MediaAnalyzeJob,
        analyzed: &MediaAnalyzed,
        hierarchy: &crate::domain::scan::orchestration::context::EpisodeScanHierarchy,
        dependency_key: DependencyKey,
    ) -> EnqueueRequest {
        let match_job = EpisodeMatchJob {
            library_id: job.library_id,
            media_id: analyzed.media_id,
            path_norm: job.path_norm.clone(),
            fingerprint: analyzed.fingerprint.clone(),
            hierarchy: hierarchy.clone(),
            node: analyzed.node.clone(),
            scan_reason: job.scan_reason,
        };

        let priority =
            priority_for_reason(&job.scan_reason).elevate(JobPriority::P0);
        EnqueueRequest::new(priority, JobPayload::EpisodeMatch(match_job))
            .with_dependency(dependency_key)
    }

    fn index_for_ready(
        &self,
        source_library_id: crate::types::ids::LibraryId,
        ready: &MediaReadyForIndex,
    ) -> EnqueueRequest {
        let index_job = IndexUpsertJob {
            library_id: ready.library_id,
            media_id: ready.media_id,
            variant: ready.variant,
            hierarchy: ready.hierarchy.clone(),
            node: ready.node.clone(),
            path_norm: ready.analyzed.path_norm.clone(),
            idempotency_key: format!(
                "index:{}:{}",
                source_library_id, ready.analyzed.path_norm
            ),
        };

        // Bias index upserts to complete the item flow promptly.
        EnqueueRequest::new(JobPriority::P0, JobPayload::IndexUpsert(index_job))
    }

    fn image_fetches_for_ready(
        &self,
        ready: &MediaReadyForIndex,
    ) -> Vec<EnqueueRequest> {
        ready
            .image_jobs
            .iter()
            .map(|fetch_job| {
                EnqueueRequest::new(
                    fetch_job.priority_hint.job_priority(),
                    JobPayload::ImageFetch(fetch_job.clone()),
                )
            })
            .collect()
    }

    fn metadata_after_episode_match(
        &self,
        job: &EpisodeMatchJob,
        hierarchy: crate::domain::scan::orchestration::context::EpisodeScanHierarchy,
    ) -> EnqueueRequest {
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
        EnqueueRequest::new(priority, JobPayload::MetadataEnrich(meta_job))
    }
}

struct FolderScanFlow<Q, E, C>
where
    Q: QueueService + Send + Sync + 'static,
    E: ScanEventBus + Send + Sync + 'static,
    C: ScanCursorRepository + Send + Sync + 'static,
{
    events: Arc<E>,
    cursors: Arc<C>,
    actors: DispatcherActors,
    series_coordinator: Arc<SeriesCoordinator>,
    deltas: Arc<dyn FolderDeltaRepository>,
    intelligence: Option<Arc<dyn IntelligenceRepository>>,
    planner: FollowUpPlanner,
    follow_ups: FollowUpEnqueuer<Q, E>,
}

impl<Q, E, C> fmt::Debug for FolderScanFlow<Q, E, C>
where
    Q: QueueService + Send + Sync + 'static,
    E: ScanEventBus + Send + Sync + 'static,
    C: ScanCursorRepository + Send + Sync + 'static,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FolderScanFlow")
            .field("events", &type_name::<E>())
            .field("cursors", &type_name::<C>())
            .field("actors", &self.actors)
            .field("series_coordinator", &self.series_coordinator)
            .field("deltas", &"FolderDeltaRepository")
            .field("intelligence", &self.intelligence.is_some())
            .field("planner", &self.planner)
            .field("follow_ups", &self.follow_ups)
            .finish()
    }
}

impl<Q, E, C> FolderScanFlow<Q, E, C>
where
    Q: QueueService + Send + Sync + 'static,
    E: ScanEventBus + Send + Sync + 'static,
    C: ScanCursorRepository + Send + Sync + 'static,
{
    fn new(
        events: Arc<E>,
        cursors: Arc<C>,
        actors: DispatcherActors,
        series_coordinator: Arc<SeriesCoordinator>,
        deltas: Arc<dyn FolderDeltaRepository>,
        planner: FollowUpPlanner,
        follow_ups: FollowUpEnqueuer<Q, E>,
    ) -> Self {
        Self {
            events,
            cursors,
            actors,
            series_coordinator,
            deltas,
            intelligence: None,
            planner,
            follow_ups,
        }
    }

    fn with_delta_repository(
        mut self,
        deltas: Arc<dyn FolderDeltaRepository>,
    ) -> Self {
        self.deltas = deltas;
        self
    }

    fn with_intelligence_repository(
        mut self,
        intelligence: Arc<dyn IntelligenceRepository>,
    ) -> Self {
        self.intelligence = Some(intelligence);
        self
    }

    async fn invalidate_intelligence_catalog_change(
        &self,
        library_id: crate::types::ids::LibraryId,
        media_id: MediaID,
        reason: &str,
    ) -> Result<()> {
        if let Some(intelligence) = &self.intelligence {
            intelligence
                .invalidate_media_catalog_change(library_id, media_id, reason)
                .await?;
        }
        Ok(())
    }

    async fn cleanup_deleted_prefixes(
        &self,
        library_id: crate::types::ids::LibraryId,
        prefixes: Vec<String>,
        reason: &str,
    ) -> DispatchStatus {
        if prefixes.is_empty() {
            return DispatchStatus::Success;
        }

        let affected_media = match self
            .deltas
            .list_media_by_prefixes(library_id, prefixes.clone())
            .await
        {
            Ok(media) => media,
            Err(err) => return classify_media_error(err),
        };

        if let Err(err) = self
            .deltas
            .mark_unavailable_by_prefixes(library_id, prefixes.clone(), reason)
            .await
        {
            return classify_media_error(err);
        }
        for media_id in affected_media {
            if let Err(err) = self
                .invalidate_intelligence_catalog_change(
                    library_id, media_id, reason,
                )
                .await
            {
                return classify_media_error(err);
            }
        }
        if let Err(err) = self
            .deltas
            .delete_folder_inventory_by_prefixes(library_id, prefixes.clone())
            .await
        {
            return classify_media_error(err);
        }
        if let Err(err) = self
            .cursors
            .delete_by_path_prefixes(library_id, prefixes)
            .await
        {
            return classify_media_error(err);
        }

        DispatchStatus::Success
    }

    async fn reconcile_folder_delta(
        &self,
        context: &FolderScanContext,
        plan: &crate::domain::scan::actors::folder::FolderListingPlan,
        discovered: Vec<
            crate::domain::scan::actors::messages::MediaFileDiscovered,
        >,
        children: &[FolderScanContext],
    ) -> std::result::Result<
        Vec<crate::domain::scan::actors::messages::MediaFileDiscovered>,
        DispatchStatus,
    > {
        let library_id = context.library_id();
        let folder_path = context.folder_path_norm().to_string();

        let stored = self
            .deltas
            .list_media_directly_under(library_id, &folder_path)
            .await
            .map_err(classify_media_error)?;
        let mut delta = reconcile_direct_media(stored, discovered);

        for move_delta in &delta.moves {
            self.deltas
                .move_media_by_path(
                    library_id,
                    &move_delta.old_path_norm,
                    &move_delta.new_path_norm,
                    &move_delta.fingerprint,
                )
                .await
                .map_err(classify_media_error)?;
        }

        let mut additions_requiring_pipeline = Vec::new();
        for media in delta.additions.into_iter() {
            let candidates = self
                .deltas
                .find_available_media_by_fingerprint(
                    library_id,
                    &media.fingerprint,
                    &media.path_norm,
                )
                .await
                .map_err(classify_media_error)?;

            let matching_candidates: Vec<_> = candidates
                .into_iter()
                .filter(|candidate| {
                    fingerprints_equivalent(
                        &candidate.fingerprint,
                        &media.fingerprint,
                    )
                })
                .collect();

            if let [candidate] = matching_candidates.as_slice()
                && !path_exists(&candidate.path_norm).await
            {
                self.deltas
                    .move_media_by_path(
                        library_id,
                        &candidate.path_norm,
                        &media.path_norm,
                        &media.fingerprint,
                    )
                    .await
                    .map_err(classify_media_error)?;
                continue;
            }

            additions_requiring_pipeline.push(media);
        }
        delta.additions = additions_requiring_pipeline;

        if !delta.removals.is_empty() {
            let removed_paths = delta
                .removals
                .iter()
                .map(|media| media.path_norm.clone())
                .collect();
            self.deltas
                .mark_unavailable_by_paths(
                    library_id,
                    removed_paths,
                    "folder_delta_file_missing",
                )
                .await
                .map_err(classify_media_error)?;
            for removed in &delta.removals {
                self.invalidate_intelligence_catalog_change(
                    library_id,
                    removed.media_id,
                    "folder_delta_file_missing",
                )
                .await
                .map_err(classify_media_error)?;
            }
        }

        let known_cursor_paths = self
            .cursors
            .list_by_library(library_id)
            .await
            .map_err(classify_media_error)?
            .into_iter()
            .map(|cursor| cursor.folder_path_norm);
        let current_child_paths = children
            .iter()
            .map(|child| child.folder_path_norm().to_string());
        let removed_prefixes = removed_child_prefixes(
            &folder_path,
            current_child_paths,
            known_cursor_paths,
        );
        if !removed_prefixes.is_empty() {
            match self
                .cleanup_deleted_prefixes(
                    library_id,
                    removed_prefixes,
                    "folder_delta_child_folder_missing",
                )
                .await
            {
                DispatchStatus::Success => {}
                status => return Err(status),
            }
        }

        let _ = plan;
        Ok(delta.media_requiring_pipeline())
    }

    async fn handle_missing_folder(
        &self,
        context: &FolderScanContext,
        plan: &crate::domain::scan::actors::folder::FolderListingPlan,
    ) -> DispatchStatus {
        let summary = match self.actors.folder.finalize(context, plan, &[], &[])
        {
            Ok(summary) => summary,
            Err(err) => return classify_media_error(err),
        };

        debug!(
            outcome = summary.outcome.as_str(),
            path = %context.folder_path_norm(),
            "emitting folder scan completion"
        );
        if let Err(err) = self
            .events
            .publish_scan_event(ScanEvent::FolderScanCompleted(summary))
            .await
        {
            return classify_media_error(err);
        }

        self.cleanup_deleted_prefixes(
            context.library_id(),
            vec![context.folder_path_norm().to_string()],
            "folder_delta_folder_missing",
        )
        .await
    }

    async fn dispatch(
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
                Err(err) => return classify_media_error(err),
            };

            if plan.folder_missing {
                return self.handle_missing_folder(&context, &plan).await;
            }

            // Check cursor to short-circuit unchanged listings
            let cursor_id = ScanCursorId::new(
                context.library_id(),
                &vec![PathBuf::from(context.folder_path_norm())],
            );
            let mut had_cursor = false;
            let mut listing_unchanged = false;
            let mut last_modified_at = None;
            match self.cursors.get(&cursor_id).await {
                Ok(Some(existing)) => {
                    had_cursor = true;
                    if existing.listing_hash == plan.generated_listing_hash {
                        listing_unchanged = true;
                        last_modified_at = existing.last_modified_at;
                    }
                }
                Ok(None) => {}
                Err(err) => return classify_media_error(err),
            }

            if listing_unchanged {
                debug!("listing hash unchanged, refreshing cursor + emitting completion");

                // Even when we short-circuit, we still emit `FolderScanCompleted` so
                // downstream consumers (scan progress, bundle finalization trackers, etc.)
                // can treat this folder scan as a completed unit of work.
                //
                // We intentionally do *not* discover media, publish FolderDiscovered,
                // or enqueue series/analyze/metadata/index follow-ups here. An unchanged
                // listing only refreshes the cursor timestamp and emits folder completion
                // for progress accounting.
                let mut summary = match self.actors.folder.finalize(
                    &context,
                    &plan,
                    &[],
                    &[],
                ) {
                    Ok(summary) => summary,
                    Err(err) => return classify_media_error(err),
                };
                summary.outcome = FolderScanOutcome::UnchangedCursor;

                debug!(
                    outcome = summary.outcome.as_str(),
                    path = %context.folder_path_norm(),
                    "emitting folder scan completion"
                );
                if let Err(err) = self
                    .events
                    .publish_scan_event(ScanEvent::FolderScanCompleted(
                        summary.clone(),
                    ))
                    .await
                {
                    return classify_media_error(err);
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
                    return classify_media_error(err);
                }

                return DispatchStatus::Success;
            }

            let discovered =
                match self.actors.folder.discover_media(&plan, job).await {
                    Ok(files) => files,
                    Err(err) => return classify_media_error(err),
                };
            let children = match self
                .actors
                .folder
                .derive_child_contexts(&plan, job)
                .await
            {
                Ok(children) => children,
                Err(err) => return classify_media_error(err),
            };

            let discovered = match self
                .reconcile_folder_delta(&context, &plan, discovered, &children)
                .await
            {
                Ok(discovered) => discovered,
                Err(status) => return status,
            };

            let mut summary = match self.actors.folder.finalize(
                &context,
                &plan,
                &discovered,
                &children,
            ) {
                Ok(summary) => summary,
                Err(err) => return classify_media_error(err),
            };
            if had_cursor {
                summary.outcome = FolderScanOutcome::Changed;
            }

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

                let discovery = match self
                    .series_coordinator
                    .record_root_discovery(
                        series_ctx.library_id,
                        series_ctx.series_root_path.clone(),
                        None,
                    )
                    .await
                {
                    Ok(discovery) => discovery,
                    Err(err) => return classify_media_error(err),
                };

                if discovery.should_enqueue_resolution() {
                    let req = self.planner.series_resolve_for_folder(
                        series_ctx,
                        folder_name,
                        job.scan_reason,
                    );
                    match self.follow_ups.enqueue(req).await {
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

                let req = self.planner.media_analyze_for_discovery(media);
                match self.follow_ups.enqueue(req).await {
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

            debug!(
                outcome = summary.outcome.as_str(),
                path = %context.folder_path_norm(),
                "emitting folder scan completion"
            );
            if let Err(err) = self
                .events
                .publish_scan_event(ScanEvent::FolderScanCompleted(
                    summary.clone(),
                ))
                .await
            {
                return classify_media_error(err);
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
                return classify_media_error(err);
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
}

#[derive(Clone)]
struct TimedTextRuntime {
    extractor: TimedTextExtractor,
    libraries: Arc<dyn LibraryRepository>,
    transcripts: Arc<dyn TranscriptRepository>,
}

impl fmt::Debug for TimedTextRuntime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TimedTextRuntime")
            .field("extractor", &self.extractor)
            .field("libraries", &"LibraryRepository")
            .field("transcripts", &"TranscriptRepository")
            .finish()
    }
}

fn transcript_media_file(media: &Media) -> Option<&MediaFile> {
    match media {
        Media::Movie(movie) => Some(&movie.file),
        Media::Episode(episode) => Some(&episode.file),
        Media::Series(_) | Media::Season(_) => None,
    }
}

struct MediaPipelineFlow<Q, E>
where
    Q: QueueService + Send + Sync + 'static,
    E: ScanEventBus + Send + Sync + 'static,
{
    events: Arc<E>,
    actors: DispatcherActors,
    series_coordinator: Arc<SeriesCoordinator>,
    intelligence: Option<Arc<dyn IntelligenceRepository>>,
    timed_text: Option<TimedTextRuntime>,
    planner: FollowUpPlanner,
    follow_ups: FollowUpEnqueuer<Q, E>,
}

impl<Q, E> fmt::Debug for MediaPipelineFlow<Q, E>
where
    Q: QueueService + Send + Sync + 'static,
    E: ScanEventBus + Send + Sync + 'static,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MediaPipelineFlow")
            .field("events", &type_name::<E>())
            .field("actors", &self.actors)
            .field("series_coordinator", &self.series_coordinator)
            .field("intelligence", &self.intelligence.is_some())
            .field("timed_text", &self.timed_text.is_some())
            .field("planner", &self.planner)
            .field("follow_ups", &self.follow_ups)
            .finish()
    }
}

impl<Q, E> MediaPipelineFlow<Q, E>
where
    Q: QueueService + Send + Sync + 'static,
    E: ScanEventBus + Send + Sync + 'static,
{
    fn new(
        events: Arc<E>,
        actors: DispatcherActors,
        series_coordinator: Arc<SeriesCoordinator>,
        planner: FollowUpPlanner,
        follow_ups: FollowUpEnqueuer<Q, E>,
    ) -> Self {
        Self {
            events,
            actors,
            series_coordinator,
            intelligence: None,
            timed_text: None,
            planner,
            follow_ups,
        }
    }

    fn with_intelligence_repository(
        mut self,
        intelligence: Arc<dyn IntelligenceRepository>,
    ) -> Self {
        self.intelligence = Some(intelligence);
        self
    }

    fn with_timed_text_extraction(
        mut self,
        libraries: Arc<dyn LibraryRepository>,
        transcripts: Arc<dyn TranscriptRepository>,
        config: TimedTextExtractionConfig,
    ) -> Self {
        self.timed_text = Some(TimedTextRuntime {
            extractor: TimedTextExtractor::new(config),
            libraries,
            transcripts,
        });
        self
    }

    async fn refresh_intelligence_read_model(
        &self,
        library_id: crate::types::ids::LibraryId,
        media_id: MediaID,
    ) -> Result<()> {
        if let Some(intelligence) = &self.intelligence {
            intelligence
                .refresh_media_read_model(library_id, media_id, None)
                .await?;
        }
        Ok(())
    }

    async fn dispatch(&self, payload: &JobPayload) -> DispatchStatus {
        match payload {
            JobPayload::FolderScan(_) => DispatchStatus::DeadLetter {
                error: "folder scan payload routed to media pipeline".into(),
            },
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

    async fn handle_media_analyze(
        &self,
        job: &MediaAnalyzeJob,
    ) -> DispatchStatus {
        // TODO: Refactor clone
        let analyzed = match self.actors.analyze.analyze(job.clone()).await {
            Ok(result) => result,
            Err(err) => return classify_media_error(err),
        };

        if let Err(err) = self
            .events
            .publish_scan_event(ScanEvent::MediaAnalyzed(Box::new(
                analyzed.clone(),
            )))
            .await
        {
            return classify_media_error(err);
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
                let decision = match self
                    .series_coordinator
                    .prepare_episode_dependency(
                        job.library_id,
                        episode_hierarchy,
                    )
                    .await
                {
                    Ok(decision) => decision,
                    Err(err) => return classify_media_error(err),
                };

                match decision {
                    EpisodeDependencyDecision::Ready(hierarchy) => {
                        let req = self
                            .planner
                            .metadata_after_resolved_episode_analysis(
                                job, &analyzed, hierarchy,
                            );
                        return self.follow_ups.enqueue(req).await;
                    }
                    EpisodeDependencyDecision::Deferred { dependency_key } => {
                        let req = self.planner.episode_match_after_analysis(
                            job,
                            &analyzed,
                            episode_hierarchy,
                            dependency_key,
                        );
                        return self.follow_ups.enqueue(req).await;
                    }
                }
            }
        }

        let req = self.planner.metadata_after_analysis(job, &analyzed);
        self.follow_ups.enqueue(req).await
    }

    async fn handle_series_resolve(
        &self,
        job: &SeriesResolveJob,
    ) -> DispatchStatus {
        let resolution = match self.series_coordinator.resolve_series(job).await
        {
            Ok(result) => result,
            Err(err) => {
                let status = classify_media_error(err);
                if let DispatchStatus::DeadLetter { error } = &status {
                    let _ = self
                        .series_coordinator
                        .record_resolution_failure(job, error.clone())
                        .await;
                    if let Err(err) = self
                        .series_coordinator
                        .release_blocked_episode_dependencies(
                            &self.follow_ups,
                            job.library_id,
                            &job.series_root_path,
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
            return classify_media_error(err);
        }

        if let Err(err) = self
            .series_coordinator
            .release_blocked_episode_dependencies(
                &self.follow_ups,
                job.library_id,
                &job.series_root_path,
            )
            .await
        {
            return classify_media_error(err);
        }

        let req = self.planner.index_for_ready(job.library_id, &ready);
        self.follow_ups.enqueue(req).await
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
            Err(err) => return classify_media_error(err),
        };

        if let Err(err) = self
            .events
            .publish_scan_event(ScanEvent::MediaReadyForIndex(Box::new(
                ready.clone(),
            )))
            .await
        {
            return classify_media_error(err);
        }

        if !ready.image_jobs.is_empty() {
            let image_requests = self.planner.image_fetches_for_ready(&ready);

            match self.follow_ups.enqueue_many(image_requests).await {
                DispatchStatus::Success => {}
                status => return status,
            }
        }

        let req = self.planner.index_for_ready(job.library_id, &ready);
        self.follow_ups.enqueue(req).await
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
            Err(err) => return classify_media_error(err),
        };

        if let Err(err) =
            self.extract_and_upsert_timed_text(job, &outcome).await
        {
            return classify_media_error(err);
        }

        if let Err(err) = self
            .refresh_intelligence_read_model(job.library_id, job.media_id)
            .await
        {
            return classify_media_error(err);
        }

        if let Err(err) = self
            .events
            .publish_scan_event(ScanEvent::Indexed(Box::new(outcome)))
            .await
        {
            return classify_media_error(err);
        }

        DispatchStatus::Success
    }

    async fn extract_and_upsert_timed_text(
        &self,
        job: &IndexUpsertJob,
        outcome: &IndexingOutcome,
    ) -> Result<()> {
        let Some(runtime) = &self.timed_text else {
            return Ok(());
        };
        if !matches!(job.media_id, MediaID::Movie(_) | MediaID::Episode(_)) {
            return Ok(());
        }

        let Some(media_file) =
            outcome.media.as_ref().and_then(transcript_media_file)
        else {
            debug!(
                target: "scan::timed_text",
                media_id = %job.media_id,
                "indexed media has no playable media file; skipping timed-text extraction"
            );
            return Ok(());
        };

        let Some(library) =
            runtime.libraries.get_library(job.library_id).await?
        else {
            warn!(
                target: "scan::timed_text",
                library_id = %job.library_id,
                media_id = %job.media_id,
                "library roots unavailable; skipping timed-text extraction"
            );
            return Ok(());
        };
        if library.paths.is_empty() {
            warn!(
                target: "scan::timed_text",
                library_id = %job.library_id,
                media_id = %job.media_id,
                "library has no configured roots; skipping timed-text extraction"
            );
            return Ok(());
        }

        let extraction: TimedTextExtractionOutcome = runtime
            .extractor
            .extract(TimedTextExtractionRequest {
                library_id: job.library_id,
                media_id: job.media_id,
                media_file_id: media_file.id,
                media_path: media_file.path.clone(),
                library_roots: library.paths,
            })
            .await?;
        let source_count = extraction.sources.len();
        let segment_count = extraction.segment_count();
        let skipped_count = extraction.skipped.len();
        let failure_count = extraction.failures.len();

        for batch in extraction.sources {
            runtime
                .transcripts
                .upsert_source_with_segments(batch.source, batch.segments)
                .await?;
        }

        debug!(
            target: "scan::timed_text",
            media_id = %job.media_id,
            source_count,
            segment_count,
            skipped_count,
            failure_count,
            "timed-text extraction completed for indexed media"
        );

        Ok(())
    }

    async fn handle_image_fetch(&self, job: &ImageFetchJob) -> DispatchStatus {
        match self.actors.image.fetch(job).await {
            Ok(_) => DispatchStatus::Success,
            Err(err) => classify_media_error(err),
        }
    }

    async fn handle_episode_match(
        &self,
        job: &EpisodeMatchJob,
    ) -> DispatchStatus {
        let hierarchy = match self
            .series_coordinator
            .resolve_episode_dependency(job.library_id, &job.hierarchy)
            .await
        {
            Ok(hierarchy) => hierarchy,
            Err(err) => return classify_media_error(err),
        };

        let req = self.planner.metadata_after_episode_match(job, hierarchy);
        self.follow_ups.enqueue(req).await
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
                self.folder_flow.dispatch(lease, job).await
            }
            payload => self.media_pipeline_flow.dispatch(payload).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::repository_ports::transcripts::{
        TranscriptProcessingStatusSummary, TranscriptSegmentUpsert,
        TranscriptSourceStatusFilter, TranscriptSourceStatusSummary,
        TranscriptSourceUpsert, TranscriptSourceUpsertResult,
        TranscriptStatusFilter,
    };
    use crate::domain::scan::actors::folder::FolderListingPlan;
    use crate::domain::scan::actors::index::{IndexingChange, IndexingOutcome};
    use crate::domain::scan::actors::messages::{
        FolderScanSummary, MediaFileDiscovered, MediaKindHint,
    };
    use crate::domain::scan::context::{
        MovieFolderScanContext, MovieRootPath, MovieScanHierarchy,
        ScanNodeKind, SeriesHint,
    };
    use crate::domain::scan::orchestration::context::{
        EpisodeHint, EpisodeLink, EpisodeScanHierarchy, SeasonFolderPath,
        SeasonFolderScanContext, SeasonLink, SeriesFolderScanContext,
        SeriesLink, SeriesRef, SeriesRootPath, SeriesScanHierarchy,
    };
    use crate::domain::scan::orchestration::events::{
        JobEvent, JobEventPayload,
    };
    use crate::domain::scan::orchestration::persistence::{
        PostgresCursorRepository, PostgresQueueService,
    };
    use crate::domain::scan::orchestration::runtime::InProcJobEventBus;
    use crate::domain::scan::orchestration::series::SeriesResolution;
    use crate::domain::scan::orchestration::series_state::{
        InMemorySeriesScanStateRepository, SeriesScanState, SeriesScanStatus,
    };
    use crate::domain::scan::orchestration::{
        job::*,
        lease::{DequeueRequest, JobLease, LeaseId, LeaseRenewal},
    };
    use crate::error::Result;
    use crate::types::ids::{LibraryId, MovieID, SeriesID};
    use crate::types::library::LibraryType;
    use ferrex_model::details::ExternalIds;
    use ferrex_model::image::MediaImages;
    use ferrex_model::titles::MovieTitle;
    use ferrex_model::urls::{MovieURL, UrlLike};
    use ferrex_model::{
        EnhancedMovieDetails, Library, Media, MediaFile, MediaID,
        MovieReference, MovieReferenceBatchSize, VideoMediaType,
    };
    use sqlx::PgPool;
    use std::collections::HashMap;
    use tokio::sync::Mutex;
    use tokio::time::Duration;
    use uuid::Uuid;

    const FIXTURE_LIB_A: LibraryId =
        LibraryId(Uuid::from_u128(0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa));
    static DB_TEST_LOCK: Mutex<()> = Mutex::const_new(());
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
                hierarchy: command.hierarchy,
                node: command.node,
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

    struct MediaIndexActor {
        media: Media,
    }

    #[async_trait]
    impl IndexerActor for MediaIndexActor {
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
                media: Some(self.media.clone()),
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

    #[derive(Clone)]
    struct StaticLibraryRepository {
        library: Library,
    }

    #[async_trait]
    impl LibraryRepository for StaticLibraryRepository {
        async fn create_library(&self, library: Library) -> Result<LibraryId> {
            Ok(library.id)
        }

        async fn get_library(&self, id: LibraryId) -> Result<Option<Library>> {
            Ok((self.library.id == id).then(|| self.library.clone()))
        }

        async fn list_libraries(&self) -> Result<Vec<Library>> {
            Ok(vec![self.library.clone()])
        }

        async fn update_library(
            &self,
            _id: LibraryId,
            _library: Library,
        ) -> Result<()> {
            Ok(())
        }

        async fn delete_library(&self, _id: LibraryId) -> Result<()> {
            Ok(())
        }

        async fn update_library_last_scan(&self, _id: LibraryId) -> Result<()> {
            Ok(())
        }

        async fn list_library_references(
            &self,
        ) -> Result<Vec<crate::types::details::LibraryReference>> {
            unimplemented!("not needed by dispatcher timed-text tests")
        }

        async fn get_library_reference(
            &self,
            _id: Uuid,
        ) -> Result<crate::types::details::LibraryReference> {
            unimplemented!("not needed by dispatcher timed-text tests")
        }
    }

    #[derive(Default)]
    struct RecordingTranscriptRepository {
        upserts:
            Mutex<Vec<(TranscriptSourceUpsert, Vec<TranscriptSegmentUpsert>)>>,
    }

    impl RecordingTranscriptRepository {
        async fn recorded(
            &self,
        ) -> Vec<(TranscriptSourceUpsert, Vec<TranscriptSegmentUpsert>)>
        {
            self.upserts.lock().await.clone()
        }
    }

    #[async_trait]
    impl TranscriptRepository for RecordingTranscriptRepository {
        async fn upsert_source_with_segments(
            &self,
            source: TranscriptSourceUpsert,
            segments: Vec<TranscriptSegmentUpsert>,
        ) -> Result<TranscriptSourceUpsertResult> {
            self.upserts
                .lock()
                .await
                .push((source.clone(), segments.clone()));
            Ok(TranscriptSourceUpsertResult {
                source_id: source.source_id.unwrap_or_else(Uuid::now_v7),
                segment_count: segments.len() as u64,
                source_content_hash: source.source_content_hash,
            })
        }

        async fn list_source_status(
            &self,
            _filter: TranscriptSourceStatusFilter,
        ) -> Result<Vec<TranscriptSourceStatusSummary>> {
            Ok(Vec::new())
        }

        async fn list_processing_status(
            &self,
            _filter: TranscriptStatusFilter,
        ) -> Result<Vec<TranscriptProcessingStatusSummary>> {
            Ok(Vec::new())
        }

        async fn invalidate_media(
            &self,
            _library_id: LibraryId,
            _media_id: MediaID,
            _reason: &str,
        ) -> Result<u64> {
            Ok(0)
        }

        async fn purge_media(
            &self,
            _library_id: LibraryId,
            _media_id: MediaID,
            _reason: &str,
        ) -> Result<u64> {
            Ok(0)
        }

        async fn invalidate_source(
            &self,
            _source_id: Uuid,
            _reason: &str,
        ) -> Result<()> {
            Ok(())
        }

        async fn purge_source(
            &self,
            _source_id: Uuid,
            _reason: &str,
        ) -> Result<()> {
            Ok(())
        }

        async fn search_snippets(
            &self,
            _request: &crate::api::types::intelligence::TimedTextSnippetSearchRequest,
        ) -> Result<
            crate::api::types::intelligence::TimedTextSnippetSearchResponse,
        > {
            unimplemented!("not needed by dispatcher timed-text tests")
        }
    }

    #[derive(Default)]
    struct RecordingQueue {
        enqueued: Mutex<Vec<EnqueueRequest>>,
        released_dependencies: Mutex<Vec<(LibraryId, DependencyKey)>>,
    }

    impl RecordingQueue {
        async fn enqueued(&self) -> Vec<EnqueueRequest> {
            self.enqueued.lock().await.clone()
        }
    }

    #[async_trait]
    impl QueueService for RecordingQueue {
        async fn enqueue(&self, request: EnqueueRequest) -> Result<JobHandle> {
            request.validate()?;
            let handle = JobHandle::accepted(
                JobId::new(),
                &request.payload,
                request.priority,
            );
            self.enqueued.lock().await.push(request);
            Ok(handle)
        }

        async fn enqueue_many(
            &self,
            requests: Vec<EnqueueRequest>,
        ) -> Result<Vec<JobHandle>> {
            let mut handles = Vec::with_capacity(requests.len());
            for request in requests {
                request.validate()?;
                handles.push(JobHandle::accepted(
                    JobId::new(),
                    &request.payload,
                    request.priority,
                ));
                self.enqueued.lock().await.push(request);
            }
            Ok(handles)
        }

        async fn dequeue(
            &self,
            _request: DequeueRequest,
        ) -> Result<Option<JobLease>> {
            Ok(None)
        }

        async fn renew(&self, _renewal: LeaseRenewal) -> Result<JobLease> {
            Err(MediaError::NotFound(
                "recording queue does not lease jobs".into(),
            ))
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

        async fn queue_depth(&self, kind: JobKind) -> Result<usize> {
            Ok(self
                .enqueued
                .lock()
                .await
                .iter()
                .filter(|request| request.payload.kind() == kind)
                .count())
        }

        async fn release_dependency(
            &self,
            library_id: LibraryId,
            dependency_key: &DependencyKey,
        ) -> Result<u64> {
            self.released_dependencies
                .lock()
                .await
                .push((library_id, dependency_key.clone()));
            Ok(0)
        }
    }

    #[derive(Default)]
    struct MemoryCursorRepository {
        cursors: Mutex<HashMap<ScanCursorId, ScanCursor>>,
    }

    #[async_trait]
    impl ScanCursorRepository for MemoryCursorRepository {
        async fn get(&self, id: &ScanCursorId) -> Result<Option<ScanCursor>> {
            Ok(self.cursors.lock().await.get(id).cloned())
        }

        async fn list_by_library(
            &self,
            library_id: LibraryId,
        ) -> Result<Vec<ScanCursor>> {
            Ok(self
                .cursors
                .lock()
                .await
                .values()
                .filter(|cursor| cursor.id.library_id == library_id)
                .cloned()
                .collect())
        }

        async fn upsert(&self, cursor: ScanCursor) -> Result<()> {
            self.cursors.lock().await.insert(cursor.id.clone(), cursor);
            Ok(())
        }

        async fn delete_by_library(
            &self,
            library_id: LibraryId,
        ) -> Result<usize> {
            let mut cursors = self.cursors.lock().await;
            let before = cursors.len();
            cursors.retain(|_, cursor| cursor.id.library_id != library_id);
            Ok(before - cursors.len())
        }

        async fn delete_by_path_prefixes(
            &self,
            library_id: LibraryId,
            prefixes: Vec<String>,
        ) -> Result<usize> {
            let mut cursors = self.cursors.lock().await;
            let before = cursors.len();
            cursors.retain(|_, cursor| {
                if cursor.id.library_id != library_id {
                    return true;
                }
                !prefixes.iter().any(|prefix| {
                    cursor.folder_path_norm == *prefix
                        || cursor
                            .folder_path_norm
                            .strip_prefix(prefix)
                            .is_some_and(|rest| rest.starts_with('/'))
                })
            });
            Ok(before - cursors.len())
        }

        async fn list_stale(
            &self,
            library_id: LibraryId,
            older_than: chrono::DateTime<Utc>,
        ) -> Result<Vec<ScanCursor>> {
            Ok(self
                .cursors
                .lock()
                .await
                .values()
                .filter(|cursor| {
                    cursor.id.library_id == library_id
                        && cursor.last_scan_at < older_than
                })
                .cloned()
                .collect())
        }
    }

    fn minimal_movie_details(title: &str) -> EnhancedMovieDetails {
        EnhancedMovieDetails {
            id: 1,
            title: title.to_string(),
            original_title: None,
            overview: None,
            release_date: None,
            runtime: None,
            vote_average: None,
            vote_count: None,
            popularity: None,
            content_rating: None,
            content_ratings: Vec::new(),
            release_dates: Vec::new(),
            genres: Vec::new(),
            spoken_languages: Vec::new(),
            production_companies: Vec::new(),
            production_countries: Vec::new(),
            homepage: None,
            status: None,
            tagline: None,
            budget: None,
            revenue: None,
            poster_path: None,
            backdrop_path: None,
            logo_path: None,
            primary_poster_iid: None,
            primary_backdrop_iid: None,
            images: MediaImages::default(),
            cast: Vec::new(),
            crew: Vec::new(),
            videos: Vec::new(),
            keywords: Vec::new(),
            external_ids: ExternalIds::default(),
            alternative_titles: Vec::new(),
            translations: Vec::new(),
            collection: None,
            recommendations: Vec::new(),
            similar: Vec::new(),
        }
    }

    fn movie_library(library_id: LibraryId, paths: Vec<PathBuf>) -> Library {
        Library {
            id: library_id,
            name: "Timed Text Library".to_string(),
            library_type: LibraryType::Movies,
            paths,
            scan_interval_minutes: 60,
            last_scan: None,
            enabled: true,
            auto_scan: true,
            watch_for_changes: true,
            analyze_on_scan: true,
            max_retry_attempts: 3,
            movie_ref_batch_size: MovieReferenceBatchSize::default(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            media: None,
        }
    }

    fn movie_media(
        library_id: LibraryId,
        movie_id: MovieID,
        media_path: PathBuf,
    ) -> Media {
        let media_id = MediaID::Movie(movie_id);
        let file = MediaFile::new(media_id, media_path, library_id).unwrap();
        Media::Movie(Box::new(MovieReference {
            id: movie_id,
            library_id,
            batch_id: None,
            tmdb_id: 1,
            title: MovieTitle::new("Timed Text Movie".to_string()).unwrap(),
            details: minimal_movie_details("Timed Text Movie"),
            endpoint: MovieURL::from_string(format!("/stream/{}", file.id)),
            file,
            theme_color: None,
        }))
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

        sqlx::query!(
            r#"
            DELETE FROM orchestrator_jobs
            WHERE library_id = $1
            "#,
            library_id.as_uuid()
        )
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
                total_entries: 2,
                folder_missing: false,
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
                outcome: FolderScanOutcome::Changed,
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

    fn series_hint(title: &str) -> SeriesHint {
        SeriesHint {
            title: title.to_string(),
            slug: Some(
                title
                    .to_ascii_lowercase()
                    .replace(|ch: char| !ch.is_ascii_alphanumeric(), "-"),
            ),
            year: None,
            region: None,
        }
    }

    fn unresolved_episode_hierarchy(
        series_root_path: SeriesRootPath,
        title: &str,
        episode_number: u16,
    ) -> EpisodeScanHierarchy {
        EpisodeScanHierarchy {
            series_root_path,
            series: SeriesLink::Hint(series_hint(title)),
            season: SeasonLink::Number(1),
            episode: EpisodeLink::Hint(EpisodeHint {
                number: episode_number,
                title: Some(format!("Episode {episode_number}")),
            }),
        }
    }

    fn episode_match_job(
        library_id: LibraryId,
        series_root_path: SeriesRootPath,
        title: &str,
        path_norm: &str,
        episode_number: u16,
    ) -> EpisodeMatchJob {
        EpisodeMatchJob {
            library_id,
            media_id: MediaID::new(VideoMediaType::Episode),
            path_norm: path_norm.to_string(),
            fingerprint: MediaFingerprint {
                device_id: None,
                inode: Some(u64::from(episode_number)),
                size: 100 + u64::from(episode_number),
                mtime: 1_700_000_000 + i64::from(episode_number),
                weak_hash: Some(format!("episode-{episode_number}")),
            },
            hierarchy: unresolved_episode_hierarchy(
                series_root_path,
                title,
                episode_number,
            ),
            node: ScanNodeKind::EpisodeFile,
            scan_reason: ScanReason::BulkSeed,
        }
    }

    fn media_analyze_episode_job(
        library_id: LibraryId,
        series_root_path: SeriesRootPath,
        title: &str,
        path_norm: &str,
        episode_number: u16,
    ) -> MediaAnalyzeJob {
        let episode = episode_match_job(
            library_id,
            series_root_path,
            title,
            path_norm,
            episode_number,
        );
        MediaAnalyzeJob {
            library_id: episode.library_id,
            path_norm: episode.path_norm,
            fingerprint: episode.fingerprint,
            discovered_at: Utc::now(),
            media_id: episode.media_id,
            variant: VideoMediaType::Episode,
            hierarchy: AnalyzeScanHierarchy::Episode(episode.hierarchy),
            node: ScanNodeKind::EpisodeFile,
            scan_reason: ScanReason::BulkSeed,
        }
    }

    async fn postgres_pool_or_skip(test_name: &str) -> Option<PgPool> {
        let database_url = match std::env::var("DATABASE_URL") {
            Ok(url) => url,
            Err(_) => {
                eprintln!("skipping {test_name}: DATABASE_URL not set");
                return None;
            }
        };

        let pool = match PgPool::connect(&database_url).await {
            Ok(pool) => pool,
            Err(err) => {
                eprintln!(
                    "skipping {test_name}: failed to connect to DATABASE_URL ({err})"
                );
                return None;
            }
        };

        if let Err(err) = crate::MIGRATOR.run(&pool).await {
            eprintln!("skipping {test_name}: migrations failed ({err})");
            return None;
        }

        Some(pool)
    }

    async fn clear_queue_rows(pool: &PgPool, library_id: LibraryId) {
        sqlx::query("DELETE FROM orchestrator_jobs WHERE library_id = $1")
            .bind(library_id.as_uuid())
            .execute(pool)
            .await
            .expect("clear fixture jobs");
    }

    async fn job_state(pool: &PgPool, job_id: JobId) -> String {
        sqlx::query_scalar::<_, String>(
            "SELECT state FROM orchestrator_jobs WHERE id = $1",
        )
        .bind(job_id.0)
        .fetch_one(pool)
        .await
        .expect("job state")
    }

    async fn job_correlation_id(pool: &PgPool, job_id: JobId) -> Option<Uuid> {
        sqlx::query_scalar::<_, Option<Uuid>>(
            "SELECT correlation_id FROM orchestrator_jobs WHERE id = $1",
        )
        .bind(job_id.0)
        .fetch_one(pool)
        .await
        .expect("job correlation id")
    }

    async fn active_episode_matches_for_series(
        pool: &PgPool,
        library_id: LibraryId,
        series_root_path: &SeriesRootPath,
    ) -> i64 {
        sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)::bigint
            FROM orchestrator_jobs
            WHERE library_id = $1
              AND kind = $2
              AND state IN ('ready','deferred','leased')
              AND payload #>> '{payload,hierarchy,series_root_path}' = $3
            "#,
        )
        .bind(library_id.as_uuid())
        .bind(JobKind::EpisodeMatch as i16)
        .bind(series_root_path.as_str())
        .fetch_one(pool)
        .await
        .expect("active episode matches for series")
    }

    async fn episode_matches_for_series_in_state(
        pool: &PgPool,
        library_id: LibraryId,
        series_root_path: &SeriesRootPath,
        state: &str,
    ) -> i64 {
        sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)::bigint
            FROM orchestrator_jobs
            WHERE library_id = $1
              AND kind = $2
              AND state = $3
              AND payload #>> '{payload,hierarchy,series_root_path}' = $4
            "#,
        )
        .bind(library_id.as_uuid())
        .bind(JobKind::EpisodeMatch as i16)
        .bind(state)
        .bind(series_root_path.as_str())
        .fetch_one(pool)
        .await
        .expect("episode matches for series in state")
    }

    fn dequeue_request(kind: JobKind, worker_id: &str) -> DequeueRequest {
        DequeueRequest {
            kind,
            worker_id: worker_id.into(),
            lease_ttl: chrono::Duration::seconds(30),
            selector: None,
        }
    }

    fn noop_folder_actor(library_id: LibraryId) -> Arc<dyn FolderScanActor> {
        let movie_root_path = MovieRootPath::try_new_under_library_root(
            "/library",
            "/library/noop",
        )
        .unwrap();
        let context = FolderScanContext::Movie(MovieFolderScanContext {
            library_id,
            movie_root_path,
        });
        Arc::new(StubFolderActor {
            plan: FolderListingPlan {
                directories: vec![],
                media_files: vec![],
                ancillary_files: vec![],
                generated_listing_hash: "noop".into(),
                total_entries: 0,
                folder_missing: false,
            },
            discovered: vec![],
            children: vec![],
            summary: FolderScanSummary {
                context,
                discovered_files: 0,
                enqueued_subfolders: 0,
                listing_hash: "noop".into(),
                outcome: FolderScanOutcome::Empty,
                completed_at: Utc::now(),
            },
        }) as Arc<dyn FolderScanActor>
    }

    #[tokio::test]
    async fn index_upsert_persists_timed_text_sidecar_upserts() {
        let temp = tempfile::TempDir::new().unwrap();
        let library_root = temp.path().join("library");
        let movie_root = library_root.join("Arrival");
        std::fs::create_dir_all(&movie_root).unwrap();
        let media_path = movie_root.join("Arrival.mkv");
        std::fs::write(&media_path, b"movie bytes").unwrap();
        std::fs::write(
            movie_root.join("Arrival.en.srt"),
            "1\n00:00:01,000 --> 00:00:02,000\nRuntime transcript\n",
        )
        .unwrap();

        let library_id =
            LibraryId(Uuid::from_u128(0x62900000000000000000000000000001));
        let movie_id =
            MovieID(Uuid::from_u128(0x62900000000000000000000000000002));
        let media_id = MediaID::Movie(movie_id);
        let media = movie_media(library_id, movie_id, media_path.clone());
        let media_file_id = match &media {
            Media::Movie(movie) => movie.file.id,
            _ => unreachable!("movie_media returns movie media"),
        };

        let queue = Arc::new(RecordingQueue::default());
        let events = Arc::new(InProcJobEventBus::new(16));
        let cursors = Arc::new(MemoryCursorRepository::default());
        let series_states: Arc<Box<dyn SeriesScanStateRepository>> =
            Arc::new(Box::new(InMemorySeriesScanStateRepository::default()));
        let series_resolver: Arc<dyn SeriesResolverPort> =
            Arc::new(StubSeriesResolver::new(Arc::clone(&series_states)));
        let actors = DispatcherActors::new(
            noop_folder_actor(library_id),
            Arc::new(StubAnalyzeActor),
            Arc::new(StubMetadataActor),
            Arc::new(MediaIndexActor { media }),
            Arc::new(StubImageActor),
        );
        let transcripts = Arc::new(RecordingTranscriptRepository::default());
        let transcript_repo: Arc<dyn TranscriptRepository> =
            transcripts.clone();
        let library_repo: Arc<dyn LibraryRepository> =
            Arc::new(StaticLibraryRepository {
                library: movie_library(library_id, vec![library_root.clone()]),
            });
        let mut timed_text_config = TimedTextExtractionConfig::default();
        timed_text_config.ffprobe_path = library_root.join("missing-ffprobe");
        timed_text_config.ffmpeg_path = library_root.join("missing-ffmpeg");

        let dispatcher = DefaultJobDispatcher::new(
            Arc::clone(&queue),
            Arc::clone(&events),
            cursors,
            series_states,
            series_resolver,
            actors,
            CorrelationCache::default(),
        )
        .with_timed_text_extraction(
            library_repo,
            transcript_repo,
            timed_text_config,
        );
        let movie_root_str = movie_root.to_string_lossy().to_string();
        let library_root_str = library_root.to_string_lossy().to_string();
        let job = IndexUpsertJob {
            library_id,
            media_id,
            variant: VideoMediaType::Movie,
            hierarchy: AnalyzeScanHierarchy::Movie(MovieScanHierarchy {
                movie_root_path: MovieRootPath::try_new_under_library_root(
                    &library_root_str,
                    movie_root_str,
                )
                .unwrap(),
                movie_id: Some(movie_id),
                extra_tag: None,
            }),
            node: ScanNodeKind::MovieFolder,
            path_norm: media_path.to_string_lossy().to_string(),
            idempotency_key: "timed-text-runtime-test".to_string(),
        };
        let lease = JobLease::new(
            JobRecord::new(JobPayload::IndexUpsert(job), JobPriority::P1),
            "timed-text-test".to_string(),
            chrono::Duration::seconds(30),
        );

        let status = dispatcher.dispatch(&lease).await;

        assert_eq!(status, DispatchStatus::Success);
        let upserts = transcripts.recorded().await;
        assert_eq!(upserts.len(), 1);
        let (source, segments) = &upserts[0];
        assert_eq!(source.library_id, library_id);
        assert_eq!(source.media_id, media_id);
        assert_eq!(source.media_file_id, media_file_id);
        assert_eq!(
            source.source_kind,
            crate::api::types::intelligence::TimedTextSourceKind::Sidecar
        );
        assert!(source.source_key.starts_with("sidecar:"));
        assert_eq!(source.language_code, "en");
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].text, "Runtime transcript");
        assert!(!source.source_locator.to_string().contains("Arrival"));
    }

    #[tokio::test]
    async fn unchanged_cursor_folder_scan_emits_completion_without_followups() {
        let library_id =
            LibraryId(Uuid::from_u128(0x57600000000000000000000000000000));
        let library_root = "/library";
        let movie_root_path = MovieRootPath::try_new_under_library_root(
            library_root,
            "/library/Stable Movie",
        )
        .unwrap();
        let context = FolderScanContext::Movie(MovieFolderScanContext {
            library_id,
            movie_root_path: movie_root_path.clone(),
        });
        let hierarchy = AnalyzeScanHierarchy::Movie(MovieScanHierarchy {
            movie_root_path: movie_root_path.clone(),
            movie_id: None,
            extra_tag: None,
        });
        let listing_hash = "stable-listing".to_string();
        let previous_scan_at = Utc::now() - chrono::Duration::hours(2);
        let previous_modified_at = Utc::now() - chrono::Duration::hours(3);

        let queue = Arc::new(RecordingQueue::default());
        let events = Arc::new(InProcJobEventBus::new(16));
        let mut scan_rx = events.subscribe_scan();
        let cursors = Arc::new(MemoryCursorRepository::default());
        let cursor_id = ScanCursorId::new(
            library_id,
            &vec![PathBuf::from(context.folder_path_norm())],
        );
        cursors
            .upsert(ScanCursor {
                id: cursor_id.clone(),
                folder_path_norm: context.folder_path_norm().to_string(),
                listing_hash: listing_hash.clone(),
                entry_count: 2,
                last_scan_at: previous_scan_at,
                last_modified_at: Some(previous_modified_at),
                device_id: Some("old-device".into()),
            })
            .await
            .expect("seed matching cursor");

        let folder_actor = Arc::new(StubFolderActor {
            plan: FolderListingPlan {
                directories: vec![PathBuf::from(
                    "/library/Stable Movie/Extras",
                )],
                media_files: vec![PathBuf::from(
                    "/library/Stable Movie/feature.mkv",
                )],
                ancillary_files: vec![],
                generated_listing_hash: listing_hash.clone(),
                total_entries: 2,
                folder_missing: false,
            },
            discovered: vec![MediaFileDiscovered {
                library_id,
                path_norm: "/library/Stable Movie/feature.mkv".into(),
                fingerprint: MediaFingerprint {
                    device_id: None,
                    inode: Some(42),
                    size: 100,
                    mtime: 1_700_000_000,
                    weak_hash: Some("stable".into()),
                },
                classified_as: MediaKindHint::Movie,
                media_id: MediaID::new(VideoMediaType::Movie),
                variant: VideoMediaType::Movie,
                node: ScanNodeKind::MovieFolder,
                hierarchy,
                context: context.clone(),
                scan_reason: ScanReason::MaintenanceSweep,
            }],
            children: vec![FolderScanContext::Movie(MovieFolderScanContext {
                library_id,
                movie_root_path: MovieRootPath::try_new_under_library_root(
                    library_root,
                    "/library/Should Not Enqueue",
                )
                .unwrap(),
            })],
            summary: FolderScanSummary {
                context: context.clone(),
                discovered_files: 1,
                enqueued_subfolders: 1,
                listing_hash: listing_hash.clone(),
                outcome: FolderScanOutcome::Changed,
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
        let series_states: Arc<Box<dyn SeriesScanStateRepository>> =
            Arc::new(Box::new(InMemorySeriesScanStateRepository::default()));
        let series_resolver =
            Arc::new(StubSeriesResolver::new(Arc::clone(&series_states)));
        let dispatcher = DefaultJobDispatcher::new(
            Arc::clone(&queue),
            Arc::clone(&events),
            Arc::clone(&cursors),
            Arc::clone(&series_states),
            series_resolver,
            actors,
            CorrelationCache::default(),
        );

        let status = dispatcher
            .dispatch(&lease_for_payload(JobPayload::FolderScan(
                FolderScanJob {
                    context: context.clone(),
                    scan_reason: ScanReason::MaintenanceSweep,
                    enqueue_time: Utc::now(),
                    device_id: Some("device-unchanged".into()),
                },
            )))
            .await;

        assert!(matches!(status, DispatchStatus::Success));
        assert!(
            queue.enqueued().await.is_empty(),
            "unchanged cursor completion must not enqueue follow-up work"
        );

        let refreshed = cursors
            .get(&cursor_id)
            .await
            .expect("cursor read")
            .expect("cursor remains present");
        assert_eq!(refreshed.listing_hash, listing_hash);
        assert_eq!(refreshed.entry_count, 2);
        assert!(refreshed.last_scan_at >= previous_scan_at);
        assert_eq!(refreshed.last_modified_at, Some(previous_modified_at));
        assert_eq!(refreshed.device_id.as_deref(), Some("device-unchanged"));

        let mut completion_count = 0;
        let mut media_discovered_count = 0;
        let mut folder_discovered_count = 0;
        while let Ok(event) = scan_rx.try_recv() {
            match event {
                ScanEvent::FolderScanCompleted(summary) => {
                    completion_count += 1;
                    assert_eq!(
                        summary.context.folder_path_norm(),
                        "/library/Stable Movie"
                    );
                    assert_eq!(
                        summary.outcome,
                        FolderScanOutcome::UnchangedCursor
                    );
                }
                ScanEvent::MediaFileDiscovered(_) => {
                    media_discovered_count += 1
                }
                ScanEvent::FolderDiscovered { .. } => {
                    folder_discovered_count += 1
                }
                _ => {}
            }
        }
        assert_eq!(completion_count, 1);
        assert_eq!(media_discovered_count, 0);
        assert_eq!(folder_discovered_count, 0);
    }

    #[tokio::test]
    async fn series_root_scan_enqueues_resolve_and_season_discovery() {
        let library_id =
            LibraryId(Uuid::from_u128(0x57600000000000000000000000000001));
        let library_root = "/library";
        let series_root_path = SeriesRootPath::try_new_under_library_root(
            library_root,
            "/library/Deterministic Show",
        )
        .unwrap();
        let (season_folder_path, season_number) =
            SeasonFolderPath::try_new_under_series_root(
                &series_root_path,
                "/library/Deterministic Show/Season 1",
            )
            .unwrap();

        let series_context =
            FolderScanContext::Series(SeriesFolderScanContext {
                library_id,
                series_root_path: series_root_path.clone(),
            });
        let season_context =
            FolderScanContext::Season(SeasonFolderScanContext {
                library_id,
                series_root_path: series_root_path.clone(),
                season_folder_path: season_folder_path.clone(),
                season_number,
            });

        let queue = Arc::new(RecordingQueue::default());
        let events = Arc::new(InProcJobEventBus::new(16));
        let mut job_rx = events.subscribe();
        let mut scan_rx = events.subscribe_scan();
        let cursors = Arc::new(MemoryCursorRepository::default());
        let correlations = CorrelationCache::default();
        let series_states: Arc<Box<dyn SeriesScanStateRepository>> =
            Arc::new(Box::new(InMemorySeriesScanStateRepository::default()));
        let series_resolver =
            Arc::new(StubSeriesResolver::new(Arc::clone(&series_states)));

        let listing_hash = "series-root-listing".to_string();
        let folder_actor = Arc::new(StubFolderActor {
            plan: FolderListingPlan {
                directories: vec![PathBuf::from(season_folder_path.as_str())],
                media_files: vec![],
                ancillary_files: vec![],
                generated_listing_hash: listing_hash.clone(),
                total_entries: 1,
                folder_missing: false,
            },
            discovered: vec![],
            children: vec![season_context.clone()],
            summary: FolderScanSummary {
                context: series_context.clone(),
                discovered_files: 0,
                enqueued_subfolders: 1,
                listing_hash,
                outcome: FolderScanOutcome::Changed,
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
        let dispatcher = DefaultJobDispatcher::new(
            Arc::clone(&queue),
            Arc::clone(&events),
            Arc::clone(&cursors),
            Arc::clone(&series_states),
            series_resolver,
            actors,
            correlations.clone(),
        );

        let lease = lease_for_payload(JobPayload::FolderScan(FolderScanJob {
            context: series_context.clone(),
            scan_reason: ScanReason::BulkSeed,
            enqueue_time: Utc::now(),
            device_id: None,
        }));

        let status = dispatcher.dispatch(&lease).await;
        assert!(matches!(status, DispatchStatus::Success));

        let state = series_states
            .get(library_id, &series_root_path)
            .await
            .expect("series state lookup")
            .expect("series state recorded");
        assert!(matches!(state.status, SeriesScanStatus::Discovered));

        let enqueued = queue.enqueued().await;
        assert_eq!(enqueued.len(), 1, "series root scan enqueues one job");
        let JobPayload::SeriesResolve(series_job) = &enqueued[0].payload else {
            panic!("expected SeriesResolve follow-up");
        };
        assert_eq!(series_job.library_id, library_id);
        assert_eq!(series_job.series_root_path, series_root_path);
        assert_eq!(series_job.folder_name, "Deterministic Show");
        assert!(matches!(series_job.scan_reason, ScanReason::BulkSeed));
        assert!(enqueued[0].correlation_id.is_none());

        let mut saw_series_resolve_enqueue = false;
        while let Ok(event) = job_rx.try_recv() {
            if let JobEventPayload::Enqueued {
                job_id,
                kind: JobKind::SeriesResolve,
                ..
            } = event.payload
            {
                assert_ne!(event.meta.correlation_id, Uuid::nil());
                assert_eq!(
                    correlations.fetch(&job_id).await,
                    Some(event.meta.correlation_id)
                );
                saw_series_resolve_enqueue = true;
            }
        }
        assert!(
            saw_series_resolve_enqueue,
            "SeriesResolve enqueue event should be published"
        );

        let mut saw_season_discovered = false;
        let mut saw_completion = false;
        while let Ok(event) = scan_rx.try_recv() {
            match event {
                ScanEvent::FolderDiscovered { context, reason } => {
                    assert!(matches!(reason, ScanReason::BulkSeed));
                    let FolderScanContext::Season(ctx) = *context else {
                        panic!("expected season FolderDiscovered context");
                    };
                    assert_eq!(ctx.library_id, library_id);
                    assert_eq!(ctx.series_root_path, series_root_path);
                    assert_eq!(ctx.season_folder_path, season_folder_path);
                    assert_eq!(ctx.season_number, 1);
                    saw_season_discovered = true;
                }
                ScanEvent::FolderScanCompleted(summary) => {
                    assert_eq!(
                        summary.context.folder_path_norm(),
                        series_root_path.as_str()
                    );
                    saw_completion = true;
                }
                _ => {}
            }
        }
        assert!(
            saw_completion,
            "series root completion event is deterministic"
        );
        assert!(
            saw_season_discovered,
            "season folder discovery should be emitted deterministically"
        );
    }

    #[tokio::test]
    async fn correlation_ids_are_stable_across_merge_dequeue_and_completion() {
        let correlations = CorrelationCache::default();
        let library_id =
            LibraryId(Uuid::from_u128(0x57600000000000000000000000000002));
        let movie_root_path = MovieRootPath::try_new_under_library_root(
            "/library",
            "/library/Correlation",
        )
        .unwrap();
        let payload = JobPayload::FolderScan(FolderScanJob {
            context: FolderScanContext::Movie(MovieFolderScanContext {
                library_id,
                movie_root_path,
            }),
            scan_reason: ScanReason::UserRequested,
            enqueue_time: Utc::now(),
            device_id: None,
        });
        let priority = JobPriority::P1;
        let handle = JobHandle::accepted(JobId::new(), &payload, priority);
        let provided = Uuid::from_u128(0x5760000000000000000000000000c011);

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

        let merged = JobHandle::merged(handle.job_id, &payload, priority);
        let merge_event = JobEvent::from_handle(
            &merged,
            correlations.fetch(&handle.job_id).await,
            JobEventPayload::Merged {
                existing_job_id: handle.job_id,
                merged_job_id: merged.job_id,
                kind: payload.kind(),
                priority,
            },
            None,
        );
        correlations
            .remember_if_absent(merged.job_id, merge_event.meta.correlation_id)
            .await;
        assert_eq!(merge_event.meta.correlation_id, provided);

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

        let generated_handle =
            JobHandle::accepted(JobId::new(), &payload, JobPriority::P2);
        let generated_enqueue = JobEvent::from_handle(
            &generated_handle,
            None,
            JobEventPayload::Enqueued {
                job_id: generated_handle.job_id,
                kind: payload.kind(),
                priority: JobPriority::P2,
            },
            None,
        );
        assert_ne!(generated_enqueue.meta.correlation_id, Uuid::nil());
        correlations
            .remember(
                generated_handle.job_id,
                generated_enqueue.meta.correlation_id,
            )
            .await;
        let generated_completion =
            correlations.take_or_generate(generated_handle.job_id).await;
        assert_eq!(generated_completion, generated_enqueue.meta.correlation_id);
        assert!(correlations.fetch(&generated_handle.job_id).await.is_none());
    }

    #[tokio::test]
    async fn episode_match_dependency_key_defers_until_release() {
        let Some(pool) = postgres_pool_or_skip(
            "episode_match_dependency_key_defers_until_release",
        )
        .await
        else {
            return;
        };
        let _db_guard = DB_TEST_LOCK.lock().await;
        let queue = PostgresQueueService::new(pool.clone())
            .await
            .expect("queue init");
        let library_id =
            LibraryId(Uuid::from_u128(0x57600000000000000000000000000003));
        upsert_library(
            &pool,
            library_id,
            "Episode Match Gating",
            LibraryType::Series,
            vec!["/library".into()],
        )
        .await
        .expect("seed series library");
        clear_queue_rows(&pool, library_id).await;

        let series_root_path = SeriesRootPath::try_new_under_library_root(
            "/library",
            "/library/Gated Show",
        )
        .unwrap();
        let dependency_key = DependencyKey::series_root(&series_root_path);
        let mut request = EnqueueRequest::new(
            JobPriority::P0,
            JobPayload::EpisodeMatch(episode_match_job(
                library_id,
                series_root_path.clone(),
                "Gated Show",
                "/library/Gated Show/Season 1/S01E01.mkv",
                1,
            )),
        )
        .with_dependency(dependency_key.clone());
        let correlation_id =
            Uuid::from_u128(0x5760000000000000000000000000c012);
        request.correlation_id = Some(correlation_id);

        let handle = queue.enqueue(request).await.expect("enqueue deferred");
        assert!(handle.accepted);
        assert_eq!(job_state(&pool, handle.job_id).await, "deferred");
        assert_eq!(
            job_correlation_id(&pool, handle.job_id).await,
            Some(correlation_id)
        );

        let before_release = queue
            .dequeue(dequeue_request(JobKind::EpisodeMatch, "before-release"))
            .await
            .expect("dequeue before release");
        assert!(
            before_release.is_none(),
            "deferred EpisodeMatch must not dequeue before dependency release"
        );

        let released = queue
            .release_dependency(library_id, &dependency_key)
            .await
            .expect("release dependency");
        assert_eq!(released, 1, "exactly one matching dependency is released");

        let lease = queue
            .dequeue(dequeue_request(JobKind::EpisodeMatch, "after-release"))
            .await
            .expect("dequeue after release")
            .expect("EpisodeMatch should be ready after release");
        assert_eq!(lease.job.id, handle.job_id);
        assert_eq!(lease.job.correlation_id, Some(correlation_id));
        assert!(lease.job.dependency_key.is_none());

        queue
            .complete(lease.lease_id)
            .await
            .expect("complete episode match");
        assert_eq!(job_state(&pool, handle.job_id).await, "completed");
        assert_eq!(
            active_episode_matches_for_series(
                &pool,
                library_id,
                &series_root_path
            )
            .await,
            0,
            "completed dependency-gated EpisodeMatch leaves no active row"
        );
    }

    #[tokio::test]
    async fn series_resolve_releases_episode_match_and_reaches_index() {
        let Some(pool) = postgres_pool_or_skip(
            "series_resolve_releases_matching_episode_match_and_pipeline_reaches_index",
        )
        .await
        else {
            return;
        };
        let _db_guard = DB_TEST_LOCK.lock().await;
        let queue = Arc::new(
            PostgresQueueService::new(pool.clone())
                .await
                .expect("queue init"),
        );
        let events = Arc::new(InProcJobEventBus::new(64));
        let mut job_rx = events.subscribe();
        let cursors = Arc::new(MemoryCursorRepository::default());
        let correlations = CorrelationCache::default();
        let library_id =
            LibraryId(Uuid::from_u128(0x57600000000000000000000000000004));
        upsert_library(
            &pool,
            library_id,
            "Series Resolve Pipeline",
            LibraryType::Series,
            vec!["/library".into()],
        )
        .await
        .expect("seed series library");
        clear_queue_rows(&pool, library_id).await;

        let series_root_path = SeriesRootPath::try_new_under_library_root(
            "/library",
            "/library/Pipeline Show",
        )
        .unwrap();
        let other_series_root_path =
            SeriesRootPath::try_new_under_library_root(
                "/library",
                "/library/Other Pipeline Show",
            )
            .unwrap();
        let episode_path = "/library/Pipeline Show/Season 1/S01E01.mkv";
        let other_episode_path =
            "/library/Other Pipeline Show/Season 1/S01E01.mkv";
        let other_dependency =
            DependencyKey::series_root(&other_series_root_path);
        queue
            .enqueue(
                EnqueueRequest::new(
                    JobPriority::P0,
                    JobPayload::EpisodeMatch(episode_match_job(
                        library_id,
                        other_series_root_path.clone(),
                        "Other Pipeline Show",
                        other_episode_path,
                        1,
                    )),
                )
                .with_dependency(other_dependency.clone()),
            )
            .await
            .expect("enqueue unrelated deferred EpisodeMatch");

        let series_states: Arc<Box<dyn SeriesScanStateRepository>> =
            Arc::new(Box::new(InMemorySeriesScanStateRepository::default()));
        let series_resolver =
            Arc::new(StubSeriesResolver::new(Arc::clone(&series_states)));
        let actors = DispatcherActors::new(
            noop_folder_actor(library_id),
            Arc::new(StubAnalyzeActor) as Arc<dyn MediaAnalyzeActor>,
            Arc::new(StubMetadataActor) as Arc<dyn MetadataActor>,
            Arc::new(StubIndexActor) as Arc<dyn IndexerActor>,
            Arc::new(StubImageActor) as Arc<dyn ImageFetchActor>,
        );
        let dispatcher = DefaultJobDispatcher::new(
            Arc::clone(&queue),
            Arc::clone(&events),
            Arc::clone(&cursors),
            Arc::clone(&series_states),
            series_resolver,
            actors,
            correlations.clone(),
        );

        let media_analyze = media_analyze_episode_job(
            library_id,
            series_root_path.clone(),
            "Pipeline Show",
            episode_path,
            1,
        );
        let status = dispatcher
            .dispatch(&lease_for_payload(JobPayload::MediaAnalyze(
                media_analyze,
            )))
            .await;
        assert!(matches!(status, DispatchStatus::Success));
        assert_eq!(
            episode_matches_for_series_in_state(
                &pool,
                library_id,
                &series_root_path,
                "deferred"
            )
            .await,
            1,
            "episode analyze defers EpisodeMatch behind series_root dependency"
        );
        assert!(
            queue
                .dequeue(dequeue_request(JobKind::EpisodeMatch, "blocked"))
                .await
                .expect("blocked dequeue")
                .is_none(),
            "EpisodeMatch cannot dequeue before SeriesResolve releases its dependency"
        );

        let series_resolve = SeriesResolveJob {
            library_id,
            series_root_path: series_root_path.clone(),
            hint: Some(series_hint("Pipeline Show")),
            folder_name: "Pipeline Show".into(),
            scan_reason: ScanReason::BulkSeed,
        };
        let status = dispatcher
            .dispatch(&lease_for_payload(JobPayload::SeriesResolve(
                series_resolve,
            )))
            .await;
        assert!(matches!(status, DispatchStatus::Success));
        assert_eq!(
            episode_matches_for_series_in_state(
                &pool,
                library_id,
                &series_root_path,
                "ready"
            )
            .await,
            1,
            "SeriesResolve releases the matching series_root dependency"
        );
        assert_eq!(
            episode_matches_for_series_in_state(
                &pool,
                library_id,
                &other_series_root_path,
                "deferred"
            )
            .await,
            1,
            "SeriesResolve must not release another series root dependency"
        );

        let episode_lease = queue
            .dequeue(dequeue_request(JobKind::EpisodeMatch, "episode"))
            .await
            .expect("episode dequeue")
            .expect("released EpisodeMatch");
        let JobPayload::EpisodeMatch(episode_job) = &episode_lease.job.payload
        else {
            panic!("expected EpisodeMatch payload");
        };
        assert_eq!(episode_job.path_norm, episode_path);
        assert_eq!(episode_job.hierarchy.series_root_path, series_root_path);
        let episode_correlation = correlations
            .fetch_persisted_or_generate(
                episode_lease.job.id,
                episode_lease.job.correlation_id,
            )
            .await;
        assert_ne!(episode_correlation, Uuid::nil());

        let status = dispatcher.dispatch(&episode_lease).await;
        assert!(matches!(status, DispatchStatus::Success));
        queue
            .complete(episode_lease.lease_id)
            .await
            .expect("complete EpisodeMatch");
        assert_eq!(
            correlations
                .take_persisted_or_generate(
                    episode_lease.job.id,
                    episode_lease.job.correlation_id,
                )
                .await,
            episode_correlation
        );

        let metadata_lease = queue
            .dequeue(dequeue_request(JobKind::MetadataEnrich, "metadata"))
            .await
            .expect("metadata dequeue")
            .expect("episode MetadataEnrich");
        let metadata_correlation = correlations
            .fetch_persisted_or_generate(
                metadata_lease.job.id,
                metadata_lease.job.correlation_id,
            )
            .await;
        assert_ne!(metadata_correlation, Uuid::nil());
        let JobPayload::MetadataEnrich(metadata_job) =
            &metadata_lease.job.payload
        else {
            panic!("expected MetadataEnrich payload");
        };
        assert_eq!(metadata_job.path_norm, episode_path);
        let AnalyzeScanHierarchy::Episode(resolved_hierarchy) =
            &metadata_job.hierarchy
        else {
            panic!("episode MetadataEnrich should keep episode hierarchy");
        };
        assert!(
            matches!(resolved_hierarchy.series, SeriesLink::Resolved(_)),
            "EpisodeMatch should resolve the episode series before metadata enrichment"
        );

        let status = dispatcher.dispatch(&metadata_lease).await;
        assert!(matches!(status, DispatchStatus::Success));
        queue
            .complete(metadata_lease.lease_id)
            .await
            .expect("complete MetadataEnrich");
        assert_eq!(
            correlations
                .take_persisted_or_generate(
                    metadata_lease.job.id,
                    metadata_lease.job.correlation_id,
                )
                .await,
            metadata_correlation
        );

        let mut saw_episode_index = false;
        loop {
            let Some(index_lease) = queue
                .dequeue(dequeue_request(JobKind::IndexUpsert, "index"))
                .await
                .expect("index dequeue")
            else {
                break;
            };
            let index_correlation = correlations
                .fetch_persisted_or_generate(
                    index_lease.job.id,
                    index_lease.job.correlation_id,
                )
                .await;
            assert_ne!(index_correlation, Uuid::nil());
            let JobPayload::IndexUpsert(index_job) = &index_lease.job.payload
            else {
                panic!("expected IndexUpsert payload");
            };
            if index_job.path_norm == episode_path {
                saw_episode_index = true;
                let AnalyzeScanHierarchy::Episode(index_hierarchy) =
                    &index_job.hierarchy
                else {
                    panic!("episode IndexUpsert should keep episode hierarchy");
                };
                assert!(matches!(
                    index_hierarchy.series,
                    SeriesLink::Resolved(_)
                ));
            }
            let status = dispatcher.dispatch(&index_lease).await;
            assert!(matches!(status, DispatchStatus::Success));
            queue
                .complete(index_lease.lease_id)
                .await
                .expect("complete IndexUpsert");
            assert_eq!(
                correlations
                    .take_persisted_or_generate(
                        index_lease.job.id,
                        index_lease.job.correlation_id,
                    )
                    .await,
                index_correlation
            );
        }
        assert!(
            saw_episode_index,
            "episode MetadataEnrich should progress to IndexUpsert"
        );
        assert_eq!(
            active_episode_matches_for_series(
                &pool,
                library_id,
                &series_root_path
            )
            .await,
            0,
            "successful scanned series leaves no ready/deferred/leased EpisodeMatch jobs"
        );

        let mut saw_generated_follow_up_correlation = false;
        while let Ok(event) = job_rx.try_recv() {
            match event.payload {
                JobEventPayload::Enqueued { job_id, .. }
                | JobEventPayload::Merged {
                    merged_job_id: job_id,
                    ..
                } => {
                    assert_ne!(event.meta.correlation_id, Uuid::nil());
                    if let Some(cached) = correlations.fetch(&job_id).await {
                        assert_eq!(cached, event.meta.correlation_id);
                    }
                    saw_generated_follow_up_correlation = true;
                }
                _ => {}
            }
        }
        assert!(
            saw_generated_follow_up_correlation,
            "dispatcher follow-up jobs should publish and cache correlation ids"
        );
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
