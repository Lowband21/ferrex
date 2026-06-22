use std::{fmt, sync::Arc};

use async_trait::async_trait;

use super::resolver::{SeriesResolution, SeriesResolverPort};

use crate::{
    domain::scan::orchestration::{
        context::{
            EpisodeScanHierarchy, SeriesHint, SeriesLink, SeriesRef,
            SeriesRootPath,
        },
        job::{DependencyKey, SeriesResolveJob},
        series_state::{
            SeriesScanState, SeriesScanStateRepository, SeriesScanStatus,
        },
    },
    error::{MediaError, Result},
    types::ids::LibraryId,
};

/// Releases the queue barrier that keeps episode work behind unresolved series
/// identity.
#[async_trait]
pub trait SeriesDependencyReleaser: Send + Sync {
    async fn release_series_root_dependency(
        &self,
        library_id: LibraryId,
        series_root_path: &SeriesRootPath,
    ) -> Result<()>;
}

/// Result of recording a series-root discovery.
#[derive(Clone, Debug)]
pub struct SeriesDiscoveryOutcome {
    state: SeriesScanState,
}

impl SeriesDiscoveryOutcome {
    fn new(state: SeriesScanState) -> Self {
        Self { state }
    }

    pub fn state(&self) -> &SeriesScanState {
        &self.state
    }

    /// Root discovery must enqueue resolution unless the root is already in a
    /// resolved state. Resolved roots are authoritative and are never demoted by
    /// later discovery or seed observations.
    pub fn should_enqueue_resolution(&self) -> bool {
        !matches!(self.state.status, SeriesScanStatus::Resolved)
    }
}

/// Coordinator decision for an episode whose analysis did not yet carry a
/// resolved `SeriesRef`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EpisodeDependencyDecision {
    /// Series identity is already known; metadata enrichment may be enqueued
    /// immediately with this resolved hierarchy.
    Ready(EpisodeScanHierarchy),
    /// Series identity is not ready; enqueue EpisodeMatch behind this queue
    /// dependency so it cannot run until SeriesResolve releases the barrier.
    Deferred { dependency_key: DependencyKey },
}

/// Boundary for scan-side series coordination.
///
/// Invariants guarded here:
/// - discovered/seeded observations must not demote an already resolved series
///   root;
/// - episode metadata may only advance with a resolved `SeriesRef`;
/// - unresolved episode work is gated by the `series_root` dependency key;
/// - terminal series-resolution failures mark the root failed before the queue
///   dependency is released.
#[derive(Clone)]
pub struct SeriesCoordinator {
    states: Arc<Box<dyn SeriesScanStateRepository>>,
    resolver: Arc<dyn SeriesResolverPort>,
}

impl fmt::Debug for SeriesCoordinator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SeriesCoordinator")
            .field("states", &"SeriesScanStateRepository")
            .field("resolver", &"SeriesResolverPort")
            .finish()
    }
}

impl SeriesCoordinator {
    pub fn new(
        states: Arc<Box<dyn SeriesScanStateRepository>>,
        resolver: Arc<dyn SeriesResolverPort>,
    ) -> Self {
        Self { states, resolver }
    }

    /// Record that a series root was discovered by folder scanning.
    pub async fn record_root_discovery(
        &self,
        library_id: LibraryId,
        series_root_path: SeriesRootPath,
        hint: Option<SeriesHint>,
    ) -> Result<SeriesDiscoveryOutcome> {
        let state = self
            .states
            .mark_discovered(library_id, series_root_path, hint)
            .await?;
        Ok(SeriesDiscoveryOutcome::new(state))
    }

    /// Record an episode's unresolved series root and decide whether it can
    /// advance immediately or must wait behind the series-root dependency.
    pub async fn prepare_episode_dependency(
        &self,
        library_id: LibraryId,
        hierarchy: &EpisodeScanHierarchy,
    ) -> Result<EpisodeDependencyDecision> {
        let state = self
            .states
            .mark_discovered(
                library_id,
                hierarchy.series_root_path.clone(),
                series_hint(&hierarchy.series),
            )
            .await?;

        if let Some(series_ref) = resolved_series_ref(&state) {
            let mut hierarchy = hierarchy.clone();
            hierarchy.series = SeriesLink::Resolved(series_ref);
            return Ok(EpisodeDependencyDecision::Ready(hierarchy));
        }

        Ok(EpisodeDependencyDecision::Deferred {
            dependency_key: DependencyKey::series_root(
                &hierarchy.series_root_path,
            ),
        })
    }

    /// Resolve the series reference required by an EpisodeMatch job.
    pub async fn resolve_episode_dependency(
        &self,
        library_id: LibraryId,
        hierarchy: &EpisodeScanHierarchy,
    ) -> Result<EpisodeScanHierarchy> {
        let state = self
            .resolver
            .get_state(library_id, &hierarchy.series_root_path)
            .await?;

        let Some(state) = state else {
            return Err(MediaError::InvalidMedia(
                "episode match missing series state".into(),
            ));
        };

        let Some(series_id) = state.series_id else {
            return Err(MediaError::InvalidMedia(
                "episode match missing resolved series id".into(),
            ));
        };

        if !matches!(state.status, SeriesScanStatus::Resolved) {
            return Err(MediaError::InvalidMedia(
                "episode match executed before series resolved".into(),
            ));
        }

        let mut hierarchy = hierarchy.clone();
        hierarchy.series = SeriesLink::Resolved(SeriesRef {
            id: series_id,
            slug: state.hint.as_ref().and_then(|hint| hint.slug.clone()),
            title: state.hint.as_ref().map(|hint| hint.title.clone()),
        });
        Ok(hierarchy)
    }

    /// Run the provider-backed series resolution stage.
    pub async fn resolve_series(
        &self,
        job: &SeriesResolveJob,
    ) -> Result<SeriesResolution> {
        self.resolver.resolve(job).await
    }

    /// Mark a terminal SeriesResolve failure in series state.
    pub async fn record_resolution_failure(
        &self,
        job: &SeriesResolveJob,
        reason: String,
    ) -> Result<()> {
        self.resolver
            .mark_failed(job.library_id, job.series_root_path.clone(), reason)
            .await
    }

    /// Release episode work blocked on this series root.
    pub async fn release_blocked_episode_dependencies(
        &self,
        releaser: &dyn SeriesDependencyReleaser,
        library_id: LibraryId,
        series_root_path: &SeriesRootPath,
    ) -> Result<()> {
        releaser
            .release_series_root_dependency(library_id, series_root_path)
            .await
    }
}

fn series_hint(link: &SeriesLink) -> Option<SeriesHint> {
    match link {
        SeriesLink::Hint(hint) => Some(hint.clone()),
        SeriesLink::Resolved(_) => None,
    }
}

fn resolved_series_ref(state: &SeriesScanState) -> Option<SeriesRef> {
    if !matches!(state.status, SeriesScanStatus::Resolved) {
        return None;
    }

    let series_id = state.series_id?;
    Some(SeriesRef {
        id: series_id,
        slug: state.hint.as_ref().and_then(|hint| hint.slug.clone()),
        title: state.hint.as_ref().map(|hint| hint.title.clone()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::scan::orchestration::context::{
        EpisodeHint, EpisodeLink, ScanNodeKind, SeriesRef,
    };
    use crate::domain::scan::orchestration::job::{
        AnalyzeScanHierarchy, MediaFingerprint, ScanReason,
    };
    use crate::domain::scan::orchestration::series::SeriesResolution;
    use crate::domain::scan::orchestration::series_state::{
        InMemorySeriesScanStateRepository, SeriesScanState,
    };
    use crate::types::ids::{EpisodeID, SeriesID};
    use ferrex_model::{MediaID, VideoMediaType};
    use tokio::sync::Mutex;
    use uuid::Uuid;

    #[derive(Clone)]
    struct NoopSeriesResolver {
        states: Arc<Box<dyn SeriesScanStateRepository>>,
    }

    #[async_trait]
    impl SeriesResolverPort for NoopSeriesResolver {
        async fn resolve(
            &self,
            _job: &SeriesResolveJob,
        ) -> Result<SeriesResolution> {
            Err(MediaError::InvalidMedia(
                "noop series resolver cannot resolve series".into(),
            ))
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

    #[derive(Default)]
    struct RecordingReleaser {
        releases: Mutex<Vec<(LibraryId, SeriesRootPath)>>,
    }

    #[async_trait]
    impl SeriesDependencyReleaser for RecordingReleaser {
        async fn release_series_root_dependency(
            &self,
            library_id: LibraryId,
            series_root_path: &SeriesRootPath,
        ) -> Result<()> {
            self.releases
                .lock()
                .await
                .push((library_id, series_root_path.clone()));
            Ok(())
        }
    }

    fn lib(id: u128) -> LibraryId {
        LibraryId(Uuid::from_u128(id))
    }

    fn root(path: &str) -> SeriesRootPath {
        SeriesRootPath::try_new(path).expect("valid series root")
    }

    fn hint(title: &str) -> SeriesHint {
        SeriesHint {
            title: title.into(),
            slug: Some(title.to_lowercase()),
            year: None,
            region: None,
        }
    }

    fn episode_hierarchy(
        series_root_path: SeriesRootPath,
    ) -> EpisodeScanHierarchy {
        EpisodeScanHierarchy {
            series_root_path,
            series: SeriesLink::Hint(hint("Example")),
            season:
                crate::domain::scan::orchestration::context::SeasonLink::Number(
                    1,
                ),
            episode: EpisodeLink::Hint(EpisodeHint {
                number: 1,
                title: None,
            }),
        }
    }

    fn coordinator_with_states()
    -> (SeriesCoordinator, Arc<Box<dyn SeriesScanStateRepository>>) {
        let states: Arc<Box<dyn SeriesScanStateRepository>> =
            Arc::new(Box::new(InMemorySeriesScanStateRepository::default()));
        let resolver: Arc<dyn SeriesResolverPort> =
            Arc::new(NoopSeriesResolver {
                states: states.clone(),
            });
        (SeriesCoordinator::new(states.clone(), resolver), states)
    }

    #[tokio::test]
    async fn root_discovery_does_not_demote_resolved_state() {
        let (coordinator, states) = coordinator_with_states();
        let library_id = lib(1);
        let series_root = root("/demo/Shows/Resolved");
        let series_ref = SeriesRef {
            id: SeriesID(Uuid::from_u128(2)),
            slug: Some("resolved".into()),
            title: Some("Resolved".into()),
        };

        states
            .mark_resolved(library_id, series_root.clone(), series_ref.clone())
            .await
            .expect("mark resolved");

        let outcome = coordinator
            .record_root_discovery(
                library_id,
                series_root.clone(),
                Some(hint("Rediscovered")),
            )
            .await
            .expect("record discovery");

        assert!(!outcome.should_enqueue_resolution());
        assert_eq!(outcome.state().series_id, Some(series_ref.id));
        assert_eq!(outcome.state().status, SeriesScanStatus::Resolved);
    }

    #[tokio::test]
    async fn already_resolved_episode_advances_without_dependency() {
        let (coordinator, states) = coordinator_with_states();
        let library_id = lib(3);
        let series_root = root("/demo/Shows/Ready");
        let series_id = SeriesID(Uuid::from_u128(4));
        states
            .mark_resolved(
                library_id,
                series_root.clone(),
                SeriesRef {
                    id: series_id,
                    slug: Some("ready".into()),
                    title: Some("Ready".into()),
                },
            )
            .await
            .expect("mark resolved");

        let decision = coordinator
            .prepare_episode_dependency(
                library_id,
                &episode_hierarchy(series_root.clone()),
            )
            .await
            .expect("prepare episode dependency");

        let EpisodeDependencyDecision::Ready(hierarchy) = decision else {
            panic!("resolved series should advance episode metadata");
        };
        let SeriesLink::Resolved(series_ref) = hierarchy.series else {
            panic!("episode hierarchy should be resolved");
        };
        assert_eq!(series_ref.id, series_id);
    }

    #[tokio::test]
    async fn unresolved_episode_is_gated_by_series_root_dependency() {
        let (coordinator, _states) = coordinator_with_states();
        let library_id = lib(5);
        let series_root = root("/demo/Shows/Pending");

        let decision = coordinator
            .prepare_episode_dependency(
                library_id,
                &episode_hierarchy(series_root.clone()),
            )
            .await
            .expect("prepare episode dependency");

        assert_eq!(
            decision,
            EpisodeDependencyDecision::Deferred {
                dependency_key: DependencyKey::series_root(&series_root),
            }
        );
    }

    #[tokio::test]
    async fn episode_match_rejects_missing_series_state() {
        let (coordinator, _states) = coordinator_with_states();
        let err = coordinator
            .resolve_episode_dependency(
                lib(6),
                &episode_hierarchy(root("/demo/Shows/Missing")),
            )
            .await
            .expect_err("missing state should fail");

        assert!(
            err.to_string()
                .contains("episode match missing series state")
        );
    }

    #[tokio::test]
    async fn episode_match_rejects_unresolved_series_id() {
        let (coordinator, states) = coordinator_with_states();
        let library_id = lib(60);
        let series_root = root("/demo/Shows/Unresolved");
        states
            .mark_discovered(
                library_id,
                series_root.clone(),
                Some(hint("Unresolved")),
            )
            .await
            .expect("mark discovered");

        let err = coordinator
            .resolve_episode_dependency(
                library_id,
                &episode_hierarchy(series_root),
            )
            .await
            .expect_err("unresolved series should fail");

        assert!(
            err.to_string()
                .contains("episode match missing resolved series id")
        );
    }

    #[tokio::test]
    async fn terminal_failure_marks_failed_and_dependency_release_is_scoped() {
        let (coordinator, states) = coordinator_with_states();
        let library_id = lib(7);
        let series_root = root("/demo/Shows/Failure");
        let job = SeriesResolveJob {
            library_id,
            series_root_path: series_root.clone(),
            hint: Some(hint("Failure")),
            folder_name: "Failure".into(),
            scan_reason: ScanReason::BulkSeed,
        };

        coordinator
            .record_resolution_failure(&job, "provider returned 404".into())
            .await
            .expect("record failure");
        let state = states
            .get(library_id, &series_root)
            .await
            .expect("state lookup")
            .expect("failed state exists");
        assert_eq!(state.status, SeriesScanStatus::Failed);
        assert_eq!(
            state.failure_reason.as_deref(),
            Some("provider returned 404")
        );

        let releaser = RecordingReleaser::default();
        coordinator
            .release_blocked_episode_dependencies(
                &releaser,
                library_id,
                &series_root,
            )
            .await
            .expect("release dependency");

        assert_eq!(
            releaser.releases.lock().await.as_slice(),
            &[(library_id, series_root)]
        );
    }

    #[test]
    fn episode_metadata_enrich_validation_rejects_unresolved_series() {
        let library_id = lib(8);
        let series_root = root("/demo/Shows/Unresolved");
        let job = crate::domain::scan::orchestration::job::MetadataEnrichJob {
            library_id,
            media_id: MediaID::Episode(EpisodeID(Uuid::from_u128(9))),
            variant: VideoMediaType::Episode,
            hierarchy: AnalyzeScanHierarchy::Episode(episode_hierarchy(
                series_root,
            )),
            node: ScanNodeKind::EpisodeFile,
            path_norm: "/demo/Shows/Unresolved/Season 1/S01E01.mkv".into(),
            fingerprint: MediaFingerprint::default(),
            scan_reason: ScanReason::BulkSeed,
        };

        let err = crate::domain::scan::orchestration::job::EnqueueRequest::new(
            crate::domain::scan::orchestration::job::JobPriority::P0,
            crate::domain::scan::orchestration::job::JobPayload::MetadataEnrich(
                job,
            ),
        )
        .validate()
        .expect_err("unresolved episode metadata should be rejected");

        assert!(
            err.to_string().contains(
                "episode metadata enrich requires resolved series id"
            )
        );
    }
}
