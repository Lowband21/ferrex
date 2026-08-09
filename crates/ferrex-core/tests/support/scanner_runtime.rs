#![allow(dead_code)]

//! Reusable scanner-runtime fixtures and assertions for integration tests.
//!
//! The helpers in this module stay under `tests/support` so they can exercise
//! production scan types without changing the runtime surface exposed by
//! `ferrex-core`.

use std::{
    collections::BTreeSet,
    future::Future,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use chrono::Utc;
use ferrex_core::{
    api::types::ScanLifecycleStatus,
    domain::scan::{
        actors::{
            analyze::{AnalysisContext, MediaAnalyzeActor, MediaAnalyzed},
            folder::{
                FolderListingPlan, FolderScanActor, ScannerFileFilterPolicy,
            },
            image_fetch::ImageFetchActor,
            index::{
                IndexCommand, IndexerActor, IndexingChange, IndexingOutcome,
            },
            messages::{
                FolderScanOutcome, FolderScanSummary, MediaFileDiscovered,
                MediaKindHint,
            },
            metadata::{MediaReadyForIndex, MetadataActor, MetadataCommand},
        },
        orchestration::{
            context::{
                EpisodeHint, EpisodeLink, EpisodeScanHierarchy,
                FolderScanContext, SeasonFolderPath, SeasonFolderScanContext,
                SeasonLink, SeasonScanHierarchy, SeriesFolderScanContext,
                SeriesHint, SeriesLink, SeriesRef, SeriesRootPath,
                SeriesScanHierarchy,
            },
            dispatcher::DispatcherActors,
            events::{
                JobEvent, JobEventPayload, ScanEvent, ScanSeedMode,
                ScanSeedSummary,
            },
            job::{
                AnalyzeScanHierarchy, FolderScanJob, ImageFetchJob,
                IndexUpsertJob, JobId, JobKind, JobPriority, MediaAnalyzeJob,
                MediaFingerprint, MetadataEnrichJob, ScanReason,
                SeriesResolveJob,
            },
            scan_cursor::{compute_listing_hash, normalize_path},
            series::{SeriesMetadataProvider, SeriesResolution},
        },
    },
    error::Result as CoreResult,
    types::{
        LibraryId, LibraryReference, LibraryType, MediaID, SeriesID,
        SubjectKey, VideoMediaType,
    },
};
use serde_json::Value;
use sqlx::{PgPool, Row};
use tempfile::TempDir;
use tokio::{sync::Mutex, sync::broadcast, time::sleep};
use uuid::Uuid;

/// Default bounded wait used by scanner runtime helpers.
pub const DEFAULT_WAIT_TIMEOUT: Duration = Duration::from_secs(5);

/// Default polling interval used by Postgres-backed assertions.
pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(25);

/// Tunable bounded wait configuration for queue and event assertions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WaitConfig {
    pub timeout: Duration,
    pub poll_interval: Duration,
}

impl Default for WaitConfig {
    fn default() -> Self {
        Self {
            timeout: DEFAULT_WAIT_TIMEOUT,
            poll_interval: DEFAULT_POLL_INTERVAL,
        }
    }
}

impl WaitConfig {
    pub fn new(timeout: Duration, poll_interval: Duration) -> Self {
        Self {
            timeout,
            poll_interval,
        }
    }
}

/// Builder for a deterministic temp TV library shaped as
/// `Show/Season 1/S01E01.mkv` by default.
#[derive(Clone, Debug)]
pub struct SeriesLibraryBuilder {
    library_id: LibraryId,
    library_name: String,
    show_title: String,
    season_number: u16,
    episode_number: u16,
    episode_extension: String,
    episode_bytes: Vec<u8>,
}

impl Default for SeriesLibraryBuilder {
    fn default() -> Self {
        Self {
            library_id: LibraryId(Uuid::from_u128(
                0x57500000000000000000000000000001,
            )),
            library_name: "Test Series".to_owned(),
            show_title: "Show".to_owned(),
            season_number: 1,
            episode_number: 1,
            episode_extension: "mkv".to_owned(),
            episode_bytes: b"ferrex scanner runtime fixture\n".to_vec(),
        }
    }
}

impl SeriesLibraryBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn library_id(mut self, library_id: LibraryId) -> Self {
        self.library_id = library_id;
        self
    }

    pub fn library_name(mut self, name: impl Into<String>) -> Self {
        self.library_name = name.into();
        self
    }

    pub fn show_title(mut self, title: impl Into<String>) -> Self {
        self.show_title = title.into();
        self
    }

    pub fn season_number(mut self, season_number: u16) -> Self {
        self.season_number = season_number;
        self
    }

    pub fn episode_number(mut self, episode_number: u16) -> Self {
        self.episode_number = episode_number;
        self
    }

    pub fn episode_extension(mut self, extension: impl Into<String>) -> Self {
        self.episode_extension = extension.into();
        self
    }

    pub fn episode_bytes(mut self, bytes: impl Into<Vec<u8>>) -> Self {
        self.episode_bytes = bytes.into();
        self
    }

    pub fn build(self) -> Result<TempSeriesLibrary> {
        let temp_dir = TempDir::new().context("create temp media root")?;
        let library_root = temp_dir.path().to_path_buf();
        let show_root = library_root.join(&self.show_title);
        let season_folder =
            show_root.join(format!("Season {}", self.season_number));
        std::fs::create_dir_all(&season_folder).with_context(|| {
            format!("create season folder {}", season_folder.display())
        })?;

        let episode_file = season_folder.join(format!(
            "S{:02}E{:02}.{}",
            self.season_number, self.episode_number, self.episode_extension
        ));
        std::fs::write(&episode_file, &self.episode_bytes).with_context(
            || format!("write fixture {}", episode_file.display()),
        )?;

        let library_root_norm = normalize_path(&library_root)?;
        let show_root_norm = normalize_path(&show_root)?;
        let season_folder_norm = normalize_path(&season_folder)?;
        let episode_file_norm = normalize_path(&episode_file)?;

        let series_root_path = SeriesRootPath::try_new_under_library_root(
            &library_root_norm,
            show_root_norm.clone(),
        )?;
        let (season_folder_path, parsed_season_number) =
            SeasonFolderPath::try_new_under_series_root(
                &series_root_path,
                season_folder_norm.clone(),
            )?;
        if parsed_season_number != self.season_number {
            bail!(
                "fixture season folder parsed as {parsed_season_number}, expected {}",
                self.season_number
            );
        }

        let library = LibraryReference {
            id: self.library_id,
            name: self.library_name,
            library_type: LibraryType::Series,
            paths: vec![library_root.clone()],
        };

        Ok(TempSeriesLibrary {
            temp_dir,
            library,
            library_root,
            library_root_norm,
            show_title: self.show_title,
            show_root,
            show_root_norm,
            season_number: self.season_number,
            episode_number: self.episode_number,
            episode_extension: self.episode_extension,
            season_folder,
            season_folder_norm,
            episode_file,
            episode_file_norm,
            episode_len: self.episode_bytes.len() as u64,
            series_root_path,
            season_folder_path,
        })
    }
}

/// Deterministic TV library fixture rooted in a temp directory.
#[derive(Debug)]
pub struct TempSeriesLibrary {
    temp_dir: TempDir,
    pub library: LibraryReference,
    pub library_root: PathBuf,
    pub library_root_norm: String,
    pub show_title: String,
    pub show_root: PathBuf,
    pub show_root_norm: String,
    pub season_number: u16,
    pub episode_number: u16,
    pub episode_extension: String,
    pub season_folder: PathBuf,
    pub season_folder_norm: String,
    pub episode_file: PathBuf,
    pub episode_file_norm: String,
    episode_len: u64,
    pub series_root_path: SeriesRootPath,
    pub season_folder_path: SeasonFolderPath,
}

impl TempSeriesLibrary {
    pub fn builder() -> SeriesLibraryBuilder {
        SeriesLibraryBuilder::default()
    }

    pub fn temp_dir(&self) -> &TempDir {
        &self.temp_dir
    }

    pub fn relative_episode_path(&self) -> PathBuf {
        PathBuf::from(&self.show_title)
            .join(format!("Season {}", self.season_number))
            .join(format!(
                "S{:02}E{:02}.{}",
                self.season_number, self.episode_number, self.episode_extension
            ))
    }

    pub fn library_actor_config(
        &self,
        max_outstanding_jobs: usize,
    ) -> ferrex_core::domain::scan::actors::library::LibraryActorConfig {
        library_actor_config(self.library.clone(), max_outstanding_jobs)
    }

    pub fn series_context(&self) -> FolderScanContext {
        series_folder_context(self.library.id, self.series_root_path.clone())
    }

    pub fn season_context(&self) -> FolderScanContext {
        season_folder_context(
            self.library.id,
            self.series_root_path.clone(),
            self.season_folder_path.clone(),
            self.season_number,
        )
    }

    pub fn series_folder_scan_job(&self, reason: ScanReason) -> FolderScanJob {
        folder_scan_job(self.series_context(), reason)
    }

    pub fn season_folder_scan_job(&self, reason: ScanReason) -> FolderScanJob {
        folder_scan_job(self.season_context(), reason)
    }

    pub fn episode_discovered(
        &self,
        reason: ScanReason,
    ) -> MediaFileDiscovered {
        episode_media_file_discovered(
            self.library.id,
            self.episode_file_norm.clone(),
            self.episode_len,
            self.season_context(),
            self.show_title.clone(),
            self.season_number,
            self.episode_number,
            reason,
        )
    }

    pub fn series_resolve_job(&self, reason: ScanReason) -> SeriesResolveJob {
        SeriesResolveJob {
            library_id: self.library.id,
            series_root_path: self.series_root_path.clone(),
            hint: Some(series_hint(&self.show_title)),
            folder_name: self.show_title.clone(),
            scan_reason: reason,
        }
    }
}

pub fn library_reference(
    id: LibraryId,
    name: impl Into<String>,
    library_type: LibraryType,
    paths: Vec<PathBuf>,
) -> LibraryReference {
    LibraryReference {
        id,
        name: name.into(),
        library_type,
        paths,
    }
}

pub fn library_actor_config(
    library: LibraryReference,
    max_outstanding_jobs: usize,
) -> ferrex_core::domain::scan::actors::library::LibraryActorConfig {
    ferrex_core::domain::scan::actors::library::LibraryActorConfig {
        root_paths: library.paths.clone(),
        library,
        max_outstanding_jobs,
    }
}

pub fn series_root_path_under_library(
    library_root: &Path,
    series_root: &Path,
) -> Result<SeriesRootPath> {
    let library_root_norm = normalize_path(library_root)?;
    let series_root_norm = normalize_path(series_root)?;
    Ok(SeriesRootPath::try_new_under_library_root(
        &library_root_norm,
        series_root_norm,
    )?)
}

pub fn season_folder_path_under_series(
    series_root_path: &SeriesRootPath,
    season_folder: &Path,
) -> Result<(SeasonFolderPath, u16)> {
    Ok(SeasonFolderPath::try_new_under_series_root(
        series_root_path,
        normalize_path(season_folder)?,
    )?)
}

pub fn series_folder_context(
    library_id: LibraryId,
    series_root_path: SeriesRootPath,
) -> FolderScanContext {
    FolderScanContext::Series(SeriesFolderScanContext {
        library_id,
        series_root_path,
    })
}

pub fn season_folder_context(
    library_id: LibraryId,
    series_root_path: SeriesRootPath,
    season_folder_path: SeasonFolderPath,
    season_number: u16,
) -> FolderScanContext {
    FolderScanContext::Season(SeasonFolderScanContext {
        library_id,
        series_root_path,
        season_folder_path,
        season_number,
    })
}

pub fn folder_scan_job(
    context: FolderScanContext,
    scan_reason: ScanReason,
) -> FolderScanJob {
    FolderScanJob {
        context,
        scan_reason,
        enqueue_time: Utc::now(),
        device_id: None,
    }
}

pub fn deterministic_uuid(scope: &str, value: impl AsRef<str>) -> Uuid {
    Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("ferrex-scanner-testkit:{scope}:{}", value.as_ref()).as_bytes(),
    )
}

pub fn deterministic_media_id(
    variant: VideoMediaType,
    stable_key: impl AsRef<str>,
) -> MediaID {
    MediaID::from((deterministic_uuid("media", stable_key), variant))
}

pub fn deterministic_job_id(stable_key: impl AsRef<str>) -> JobId {
    JobId(deterministic_uuid("job", stable_key))
}

pub fn deterministic_series_id(stable_key: impl AsRef<str>) -> SeriesID {
    SeriesID(deterministic_uuid("series", stable_key))
}

pub fn media_fingerprint(size: u64) -> MediaFingerprint {
    MediaFingerprint {
        device_id: None,
        inode: None,
        size,
        mtime: 0,
        weak_hash: Some(format!("testkit:{size}")),
    }
}

pub fn series_hint(title: impl Into<String>) -> SeriesHint {
    let title = title.into();
    let slug = title
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");

    SeriesHint {
        title,
        slug: (!slug.is_empty()).then_some(slug),
        year: None,
        region: None,
    }
}

pub fn episode_hierarchy(
    series_root_path: SeriesRootPath,
    show_title: impl Into<String>,
    season_number: u16,
    episode_number: u16,
) -> AnalyzeScanHierarchy {
    let series_hierarchy = SeriesScanHierarchy {
        series_root_path,
        series: SeriesLink::Hint(series_hint(show_title)),
    };
    let season_hierarchy = SeasonScanHierarchy::from_series_hierarch(
        series_hierarchy,
        SeasonLink::Number(season_number),
    );
    AnalyzeScanHierarchy::Episode(EpisodeScanHierarchy::from_season_hierarch(
        season_hierarchy,
        EpisodeLink::Hint(EpisodeHint {
            number: episode_number,
            title: None,
        }),
    ))
}

pub fn episode_media_file_discovered(
    library_id: LibraryId,
    path_norm: String,
    file_size: u64,
    context: FolderScanContext,
    show_title: impl Into<String>,
    season_number: u16,
    episode_number: u16,
    scan_reason: ScanReason,
) -> MediaFileDiscovered {
    let media_id = deterministic_media_id(VideoMediaType::Episode, &path_norm);
    let series_root_path = context
        .series_root_path()
        .cloned()
        .unwrap_or_else(|| SeriesRootPath::try_new("/fixture/Show").unwrap());

    MediaFileDiscovered {
        library_id,
        path_norm: path_norm.clone(),
        fingerprint: media_fingerprint(file_size),
        classified_as: MediaKindHint::Episode,
        media_id,
        variant: VideoMediaType::Episode,
        node: ferrex_core::domain::scan::orchestration::context::ScanNodeKind::EpisodeFile,
        hierarchy: episode_hierarchy(
            series_root_path,
            show_title,
            season_number,
            episode_number,
        ),
        context,
        scan_reason,
    }
}

pub fn media_analyze_job_from_discovered(
    discovered: &MediaFileDiscovered,
) -> MediaAnalyzeJob {
    MediaAnalyzeJob {
        library_id: discovered.library_id,
        path_norm: discovered.path_norm.clone(),
        fingerprint: discovered.fingerprint.clone(),
        discovered_at: Utc::now(),
        media_id: discovered.media_id,
        variant: discovered.variant,
        hierarchy: discovered.hierarchy.clone(),
        node: discovered.node.clone(),
        scan_reason: discovered.scan_reason,
    }
}

pub fn metadata_enrich_job_from_analyzed(
    analyzed: &MediaAnalyzed,
    scan_reason: ScanReason,
) -> MetadataEnrichJob {
    MetadataEnrichJob {
        library_id: analyzed.library_id,
        media_id: analyzed.media_id,
        variant: analyzed.variant,
        hierarchy: analyzed.hierarchy.clone(),
        node: analyzed.node.clone(),
        path_norm: analyzed.path_norm.clone(),
        fingerprint: analyzed.fingerprint.clone(),
        scan_reason,
    }
}

pub fn index_upsert_job_from_ready(
    ready: &MediaReadyForIndex,
    path_norm: impl Into<String>,
) -> IndexUpsertJob {
    let path_norm = path_norm.into();
    IndexUpsertJob {
        library_id: ready.library_id,
        media_id: ready.media_id,
        variant: ready.variant,
        hierarchy: ready.hierarchy.clone(),
        node: ready.node.clone(),
        idempotency_key: format!("index:{}:{path_norm}", ready.library_id),
        path_norm,
    }
}

pub fn indexing_outcome_from_discovered(
    discovered: &MediaFileDiscovered,
) -> IndexingOutcome {
    IndexingOutcome {
        library_id: discovered.library_id,
        path_norm: discovered.path_norm.clone(),
        media_id: discovered.media_id,
        hierarchy: discovered.hierarchy.clone(),
        indexed_at: Utc::now(),
        upserted: true,
        media: None,
        change: IndexingChange::Created,
    }
}

pub fn scan_event_folder_discovered(
    context: FolderScanContext,
    reason: ScanReason,
) -> ScanEvent {
    ScanEvent::FolderDiscovered {
        context: Box::new(context),
        reason,
        correlation_id: None,
        durable_job_id: None,
    }
}

pub fn scan_event_media_discovered(
    discovered: MediaFileDiscovered,
) -> ScanEvent {
    ScanEvent::MediaFileDiscovered(Box::new(discovered))
}

pub fn scan_event_indexed(outcome: IndexingOutcome) -> ScanEvent {
    ScanEvent::Indexed(Box::new(outcome))
}

pub fn scan_event_seed_completed(
    library_id: LibraryId,
    mode: ScanSeedMode,
    queued_folders: usize,
) -> ScanEvent {
    ScanEvent::SeedCompleted(ScanSeedSummary {
        library_id,
        correlation_id: None,
        mode,
        queued_folders,
        enrolled_job_ids: Vec::new(),
        completed_at: Utc::now(),
    })
}

pub fn job_event(
    library_id: LibraryId,
    idempotency_key: impl Into<String>,
    path_norm: Option<&str>,
    payload: JobEventPayload,
) -> JobEvent {
    JobEvent::from_job(
        None,
        library_id,
        idempotency_key.into(),
        path_norm.and_then(|path| SubjectKey::path(path.to_owned()).ok()),
        payload,
    )
}

pub fn job_event_enqueued(
    library_id: LibraryId,
    kind: JobKind,
    priority: JobPriority,
    path_norm: impl AsRef<str>,
) -> JobEvent {
    let path_norm = path_norm.as_ref();
    let job_id = deterministic_job_id(format!("{kind:?}:{path_norm}"));
    job_event(
        library_id,
        format!("enqueue:{kind:?}:{path_norm}"),
        Some(path_norm),
        JobEventPayload::Enqueued {
            job_id,
            kind,
            priority,
        },
    )
}

pub fn job_event_completed(
    library_id: LibraryId,
    kind: JobKind,
    priority: JobPriority,
    path_norm: impl AsRef<str>,
) -> JobEvent {
    let path_norm = path_norm.as_ref();
    let job_id = deterministic_job_id(format!("{kind:?}:{path_norm}"));
    job_event(
        library_id,
        format!("complete:{kind:?}:{path_norm}"),
        Some(path_norm),
        JobEventPayload::Completed {
            job_id,
            kind,
            priority,
        },
    )
}

/// Folder actor whose outputs are supplied by the test.
#[derive(Clone, Debug)]
pub struct StaticFolderScanActor {
    plan: FolderListingPlan,
    discovered: Vec<MediaFileDiscovered>,
    children: Vec<FolderScanContext>,
}

impl StaticFolderScanActor {
    pub fn new(
        discovered: Vec<MediaFileDiscovered>,
        children: Vec<FolderScanContext>,
    ) -> Self {
        let plan = FolderListingPlan {
            directories: Vec::new(),
            media_files: discovered
                .iter()
                .map(|event| PathBuf::from(&event.path_norm))
                .collect(),
            ancillary_files: Vec::new(),
            generated_listing_hash: compute_listing_hash(&[]),
            total_entries: discovered.len() + children.len(),
            folder_missing: false,
        };
        Self {
            plan,
            discovered,
            children,
        }
    }

    pub fn with_plan(mut self, plan: FolderListingPlan) -> Self {
        self.plan = plan;
        self
    }
}

#[async_trait]
impl FolderScanActor for StaticFolderScanActor {
    async fn plan_listing(
        &self,
        _job: &FolderScanJob,
    ) -> CoreResult<FolderListingPlan> {
        Ok(self.plan.clone())
    }

    async fn discover_media(
        &self,
        _plan: &FolderListingPlan,
        _job: &FolderScanJob,
    ) -> CoreResult<Vec<MediaFileDiscovered>> {
        Ok(self.discovered.clone())
    }

    async fn derive_child_contexts(
        &self,
        _plan: &FolderListingPlan,
        _command: &FolderScanJob,
    ) -> CoreResult<Vec<FolderScanContext>> {
        Ok(self.children.clone())
    }

    fn finalize(
        &self,
        context: &FolderScanContext,
        plan: &FolderListingPlan,
        discovered: &[MediaFileDiscovered],
        children: &[FolderScanContext],
    ) -> CoreResult<FolderScanSummary> {
        Ok(FolderScanSummary {
            context: context.clone(),
            discovered_files: discovered.len(),
            enqueued_subfolders: children.len(),
            listing_hash: plan.generated_listing_hash.clone(),
            outcome: if discovered.is_empty() && children.is_empty() {
                FolderScanOutcome::Empty
            } else {
                FolderScanOutcome::Changed
            },
            completed_at: Utc::now(),
        })
    }
}

/// Analyze actor that converts jobs into analyzed events without probing media.
#[derive(Clone, Copy, Debug, Default)]
pub struct PassthroughAnalyzeActor;

#[async_trait]
impl MediaAnalyzeActor for PassthroughAnalyzeActor {
    async fn analyze(
        &self,
        command: MediaAnalyzeJob,
    ) -> CoreResult<MediaAnalyzed> {
        Ok(MediaAnalyzed {
            library_id: command.library_id,
            media_id: command.media_id,
            variant: command.variant,
            hierarchy: command.hierarchy,
            node: command.node,
            path_norm: command.path_norm,
            fingerprint: command.fingerprint,
            analyzed_at: Utc::now(),
            analysis: AnalysisContext::default(),
            thumbnails: Vec::new(),
        })
    }
}

/// Metadata actor that forwards analysis into index-ready media.
#[derive(Clone, Copy, Debug, Default)]
pub struct PassthroughMetadataActor;

#[async_trait]
impl MetadataActor for PassthroughMetadataActor {
    async fn enrich(
        &self,
        command: MetadataCommand,
    ) -> CoreResult<MediaReadyForIndex> {
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

/// Index actor that records every deterministic indexing outcome it returns.
#[derive(Clone, Debug, Default)]
pub struct RecordingIndexerActor {
    outcomes: Arc<Mutex<Vec<IndexingOutcome>>>,
}

impl RecordingIndexerActor {
    pub async fn outcomes(&self) -> Vec<IndexingOutcome> {
        self.outcomes.lock().await.clone()
    }
}

#[async_trait]
impl IndexerActor for RecordingIndexerActor {
    async fn index(
        &self,
        command: IndexCommand,
    ) -> CoreResult<IndexingOutcome> {
        let outcome = IndexingOutcome {
            library_id: command.job.library_id,
            path_norm: command.job.path_norm,
            media_id: command.ready.media_id,
            hierarchy: command.job.hierarchy,
            indexed_at: Utc::now(),
            upserted: true,
            media: None,
            change: IndexingChange::Created,
        };
        self.outcomes.lock().await.push(outcome.clone());
        Ok(outcome)
    }
}

/// Image actor that acknowledges image jobs without external IO.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoopImageFetchActor;

#[async_trait]
impl ImageFetchActor for NoopImageFetchActor {
    async fn fetch(&self, _job: &ImageFetchJob) -> CoreResult<()> {
        Ok(())
    }
}

/// Series provider that resolves a stable `SeriesRef` from the requested root.
#[derive(Clone, Copy, Debug, Default)]
pub struct DeterministicSeriesProvider;

#[async_trait]
impl SeriesMetadataProvider for DeterministicSeriesProvider {
    async fn resolve_series(
        &self,
        library_id: LibraryId,
        series_root_path: &SeriesRootPath,
        hint: &SeriesHint,
        _folder_name: &str,
    ) -> CoreResult<SeriesResolution> {
        Ok(series_resolution(
            library_id,
            series_root_path.clone(),
            hint.clone(),
        ))
    }
}

pub fn series_resolution(
    library_id: LibraryId,
    series_root_path: SeriesRootPath,
    hint: SeriesHint,
) -> SeriesResolution {
    let series_id = deterministic_series_id(series_root_path.as_str());
    let series_ref = SeriesRef {
        id: series_id,
        slug: hint.slug.clone(),
        title: Some(hint.title.clone()),
    };
    let hierarchy = AnalyzeScanHierarchy::Series(SeriesScanHierarchy {
        series_root_path: series_root_path.clone(),
        series: SeriesLink::Resolved(series_ref.clone()),
    });
    let analyzed = MediaAnalyzed {
        library_id,
        media_id: MediaID::Series(series_id),
        variant: VideoMediaType::Series,
        hierarchy: hierarchy.clone(),
        node: ferrex_core::domain::scan::orchestration::context::ScanNodeKind::SeriesRoot,
        path_norm: series_root_path.as_str().to_owned(),
        fingerprint: MediaFingerprint::default(),
        analyzed_at: Utc::now(),
        analysis: AnalysisContext::default(),
        thumbnails: Vec::new(),
    };
    let ready = MediaReadyForIndex {
        library_id,
        media_id: analyzed.media_id,
        variant: analyzed.variant,
        hierarchy,
        node: analyzed.node.clone(),
        normalized_title: Some(hint.title),
        analyzed,
        prepared_at: Utc::now(),
        image_jobs: Vec::new(),
    };

    SeriesResolution { series_ref, ready }
}

pub fn fake_dispatcher_actors(
    folder: Arc<dyn FolderScanActor>,
) -> (DispatcherActors, RecordingIndexerActor) {
    let indexer = RecordingIndexerActor::default();
    let actors = DispatcherActors::new(
        folder,
        Arc::new(PassthroughAnalyzeActor) as Arc<dyn MediaAnalyzeActor>,
        Arc::new(PassthroughMetadataActor) as Arc<dyn MetadataActor>,
        Arc::new(indexer.clone()) as Arc<dyn IndexerActor>,
        Arc::new(NoopImageFetchActor) as Arc<dyn ImageFetchActor>,
    );
    (actors, indexer)
}

pub fn scanner_file_filter_policy_for(
    media_extensions: impl IntoIterator<Item = impl Into<String>>,
) -> ScannerFileFilterPolicy {
    ScannerFileFilterPolicy::new(
        media_extensions.into_iter().map(Into::into),
        Vec::<String>::new(),
    )
}

pub async fn wait_until<T, F, Fut>(
    description: impl AsRef<str>,
    wait: WaitConfig,
    mut probe: F,
) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<Option<T>>>,
{
    let start = Instant::now();
    loop {
        if let Some(value) = probe().await? {
            return Ok(value);
        }

        if start.elapsed() >= wait.timeout {
            bail!("timed out waiting for {}", description.as_ref());
        }

        let remaining = wait.timeout.saturating_sub(start.elapsed());
        sleep(wait.poll_interval.min(remaining)).await;
    }
}

pub async fn wait_for_broadcast<T, F>(
    rx: &mut broadcast::Receiver<T>,
    wait: WaitConfig,
    mut predicate: F,
) -> Result<T>
where
    T: Clone,
    F: FnMut(&T) -> bool,
{
    let start = Instant::now();
    loop {
        let elapsed = start.elapsed();
        if elapsed >= wait.timeout {
            bail!("timed out waiting for broadcast event");
        }
        let remaining = wait.timeout - elapsed;
        match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Ok(event)) if predicate(&event) => return Ok(event),
            Ok(Ok(_)) => continue,
            Ok(Err(broadcast::error::RecvError::Lagged(_))) => continue,
            Ok(Err(broadcast::error::RecvError::Closed)) => {
                bail!("broadcast channel closed while waiting for event")
            }
            Err(_) => bail!("timed out waiting for broadcast event"),
        }
    }
}

pub async fn wait_for_job_event<F>(
    rx: &mut broadcast::Receiver<JobEvent>,
    wait: WaitConfig,
    predicate: F,
) -> Result<JobEvent>
where
    F: FnMut(&JobEvent) -> bool,
{
    wait_for_broadcast(rx, wait, predicate).await
}

pub async fn wait_for_scan_event<F>(
    rx: &mut broadcast::Receiver<ScanEvent>,
    wait: WaitConfig,
    predicate: F,
) -> Result<ScanEvent>
where
    F: FnMut(&ScanEvent) -> bool,
{
    wait_for_broadcast(rx, wait, predicate).await
}

pub async fn queue_state_count(pool: &PgPool, state: &str) -> Result<i64> {
    let row = sqlx::query(
        r#"SELECT COUNT(*)::bigint AS count FROM orchestrator_jobs WHERE state = $1"#,
    )
    .bind(state)
    .fetch_one(pool)
    .await?;
    Ok(row.try_get::<i64, _>("count")?)
}

pub async fn queue_kind_state_count(
    pool: &PgPool,
    kind: JobKind,
    state: &str,
) -> Result<i64> {
    let row = sqlx::query(
        r#"
        SELECT COUNT(*)::bigint AS count
        FROM orchestrator_jobs
        WHERE kind = $1 AND state = $2
        "#,
    )
    .bind(kind as i16)
    .bind(state)
    .fetch_one(pool)
    .await?;
    Ok(row.try_get::<i64, _>("count")?)
}

pub async fn wait_for_queue_kind_state_count(
    pool: &PgPool,
    kind: JobKind,
    state: &str,
    expected: i64,
    wait: WaitConfig,
) -> Result<i64> {
    wait_until(
        format!("{kind:?} queue state {state} count {expected}"),
        wait,
        || async {
            let observed = queue_kind_state_count(pool, kind, state).await?;
            Ok((observed == expected).then_some(observed))
        },
    )
    .await
}

pub async fn assert_queue_kind_state_count(
    pool: &PgPool,
    kind: JobKind,
    state: &str,
    expected: i64,
) -> Result<()> {
    let observed = queue_kind_state_count(pool, kind, state).await?;
    if observed != expected {
        bail!(
            "expected {expected} {kind:?} jobs in state {state}, observed {observed}"
        );
    }
    Ok(())
}

pub fn job_payload_path_norm(value: &Value) -> Option<String> {
    let payload = value.get("payload").unwrap_or(value);

    for key in ["path_norm", "series_root_path", "folder_path_norm"] {
        if let Some(path) = payload.get(key).and_then(Value::as_str) {
            return Some(path.to_owned());
        }
    }

    let context = payload.get("context")?;
    for (variant, key) in [
        ("Movie", "movie_root_path"),
        ("Series", "series_root_path"),
        ("Season", "season_folder_path"),
    ] {
        if let Some(path) = context
            .get(variant)
            .and_then(|ctx| ctx.get(key))
            .and_then(Value::as_str)
        {
            return Some(path.to_owned());
        }
    }

    None
}

pub async fn ready_queue_paths(
    pool: &PgPool,
    kind: JobKind,
) -> Result<BTreeSet<String>> {
    let rows = sqlx::query(
        r#"
        SELECT payload
        FROM orchestrator_jobs
        WHERE kind = $1 AND state = 'ready'
        ORDER BY created_at ASC
        "#,
    )
    .bind(kind as i16)
    .fetch_all(pool)
    .await?;

    let mut paths = BTreeSet::new();
    for row in rows {
        let payload: Value = row.try_get("payload")?;
        if let Some(path) = job_payload_path_norm(&payload) {
            paths.insert(path);
        }
    }
    Ok(paths)
}

pub async fn wait_for_ready_queue_paths(
    pool: &PgPool,
    kind: JobKind,
    expected: BTreeSet<String>,
    wait: WaitConfig,
) -> Result<BTreeSet<String>> {
    wait_until(format!("ready {kind:?} queue paths"), wait, || async {
        let observed = ready_queue_paths(pool, kind).await?;
        Ok(expected.is_subset(&observed).then_some(observed))
    })
    .await
}

pub async fn assert_ready_queue_paths(
    pool: &PgPool,
    kind: JobKind,
    expected: BTreeSet<String>,
) -> Result<()> {
    let observed = ready_queue_paths(pool, kind).await?;
    if !expected.is_subset(&observed) {
        bail!(
            "ready {kind:?} queue missing expected paths; expected subset {expected:?}, observed {observed:?}"
        );
    }
    Ok(())
}

pub async fn latest_scan_run_status(
    pool: &PgPool,
    library_id: LibraryId,
) -> Result<Option<ScanLifecycleStatus>> {
    let row = sqlx::query(
        r#"
        SELECT status
        FROM library_scan_runs
        WHERE library_id = $1
        ORDER BY created_at DESC
        LIMIT 1
        "#,
    )
    .bind(library_id.to_uuid())
    .fetch_optional(pool)
    .await?;

    row.map(|row| {
        let status: String = row.try_get("status")?;
        ScanLifecycleStatus::from_db(&status)
            .ok_or_else(|| anyhow!("unknown library_scan_runs.status {status}"))
    })
    .transpose()
}

pub async fn wait_for_scan_run_status(
    pool: &PgPool,
    library_id: LibraryId,
    expected: ScanLifecycleStatus,
    wait: WaitConfig,
) -> Result<ScanLifecycleStatus> {
    wait_until(
        format!("latest scan run status {}", expected.as_str()),
        wait,
        || async {
            let observed = latest_scan_run_status(pool, library_id).await?;
            Ok(
                (observed == Some(expected.clone()))
                    .then_some(expected.clone()),
            )
        },
    )
    .await
}

pub async fn assert_latest_scan_run_status(
    pool: &PgPool,
    library_id: LibraryId,
    expected: ScanLifecycleStatus,
) -> Result<()> {
    let observed = latest_scan_run_status(pool, library_id).await?;
    if observed != Some(expected.clone()) {
        bail!(
            "expected latest scan run status {}, observed {:?}",
            expected.as_str(),
            observed
        );
    }
    Ok(())
}
