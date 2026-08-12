use std::collections::{HashMap, HashSet};

use crate::{
    domain::scan::{
        actors::index::IndexingOutcome,
        actors::{FolderScanSummary, MediaFileDiscovered},
        orchestration::context::{
            FolderScanContext, SeasonFolderPath, SeasonLink, SeriesLink,
            SeriesRootPath,
        },
        orchestration::events::{JobEvent, JobEventPayload},
        orchestration::{
            job::{AnalyzeScanHierarchy, JobKind, JobState},
            queue::DurableJobState,
        },
    },
    types::{LibraryId, SeriesID},
};
use chrono::{DateTime, Utc};
use ferrex_model::{EpisodeID, MediaID};

#[derive(Debug, Clone)]
pub struct SeriesBundleFinalization {
    pub library_id: LibraryId,
    pub series_id: SeriesID,
    pub series_root_path: SeriesRootPath,
    generation: u64,
}

#[derive(Debug, Default)]
pub struct SeriesBundleTracker {
    by_root: HashMap<SeriesRootPath, SeriesBundleProgress>,
}

impl SeriesBundleTracker {
    pub fn observe_folder_discovered(&mut self, context: &FolderScanContext) {
        match context {
            FolderScanContext::Series(ctx) => {
                self.by_root
                    .entry(ctx.series_root_path.clone())
                    .or_insert_with(|| {
                        SeriesBundleProgress::new(
                            ctx.library_id,
                            ctx.series_root_path.clone(),
                        )
                    });
            }
            FolderScanContext::Season(ctx) => {
                let progress = self
                    .by_root
                    .entry(ctx.series_root_path.clone())
                    .or_insert_with(|| {
                        SeriesBundleProgress::new(
                            ctx.library_id,
                            ctx.series_root_path.clone(),
                        )
                    });
                let folder_added = progress
                    .expected_season_folders
                    .insert(ctx.season_folder_path.clone());
                let number_added =
                    progress.expected_season_numbers.insert(ctx.season_number);
                progress.bump_generation_if(folder_added || number_added);
            }
            FolderScanContext::Movie(_) => {}
        }
    }

    pub fn observe_folder_scan_completed(
        &mut self,
        summary: &FolderScanSummary,
    ) {
        match &summary.context {
            FolderScanContext::Series(ctx) => {
                let progress = self
                    .by_root
                    .entry(ctx.series_root_path.clone())
                    .or_insert_with(|| {
                        SeriesBundleProgress::new(
                            ctx.library_id,
                            ctx.series_root_path.clone(),
                        )
                    });
                let changed = !progress.root_scan_completed;
                progress.root_scan_completed = true;
                progress.bump_generation_if(changed);
            }
            FolderScanContext::Season(ctx) => {
                let progress = self
                    .by_root
                    .entry(ctx.series_root_path.clone())
                    .or_insert_with(|| {
                        SeriesBundleProgress::new(
                            ctx.library_id,
                            ctx.series_root_path.clone(),
                        )
                    });
                let changed = progress
                    .completed_season_folders
                    .insert(ctx.season_folder_path.clone());
                progress.bump_generation_if(changed);
            }
            FolderScanContext::Movie(_) => {}
        }
    }

    pub fn observe_media_discovered(&mut self, event: &MediaFileDiscovered) {
        if event.variant != ferrex_model::VideoMediaType::Episode {
            return;
        }

        let AnalyzeScanHierarchy::Episode(hierarchy) = &event.hierarchy else {
            return;
        };

        let MediaID::Episode(episode_id) = event.media_id else {
            return;
        };

        let progress = self
            .by_root
            .entry(hierarchy.series_root_path.clone())
            .or_insert_with(|| {
                SeriesBundleProgress::new(
                    event.library_id,
                    hierarchy.series_root_path.clone(),
                )
            });

        let mut changed = progress.expected_episode_ids.insert(episode_id);
        if let Some(episode_path) =
            EpisodeFilePathNorm::try_new(event.path_norm.clone())
        {
            changed |= progress.expected_episode_paths.insert(episode_path);
        }
        if let Some(season_number) = SeasonNumber::from_link(&hierarchy.season)
        {
            changed |= progress.expected_season_numbers.insert(season_number.0);
        }
        progress.bump_generation_if(changed);
    }

    pub fn observe_indexed(&mut self, outcome: &IndexingOutcome) {
        match &outcome.hierarchy {
            AnalyzeScanHierarchy::Series(hierarchy) => {
                let progress = self
                    .by_root
                    .entry(hierarchy.series_root_path.clone())
                    .or_insert_with(|| {
                        SeriesBundleProgress::new(
                            outcome.library_id,
                            hierarchy.series_root_path.clone(),
                        )
                    });

                let changed = if let Some(series_id) =
                    SeriesIdResolution::from_link(&hierarchy.series)
                {
                    progress.update_series_id(series_id)
                } else {
                    false
                };
                progress.bump_generation_if(changed);
            }
            AnalyzeScanHierarchy::Season(hierarchy) => {
                let progress = self
                    .by_root
                    .entry(hierarchy.series_root_path.clone())
                    .or_insert_with(|| {
                        SeriesBundleProgress::new(
                            outcome.library_id,
                            hierarchy.series_root_path.clone(),
                        )
                    });

                let mut changed = if let Some(series_id) =
                    SeriesIdResolution::from_link(&hierarchy.series)
                {
                    progress.update_series_id(series_id)
                } else {
                    false
                };

                if let Some(season_number) =
                    SeasonNumber::from_link(&hierarchy.season)
                {
                    changed |=
                        progress.indexed_season_numbers.insert(season_number.0);
                }
                progress.bump_generation_if(changed);
            }
            AnalyzeScanHierarchy::Episode(hierarchy) => {
                let MediaID::Episode(episode_id) = outcome.media_id else {
                    return;
                };

                let progress = self
                    .by_root
                    .entry(hierarchy.series_root_path.clone())
                    .or_insert_with(|| {
                        SeriesBundleProgress::new(
                            outcome.library_id,
                            hierarchy.series_root_path.clone(),
                        )
                    });

                let mut changed = if let Some(series_id) =
                    SeriesIdResolution::from_link(&hierarchy.series)
                {
                    progress.update_series_id(series_id)
                } else {
                    false
                };

                changed |= progress.indexed_episode_ids.insert(episode_id);
                if let Some(episode_path) =
                    EpisodeFilePathNorm::try_new(outcome.path_norm.clone())
                {
                    changed |= progress
                        .episode_jobs_by_path
                        .entry(episode_path)
                        .or_default()
                        .observe_catalog_indexed();
                }

                if let Some(season_number) =
                    SeasonNumber::from_link(&hierarchy.season)
                {
                    changed |=
                        progress.indexed_season_numbers.insert(season_number.0);
                }
                progress.bump_generation_if(changed);
            }
            AnalyzeScanHierarchy::Movie(_) => {}
        }
    }

    pub fn observe_job_event(&mut self, event: &JobEvent) {
        let Some(path_key) = &event.meta.path_key else {
            return;
        };
        let path_norm = match path_key {
            ferrex_model::SubjectKey::Path(path) => path.as_str(),
            ferrex_model::SubjectKey::Opaque(_) => return,
        };

        let Some(series_root_path) =
            SeriesRootPath::try_from_episode_file_path(path_norm).ok()
        else {
            return;
        };

        let Some(episode_path) =
            EpisodeFilePathNorm::try_new(path_norm.to_string())
        else {
            return;
        };

        let progress = self
            .by_root
            .entry(series_root_path.clone())
            .or_insert_with(|| {
                SeriesBundleProgress::new(
                    event.meta.library_id,
                    series_root_path,
                )
            });

        let changed = match &event.payload {
            JobEventPayload::Enqueued { job_id, kind, .. }
                if is_episode_pipeline_kind(*kind) =>
            {
                let expected = progress
                    .expected_episode_paths
                    .insert(episode_path.clone());
                let enrolled = progress
                    .episode_jobs_by_path
                    .entry(episode_path)
                    .or_default()
                    .enroll(*job_id, *kind);
                expected || enrolled
            }
            JobEventPayload::Merged {
                existing_job_id,
                kind,
                ..
            } if is_episode_pipeline_kind(*kind) => {
                let expected = progress
                    .expected_episode_paths
                    .insert(episode_path.clone());
                let enrolled = progress
                    .episode_jobs_by_path
                    .entry(episode_path)
                    .or_default()
                    .enroll(*existing_job_id, *kind);
                expected || enrolled
            }
            JobEventPayload::Dequeued { job_id, kind, .. }
                if is_episode_pipeline_kind(*kind) =>
            {
                let expected = progress
                    .expected_episode_paths
                    .insert(episode_path.clone());
                let active = progress
                    .episode_jobs_by_path
                    .entry(episode_path)
                    .or_default()
                    .observe_transition(
                        *job_id,
                        *kind,
                        EpisodeJobStatus::Active,
                    );
                expected || active
            }
            JobEventPayload::Completed { job_id, kind, .. }
                if is_episode_pipeline_kind(*kind) =>
            {
                let expected = progress
                    .expected_episode_paths
                    .insert(episode_path.clone());
                let status = if *kind == JobKind::IndexUpsert {
                    EpisodeJobStatus::Complete
                } else {
                    EpisodeJobStatus::StageCompleted
                };
                let terminal = progress
                    .episode_jobs_by_path
                    .entry(episode_path)
                    .or_default()
                    .observe_transition(*job_id, *kind, status);
                expected || terminal
            }
            JobEventPayload::DeadLettered { job_id, kind, .. }
                if is_episode_pipeline_kind(*kind) =>
            {
                let expected = progress
                    .expected_episode_paths
                    .insert(episode_path.clone());
                let terminal = progress
                    .episode_jobs_by_path
                    .entry(episode_path)
                    .or_default()
                    .observe_transition(
                        *job_id,
                        *kind,
                        EpisodeJobStatus::Complete,
                    );
                expected || terminal
            }
            JobEventPayload::Failed {
                job_id,
                kind,
                retryable,
                ..
            } if is_episode_pipeline_kind(*kind) => {
                let expected = progress
                    .expected_episode_paths
                    .insert(episode_path.clone());
                let status = if *retryable {
                    EpisodeJobStatus::Active
                } else {
                    EpisodeJobStatus::Complete
                };
                let transitioned = progress
                    .episode_jobs_by_path
                    .entry(episode_path)
                    .or_default()
                    .observe_transition(*job_id, *kind, status);
                expected || transitioned
            }
            JobEventPayload::LeaseRenewed { job_id, .. }
            | JobEventPayload::LeaseExpired { job_id, .. } => progress
                .episode_jobs_by_path
                .get_mut(&episode_path)
                .is_some_and(|path| path.observe_active_existing(*job_id)),
            JobEventPayload::Enqueued { .. }
            | JobEventPayload::Merged { .. }
            | JobEventPayload::Dequeued { .. }
            | JobEventPayload::Completed { .. }
            | JobEventPayload::DeadLettered { .. }
            | JobEventPayload::Failed { .. }
            | JobEventPayload::ThroughputTick { .. } => false,
        };
        progress.bump_generation_if(changed);
    }

    /// Rebuild finalization-relevant episode pipeline state from PostgreSQL.
    ///
    /// Broadcast job events are only hints. When a consumer lags, the durable
    /// snapshot restores episode enrollment, terminal paths, and the compact
    /// identity emitted by completed index jobs without retaining job payloads
    /// in the tracker.
    pub fn reconcile_durable_job_states(
        &mut self,
        library_id: LibraryId,
        jobs: &[DurableJobState],
    ) {
        let mut latest_by_path: HashMap<
            (SeriesRootPath, EpisodeFilePathNorm),
            &DurableJobState,
        > = HashMap::new();

        for job in jobs {
            if !is_episode_pipeline_kind(job.kind) {
                continue;
            }

            let Some(identity) = &job.series_identity else {
                continue;
            };
            let episode_path =
                job.path_key.as_ref().and_then(|key| match key {
                    ferrex_model::SubjectKey::Path(path) => {
                        EpisodeFilePathNorm::try_new(path.to_string())
                    }
                    ferrex_model::SubjectKey::Opaque(_) => None,
                });

            let progress = self
                .by_root
                .entry(identity.series_root_path.clone())
                .or_insert_with(|| {
                    SeriesBundleProgress::new(
                        library_id,
                        identity.series_root_path.clone(),
                    )
                });
            if progress.library_id != library_id {
                continue;
            }

            let mut changed = false;
            if let Some(episode_path) = &episode_path {
                changed |= progress
                    .expected_episode_paths
                    .insert(episode_path.clone());
                let key =
                    (identity.series_root_path.clone(), episode_path.clone());
                let replace = latest_by_path
                    .get(&key)
                    .is_none_or(|current| durable_job_is_newer(job, current));
                if replace {
                    latest_by_path.insert(key, job);
                }
            }

            if job.kind == JobKind::IndexUpsert
                && job.state == JobState::Completed
            {
                if let Some(series_id) = identity.series_id {
                    changed |= progress.update_series_id(series_id);
                }
                if let Some(season_number) = identity.season_number {
                    changed |=
                        progress.indexed_season_numbers.insert(season_number);
                }
                if let Some(MediaID::Episode(episode_id)) = job.media_id {
                    changed |= progress.indexed_episode_ids.insert(episode_id);
                }
                if let Some(episode_path) = episode_path {
                    changed |= progress
                        .episode_jobs_by_path
                        .entry(episode_path)
                        .or_default()
                        .observe_catalog_indexed();
                }
            }

            progress.bump_generation_if(changed);
        }

        // Only the newest durable job generation for an episode path may
        // decide whether that path is complete. Historical terminal rows stay
        // useful for catalog identity, but a later active generation reopens
        // the path and invalidates a finalization claim.
        for ((series_root_path, episode_path), job) in latest_by_path {
            let Some(progress) = self.by_root.get_mut(&series_root_path) else {
                continue;
            };
            if progress.library_id != library_id {
                continue;
            }
            let changed = progress
                .episode_jobs_by_path
                .entry(episode_path)
                .or_default()
                .reconcile_durable(job);
            progress.bump_generation_if(changed);
        }
    }

    pub fn finalization_candidates(&self) -> Vec<SeriesBundleFinalization> {
        let mut out = Vec::new();

        for progress in self.by_root.values() {
            if !progress.ready_for_finalization() {
                continue;
            }

            let Some(series_id) = progress.series_id else {
                continue;
            };

            out.push(SeriesBundleFinalization {
                library_id: progress.library_id,
                series_id,
                series_root_path: progress.series_root_path.clone(),
                generation: progress.generation,
            });
        }

        out
    }

    /// Marks a claimed bundle finalized only if it is still the same eligible
    /// bundle after the caller's asynchronous publication work completes.
    ///
    /// Folder discovery and episode enrollment can race publication. Rechecking
    /// here prevents a stale claim from hiding work that arrived while the
    /// publication was in flight.
    pub fn mark_finalized_if_still_eligible(
        &mut self,
        candidate: &SeriesBundleFinalization,
    ) -> bool {
        let Some(progress) = self.by_root.get_mut(&candidate.series_root_path)
        else {
            return false;
        };

        if progress.library_id != candidate.library_id
            || progress.series_id != Some(candidate.series_id)
            || progress.generation != candidate.generation
            || !progress.ready_for_finalization()
        {
            return false;
        }

        progress.finalized = true;
        true
    }

    pub fn mark_finalized(&mut self, series_root_path: &SeriesRootPath) {
        let Some(progress) = self.by_root.get_mut(series_root_path) else {
            return;
        };
        progress.finalized = true;
    }

    pub fn clear(&mut self) {
        self.by_root.clear();
    }
}

#[derive(Debug)]
struct SeriesBundleProgress {
    library_id: LibraryId,
    series_root_path: SeriesRootPath,
    series_id: Option<SeriesID>,
    root_scan_completed: bool,
    expected_season_folders: HashSet<SeasonFolderPath>,
    completed_season_folders: HashSet<SeasonFolderPath>,
    expected_season_numbers: HashSet<u16>,
    indexed_season_numbers: HashSet<u16>,
    expected_episode_ids: HashSet<EpisodeID>,
    indexed_episode_ids: HashSet<EpisodeID>,
    expected_episode_paths: HashSet<EpisodeFilePathNorm>,
    episode_jobs_by_path: HashMap<EpisodeFilePathNorm, EpisodePathProgress>,
    finalized: bool,
    generation: u64,
}

impl SeriesBundleProgress {
    fn new(library_id: LibraryId, series_root_path: SeriesRootPath) -> Self {
        Self {
            library_id,
            series_root_path,
            series_id: None,
            root_scan_completed: false,
            expected_season_folders: HashSet::new(),
            completed_season_folders: HashSet::new(),
            expected_season_numbers: HashSet::new(),
            indexed_season_numbers: HashSet::new(),
            expected_episode_ids: HashSet::new(),
            indexed_episode_ids: HashSet::new(),
            expected_episode_paths: HashSet::new(),
            episode_jobs_by_path: HashMap::new(),
            finalized: false,
            generation: 0,
        }
    }

    fn discovery_complete(&self) -> bool {
        if !self.root_scan_completed {
            return false;
        }
        self.completed_season_folders.len()
            == self.expected_season_folders.len()
            && self
                .expected_season_folders
                .is_subset(&self.completed_season_folders)
    }

    fn seasons_complete(&self) -> bool {
        self.expected_season_numbers
            .is_subset(&self.indexed_season_numbers)
    }

    fn episodes_complete(&self) -> bool {
        if !self.expected_episode_paths.is_empty() {
            return self.expected_episode_paths.iter().all(|path| {
                self.episode_jobs_by_path
                    .get(path)
                    .is_some_and(EpisodePathProgress::is_complete)
            });
        }
        self.expected_episode_ids
            .is_subset(&self.indexed_episode_ids)
    }

    fn ready_for_finalization(&self) -> bool {
        !self.finalized
            && self.discovery_complete()
            && self.seasons_complete()
            && self.episodes_complete()
            && self.series_id.is_some()
    }

    fn update_series_id(&mut self, series_id: SeriesID) -> bool {
        if self.series_id == Some(series_id) {
            return false;
        }
        self.series_id = Some(series_id);
        true
    }

    fn bump_generation_if(&mut self, changed: bool) {
        if changed {
            self.generation = self.generation.saturating_add(1);
        }
    }
}

#[derive(Debug, Default)]
struct EpisodePathProgress {
    /// Catalog indexing outcomes do not carry a job ID. They are valid
    /// completion evidence only until a concrete job generation is observed.
    catalog_indexed: bool,
    seen_job_ids: HashSet<crate::domain::scan::orchestration::job::JobId>,
    latest_job: Option<EpisodeJobGeneration>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct EpisodeJobGeneration {
    job_id: crate::domain::scan::orchestration::job::JobId,
    kind: JobKind,
    status: EpisodeJobStatus,
    durable_order: Option<DurableEpisodeJobOrder>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum EpisodeJobStatus {
    Active,
    StageCompleted,
    Complete,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct DurableEpisodeJobOrder {
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    job_id: crate::domain::scan::orchestration::job::JobId,
}

impl DurableEpisodeJobOrder {
    fn from_job(job: &DurableJobState) -> Self {
        Self {
            created_at: job.created_at,
            updated_at: job.updated_at,
            job_id: job.job_id,
        }
    }

    fn is_newer_than(&self, other: &Self) -> bool {
        self.created_at > other.created_at
            || (self.created_at == other.created_at
                && (self.updated_at > other.updated_at
                    || (self.updated_at == other.updated_at
                        && self.job_id.0 > other.job_id.0)))
    }
}

impl EpisodePathProgress {
    fn is_complete(&self) -> bool {
        self.latest_job
            .as_ref()
            .map(|job| job.status == EpisodeJobStatus::Complete)
            .unwrap_or(self.catalog_indexed)
    }

    fn observe_catalog_indexed(&mut self) -> bool {
        if self.catalog_indexed {
            return false;
        }
        self.catalog_indexed = true;
        true
    }

    fn enroll(
        &mut self,
        job_id: crate::domain::scan::orchestration::job::JobId,
        kind: JobKind,
    ) -> bool {
        if !self.seen_job_ids.insert(job_id) {
            return false;
        }

        self.latest_job = Some(EpisodeJobGeneration {
            job_id,
            kind,
            status: EpisodeJobStatus::Active,
            durable_order: None,
        });
        true
    }

    fn observe_transition(
        &mut self,
        job_id: crate::domain::scan::orchestration::job::JobId,
        kind: JobKind,
        status: EpisodeJobStatus,
    ) -> bool {
        if let Some(latest) = self.latest_job.as_mut()
            && latest.job_id == job_id
        {
            let changed = latest.kind != kind || latest.status != status;
            latest.kind = kind;
            latest.status = status;
            return changed;
        }

        if !self.seen_job_ids.insert(job_id) {
            return false;
        }

        let replace = self
            .latest_job
            .as_ref()
            .is_none_or(|latest| job_id.0 > latest.job_id.0);
        if !replace {
            return false;
        }

        self.latest_job = Some(EpisodeJobGeneration {
            job_id,
            kind,
            status,
            durable_order: None,
        });
        true
    }

    fn observe_active_existing(
        &mut self,
        job_id: crate::domain::scan::orchestration::job::JobId,
    ) -> bool {
        let Some(latest) = self.latest_job.as_mut() else {
            return false;
        };
        if latest.job_id != job_id || latest.status == EpisodeJobStatus::Active
        {
            return false;
        }
        latest.status = EpisodeJobStatus::Active;
        true
    }

    fn reconcile_durable(&mut self, job: &DurableJobState) -> bool {
        let durable_order = DurableEpisodeJobOrder::from_job(job);
        let status = durable_episode_job_status(job);

        if let Some(latest) = self.latest_job.as_mut()
            && latest.job_id == job.job_id
        {
            if latest
                .durable_order
                .as_ref()
                .is_some_and(|current| !durable_order.is_newer_than(current))
            {
                return false;
            }
            let changed = latest.kind != job.kind || latest.status != status;
            latest.kind = job.kind;
            latest.status = status;
            latest.durable_order = Some(durable_order);
            return changed;
        }

        let replace = match self.latest_job.as_ref() {
            None => true,
            Some(latest) => match latest.durable_order.as_ref() {
                Some(current) => durable_order.is_newer_than(current),
                None => {
                    !self.seen_job_ids.contains(&job.job_id)
                        && job.job_id.0 > latest.job_id.0
                }
            },
        };
        self.seen_job_ids.insert(job.job_id);
        if !replace {
            return false;
        }

        self.latest_job = Some(EpisodeJobGeneration {
            job_id: job.job_id,
            kind: job.kind,
            status,
            durable_order: Some(durable_order),
        });
        true
    }
}

fn durable_episode_job_status(job: &DurableJobState) -> EpisodeJobStatus {
    if job.is_terminal_failure()
        || (job.kind == JobKind::IndexUpsert
            && job.state == JobState::Completed)
    {
        EpisodeJobStatus::Complete
    } else if job.state == JobState::Completed {
        EpisodeJobStatus::StageCompleted
    } else {
        EpisodeJobStatus::Active
    }
}

fn is_episode_pipeline_kind(kind: JobKind) -> bool {
    matches!(
        kind,
        JobKind::MediaAnalyze
            | JobKind::EpisodeMatch
            | JobKind::MetadataEnrich
            | JobKind::IndexUpsert
    )
}

fn durable_job_is_newer(
    candidate: &DurableJobState,
    current: &DurableJobState,
) -> bool {
    DurableEpisodeJobOrder::from_job(candidate)
        .is_newer_than(&DurableEpisodeJobOrder::from_job(current))
}

#[derive(Debug, Clone, Copy)]
struct SeasonNumber(u16);

impl SeasonNumber {
    fn from_link(link: &SeasonLink) -> Option<Self> {
        match link {
            SeasonLink::Number(value) => Some(SeasonNumber(*value)),
            SeasonLink::Resolved(reference) => {
                reference.number.map(SeasonNumber)
            }
        }
    }
}

struct SeriesIdResolution;

impl SeriesIdResolution {
    fn from_link(link: &SeriesLink) -> Option<SeriesID> {
        match link {
            SeriesLink::Resolved(reference) => Some(reference.id),
            SeriesLink::Hint(_) => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct EpisodeFilePathNorm(String);

impl EpisodeFilePathNorm {
    fn try_new(path_norm: String) -> Option<Self> {
        if SeriesRootPath::try_from_episode_file_path(&path_norm).is_ok() {
            Some(Self(path_norm))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::scan::actors::{FolderScanOutcome, MediaKindHint};
    use crate::domain::scan::orchestration::ScanReason;
    use crate::domain::scan::orchestration::context::{
        EpisodeLink, EpisodeScanHierarchy, FolderScanContext, ScanNodeKind,
        SeasonFolderScanContext, SeasonScanHierarchy, SeriesFolderScanContext,
    };
    use crate::domain::scan::orchestration::job::{JobId, MediaFingerprint};
    use crate::domain::scan::orchestration::queue::DurableSeriesIdentity;
    use chrono::Utc;
    use ferrex_model::{LibraryId as ModelLibraryId, VideoMediaType};
    use uuid::Uuid;

    fn pending_durable_episode_bundle() -> (
        SeriesBundleTracker,
        LibraryId,
        SeriesRootPath,
        SeasonFolderScanContext,
        EpisodeID,
        SeriesID,
        String,
    ) {
        let library_id = LibraryId(Uuid::from_u128(101));
        let series_id = SeriesID(Uuid::from_u128(102));
        let episode_id = EpisodeID(Uuid::from_u128(103));
        let series_root =
            SeriesRootPath::try_new("/demo/Shows/Durable").unwrap();
        let (season_folder_path, season_number) =
            SeasonFolderPath::try_new_under_series_root(
                &series_root,
                "/demo/Shows/Durable/Season 1",
            )
            .unwrap();
        let series_context = SeriesFolderScanContext {
            library_id,
            series_root_path: series_root.clone(),
        };
        let season_context = SeasonFolderScanContext {
            library_id,
            series_root_path: series_root.clone(),
            season_folder_path,
            season_number,
        };
        let episode_path =
            "/demo/Shows/Durable/Season 1/S01E01.mkv".to_string();
        let mut tracker = SeriesBundleTracker::default();
        tracker.observe_folder_discovered(&FolderScanContext::Series(
            series_context.clone(),
        ));
        tracker.observe_folder_discovered(&FolderScanContext::Season(
            season_context.clone(),
        ));
        tracker.observe_media_discovered(&MediaFileDiscovered {
            library_id: ModelLibraryId(library_id.0),
            path_norm: episode_path.clone(),
            fingerprint: MediaFingerprint::default(),
            classified_as: MediaKindHint::Episode,
            media_id: MediaID::Episode(episode_id),
            variant: VideoMediaType::Episode,
            node: ScanNodeKind::EpisodeFile,
            hierarchy: AnalyzeScanHierarchy::Episode(EpisodeScanHierarchy {
                series_root_path: series_root.clone(),
                series: SeriesLink::Hint(
                    crate::domain::scan::orchestration::context::SeriesHint {
                        title: "Durable".into(),
                        slug: None,
                        year: None,
                        region: None,
                    },
                ),
                season: SeasonLink::Number(season_number),
                episode: EpisodeLink::Hint(
                    crate::domain::scan::orchestration::context::EpisodeHint {
                        number: 1,
                        title: None,
                    },
                ),
            }),
            context: FolderScanContext::Season(season_context.clone()),
            scan_reason: ScanReason::BulkSeed,
        });
        tracker.observe_folder_scan_completed(&FolderScanSummary {
            context: FolderScanContext::Season(season_context.clone()),
            discovered_files: 1,
            enqueued_subfolders: 0,
            listing_hash: "durable-season".into(),
            outcome: FolderScanOutcome::Changed,
            completed_at: Utc::now(),
        });
        tracker.observe_folder_scan_completed(&FolderScanSummary {
            context: FolderScanContext::Series(series_context),
            discovered_files: 0,
            enqueued_subfolders: 1,
            listing_hash: "durable-root".into(),
            outcome: FolderScanOutcome::Changed,
            completed_at: Utc::now(),
        });

        (
            tracker,
            library_id,
            series_root,
            season_context,
            episode_id,
            series_id,
            episode_path,
        )
    }

    fn durable_series_job(
        kind: JobKind,
        state: JobState,
        series_root_path: SeriesRootPath,
        series_id: Option<SeriesID>,
        season_number: Option<u16>,
        path: &str,
        media_id: Option<MediaID>,
    ) -> DurableJobState {
        let now = Utc::now();
        DurableJobState {
            job_id: JobId::new(),
            kind,
            media_id,
            indexing_change: None,
            series_identity: Some(DurableSeriesIdentity {
                series_root_path,
                series_id,
                season_number,
            }),
            state,
            attempts: 1,
            dedupe_key: format!("durable:{kind:?}:{path}"),
            correlation_id: Some(Uuid::now_v7()),
            path_key: ferrex_model::SubjectKey::path(path).ok(),
            last_error: None,
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn drains_finalized_once_series_discovery_and_episodes_done() {
        let library_id = LibraryId(Uuid::from_u128(1));
        let series_root =
            SeriesRootPath::try_new("/demo/Shows/Example").unwrap();
        let (season_folder, season_number) =
            SeasonFolderPath::try_new_under_series_root(
                &series_root,
                "/demo/Shows/Example/Season 1",
            )
            .unwrap();

        let series_ctx = SeriesFolderScanContext {
            library_id,
            series_root_path: series_root.clone(),
        };
        let season_ctx = SeasonFolderScanContext {
            library_id,
            series_root_path: series_root.clone(),
            season_folder_path: season_folder.clone(),
            season_number,
        };

        let mut tracker = SeriesBundleTracker::default();
        tracker
            .observe_folder_discovered(&FolderScanContext::Series(series_ctx));
        tracker.observe_folder_discovered(&FolderScanContext::Season(
            season_ctx.clone(),
        ));

        let episode_id = EpisodeID(Uuid::from_u128(2));
        let discovered = MediaFileDiscovered {
            library_id: ModelLibraryId(library_id.0),
            path_norm: "/demo/Shows/Example/Season 1/S01E01.mkv".into(),
            fingerprint: MediaFingerprint::default(),
            classified_as: MediaKindHint::Episode,
            media_id: MediaID::Episode(episode_id),
            variant: VideoMediaType::Episode,
            node: ScanNodeKind::EpisodeFile,
            hierarchy: AnalyzeScanHierarchy::Episode(EpisodeScanHierarchy {
                series_root_path: series_root.clone(),
                series: SeriesLink::Hint(
                    crate::domain::scan::orchestration::context::SeriesHint {
                        title: "Example".into(),
                        slug: None,
                        year: None,
                        region: None,
                    },
                ),
                season: SeasonLink::Number(1),
                episode: EpisodeLink::Hint(
                    crate::domain::scan::orchestration::context::EpisodeHint {
                        number: 1,
                        title: None,
                    },
                ),
            }),
            context: FolderScanContext::Season(season_ctx.clone()),
            scan_reason: ScanReason::BulkSeed,
        };
        tracker.observe_media_discovered(&discovered);

        tracker.observe_folder_scan_completed(&FolderScanSummary {
            context: FolderScanContext::Season(season_ctx.clone()),
            discovered_files: 1,
            enqueued_subfolders: 0,
            listing_hash: "abc".into(),
            outcome: FolderScanOutcome::Changed,
            completed_at: Utc::now(),
        });
        tracker.observe_folder_scan_completed(&FolderScanSummary {
            context: FolderScanContext::Series(SeriesFolderScanContext {
                library_id,
                series_root_path: series_root.clone(),
            }),
            discovered_files: 0,
            enqueued_subfolders: 1,
            listing_hash: "def".into(),
            outcome: FolderScanOutcome::Changed,
            completed_at: Utc::now(),
        });

        let series_id = SeriesID(Uuid::from_u128(3));
        let indexed = IndexingOutcome {
            library_id: ModelLibraryId(library_id.0),
            path_norm: discovered.path_norm.clone(),
            media_id: MediaID::Episode(episode_id),
            hierarchy: AnalyzeScanHierarchy::Episode(EpisodeScanHierarchy {
                series_root_path: series_root.clone(),
                series: SeriesLink::Resolved(
                    crate::domain::scan::orchestration::context::SeriesRef {
                        id: series_id,
                        slug: None,
                        title: Some("Example".into()),
                    },
                ),
                season: SeasonLink::Number(1),
                episode: EpisodeLink::Resolved(
                    crate::domain::scan::orchestration::context::EpisodeRef {
                        id: episode_id,
                        number: Some(1),
                        title: None,
                    },
                ),
            }),
            indexed_at: Utc::now(),
            upserted: true,
            media: None,
            change: crate::domain::scan::actors::index::IndexingChange::Created,
        };
        tracker.observe_indexed(&indexed);

        // Season indexed (required for finalization).
        tracker.observe_indexed(&IndexingOutcome {
            library_id: ModelLibraryId(library_id.0),
            path_norm: "/demo/Shows/Example/Season 1".into(),
            media_id: MediaID::Season(ferrex_model::SeasonID(Uuid::from_u128(
                9,
            ))),
            hierarchy: AnalyzeScanHierarchy::Season(SeasonScanHierarchy {
                series_root_path: series_root.clone(),
                series: SeriesLink::Resolved(
                    crate::domain::scan::orchestration::context::SeriesRef {
                        id: series_id,
                        slug: None,
                        title: Some("Example".into()),
                    },
                ),
                season: SeasonLink::Number(1),
            }),
            indexed_at: Utc::now(),
            upserted: true,
            media: None,
            change: crate::domain::scan::actors::index::IndexingChange::Created,
        });

        let finalized = tracker.finalization_candidates();
        assert_eq!(finalized.len(), 1);
        assert_eq!(finalized[0].library_id, library_id);
        assert_eq!(finalized[0].series_id, series_id);

        // Replaying an already-observed durable outcome is idempotent and must
        // not invalidate the generation captured by the candidate.
        tracker.observe_indexed(&indexed);
        assert!(tracker.mark_finalized_if_still_eligible(&finalized[0]));

        // Only yields once once marked.
        assert!(tracker.finalization_candidates().is_empty());
    }

    #[test]
    fn terminal_episode_failure_can_complete_bundle_after_root_and_season() {
        let library_id = LibraryId(Uuid::from_u128(10));
        let series_id = SeriesID(Uuid::from_u128(11));
        let series_root =
            SeriesRootPath::try_new("/demo/Shows/Terminal").unwrap();
        let (season_folder, season_number) =
            SeasonFolderPath::try_new_under_series_root(
                &series_root,
                "/demo/Shows/Terminal/Season 1",
            )
            .unwrap();
        let series_ctx = SeriesFolderScanContext {
            library_id,
            series_root_path: series_root.clone(),
        };
        let season_ctx = SeasonFolderScanContext {
            library_id,
            series_root_path: series_root.clone(),
            season_folder_path: season_folder,
            season_number,
        };
        let episode_path = "/demo/Shows/Terminal/Season 1/S01E01.mkv";
        let mut tracker = SeriesBundleTracker::default();

        tracker.observe_folder_discovered(&FolderScanContext::Series(
            series_ctx.clone(),
        ));
        tracker.observe_folder_discovered(&FolderScanContext::Season(
            season_ctx.clone(),
        ));
        assert!(tracker.finalization_candidates().is_empty());

        tracker.observe_job_event(&JobEvent::from_job(
            None,
            library_id,
            "episode:index".into(),
            Some(ferrex_model::SubjectKey::path(episode_path).unwrap()),
            JobEventPayload::Enqueued {
                job_id: crate::domain::scan::orchestration::job::JobId::new(),
                kind: crate::domain::scan::orchestration::job::JobKind::IndexUpsert,
                priority: crate::domain::scan::orchestration::job::JobPriority::P0,
            },
        ));
        tracker.observe_folder_scan_completed(&FolderScanSummary {
            context: FolderScanContext::Series(series_ctx),
            discovered_files: 0,
            enqueued_subfolders: 1,
            listing_hash: "root".into(),
            outcome: FolderScanOutcome::Changed,
            completed_at: Utc::now(),
        });
        tracker.observe_folder_scan_completed(&FolderScanSummary {
            context: FolderScanContext::Season(season_ctx.clone()),
            discovered_files: 1,
            enqueued_subfolders: 0,
            listing_hash: "season".into(),
            outcome: FolderScanOutcome::Changed,
            completed_at: Utc::now(),
        });
        assert!(tracker.finalization_candidates().is_empty());

        tracker.observe_indexed(&IndexingOutcome {
            library_id: ModelLibraryId(library_id.0),
            path_norm: "/demo/Shows/Terminal/Season 1".into(),
            media_id: MediaID::Season(ferrex_model::SeasonID(Uuid::from_u128(
                12,
            ))),
            hierarchy: AnalyzeScanHierarchy::Season(SeasonScanHierarchy {
                series_root_path: series_root.clone(),
                series: SeriesLink::Resolved(
                    crate::domain::scan::orchestration::context::SeriesRef {
                        id: series_id,
                        slug: Some("terminal".into()),
                        title: Some("Terminal".into()),
                    },
                ),
                season: SeasonLink::Number(1),
            }),
            indexed_at: Utc::now(),
            upserted: true,
            media: None,
            change: crate::domain::scan::actors::index::IndexingChange::Created,
        });
        assert!(tracker.finalization_candidates().is_empty());

        tracker.observe_job_event(&JobEvent::from_job(
            None,
            library_id,
            "episode:index".into(),
            Some(ferrex_model::SubjectKey::path(episode_path).unwrap()),
            JobEventPayload::DeadLettered {
                job_id: crate::domain::scan::orchestration::job::JobId::new(),
                kind: crate::domain::scan::orchestration::job::JobKind::IndexUpsert,
                priority: crate::domain::scan::orchestration::job::JobPriority::P0,
            },
        ));

        let finalized = tracker.finalization_candidates();
        assert_eq!(finalized.len(), 1);
        assert_eq!(finalized[0].series_id, series_id);
        tracker.mark_finalized(&series_root);
        assert!(tracker.finalization_candidates().is_empty());
    }

    #[test]
    fn durable_completed_index_restores_dropped_bundle_terminal_event() {
        let (
            mut tracker,
            library_id,
            series_root,
            _season_context,
            episode_id,
            series_id,
            episode_path,
        ) = pending_durable_episode_bundle();
        assert!(tracker.finalization_candidates().is_empty());

        let completed_index = durable_series_job(
            JobKind::IndexUpsert,
            JobState::Completed,
            series_root,
            Some(series_id),
            Some(1),
            &episode_path,
            Some(MediaID::Episode(episode_id)),
        );
        tracker.reconcile_durable_job_states(
            library_id,
            std::slice::from_ref(&completed_index),
        );

        let candidates = tracker.finalization_candidates();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].series_id, series_id);
        let generation = candidates[0].generation;

        // Re-reading the same authoritative snapshot must not invalidate a
        // claim by advancing the tracker generation.
        tracker.reconcile_durable_job_states(library_id, &[completed_index]);
        assert_eq!(tracker.finalization_candidates()[0].generation, generation);
    }

    #[test]
    fn durable_dead_letter_restores_dropped_bundle_terminal_event() {
        let (
            mut tracker,
            library_id,
            series_root,
            season_context,
            _episode_id,
            series_id,
            episode_path,
        ) = pending_durable_episode_bundle();

        // The catalog already has enough series/season identity to publish the
        // bundle. Only the episode pipeline terminal notification is dropped.
        tracker.observe_indexed(&IndexingOutcome {
            library_id: ModelLibraryId(library_id.0),
            path_norm: season_context.season_folder_path.as_str().to_string(),
            media_id: MediaID::Season(ferrex_model::SeasonID(Uuid::from_u128(
                104,
            ))),
            hierarchy: AnalyzeScanHierarchy::Season(SeasonScanHierarchy {
                series_root_path: series_root.clone(),
                series: SeriesLink::Resolved(
                    crate::domain::scan::orchestration::context::SeriesRef {
                        id: series_id,
                        slug: None,
                        title: Some("Durable".into()),
                    },
                ),
                season: SeasonLink::Number(1),
            }),
            indexed_at: Utc::now(),
            upserted: true,
            media: None,
            change: crate::domain::scan::actors::index::IndexingChange::Created,
        });
        assert!(tracker.finalization_candidates().is_empty());

        let dead_letter = durable_series_job(
            JobKind::MetadataEnrich,
            JobState::DeadLetter,
            series_root,
            Some(series_id),
            Some(1),
            &episode_path,
            None,
        );
        tracker.reconcile_durable_job_states(library_id, &[dead_letter]);

        let candidates = tracker.finalization_candidates();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].series_id, series_id);
    }

    #[test]
    fn same_path_reenrollment_during_publish_invalidates_stale_claim() {
        let (
            mut tracker,
            library_id,
            series_root,
            _season_context,
            episode_id,
            series_id,
            episode_path,
        ) = pending_durable_episode_bundle();
        let completed_index = durable_series_job(
            JobKind::IndexUpsert,
            JobState::Completed,
            series_root.clone(),
            Some(series_id),
            Some(1),
            &episode_path,
            Some(MediaID::Episode(episode_id)),
        );
        tracker.reconcile_durable_job_states(library_id, &[completed_index]);
        let stale_claim = tracker
            .finalization_candidates()
            .into_iter()
            .next()
            .expect("the original path generation is complete");

        let replacement_job_id = JobId::new();
        let reenqueued = JobEvent::from_job(
            None,
            library_id,
            "episode:same-path:index:new".into(),
            ferrex_model::SubjectKey::path(&episode_path).ok(),
            JobEventPayload::Enqueued {
                job_id: replacement_job_id,
                kind: JobKind::IndexUpsert,
                priority:
                    crate::domain::scan::orchestration::job::JobPriority::P0,
            },
        );
        tracker.observe_job_event(&reenqueued);

        assert!(
            tracker.finalization_candidates().is_empty(),
            "a new job ID for the same episode path must reopen the bundle"
        );
        assert!(
            !tracker.mark_finalized_if_still_eligible(&stale_claim),
            "the pre-reenrollment publication claim must be rejected"
        );
        let generation_after_reenrollment =
            tracker.by_root[&series_root].generation;

        tracker.observe_job_event(&reenqueued);
        assert_eq!(
            tracker.by_root[&series_root].generation,
            generation_after_reenrollment,
            "replaying the same job enrollment must be idempotent"
        );

        tracker.observe_job_event(&JobEvent::from_job(
            None,
            library_id,
            "episode:same-path:index:new".into(),
            ferrex_model::SubjectKey::path(&episode_path).ok(),
            JobEventPayload::DeadLettered {
                job_id: replacement_job_id,
                kind: JobKind::IndexUpsert,
                priority:
                    crate::domain::scan::orchestration::job::JobPriority::P0,
            },
        ));
        let replacement_claim = tracker
            .finalization_candidates()
            .into_iter()
            .next()
            .expect("the replacement terminal generation is publishable");
        assert!(replacement_claim.generation > stale_claim.generation);
    }

    #[test]
    fn durable_new_active_generation_reopens_old_completed_path() {
        let (
            mut tracker,
            library_id,
            series_root,
            _season_context,
            episode_id,
            series_id,
            episode_path,
        ) = pending_durable_episode_bundle();
        let now = Utc::now();
        let mut old_completed = durable_series_job(
            JobKind::IndexUpsert,
            JobState::Completed,
            series_root.clone(),
            Some(series_id),
            Some(1),
            &episode_path,
            Some(MediaID::Episode(episode_id)),
        );
        old_completed.created_at = now - chrono::Duration::seconds(2);
        old_completed.updated_at = now - chrono::Duration::seconds(1);

        let mut new_active = durable_series_job(
            JobKind::MediaAnalyze,
            JobState::Leased,
            series_root.clone(),
            Some(series_id),
            Some(1),
            &episode_path,
            None,
        );
        new_active.created_at = now;
        new_active.updated_at = now;

        // Reverse input order to prove reconciliation selects by durable job
        // generation, not iteration order or set union.
        tracker.reconcile_durable_job_states(
            library_id,
            &[new_active.clone(), old_completed.clone()],
        );
        assert!(
            tracker.finalization_candidates().is_empty(),
            "the newer active row must override historical completion for the path"
        );
        let active_generation = tracker.by_root[&series_root].generation;

        tracker.reconcile_durable_job_states(
            library_id,
            &[old_completed.clone(), new_active.clone()],
        );
        assert_eq!(
            tracker.by_root[&series_root].generation, active_generation,
            "replaying the same durable snapshot must be idempotent"
        );

        new_active.state = JobState::DeadLetter;
        new_active.updated_at = now + chrono::Duration::seconds(1);
        tracker.reconcile_durable_job_states(
            library_id,
            &[old_completed, new_active],
        );
        let candidate =
            tracker.finalization_candidates().into_iter().next().expect(
                "the newest terminal durable generation completes the path",
            );
        assert_eq!(candidate.series_id, series_id);
    }
}
