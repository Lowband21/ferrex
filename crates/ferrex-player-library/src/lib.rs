//! Library data domain for Ferrex player clients.
//!
//! This crate owns the library state container, library messages,
//! repository snapshot/index structures, and server-backed library scan/media
//! subscriptions. The desktop app supplies app-shell context around these data
//! primitives instead of forcing the data domain to import the final root state.

/// Media-root browser state for selecting/scanning server directories.
pub mod media_root_browser;
/// Library messages and realtime subscription DTOs.
pub mod messages;
/// Repository snapshot DTOs used to hydrate cache/index state.
pub mod repo_snapshot;
/// Compatibility re-exports for the extracted repository crate.
pub mod repository;
/// Server-backed HLS and scan subscription helpers.
pub mod server;
/// Library form/state helper types.
pub mod types;
/// UI-agnostic library reducer logic.
pub mod update;
/// Focused update-handler helpers.
pub mod update_handlers;

use self::{
    media_root_browser::State as MediaRootBrowserState, types::LibraryFormData,
};
use ferrex_core::player_prelude::{
    Library, LibraryId, LibraryMediaCache, ScanCommandAcceptedResponse,
    ScanConfig, ScanLifecycleStatus, ScanMetrics, ScanProgressEvent,
    ScanRunMode, ScanSnapshotDto,
};
use ferrex_player_api::services::api::ApiService;
use ferrex_player_foundation::domain::DomainTask;
use repository::accessor::{Accessor, ReadWrite};
#[cfg(feature = "demo")]
use std::path::PathBuf;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use uuid::Uuid;

/// Cross-domain events relevant to the library data domain.
pub trait LibraryExternalEvent {
    /// Whether the event represents an authenticated user.
    fn is_user_authenticated(&self) -> bool {
        false
    }

    /// Whether the backing database/cache was cleared.
    fn is_database_cleared(&self) -> bool {
        false
    }

    /// Whether library state should be cleared for the current session.
    fn is_clear_libraries(&self) -> bool {
        false
    }
}

/// Active scan identity from the user's perspective.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ActiveScanRunKey {
    /// Library being scanned.
    pub library_id: LibraryId,
    /// Scan run mode for the active operation.
    pub mode: ScanRunMode,
}

impl ActiveScanRunKey {
    /// Build a scan-run key from a library and scan mode.
    pub fn new(library_id: LibraryId, mode: ScanRunMode) -> Self {
        Self { library_id, mode }
    }

    /// Build a scan-run key from an active scan snapshot.
    pub fn from_snapshot(snapshot: &ScanSnapshotDto) -> Self {
        Self::new(snapshot.library_id, snapshot.mode)
    }
}

/// Library domain state owned by this crate.
#[derive(Debug)]
pub struct LibraryDomainState {
    /// Whether the library management panel is visible.
    pub show_library_management: bool,
    /// Current add/edit library form data.
    pub library_form_data: Option<LibraryFormData>,
    /// Validation errors for the current library form.
    pub library_form_errors: Vec<String>,
    /// Success message from the last library form action.
    pub library_form_success: Option<String>,
    /// Cached media references keyed by library id.
    pub library_media_cache: HashMap<Uuid, LibraryMediaCache>,
    /// Active scan snapshots keyed by scan id.
    pub active_scans: HashMap<Uuid, ScanSnapshotDto>,
    /// Active scan ids keyed by user-visible library/mode identity.
    pub active_scan_runs: HashMap<ActiveScanRunKey, Uuid>,
    /// Scan starts requested locally but not yet acknowledged by the server.
    pub pending_scan_starts: HashSet<ActiveScanRunKey>,
    /// Most recent progress frame for each scan id.
    pub latest_progress: HashMap<Uuid, ScanProgressEvent>,
    /// Load-state marker for library bootstrap.
    pub load_state: LibrariesLoadState,

    /// Last loaded scan metrics summary.
    pub scan_metrics: Option<ScanMetrics>,
    /// Last loaded scan configuration.
    pub scan_config: Option<ScanConfig>,
    /// Media-root browser state for choosing scan roots.
    pub media_root_browser: MediaRootBrowserState,

    /// Optional API service used by server-backed library actions.
    pub api_service: Option<Arc<dyn ApiService>>,

    /// Loaded libraries for the current session/user.
    pub libraries: Vec<Library>,

    /// Read/write repository accessor for cached media references.
    pub repo_accessor: Accessor<ReadWrite>,
    /// Demo-mode controls and seeded library ids.
    #[cfg(feature = "demo")]
    pub demo_controls: DemoControlsState,
}

/// Demo-mode controls mirrored into the library domain.
#[cfg(feature = "demo")]
#[derive(Debug, Clone, Default)]
pub struct DemoControlsState {
    /// Whether a demo operation is loading initial state.
    pub is_loading: bool,
    /// Whether a demo resize/reset operation is in flight.
    pub is_updating: bool,
    /// Last demo operation error.
    pub error: Option<String>,
    /// Libraries managed by demo mode.
    pub demo_library_ids: HashSet<LibraryId>,
    /// Current movie count in the demo library.
    pub movies_current: Option<usize>,
    /// Current series count in the demo library.
    pub series_current: Option<usize>,
    /// Pending movie-count input from UI.
    pub movies_input: String,
    /// Pending series-count input from UI.
    pub series_input: String,
    /// Demo filesystem root, when known.
    pub demo_root: Option<PathBuf>,
    /// Demo username, when known.
    pub demo_username: Option<String>,
}

impl LibraryDomainState {
    /// Build library domain state from optional API and required repository access.
    pub fn new(
        api_service: Option<Arc<dyn ApiService>>,
        repo_accessor: Accessor<ReadWrite>,
    ) -> Self {
        Self {
            show_library_management: false,
            library_form_data: None,
            library_form_errors: Vec::new(),
            library_form_success: None,
            library_media_cache: HashMap::new(),
            active_scans: HashMap::new(),
            active_scan_runs: HashMap::new(),
            pending_scan_starts: HashSet::new(),
            latest_progress: HashMap::new(),
            load_state: LibrariesLoadState::NotStarted,
            scan_metrics: None,
            scan_config: None,
            media_root_browser: MediaRootBrowserState::default(),
            api_service,
            libraries: Vec::new(),
            repo_accessor,
            #[cfg(feature = "demo")]
            demo_controls: DemoControlsState::default(),
        }
    }

    #[cfg(test)]
    fn active_scan_by_id(&self, scan_id: Uuid) -> Option<&ScanSnapshotDto> {
        self.active_scans.get(&scan_id)
    }

    /// Return the active scan id for a library/mode pair if the snapshot is still present.
    pub fn active_scan_id_by_library_mode(
        &self,
        library_id: LibraryId,
        mode: ScanRunMode,
    ) -> Option<Uuid> {
        self.active_scan_runs
            .get(&ActiveScanRunKey::new(library_id, mode))
            .copied()
            .filter(|scan_id| self.active_scans.contains_key(scan_id))
    }

    /// Borrow the active scan snapshot for a library/mode pair.
    pub fn active_scan_by_library_mode(
        &self,
        library_id: LibraryId,
        mode: ScanRunMode,
    ) -> Option<&ScanSnapshotDto> {
        self.active_scan_id_by_library_mode(library_id, mode)
            .and_then(|scan_id| self.active_scans.get(&scan_id))
    }

    /// Whether a scan start has been requested but not acknowledged.
    pub fn is_scan_start_pending(
        &self,
        library_id: LibraryId,
        mode: ScanRunMode,
    ) -> bool {
        self.pending_scan_starts
            .contains(&ActiveScanRunKey::new(library_id, mode))
    }

    /// Mark a scan start as pending if no active or pending scan already covers it.
    pub fn begin_scan_start(
        &mut self,
        library_id: LibraryId,
        mode: ScanRunMode,
    ) -> bool {
        let key = ActiveScanRunKey::new(library_id, mode);
        if let Some(scan_id) = self.active_scan_runs.get(&key).copied() {
            if self.active_scans.contains_key(&scan_id) {
                return false;
            }
            self.active_scan_runs.remove(&key);
        }

        if self.pending_scan_starts.contains(&key) {
            return false;
        }
        self.pending_scan_starts.insert(key)
    }

    /// Clear a pending scan-start marker after the request completes.
    pub fn finish_scan_start(
        &mut self,
        library_id: LibraryId,
        mode: ScanRunMode,
    ) {
        self.pending_scan_starts
            .remove(&ActiveScanRunKey::new(library_id, mode));
    }

    /// Apply a server acknowledgement for a newly accepted scan command.
    pub fn apply_scan_start_response(
        &mut self,
        library_id: LibraryId,
        response: &ScanCommandAcceptedResponse,
    ) {
        let run_key = if response.run_key.is_empty() {
            response.mode.run_key(library_id)
        } else {
            response.run_key.clone()
        };

        self.upsert_active_scan(ScanSnapshotDto {
            scan_id: response.scan_id,
            library_id,
            status: response.status.clone(),
            mode: response.mode,
            completed_items: 0,
            total_items: 0,
            validated_items: 0,
            known_unchanged_items: 0,
            skipped_items: 0,
            failed_items: 0,
            needs_attention_items: 0,
            retrying_items: 0,
            correlation_id: response.correlation_id,
            idempotency_key: response.idempotency_key.clone(),
            run_key,
            disposition: Some(response.disposition),
            current_path: None,
            started_at: chrono::Utc::now(),
            terminal_at: None,
            sequence: 0,
            reason_details: Vec::new(),
        });
    }

    /// Insert or update an active scan snapshot and maintain lookup indexes.
    pub fn upsert_active_scan(&mut self, snapshot: ScanSnapshotDto) {
        let key = ActiveScanRunKey::from_snapshot(&snapshot);
        self.pending_scan_starts.remove(&key);

        if snapshot.status.is_terminal() {
            self.remove_active_scan(snapshot.scan_id);
            if self.active_scan_runs.get(&key).copied()
                == Some(snapshot.scan_id)
            {
                self.active_scan_runs.remove(&key);
            }
            return;
        }

        if let Some(existing) = self.active_scans.get(&snapshot.scan_id) {
            let existing_key = ActiveScanRunKey::from_snapshot(existing);
            if existing_key != key {
                self.active_scan_runs.remove(&existing_key);
            }
        }

        if let Some(previous_scan_id) =
            self.active_scan_runs.insert(key, snapshot.scan_id)
        {
            if previous_scan_id != snapshot.scan_id {
                self.active_scans.remove(&previous_scan_id);
                self.latest_progress.remove(&previous_scan_id);
            }
        }

        self.active_scans.insert(snapshot.scan_id, snapshot);
    }

    /// Replace active scan state with a deduplicated server snapshot set.
    pub fn replace_active_scan_snapshots(
        &mut self,
        snapshots: Vec<ScanSnapshotDto>,
    ) {
        let mut deduped: HashMap<ActiveScanRunKey, ScanSnapshotDto> =
            HashMap::new();

        for snapshot in snapshots
            .into_iter()
            .filter(|snapshot| snapshot.status.is_active())
        {
            let key = ActiveScanRunKey::from_snapshot(&snapshot);
            match deduped.get_mut(&key) {
                Some(existing) => {
                    if should_replace_active_snapshot(existing, &snapshot) {
                        *existing = snapshot;
                    }
                }
                None => {
                    deduped.insert(key, snapshot);
                }
            }
        }

        self.active_scans.clear();
        self.active_scan_runs.clear();

        for (key, snapshot) in deduped {
            self.active_scan_runs.insert(key, snapshot.scan_id);
            self.active_scans.insert(snapshot.scan_id, snapshot);
        }

        let active_scan_ids: HashSet<Uuid> =
            self.active_scans.keys().copied().collect();
        self.latest_progress
            .retain(|scan_id, _| active_scan_ids.contains(scan_id));
        self.pending_scan_starts
            .retain(|key| !self.active_scan_runs.contains_key(key));
    }

    /// Merge a realtime progress frame into the active scan cache.
    pub fn apply_scan_progress_frame(
        &mut self,
        frame: ScanProgressEvent,
    ) -> bool {
        let (old_key, new_key, is_active, scan_id, keep_snapshot) = {
            let Some(snapshot) = self.active_scans.get_mut(&frame.scan_id)
            else {
                return false;
            };
            let old_key = ActiveScanRunKey::from_snapshot(snapshot);

            snapshot.completed_items = frame.completed_items;
            snapshot.total_items = frame.total_items;
            snapshot.validated_items = frame.validated_items;
            snapshot.known_unchanged_items = frame.known_unchanged_items;
            snapshot.skipped_items = frame.skipped_items;
            snapshot.failed_items = frame.failed_items;
            snapshot.needs_attention_items = frame.needs_attention_items;
            snapshot.retrying_items = frame.retrying_items;
            snapshot.reason_details = frame.reason_details.clone();
            snapshot.current_path = frame.current_path.clone();
            snapshot.terminal_at = frame.terminal_at;
            snapshot.sequence = frame.sequence;

            if let Some(status) = scan_status_from_progress(&frame.status) {
                snapshot.status = status;
            }

            let terminal = snapshot.status.is_terminal();
            (
                old_key,
                ActiveScanRunKey::from_snapshot(snapshot),
                snapshot.status.is_active(),
                snapshot.scan_id,
                !terminal || progress_frame_has_recovery_affordance(&frame),
            )
        };

        if !keep_snapshot {
            self.remove_active_scan(scan_id);
            return true;
        }

        self.active_scan_runs.remove(&old_key);
        if is_active {
            self.active_scan_runs.insert(new_key, scan_id);
        } else {
            self.active_scan_runs.remove(&new_key);
        }

        self.latest_progress.insert(frame.scan_id, frame);
        true
    }

    /// Remove a scan from all active scan indexes.
    pub fn remove_active_scan(&mut self, scan_id: Uuid) {
        if let Some(snapshot) = self.active_scans.remove(&scan_id) {
            self.active_scan_runs
                .remove(&ActiveScanRunKey::from_snapshot(&snapshot));
        } else {
            self.active_scan_runs
                .retain(|_, active_scan_id| *active_scan_id != scan_id);
        }
        self.latest_progress.remove(&scan_id);
    }

    /// Clear all active and pending scan tracking state.
    pub fn clear_scan_tracking(&mut self) {
        self.active_scans.clear();
        self.active_scan_runs.clear();
        self.pending_scan_starts.clear();
        self.latest_progress.clear();
    }
}

fn should_replace_active_snapshot(
    existing: &ScanSnapshotDto,
    candidate: &ScanSnapshotDto,
) -> bool {
    match candidate.started_at.cmp(&existing.started_at) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => {
            (candidate.sequence, candidate.scan_id.as_u128())
                > (existing.sequence, existing.scan_id.as_u128())
        }
    }
}

fn progress_frame_has_recovery_affordance(frame: &ScanProgressEvent) -> bool {
    frame.needs_attention_items > 0
        || frame.failed_items > 0
        || frame.skipped_items > 0
        || !frame.reason_details.is_empty()
        || matches!(
            frame.status.as_str(),
            "failed_needs_attention" | "needs_attention" | "skipped"
        )
}

fn scan_status_from_progress(status: &str) -> Option<ScanLifecycleStatus> {
    match status {
        "pending" | "initializing" => Some(ScanLifecycleStatus::Pending),
        "running" | "discovering" | "processing" | "quiescing" | "retrying" => {
            Some(ScanLifecycleStatus::Running)
        }
        "paused" => Some(ScanLifecycleStatus::Paused),
        "completed" => Some(ScanLifecycleStatus::Completed),
        "failed" | "failed_needs_attention" | "needs_attention" | "skipped" => {
            Some(ScanLifecycleStatus::Failed)
        }
        "canceled" | "cancelled" => Some(ScanLifecycleStatus::Canceled),
        _ => None,
    }
}

/// Library domain wrapper used by app shells to route cross-domain events.
#[derive(Debug)]
pub struct LibraryDomain {
    /// Mutable library state owned by the domain.
    pub state: LibraryDomainState,
}

impl LibraryDomain {
    /// Build a library domain wrapper from existing state.
    pub fn new(state: LibraryDomainState) -> Self {
        Self { state }
    }

    /// Handle a cross-domain event through an explicit data-domain view of the event.
    pub fn handle_event<E>(
        &mut self,
        event: &E,
    ) -> DomainTask<messages::LibraryMessage>
    where
        E: LibraryExternalEvent,
    {
        if event.is_database_cleared() || event.is_clear_libraries() {
            self.state.library_media_cache.clear();
            self.state.clear_scan_tracking();
            self.state.load_state = LibrariesLoadState::NotStarted;
        }
        DomainTask::none()
    }
}

/// Bootstrap status for loading libraries into the player domain.
#[derive(Debug, Clone)]
pub enum LibrariesLoadState {
    /// No library load has been attempted in this state instance.
    NotStarted,
    /// A library load request is in flight.
    InProgress,
    /// Libraries were loaded successfully for the optional user/server pair.
    Succeeded {
        /// User id associated with the loaded libraries, when authenticated.
        user_id: Option<Uuid>,
        /// Server URL used to load the library state.
        server_url: String,
    },
    /// Library loading failed and retained an error for display/retry.
    Failed {
        /// Last load error message.
        last_error: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository::{accessor::ReadWrite, media_repo::MediaRepo};
    use chrono::{Duration, Utc};
    use parking_lot::RwLock;
    use std::sync::Arc;

    use ferrex_core::player_prelude::{
        ScanStageLatencySummary, ScanStartDisposition,
    };

    fn empty_state() -> LibraryDomainState {
        LibraryDomainState::new(
            None,
            Accessor::<ReadWrite>::new(Arc::new(RwLock::new(
                None::<MediaRepo>,
            ))),
        )
    }

    fn accepted_response(
        scan_id: Uuid,
        mode: ScanRunMode,
        status: ScanLifecycleStatus,
        disposition: ScanStartDisposition,
    ) -> ScanCommandAcceptedResponse {
        ScanCommandAcceptedResponse {
            scan_id,
            correlation_id: Uuid::now_v7(),
            status,
            mode,
            idempotency_key: format!("scan-{scan_id}"),
            run_key: String::new(),
            disposition,
        }
    }

    fn snapshot(
        library_id: LibraryId,
        scan_id: Uuid,
        mode: ScanRunMode,
        status: ScanLifecycleStatus,
        sequence: u64,
        started_offset_secs: i64,
    ) -> ScanSnapshotDto {
        ScanSnapshotDto {
            scan_id,
            library_id,
            status,
            mode,
            completed_items: sequence,
            total_items: 100,
            validated_items: sequence,
            known_unchanged_items: 0,
            skipped_items: 0,
            failed_items: 0,
            needs_attention_items: 0,
            retrying_items: 0,
            correlation_id: Uuid::now_v7(),
            idempotency_key: format!("scan-{scan_id}"),
            run_key: mode.run_key(library_id),
            disposition: None,
            current_path: None,
            started_at: Utc::now() + Duration::seconds(started_offset_secs),
            terminal_at: None,
            sequence,
            reason_details: Vec::new(),
        }
    }

    fn progress(
        library_id: LibraryId,
        scan_id: Uuid,
        status: &str,
    ) -> ScanProgressEvent {
        ScanProgressEvent {
            version: "1".into(),
            scan_id,
            library_id,
            status: status.into(),
            completed_items: 12,
            total_items: 24,
            validated_items: 11,
            known_unchanged_items: 1,
            skipped_items: 0,
            failed_items: 0,
            needs_attention_items: 0,
            retrying_items: 1,
            sequence: 2,
            current_path: Some("/media/movie.mkv".into()),
            path_key: None,
            p95_stage_latencies_ms: ScanStageLatencySummary {
                scan: 1,
                analyze: 1,
                index: 1,
            },
            correlation_id: Uuid::now_v7(),
            idempotency_key: format!("scan-{scan_id}"),
            emitted_at: Utc::now(),
            terminal_at: None,
            reason_details: Vec::new(),
        }
    }

    #[test]
    fn scan_start_responses_reuse_active_library_mode_entry() {
        let mut state = empty_state();
        let library_id = LibraryId(Uuid::now_v7());
        let first_scan_id = Uuid::now_v7();
        let replacement_scan_id = Uuid::now_v7();

        assert!(state.begin_scan_start(library_id, ScanRunMode::Manual));
        state.apply_scan_start_response(
            library_id,
            &accepted_response(
                first_scan_id,
                ScanRunMode::Manual,
                ScanLifecycleStatus::Running,
                ScanStartDisposition::Created,
            ),
        );
        assert!(!state.is_scan_start_pending(library_id, ScanRunMode::Manual));

        state.apply_scan_start_response(
            library_id,
            &accepted_response(
                first_scan_id,
                ScanRunMode::Manual,
                ScanLifecycleStatus::Running,
                ScanStartDisposition::Reused,
            ),
        );

        assert_eq!(state.active_scans.len(), 1);
        assert_eq!(
            state
                .active_scan_by_library_mode(library_id, ScanRunMode::Manual)
                .map(|scan| (scan.scan_id, scan.disposition)),
            Some((first_scan_id, Some(ScanStartDisposition::Reused)))
        );

        state.upsert_active_scan(snapshot(
            library_id,
            replacement_scan_id,
            ScanRunMode::Manual,
            ScanLifecycleStatus::Running,
            0,
            10,
        ));

        assert_eq!(state.active_scans.len(), 1);
        assert!(state.active_scan_by_id(first_scan_id).is_none());
        assert_eq!(
            state.active_scan_id_by_library_mode(
                library_id,
                ScanRunMode::Manual
            ),
            Some(replacement_scan_id)
        );
    }

    #[test]
    fn active_snapshots_are_deduplicated_by_library_and_mode() {
        let mut state = empty_state();
        let library_id = LibraryId(Uuid::now_v7());
        let old_manual_scan_id = Uuid::now_v7();
        let current_manual_scan_id = Uuid::now_v7();
        let maintenance_scan_id = Uuid::now_v7();
        let terminal_scan_id = Uuid::now_v7();

        state.upsert_active_scan(snapshot(
            library_id,
            old_manual_scan_id,
            ScanRunMode::Manual,
            ScanLifecycleStatus::Running,
            1,
            -10,
        ));
        assert!(state.apply_scan_progress_frame(progress(
            library_id,
            old_manual_scan_id,
            "running"
        )));

        state.replace_active_scan_snapshots(vec![
            snapshot(
                library_id,
                old_manual_scan_id,
                ScanRunMode::Manual,
                ScanLifecycleStatus::Running,
                9,
                -10,
            ),
            snapshot(
                library_id,
                current_manual_scan_id,
                ScanRunMode::Manual,
                ScanLifecycleStatus::Running,
                1,
                10,
            ),
            snapshot(
                library_id,
                maintenance_scan_id,
                ScanRunMode::Maintenance,
                ScanLifecycleStatus::Paused,
                1,
                0,
            ),
            snapshot(
                library_id,
                terminal_scan_id,
                ScanRunMode::Resume,
                ScanLifecycleStatus::Completed,
                1,
                20,
            ),
        ]);

        assert_eq!(state.active_scans.len(), 2);
        assert_eq!(
            state.active_scan_id_by_library_mode(
                library_id,
                ScanRunMode::Manual
            ),
            Some(current_manual_scan_id)
        );
        assert_eq!(
            state.active_scan_id_by_library_mode(
                library_id,
                ScanRunMode::Maintenance
            ),
            Some(maintenance_scan_id)
        );
        assert!(state.active_scan_by_id(old_manual_scan_id).is_none());
        assert!(state.active_scan_by_id(terminal_scan_id).is_none());
        assert!(state.latest_progress.get(&old_manual_scan_id).is_none());
    }

    #[test]
    fn terminal_progress_removes_scan_and_run_indexes() {
        let mut state = empty_state();
        let library_id = LibraryId(Uuid::now_v7());
        let scan_id = Uuid::now_v7();

        state.upsert_active_scan(snapshot(
            library_id,
            scan_id,
            ScanRunMode::Manual,
            ScanLifecycleStatus::Running,
            1,
            0,
        ));
        assert!(state.apply_scan_progress_frame(progress(
            library_id, scan_id, "running"
        )));
        assert!(state.latest_progress.contains_key(&scan_id));

        assert!(state.apply_scan_progress_frame(progress(
            library_id,
            scan_id,
            "completed"
        )));

        assert!(state.active_scan_by_id(scan_id).is_none());
        assert_eq!(
            state.active_scan_id_by_library_mode(
                library_id,
                ScanRunMode::Manual
            ),
            None
        );
        assert!(!state.latest_progress.contains_key(&scan_id));
    }
}
