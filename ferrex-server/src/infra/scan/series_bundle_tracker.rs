use std::collections::{HashMap, HashSet};

use ferrex_core::{
    domain::scan::{
        AnalyzeScanHierarchy,
        actors::index::IndexingOutcome,
        actors::{FolderScanSummary, MediaFileDiscovered},
        orchestration::context::{
            FolderScanContext, SeasonFolderPath, SeasonLink, SeriesLink,
            SeriesRootPath,
        },
        orchestration::events::{JobEvent, JobEventPayload},
        orchestration::job::JobKind,
    },
    types::{LibraryId, SeriesID},
};
use ferrex_model::{EpisodeID, MediaID};

#[derive(Debug, Clone)]
pub struct SeriesBundleFinalization {
    pub library_id: LibraryId,
    pub series_id: SeriesID,
    pub series_root_path: SeriesRootPath,
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
                progress
                    .expected_season_folders
                    .insert(ctx.season_folder_path.clone());
                progress.expected_season_numbers.insert(ctx.season_number);
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
                progress.root_scan_completed = true;
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
                progress
                    .completed_season_folders
                    .insert(ctx.season_folder_path.clone());
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

        progress.expected_episode_ids.insert(episode_id);
        if let Some(episode_path) =
            EpisodeFilePathNorm::try_new(event.path_norm.clone())
        {
            progress.expected_episode_paths.insert(episode_path);
        }
        if let Some(season_number) = SeasonNumber::from_link(&hierarchy.season)
        {
            progress.expected_season_numbers.insert(season_number.0);
        }
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

                if let Some(series_id) =
                    SeriesIdResolution::from_link(&hierarchy.series)
                {
                    progress.series_id = Some(series_id);
                }
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

                if let Some(series_id) =
                    SeriesIdResolution::from_link(&hierarchy.series)
                {
                    progress.series_id = Some(series_id);
                }

                if let Some(season_number) =
                    SeasonNumber::from_link(&hierarchy.season)
                {
                    progress.indexed_season_numbers.insert(season_number.0);
                }
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

                if let Some(series_id) =
                    SeriesIdResolution::from_link(&hierarchy.series)
                {
                    progress.series_id = Some(series_id);
                }

                progress.indexed_episode_ids.insert(episode_id);
                if let Some(episode_path) =
                    EpisodeFilePathNorm::try_new(outcome.path_norm.clone())
                {
                    progress.completed_episode_paths.insert(episode_path);
                }

                if let Some(season_number) =
                    SeasonNumber::from_link(&hierarchy.season)
                {
                    progress.indexed_season_numbers.insert(season_number.0);
                }
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

        match &event.payload {
            JobEventPayload::Enqueued { kind, .. } => {
                if matches!(
                    kind,
                    JobKind::MediaAnalyze
                        | JobKind::EpisodeMatch
                        | JobKind::MetadataEnrich
                        | JobKind::IndexUpsert
                ) {
                    progress.expected_episode_paths.insert(episode_path);
                }
            }
            JobEventPayload::Merged { kind, .. } => {
                if matches!(
                    kind,
                    JobKind::MediaAnalyze
                        | JobKind::EpisodeMatch
                        | JobKind::MetadataEnrich
                        | JobKind::IndexUpsert
                ) {
                    progress.expected_episode_paths.insert(episode_path);
                }
            }
            JobEventPayload::Completed { kind, .. } => {
                // Index-upsert is the end of the episode pipeline; earlier stages
                // must not count as episode completion.
                if *kind == JobKind::IndexUpsert {
                    progress.completed_episode_paths.insert(episode_path);
                }
            }
            JobEventPayload::DeadLettered { .. } => {
                // Dead-letter at any stage is terminal for this episode file.
                progress.completed_episode_paths.insert(episode_path);
            }
            JobEventPayload::Failed { retryable, .. } => {
                // Non-retryable failures are terminal for this episode file.
                if !retryable {
                    progress.completed_episode_paths.insert(episode_path);
                }
            }
            JobEventPayload::Dequeued { .. }
            | JobEventPayload::LeaseRenewed { .. }
            | JobEventPayload::LeaseExpired { .. }
            | JobEventPayload::ThroughputTick { .. } => {}
        }
    }

    pub fn finalization_candidates(&self) -> Vec<SeriesBundleFinalization> {
        let mut out = Vec::new();

        for progress in self.by_root.values() {
            if progress.finalized {
                continue;
            }

            if !progress.discovery_complete() {
                continue;
            }

            if !progress.seasons_complete() {
                continue;
            }

            if !progress.episodes_complete() {
                continue;
            }

            let Some(series_id) = progress.series_id else {
                continue;
            };

            out.push(SeriesBundleFinalization {
                library_id: progress.library_id,
                series_id,
                series_root_path: progress.series_root_path.clone(),
            });
        }

        out
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
    completed_episode_paths: HashSet<EpisodeFilePathNorm>,
    finalized: bool,
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
            completed_episode_paths: HashSet::new(),
            finalized: false,
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
            return self
                .expected_episode_paths
                .is_subset(&self.completed_episode_paths);
        }
        self.expected_episode_ids
            .is_subset(&self.indexed_episode_ids)
    }
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
    use chrono::Utc;
    use ferrex_core::domain::scan::actors::MediaKindHint;
    use ferrex_core::domain::scan::orchestration::ScanReason;
    use ferrex_core::domain::scan::orchestration::context::{
        EpisodeLink, EpisodeScanHierarchy, FolderScanContext, ScanNodeKind,
        SeasonFolderScanContext, SeasonScanHierarchy, SeriesFolderScanContext,
    };
    use ferrex_core::domain::scan::orchestration::job::MediaFingerprint;
    use ferrex_model::{LibraryId as ModelLibraryId, VideoMediaType};
    use uuid::Uuid;

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
                series: SeriesLink::Hint(ferrex_core::domain::scan::orchestration::context::SeriesHint {
                    title: "Example".into(),
                    slug: None,
                    year: None,
                    region: None,
                }),
                season: SeasonLink::Number(1),
                episode: EpisodeLink::Hint(ferrex_core::domain::scan::orchestration::context::EpisodeHint {
                    number: 1,
                    title: None,
                }),
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
            completed_at: Utc::now(),
        });

        let series_id = SeriesID(Uuid::from_u128(3));
        let indexed = IndexingOutcome {
            library_id: ModelLibraryId(library_id.0),
            path_norm: discovered.path_norm.clone(),
            media_id: MediaID::Episode(episode_id),
            hierarchy: AnalyzeScanHierarchy::Episode(EpisodeScanHierarchy {
                series_root_path: series_root.clone(),
                series: SeriesLink::Resolved(ferrex_core::domain::scan::orchestration::context::SeriesRef {
                    id: series_id,
                    slug: None,
                    title: Some("Example".into()),
                }),
                season: SeasonLink::Number(1),
                episode: EpisodeLink::Resolved(ferrex_core::domain::scan::orchestration::context::EpisodeRef {
                    id: episode_id,
                    number: Some(1),
                    title: None,
                }),
            }),
            indexed_at: Utc::now(),
            upserted: true,
            media: None,
            change: ferrex_core::domain::scan::actors::index::IndexingChange::Created,
        };
        tracker.observe_indexed(&indexed);

        // Season indexed (required for finalization).
        tracker.observe_indexed(&IndexingOutcome {
            library_id: ModelLibraryId(library_id.0),
            path_norm: "/demo/Shows/Example/Season 1".into(),
            media_id: MediaID::Season(ferrex_model::SeasonID(Uuid::from_u128(9))),
            hierarchy: AnalyzeScanHierarchy::Season(SeasonScanHierarchy {
                series_root_path: series_root.clone(),
                series: SeriesLink::Resolved(
                    ferrex_core::domain::scan::orchestration::context::SeriesRef {
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
            change:
                ferrex_core::domain::scan::actors::index::IndexingChange::Created,
        });

        let finalized = tracker.finalization_candidates();
        assert_eq!(finalized.len(), 1);
        assert_eq!(finalized[0].library_id, library_id);
        assert_eq!(finalized[0].series_id, series_id);

        tracker.mark_finalized(&series_root);

        // Only yields once once marked.
        assert!(tracker.finalization_candidates().is_empty());
    }
}
