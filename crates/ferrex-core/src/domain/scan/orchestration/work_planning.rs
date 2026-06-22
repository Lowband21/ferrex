//! Bounded scan work-planning contracts shared by library actors and schedulers.
//!
//! The planner APIs take explicit library/root configuration, start modes,
//! filesystem event bursts, scan reasons, correlations, and limits, then return
//! stable `EnqueueRequest`/`FolderScanJob` plans. Runtime actors remain
//! responsible for mailbox state, outstanding-job admission, and throttling.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use tokio::fs;
use tracing::warn;
use uuid::Uuid;

use crate::domain::scan::actors::folder::ScannerFileFilterPolicy;
use crate::domain::scan::orchestration::context::{
    FolderScanContext, MovieFolderScanContext, MovieRootPath,
    SeriesFolderScanContext, SeriesRootPath,
};
use crate::domain::scan::orchestration::job::{
    EnqueueRequest, FolderScanJob, JobPayload, JobPriority, ScanReason,
};
use crate::domain::scan::orchestration::scan_cursor::normalize_path;
use crate::error::Result;
use crate::types::prelude::LibraryReference;

/// Start command mode used by scan work planners.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScanStartPlanningMode {
    Bulk,
    Maintenance,
    Resume,
}

/// Filesystem event kind used by scan work planners.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScanFilesystemEventKind {
    Created,
    Modified,
    Deleted,
    Moved,
    Overflow,
}

/// Minimal filesystem event fields needed to plan folder scan work.
#[derive(Clone, Debug)]
pub struct ScanFilesystemEvent {
    pub correlation_id: Option<Uuid>,
    pub path: PathBuf,
    pub kind: ScanFilesystemEventKind,
}

/// Library root descriptor consumed by scan work planners.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScanPlanningRoot {
    pub root_id: u16,
    pub path: PathBuf,
    pub path_norm: String,
}

impl ScanPlanningRoot {
    pub fn new(root_id: u16, path: PathBuf) -> Self {
        let path_norm = path.to_string_lossy().to_string();
        Self {
            root_id,
            path,
            path_norm,
        }
    }

    pub fn with_path_norm(
        root_id: u16,
        path: PathBuf,
        path_norm: String,
    ) -> Self {
        Self {
            root_id,
            path,
            path_norm,
        }
    }
}

/// Bounds applied to one scan planning pass.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScanPlanningLimits {
    pub max_jobs: usize,
    pub max_root_entries: usize,
}

impl ScanPlanningLimits {
    pub fn new(max_jobs: usize, max_root_entries: usize) -> Self {
        Self {
            max_jobs: max_jobs.max(1),
            max_root_entries: max_root_entries.max(1),
        }
    }

    /// Preserve legacy actor behavior when the caller only wants the planner
    /// boundary and will apply admission/throttling elsewhere.
    pub fn unbounded() -> Self {
        Self {
            max_jobs: usize::MAX,
            max_root_entries: usize::MAX,
        }
    }
}

/// Bounded folder scan work selected by one planner invocation.
#[derive(Clone, Debug, Default)]
pub struct ScanWorkPlan {
    pub requests: Vec<EnqueueRequest>,
    pub skipped_entries: usize,
    pub dropped_events: usize,
    pub errors: Vec<String>,
}

impl ScanWorkPlan {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn planned_jobs(&self) -> usize {
        self.requests.len()
    }

    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }
}

/// Explicit inputs for library start-command planning.
#[derive(Debug)]
pub struct LibraryStartPlanningInput<'a> {
    pub library: &'a LibraryReference,
    pub roots: &'a [ScanPlanningRoot],
    pub mode: ScanStartPlanningMode,
    pub correlation_id: Option<Uuid>,
    pub limits: ScanPlanningLimits,
    pub now: DateTime<Utc>,
}

/// Explicit inputs for filesystem watcher burst planning.
#[derive(Debug)]
pub struct FsEventPlanningInput<'a> {
    pub library: &'a LibraryReference,
    pub root: ScanPlanningRoot,
    pub events: Vec<ScanFilesystemEvent>,
    pub command_correlation_id: Option<Uuid>,
    pub state_correlation_id: Option<Uuid>,
    pub file_filters: &'a ScannerFileFilterPolicy,
    pub limits: ScanPlanningLimits,
    pub now: DateTime<Utc>,
}

#[derive(Clone, Debug)]
struct RootFolderPlanEntry {
    root_path_norm: String,
    folder_path_norm: String,
}

#[derive(Clone, Debug, Default)]
struct RootFolderEnumeration {
    folders: Vec<RootFolderPlanEntry>,
    skipped_entries: usize,
    errors: Vec<String>,
}

/// Plan folder scans for a library start command.
pub async fn plan_library_start(
    input: LibraryStartPlanningInput<'_>,
) -> Result<ScanWorkPlan> {
    match input.mode {
        ScanStartPlanningMode::Bulk => {
            plan_root_child_folder_scans(
                input.library,
                input.roots,
                JobPriority::P1,
                ScanReason::BulkSeed,
                input.correlation_id,
                input.limits,
                input.now,
            )
            .await
        }
        ScanStartPlanningMode::Maintenance | ScanStartPlanningMode::Resume => {
            Ok(ScanWorkPlan::empty())
        }
    }
}

/// Plan folder scans for one debounced filesystem event burst.
pub async fn plan_fs_event_burst(
    input: FsEventPlanningInput<'_>,
) -> Result<ScanWorkPlan> {
    let burst_correlation = input
        .command_correlation_id
        .or(input.state_correlation_id)
        .or_else(|| input.events.iter().find_map(|event| event.correlation_id));

    let (overflow, changes): (Vec<_>, Vec<_>) = input
        .events
        .into_iter()
        .partition(|event| event.kind == ScanFilesystemEventKind::Overflow);

    let mut plan = ScanWorkPlan::empty();

    if !overflow.is_empty() && plan.requests.len() < input.limits.max_jobs {
        let roots = [input.root.clone()];
        let overflow_plan = plan_root_child_folder_scans(
            input.library,
            &roots,
            JobPriority::P0,
            ScanReason::WatcherOverflow,
            burst_correlation,
            input.limits,
            input.now,
        )
        .await?;
        plan.skipped_entries += overflow_plan.skipped_entries;
        plan.errors.extend(overflow_plan.errors);
        plan.requests.extend(overflow_plan.requests);
    }

    if changes.is_empty() || plan.requests.len() >= input.limits.max_jobs {
        return Ok(plan);
    }

    let root_path = input.root.path;
    let root_path_norm = input.root.path_norm;
    let total_changes = changes.len();
    let filtered: Vec<ScanFilesystemEvent> = changes
        .into_iter()
        .filter(|event| {
            if event.path.is_dir() {
                return true;
            }
            input.file_filters.is_media_file_path(&event.path)
        })
        .collect();
    plan.dropped_events += total_changes.saturating_sub(filtered.len());

    let mut targets = BTreeSet::new();
    for event in &filtered {
        if let Some(target) = scan_target_under_root(&root_path, &event.path) {
            targets.insert(target);
        }
    }

    for folder_path_norm in targets {
        if plan.requests.len() >= input.limits.max_jobs {
            break;
        }
        let context = build_root_scan_context(
            input.library,
            &root_path_norm,
            folder_path_norm,
        )?;
        plan.requests.push(folder_scan_enqueue_request(
            context,
            JobPriority::P0,
            ScanReason::HotChange,
            burst_correlation,
            input.now,
        ));
    }

    Ok(plan)
}

async fn plan_root_child_folder_scans(
    library: &LibraryReference,
    roots: &[ScanPlanningRoot],
    priority: JobPriority,
    reason: ScanReason,
    correlation_id: Option<Uuid>,
    limits: ScanPlanningLimits,
    now: DateTime<Utc>,
) -> Result<ScanWorkPlan> {
    let enumeration =
        enumerate_first_level_folders(roots, limits.max_root_entries).await;
    let mut plan = ScanWorkPlan {
        skipped_entries: enumeration.skipped_entries,
        errors: enumeration.errors,
        ..ScanWorkPlan::empty()
    };

    for entry in enumeration.folders {
        if plan.requests.len() >= limits.max_jobs {
            break;
        }
        let context = build_root_scan_context(
            library,
            &entry.root_path_norm,
            entry.folder_path_norm,
        )?;
        plan.requests.push(folder_scan_enqueue_request(
            context,
            priority,
            reason,
            correlation_id,
            now,
        ));
    }

    Ok(plan)
}

/// Construct the queue request for one folder scan unit.
pub fn folder_scan_enqueue_request(
    context: FolderScanContext,
    priority: JobPriority,
    reason: ScanReason,
    correlation_id: Option<Uuid>,
    now: DateTime<Utc>,
) -> EnqueueRequest {
    let job = FolderScanJob {
        context,
        scan_reason: reason,
        enqueue_time: now,
        device_id: None,
    };
    let mut request =
        EnqueueRequest::new(priority, JobPayload::FolderScan(job));
    request.requested_at = now;
    request.correlation_id = correlation_id;
    request
}

/// Build the supported folder scan unit for a root child under a library root.
pub fn build_root_scan_context(
    library: &LibraryReference,
    library_root_path_norm: &str,
    folder_path_norm: String,
) -> Result<FolderScanContext> {
    match library.library_type {
        crate::types::library::LibraryType::Movies => {
            let movie_root_path = MovieRootPath::try_new_under_library_root(
                library_root_path_norm,
                folder_path_norm,
            )?;
            Ok(FolderScanContext::Movie(MovieFolderScanContext {
                library_id: library.id,
                movie_root_path,
            }))
        }
        crate::types::library::LibraryType::Series => {
            let series_root_path = SeriesRootPath::try_new_under_library_root(
                library_root_path_norm,
                folder_path_norm,
            )?;
            Ok(FolderScanContext::Series(SeriesFolderScanContext {
                library_id: library.id,
                series_root_path,
            }))
        }
    }
}

async fn enumerate_first_level_folders(
    roots: &[ScanPlanningRoot],
    max_root_entries: usize,
) -> RootFolderEnumeration {
    let mut enumeration = RootFolderEnumeration::default();
    let mut visited_entries = 0usize;

    'roots: for root in roots {
        let mut rd = match fs::read_dir(&root.path_norm).await {
            Ok(rd) => rd,
            Err(err) => {
                enumeration.skipped_entries += 1;
                let message = format!(
                    "failed to enumerate root {}: {err}",
                    root.path_norm
                );
                warn!(
                    target: "scan::planning",
                    root_id = root.root_id,
                    path = %root.path_norm,
                    error = %err,
                    "skipping directory due to read_dir error"
                );
                enumeration.errors.push(message);
                continue;
            }
        };

        loop {
            if visited_entries >= max_root_entries {
                break 'roots;
            }

            let entry = match rd.next_entry().await {
                Ok(Some(entry)) => entry,
                Ok(None) => break,
                Err(err) => {
                    enumeration.skipped_entries += 1;
                    let message = format!(
                        "failed to read entry under {}: {err}",
                        root.path_norm
                    );
                    warn!(
                        target: "scan::planning",
                        root_id = root.root_id,
                        path = %root.path_norm,
                        error = %err,
                        "skipping entry due to read_dir error"
                    );
                    enumeration.errors.push(message);
                    continue;
                }
            };
            visited_entries += 1;

            let name = entry.file_name();
            if name.to_string_lossy().starts_with('.') {
                continue;
            }

            match entry.metadata().await {
                Ok(meta) if meta.is_dir() => {
                    if let Ok(folder_path_norm) = normalize_path(&entry.path())
                    {
                        enumeration.folders.push(RootFolderPlanEntry {
                            root_path_norm: root.path_norm.clone(),
                            folder_path_norm,
                        });
                    }
                }
                Ok(_) => {}
                Err(err) => {
                    enumeration.skipped_entries += 1;
                    let message = format!(
                        "failed to read metadata for {}: {err}",
                        entry.path().display()
                    );
                    warn!(
                        target: "scan::planning",
                        root_id = root.root_id,
                        path = %entry.path().display(),
                        error = %err,
                        "skipping entry due to metadata error"
                    );
                    enumeration.errors.push(message);
                }
            }
        }
    }

    enumeration
}

fn scan_target_under_root(root_path: &Path, path: &Path) -> Option<String> {
    if path.as_os_str().is_empty() {
        return None;
    }

    let candidate = if path.is_dir() { path } else { path.parent()? };

    if !candidate.starts_with(root_path) {
        return None;
    }

    let rel = candidate.strip_prefix(root_path).ok()?;
    let child = rel.components().next()?;

    let mut target = root_path.to_path_buf();
    target.push(child.as_os_str());
    normalize_path(&target).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{LibraryType, ids::LibraryId};

    fn test_library(root: PathBuf) -> LibraryReference {
        LibraryReference {
            id: LibraryId(Uuid::from_u128(0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa)),
            name: "Test".into(),
            library_type: LibraryType::Movies,
            paths: vec![root],
        }
    }

    fn folder_jobs(plan: &ScanWorkPlan) -> Vec<&FolderScanJob> {
        plan.requests
            .iter()
            .filter_map(|request| match &request.payload {
                JobPayload::FolderScan(job) => Some(job),
                _ => None,
            })
            .collect()
    }

    fn event(
        path: PathBuf,
        kind: ScanFilesystemEventKind,
    ) -> ScanFilesystemEvent {
        ScanFilesystemEvent {
            correlation_id: None,
            path,
            kind,
        }
    }

    #[tokio::test]
    async fn bulk_seed_plans_root_child_folder_requests() -> Result<()> {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().to_path_buf();
        let movie = root.join("Seed Movie");
        std::fs::create_dir_all(&movie).expect("movie dir");
        std::fs::write(root.join("root-video.mkv"), b"fixture")
            .expect("root media file");
        let library = test_library(root.clone());
        let correlation = Uuid::now_v7();
        let now = Utc::now();
        let roots = [ScanPlanningRoot::new(0, root)];

        let plan = plan_library_start(LibraryStartPlanningInput {
            library: &library,
            roots: &roots,
            mode: ScanStartPlanningMode::Bulk,
            correlation_id: Some(correlation),
            limits: ScanPlanningLimits::new(8, 8),
            now,
        })
        .await?;

        assert_eq!(plan.planned_jobs(), 1);
        assert_eq!(plan.requests[0].priority, JobPriority::P1);
        assert_eq!(plan.requests[0].requested_at, now);
        assert_eq!(plan.requests[0].correlation_id, Some(correlation));
        let jobs = folder_jobs(&plan);
        assert_eq!(jobs[0].scan_reason, ScanReason::BulkSeed);
        assert_eq!(jobs[0].enqueue_time, now);
        assert_eq!(jobs[0].context.folder_path_norm(), normalize_path(&movie)?);

        Ok(())
    }

    #[tokio::test]
    async fn watcher_burst_filters_and_coalesces_to_folder_scan_units()
    -> Result<()> {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().to_path_buf();
        let movie = root.join("Changed Movie");
        std::fs::create_dir_all(&movie).expect("movie dir");
        let media = movie.join("feature.mkv");
        let sidecar = movie.join("poster.jpg");
        std::fs::write(&media, b"fixture").expect("media file");
        std::fs::write(&sidecar, b"fixture").expect("sidecar file");
        let library = test_library(root.clone());
        let command_correlation = Uuid::now_v7();
        let event_correlation = Uuid::now_v7();
        let roots = ScanPlanningRoot::new(0, root.clone());
        let now = Utc::now();

        let plan = plan_fs_event_burst(FsEventPlanningInput {
            library: &library,
            root: roots,
            events: vec![
                ScanFilesystemEvent {
                    correlation_id: Some(event_correlation),
                    path: media.clone(),
                    kind: ScanFilesystemEventKind::Created,
                },
                event(media.clone(), ScanFilesystemEventKind::Modified),
                event(sidecar, ScanFilesystemEventKind::Modified),
            ],
            command_correlation_id: Some(command_correlation),
            state_correlation_id: None,
            file_filters: &ScannerFileFilterPolicy::default(),
            limits: ScanPlanningLimits::new(8, 8),
            now,
        })
        .await?;

        assert_eq!(plan.planned_jobs(), 1);
        assert_eq!(plan.dropped_events, 1);
        assert_eq!(plan.requests[0].priority, JobPriority::P0);
        assert_eq!(plan.requests[0].correlation_id, Some(command_correlation));
        let jobs = folder_jobs(&plan);
        assert_eq!(jobs[0].scan_reason, ScanReason::HotChange);
        assert_eq!(jobs[0].context.folder_path_norm(), normalize_path(&movie)?);

        Ok(())
    }

    #[tokio::test]
    async fn overflow_falls_back_to_root_child_folder_planning() -> Result<()> {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().to_path_buf();
        let movie = root.join("Overflow Movie");
        std::fs::create_dir_all(&movie).expect("movie dir");
        let library = test_library(root.clone());
        let state_correlation = Uuid::now_v7();
        let now = Utc::now();

        let plan = plan_fs_event_burst(FsEventPlanningInput {
            library: &library,
            root: ScanPlanningRoot::new(0, root),
            events: vec![event(
                PathBuf::from("overflow"),
                ScanFilesystemEventKind::Overflow,
            )],
            command_correlation_id: None,
            state_correlation_id: Some(state_correlation),
            file_filters: &ScannerFileFilterPolicy::default(),
            limits: ScanPlanningLimits::new(8, 8),
            now,
        })
        .await?;

        assert_eq!(plan.planned_jobs(), 1);
        assert_eq!(plan.requests[0].priority, JobPriority::P0);
        assert_eq!(plan.requests[0].correlation_id, Some(state_correlation));
        let jobs = folder_jobs(&plan);
        assert_eq!(jobs[0].scan_reason, ScanReason::WatcherOverflow);
        assert_eq!(jobs[0].context.folder_path_norm(), normalize_path(&movie)?);

        Ok(())
    }
}
