//! Scan orchestration domain ports, adapters, and runtime wiring.
//!
//! This namespace is the boundary between scanner actors, durable queue state,
//! and server runtime composition. `BOUNDARY_CONTRACTS.md` maps the public
//! black-box seams to characterization tests that pin behavior while the
//! implementation continues to be split into focused modules.
//!
//! # Deep module map
//!
//! - `config`, `budget`, and `scheduler` define runtime knobs, workload tokens,
//!   and fair scheduling state.
//! - `job`, `lease`, `queue`, and `persistence` define the durable queue port and
//!   its Postgres adapter.
//! - `runtime` owns mailbox execution, lease supervision, scan-event fan-out,
//!   and the `LibraryCommandExecutor` entry point used by production producers.
//! - `work_planning`, `maintenance`, and `scan_cursor` plan library/fs-watch
//!   enqueue requests and persist cursor state.
//! - `context`, `dispatcher`, `delta`, `series`, and `series_state` hold the
//!   scan-unit shapes, dispatch pipeline, media delta reconciliation, and series
//!   dependency gates.
//! - `events`, `correlation`, and `scan_run` carry domain events, correlation
//!   continuity, and durable scan-run progress read models.
//!
//! # Facade invariants
//!
//! - Production enqueue paths go through `LibraryCommandExecutor`, `QueueService`,
//!   or the server `ScanOrchestrator` facade; callers must not pair direct queue
//!   writes with ad-hoc event publication.
//! - Progress and catalog projection are driven by scan events/read models behind
//!   the server scan control facade, not by route-level duplicate publishers.
//! - Root re-exports below are a curated compatibility surface for existing
//!   server wiring and downstream callers. New code should prefer the deep module
//!   path that owns the type or function.

pub mod budget;
pub mod config;
pub mod context;
pub mod correlation;
pub mod delta;
pub mod dispatcher;
pub mod enqueuer;
pub mod events;
pub mod job;
pub mod lease;
pub mod maintenance;
pub mod persistence;
pub mod queue;
pub mod runtime;
pub mod scan_cursor;
pub mod scan_run;
pub mod scheduler;
pub mod series;
pub mod series_state;
pub mod work_planning;

pub use crate::domain::scan::actors::{
    ActorObserver, DefaultLibraryActor, FileSystemEvent, FileSystemEventKind,
    FolderScanOutcome, FolderScanSummary, IssuedJobRecord, LibraryActor,
    LibraryActorCommand, LibraryActorConfig, LibraryActorEvent,
    LibraryActorState, LibraryRootDescriptor, LibraryRootState, LibraryRootsId,
    MaintenanceBatch, MaintenancePartition, MaintenanceSnapshot, MediaAnalyzed,
    MediaFileDiscovered, MediaKindHint, NoopActorObserver, SeedFoldersRequest,
    SeedMode, SeededFolder, StartMode,
};
pub use budget::{
    BudgetConfig, BudgetToken, InMemoryBudget, WorkloadBudget, WorkloadType,
};
pub use config::{
    BulkModeTuning, LeaseConfig, LibraryQueuePolicy, MaintenanceConfig,
    MetadataLimits, OrchestratorConfig, PriorityWeights, QueueConfig,
    RetryConfig, WatchConfig, WatchStrategy,
};
pub use correlation::CorrelationCache;
pub use delta::{
    DirectMediaDelta, FolderDeltaRepository, MediaMoveDelta,
    NoopFolderDeltaRepository, StoredMediaFile, fingerprints_equivalent,
    is_direct_child_file, is_immediate_child, reconcile_direct_media,
    removed_child_prefixes,
};
pub use dispatcher::{
    DefaultJobDispatcher, DispatchStatus, DispatcherActors, JobDispatcher,
};
pub use enqueuer::{JobPublisher, PipelineEnqueuer};
#[cfg(feature = "compat")]
pub use events::{DomainEvent, DomainEventPublisher, EventBus};
pub use events::{
    EventMeta, JobEvent, JobEventPayload, JobEventPublisher,
    ManualEnqueueRequest, ManualEnqueueResponse, ScanEvent, ScanEventBus,
    ScanEventPublisher, ScanSeedMode, ScanSeedSummary, stable_path_key,
};
pub use job::{
    AnalyzeScanHierarchy, DedupeKey, DependencyKey, EnqueueRequest,
    EpisodeMatchJob, FolderScanJob, ImageFetchJob, ImageFetchPriority,
    IndexUpsertJob, JobHandle, JobId, JobKind, JobPayload, JobPriority,
    JobRecord, JobState, MediaAnalyzeJob, MediaCandidate, MediaFingerprint,
    MetadataEnrichJob, ScanReason, SeriesResolveJob, TranscriptExtractJob,
    TranscriptExtractTrigger,
};
pub use lease::{
    CompletionOutcome, DequeueRequest, JobLease, LeaseId, LeaseRenewal,
    QueueSelector,
};
pub use maintenance::{
    MaintenanceCursorSummary, MaintenanceLibrary, MaintenancePlan,
    MaintenancePlanningLimits, MaintenanceSweepPlanningInput,
    build_maintenance_context, library_due_for_maintenance,
    plan_maintenance_sweep, plan_maintenance_sweep_from_summaries,
};
pub use persistence::{PostgresCursorRepository, PostgresQueueService};
pub use queue::{
    DurableJobState, FailOutcome, LeaseExpiryScanner, QueueInstrumentation,
    QueueService, QueueSnapshot, QueueSnapshotEntry, QueueTransitionOutcome,
    ReadyQueueCount,
};
pub use runtime::{
    InProcJobEventBus, JobEventStream, LibraryActorHandle,
    LibraryCommandExecutor, OrchestratorCommand, OrchestratorRuntime,
    OrchestratorRuntimeBuilder, OrchestratorRuntimeHandle, ScanEventStream,
};
pub use scan_cursor::{
    CursorDiff, ListingEntry, ScanCursor, ScanCursorId, ScanCursorRepository,
    compute_listing_hash, diff_cursor, normalize_path,
};
pub use scan_run::{
    LibraryScanRun, LibraryScanRunGetOrCreate, LibraryScanRunProgressUpdate,
    NewLibraryScanRun, PostgresScanRunRepository, ScanRunRepository,
};
pub use scheduler::{
    ReadyCountEntry, SchedulingReservation, WeightedFairScheduler,
};
pub use series::{
    DefaultSeriesResolver, EpisodeDependencyDecision, SeriesBundleFinalization,
    SeriesBundleTracker, SeriesCoordinator, SeriesDependencyReleaser,
    SeriesDiscoveryOutcome, SeriesFolderClues, SeriesLocator,
    SeriesMetadataProvider, SeriesResolution, SeriesResolverPort,
    clean_series_title, collapse_whitespace, slugify_series_title,
};
pub use series_state::{
    InMemorySeriesScanStateRepository, PostgresSeriesScanStateRepository,
    SeriesScanState, SeriesScanStateRepository, SeriesScanStatus,
};
pub use work_planning::{
    FsEventPlanningInput, LibraryStartPlanningInput, ScanFilesystemEvent,
    ScanFilesystemEventKind, ScanPlanningLimits, ScanPlanningRoot,
    ScanStartPlanningMode, ScanWorkPlan, build_root_scan_context,
    folder_scan_enqueue_request, plan_fs_event_burst, plan_library_start,
};
