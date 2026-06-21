use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use chrono::{DateTime, Utc};
use ferrex_core::{
    api::types::scan::ScanDisplayText,
    player_prelude::{
        ScanFailureDto, ScanPageMeta, ScanProgressEvent, ScanReplayInfo,
        ScanRunDetailResponse, ScanRunDto, ScanRunEventDto,
        ScanRunEventsPageResponse, ScanRunFailuresPageResponse,
        ScanRunListResponse, ScanSnapshotDto, ScannerHealthResponse,
        display_text_for_scan_failure, display_text_for_scan_status,
    },
};
use uuid::Uuid;

/// Minimum interval for refreshes requested from telemetry-style media SSE
/// scan events. User, command, recovery, and terminal-progress refreshes bypass
/// this throttle and are only deduped while an overview request is in flight.
pub const MEDIA_SCAN_EVENT_REFRESH_MIN_INTERVAL: Duration =
    Duration::from_secs(1);

/// Why the dashboard is being refreshed. This lets app shells debounce or log
/// refreshes without treating scan progress frames as direct durable state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanDashboardRefreshReason {
    InitialLoad,
    UserRequested,
    MediaScanEvent,
    TerminalProgress,
    CommandAccepted,
    RecoveryAccepted,
}

impl ScanDashboardRefreshReason {
    pub fn is_telemetry_derived(self) -> bool {
        matches!(self, Self::MediaScanEvent)
    }
}

impl Default for ScanDashboardRefreshReason {
    fn default() -> Self {
        Self::InitialLoad
    }
}

/// Loading state for durable scan dashboard surfaces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanDashboardLoadState {
    Idle,
    Loading,
    Loaded,
    Failed { error: String },
}

impl Default for ScanDashboardLoadState {
    fn default() -> Self {
        Self::Idle
    }
}

/// Durable overview fetched for the scan dashboard.
#[derive(Debug, Clone)]
pub struct ScanDashboardOverviewPayload {
    pub health: ScannerHealthResponse,
    pub active_runs: Vec<ScanRunDto>,
    pub recent_runs: ScanRunListResponse,
}

/// Durable run detail fetched for the selected scan dashboard run.
#[derive(Debug, Clone)]
pub struct ScanDashboardRunPayload {
    pub detail: ScanRunDetailResponse,
    pub events: ScanRunEventsPageResponse,
    pub failures: ScanRunFailuresPageResponse,
}

/// Terminal context retained after a scan leaves the active SSE set.
#[derive(Debug, Clone)]
pub struct TerminalScanSummary {
    pub scan_id: Uuid,
    pub library_id: ferrex_core::player_prelude::LibraryId,
    pub status: String,
    pub status_label: String,
    pub status_message: String,
    pub completed_items: u64,
    pub total_items: u64,
    pub retrying_items: u64,
    pub dead_lettered_items: u64,
    pub terminal_at: DateTime<Utc>,
    pub last_event_at: DateTime<Utc>,
    pub terminal_summary: serde_json::Value,
}

impl TerminalScanSummary {
    fn from_run(
        run: &ScanRunDto,
        terminal_summary: serde_json::Value,
    ) -> Option<Self> {
        let terminal_at = run.terminal_at.or_else(|| {
            if is_terminal_status(&run.status) {
                Some(run.last_event_at)
            } else {
                None
            }
        })?;

        Some(Self {
            scan_id: run.scan_id,
            library_id: run.library_id,
            status: run.status.clone(),
            status_label: run.status_label.clone(),
            status_message: run.status_message.clone(),
            completed_items: run.completed_items,
            total_items: run.total_items,
            retrying_items: run.retrying_items,
            dead_lettered_items: run.dead_lettered_items,
            terminal_at,
            last_event_at: run.last_event_at,
            terminal_summary,
        })
    }

    fn from_progress(frame: &ScanProgressEvent) -> Option<Self> {
        if !is_terminal_status(&frame.status) {
            return None;
        }

        let display = scan_status_display_text(&frame.status);
        Some(Self {
            scan_id: frame.scan_id,
            library_id: frame.library_id,
            status: normalized_status(&frame.status).to_string(),
            status_label: display.label,
            status_message: display.message,
            completed_items: frame.completed_items,
            total_items: frame.total_items,
            retrying_items: frame.retrying_items,
            dead_lettered_items: frame.failed_items,
            terminal_at: frame.emitted_at,
            last_event_at: frame.emitted_at,
            terminal_summary: serde_json::json!({
                "source": "live_progress",
                "sequence": frame.sequence,
                "current_path": frame.current_path,
                "path_key": frame.path_key,
            }),
        })
    }
}

/// First-class scan dashboard state for the player library domain.
#[derive(Debug, Clone)]
pub struct ScanDashboardState {
    pub overview_state: ScanDashboardLoadState,
    pub selected_run_state: ScanDashboardLoadState,
    pub health: Option<ScannerHealthResponse>,
    pub active_runs: Vec<ScanRunDto>,
    pub recent_runs: Vec<ScanRunDto>,
    pub runs_page: Option<ScanPageMeta>,
    pub selected_run_id: Option<Uuid>,
    pub selected_run: Option<ScanRunDto>,
    pub selected_terminal_summary: Option<serde_json::Value>,
    pub selected_events: Vec<ScanRunEventDto>,
    pub selected_events_page: Option<ScanPageMeta>,
    pub selected_replay: Option<ScanReplayInfo>,
    pub selected_failures: Vec<ScanFailureDto>,
    pub selected_failures_page: Option<ScanPageMeta>,
    pub terminal_summaries: HashMap<Uuid, TerminalScanSummary>,
    pub last_refresh_reason: ScanDashboardRefreshReason,
    pub overview_refresh_in_flight: bool,
    pub last_media_scan_event_refresh_started_at: Option<Instant>,
}

impl Default for ScanDashboardState {
    fn default() -> Self {
        Self {
            overview_state: ScanDashboardLoadState::Idle,
            selected_run_state: ScanDashboardLoadState::Idle,
            health: None,
            active_runs: Vec::new(),
            recent_runs: Vec::new(),
            runs_page: None,
            selected_run_id: None,
            selected_run: None,
            selected_terminal_summary: None,
            selected_events: Vec::new(),
            selected_events_page: None,
            selected_replay: None,
            selected_failures: Vec::new(),
            selected_failures_page: None,
            terminal_summaries: HashMap::new(),
            last_refresh_reason: ScanDashboardRefreshReason::InitialLoad,
            overview_refresh_in_flight: false,
            last_media_scan_event_refresh_started_at: None,
        }
    }
}

impl ScanDashboardState {
    pub fn try_begin_overview_load(
        &mut self,
        reason: ScanDashboardRefreshReason,
        now: Instant,
    ) -> bool {
        if self.overview_refresh_in_flight {
            if !reason.is_telemetry_derived() {
                self.last_refresh_reason = reason;
            }
            return false;
        }

        if reason.is_telemetry_derived()
            && self.media_scan_event_refresh_is_rate_limited(now)
        {
            return false;
        }

        self.begin_overview_load(reason);
        if reason.is_telemetry_derived() {
            self.last_media_scan_event_refresh_started_at = Some(now);
        }
        true
    }

    pub fn begin_overview_load(&mut self, reason: ScanDashboardRefreshReason) {
        self.overview_state = ScanDashboardLoadState::Loading;
        self.last_refresh_reason = reason;
        self.overview_refresh_in_flight = true;
    }

    pub fn fail_overview_load(&mut self, error: impl Into<String>) {
        self.overview_state = ScanDashboardLoadState::Failed {
            error: error.into(),
        };
        self.overview_refresh_in_flight = false;
    }

    pub fn apply_overview(&mut self, payload: ScanDashboardOverviewPayload) {
        self.health = Some(payload.health);
        self.active_runs = payload
            .active_runs
            .into_iter()
            .filter(|run| !is_terminal_status(&run.status))
            .collect();
        self.runs_page = Some(payload.recent_runs.page.clone());
        self.recent_runs = payload.recent_runs.runs;

        let terminal_runs: Vec<_> = self
            .recent_runs
            .iter()
            .filter(|run| is_terminal_status(&run.status))
            .cloned()
            .collect();
        for run in terminal_runs {
            self.remember_terminal_run(&run, serde_json::Value::Null);
        }

        if let Some(selected_id) = self.selected_run_id {
            if let Some(updated) = self
                .recent_runs
                .iter()
                .chain(self.active_runs.iter())
                .find(|run| run.scan_id == selected_id)
                .cloned()
            {
                self.selected_run = Some(updated);
            }
        }

        self.overview_state = ScanDashboardLoadState::Loaded;
        self.overview_refresh_in_flight = false;
    }

    pub fn begin_run_load(&mut self, scan_id: Uuid) {
        self.selected_run_id = Some(scan_id);
        self.selected_run_state = ScanDashboardLoadState::Loading;
    }

    pub fn fail_run_load(&mut self, error: impl Into<String>) {
        self.selected_run_state = ScanDashboardLoadState::Failed {
            error: error.into(),
        };
    }

    pub fn apply_run_payload(&mut self, payload: ScanDashboardRunPayload) {
        let scan_id = payload.detail.run.scan_id;
        self.selected_run_id = Some(scan_id);
        self.selected_run = Some(payload.detail.run.clone());
        self.selected_terminal_summary =
            Some(payload.detail.terminal_summary.clone());
        self.selected_events_page = Some(payload.events.page.clone());
        self.selected_replay = payload.events.replay.clone();
        self.selected_events = payload.events.events;
        self.selected_failures_page = Some(payload.failures.page.clone());
        self.selected_failures = payload.failures.failures;

        self.apply_run_summary(
            payload.detail.run,
            payload.detail.terminal_summary,
        );

        self.selected_run_state = ScanDashboardLoadState::Loaded;
    }

    /// Apply durable detail/history fetched for a run without changing the
    /// user's current selection. If the refreshed run is selected, the full
    /// timeline/failure payload is installed as selected-run state.
    pub fn apply_run_refresh_payload(
        &mut self,
        payload: ScanDashboardRunPayload,
    ) {
        if self.selected_run_id == Some(payload.detail.run.scan_id) {
            self.apply_run_payload(payload);
            return;
        }

        self.apply_run_summary(
            payload.detail.run,
            payload.detail.terminal_summary,
        );
    }

    /// Apply one active scan snapshot without replacing the rest of the
    /// dashboard's active-run cache.
    pub fn apply_active_snapshot(&mut self, snapshot: &ScanSnapshotDto) {
        let run = scan_run_from_snapshot(snapshot);
        self.upsert_recent_run(run.clone());
        if is_terminal_lifecycle(snapshot) {
            self.active_runs
                .retain(|active| active.scan_id != run.scan_id);
            self.remember_terminal_run(&run, serde_json::Value::Null);
        } else {
            self.upsert_active_run(run);
        }
    }

    /// Apply active scan snapshots from the compatibility API into the durable
    /// dashboard without clearing retained terminal context.
    pub fn apply_active_snapshots(&mut self, snapshots: &[ScanSnapshotDto]) {
        let mut active = Vec::with_capacity(snapshots.len());
        for snapshot in snapshots {
            if is_terminal_lifecycle(snapshot) {
                let run = scan_run_from_snapshot(snapshot);
                self.upsert_recent_run(run.clone());
                self.remember_terminal_run(&run, serde_json::Value::Null);
            } else {
                let run = scan_run_from_snapshot(snapshot);
                self.upsert_recent_run(run.clone());
                active.push(run);
            }
        }
        self.active_runs = active;
    }

    /// Apply a live SSE progress frame. Non-terminal frames update active run
    /// summaries; terminal frames retire the active run but retain a terminal
    /// summary until durable detail/history arrives.
    pub fn apply_progress_frame(&mut self, frame: &ScanProgressEvent) {
        let run = scan_run_from_progress(frame);
        self.upsert_recent_run(run.clone());

        if is_terminal_status(&frame.status) {
            self.active_runs.retain(|r| r.scan_id != frame.scan_id);
            if let Some(summary) = TerminalScanSummary::from_progress(frame) {
                self.terminal_summaries.insert(frame.scan_id, summary);
            }
        } else {
            self.upsert_active_run(run.clone());
        }

        if self.selected_run_id == Some(frame.scan_id) {
            self.selected_run = Some(run);
        }
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    fn upsert_active_run(&mut self, run: ScanRunDto) {
        upsert_run(&mut self.active_runs, run);
        self.active_runs
            .retain(|run| !is_terminal_status(&run.status));
    }

    fn upsert_recent_run(&mut self, run: ScanRunDto) {
        upsert_run(&mut self.recent_runs, run);
        self.recent_runs
            .sort_by(|a, b| b.last_event_at.cmp(&a.last_event_at));
    }

    fn apply_run_summary(
        &mut self,
        run: ScanRunDto,
        terminal_summary: serde_json::Value,
    ) {
        self.upsert_recent_run(run.clone());
        if is_terminal_status(&run.status) {
            self.active_runs
                .retain(|active| active.scan_id != run.scan_id);
            self.remember_terminal_run(&run, terminal_summary);
        } else {
            self.upsert_active_run(run);
        }
    }

    fn remember_terminal_run(
        &mut self,
        run: &ScanRunDto,
        terminal_summary: serde_json::Value,
    ) {
        if let Some(summary) =
            TerminalScanSummary::from_run(run, terminal_summary)
        {
            self.terminal_summaries.insert(run.scan_id, summary);
        }
    }

    fn media_scan_event_refresh_is_rate_limited(&self, now: Instant) -> bool {
        let Some(last_started_at) =
            self.last_media_scan_event_refresh_started_at
        else {
            return false;
        };

        match now.checked_duration_since(last_started_at) {
            Some(elapsed) => elapsed < MEDIA_SCAN_EVENT_REFRESH_MIN_INTERVAL,
            None => true,
        }
    }
}

fn upsert_run(runs: &mut Vec<ScanRunDto>, run: ScanRunDto) {
    if let Some(existing) =
        runs.iter_mut().find(|item| item.scan_id == run.scan_id)
    {
        *existing = run;
    } else {
        runs.push(run);
    }
}

pub fn is_terminal_status(status: &str) -> bool {
    matches!(
        normalized_status(status),
        "completed" | "failed" | "canceled"
    )
}

pub fn is_active_status(status: &str) -> bool {
    matches!(normalized_status(status), "pending" | "running" | "paused")
}

pub fn normalized_status(status: &str) -> &str {
    match status {
        "cancelled" => "canceled",
        other => other,
    }
}

pub fn scan_status_display_text(status: &str) -> ScanDisplayText {
    display_text_for_scan_status(normalized_status(status))
}

pub fn scan_failure_display_text(
    category: &str,
    message_code: &str,
) -> ScanDisplayText {
    display_text_for_scan_failure(category, message_code)
}

fn is_terminal_lifecycle(snapshot: &ScanSnapshotDto) -> bool {
    is_terminal_status(scan_lifecycle_status_str(&snapshot.status))
}

fn scan_lifecycle_status_str(
    status: &ferrex_core::player_prelude::ScanLifecycleStatus,
) -> &'static str {
    match status {
        ferrex_core::player_prelude::ScanLifecycleStatus::Pending => "pending",
        ferrex_core::player_prelude::ScanLifecycleStatus::Running => "running",
        ferrex_core::player_prelude::ScanLifecycleStatus::Paused => "paused",
        ferrex_core::player_prelude::ScanLifecycleStatus::Completed => {
            "completed"
        }
        ferrex_core::player_prelude::ScanLifecycleStatus::Failed => "failed",
        ferrex_core::player_prelude::ScanLifecycleStatus::Canceled => {
            "canceled"
        }
    }
}

fn scan_run_from_snapshot(snapshot: &ScanSnapshotDto) -> ScanRunDto {
    let status = scan_lifecycle_status_str(&snapshot.status).to_string();
    let display = scan_status_display_text(&status);
    ScanRunDto {
        scan_id: snapshot.scan_id,
        library_id: snapshot.library_id,
        source: "active".to_string(),
        status,
        status_label: display.label,
        status_message: display.message,
        completed_items: snapshot.completed_items,
        total_items: snapshot.total_items,
        retrying_items: snapshot.retrying_items,
        dead_lettered_items: snapshot.failed_items,
        correlation_id: snapshot.correlation_id,
        idempotency_key: snapshot.idempotency_key.clone(),
        current_path: snapshot.current_path.clone(),
        started_at: snapshot.started_at,
        last_event_at: snapshot.terminal_at.unwrap_or(snapshot.started_at),
        terminal_at: snapshot.terminal_at,
        sequence: snapshot.sequence,
        has_failures: snapshot.failed_items > 0,
    }
}

fn scan_run_from_progress(frame: &ScanProgressEvent) -> ScanRunDto {
    let status = normalized_status(&frame.status).to_string();
    let display = scan_status_display_text(&status);
    let dead_lettered_items = frame.failed_items;
    ScanRunDto {
        scan_id: frame.scan_id,
        library_id: frame.library_id,
        source: "live".to_string(),
        status,
        status_label: display.label,
        status_message: display.message,
        completed_items: frame.completed_items,
        total_items: frame.total_items,
        retrying_items: frame.retrying_items,
        dead_lettered_items,
        correlation_id: frame.correlation_id,
        idempotency_key: frame.idempotency_key.clone(),
        current_path: frame.current_path.clone(),
        started_at: frame.emitted_at,
        last_event_at: frame.emitted_at,
        terminal_at: if is_terminal_status(&frame.status) {
            Some(frame.emitted_at)
        } else {
            None
        },
        sequence: frame.sequence,
        has_failures: frame.status == "failed" || dead_lettered_items > 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use ferrex_core::{
        api::scan::{IncrementalScanStatusView, ScanQueueDepths},
        player_prelude::{
            LibraryId, ScanFailureDto, ScanPageMeta, ScanReplayInfo,
            ScanStageLatencySummary,
        },
    };

    fn id(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    fn library_id() -> LibraryId {
        LibraryId(id(10))
    }

    fn ts(seconds: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(seconds, 0).single().unwrap()
    }

    fn frame(scan_id: Uuid, status: &str, sequence: u64) -> ScanProgressEvent {
        ScanProgressEvent {
            version: "1".to_string(),
            scan_id,
            library_id: library_id(),
            status: status.to_string(),
            completed_items: sequence,
            total_items: 10,
            sequence,
            current_path: Some(format!("/media/{sequence}.mkv")),
            path_key: None,
            p95_stage_latencies_ms: ScanStageLatencySummary {
                scan: 1,
                analyze: 2,
                index: 3,
            },
            correlation_id: id(20),
            idempotency_key: "scan-key".to_string(),
            emitted_at: ts(sequence as i64),
            terminal_at: if is_terminal_status(status) {
                Some(ts(sequence as i64))
            } else {
                None
            },
            validated_items: sequence,
            known_unchanged_items: 0,
            skipped_items: 0,
            failed_items: if status == "failed" { 1 } else { 0 },
            needs_attention_items: if status == "failed" { 1 } else { 0 },
            retrying_items: 0,
            reason_details: Vec::new(),
        }
    }

    fn run(scan_id: Uuid, status: &str, sequence: u64) -> ScanRunDto {
        let display = scan_status_display_text(status);
        ScanRunDto {
            scan_id,
            library_id: library_id(),
            source: "manual".to_string(),
            status: normalized_status(status).to_string(),
            status_label: display.label,
            status_message: display.message,
            completed_items: sequence,
            total_items: 10,
            retrying_items: 0,
            dead_lettered_items: if status == "failed" { 1 } else { 0 },
            correlation_id: id(30),
            idempotency_key: "run-key".to_string(),
            current_path: None,
            started_at: ts(1),
            last_event_at: ts(sequence as i64),
            terminal_at: if is_terminal_status(status) {
                Some(ts(sequence as i64))
            } else {
                None
            },
            sequence,
            has_failures: status == "failed",
        }
    }

    fn page(count: usize) -> ScanPageMeta {
        ScanPageMeta::new(100, 0, count, count)
    }

    fn health() -> ScannerHealthResponse {
        ScannerHealthResponse {
            queue_depths: ScanQueueDepths {
                folder_scan: 0,
                manifest_scan: 0,
                analyze: 0,
                metadata: 0,
                index: 0,
                image_fetch: 0,
            },
            active_scans: 0,
            retained_runs: 0,
            failed_runs: 0,
            incremental: IncrementalScanStatusView::default(),
        }
    }

    #[test]
    fn active_to_terminal_retains_terminal_context() {
        let scan_id = id(1);
        let mut state = ScanDashboardState::default();

        state.apply_progress_frame(&frame(scan_id, "running", 1));
        assert_eq!(state.active_runs.len(), 1);
        assert!(!state.terminal_summaries.contains_key(&scan_id));

        state.apply_progress_frame(&frame(scan_id, "completed", 10));
        assert!(state.active_runs.is_empty());
        let terminal = state
            .terminal_summaries
            .get(&scan_id)
            .expect("terminal summary retained");
        assert_eq!(terminal.status_label, "Completed");
        assert_eq!(terminal.completed_items, 10);
        assert!(state.recent_runs.iter().any(|run| run.scan_id == scan_id));
    }

    #[test]
    fn missed_replayed_events_are_recorded_on_selected_run() {
        let scan_id = id(2);
        let mut state = ScanDashboardState::default();
        state.begin_run_load(scan_id);

        let replay = ScanReplayInfo {
            requested_after_sequence: Some(10),
            min_available_sequence: Some(11),
            max_available_sequence: Some(12),
            next_sequence: Some(13),
            recoverable: true,
            recovery_hint: "Continue from the returned next sequence."
                .to_string(),
        };

        state.apply_run_payload(ScanDashboardRunPayload {
            detail: ScanRunDetailResponse {
                run: run(scan_id, "running", 12),
                terminal_summary: serde_json::Value::Null,
            },
            events: ScanRunEventsPageResponse {
                scan_id,
                events: vec![
                    ScanRunEventDto {
                        event_id: id(21),
                        scan_id,
                        library_id: library_id(),
                        sequence: 11,
                        event_kind: "progress".to_string(),
                        status: "running".to_string(),
                        status_label: "Running".to_string(),
                        status_message: "Scan is processing library changes."
                            .to_string(),
                        correlation_id: id(30),
                        idempotency_key: "event-11".to_string(),
                        subject_key: None,
                        current_path: None,
                        occurred_at: ts(11),
                        completed_items: 5,
                        total_items: 10,
                        retrying_items: 0,
                        dead_lettered_items: 0,
                        payload: serde_json::Value::Null,
                    },
                    ScanRunEventDto {
                        event_id: id(22),
                        scan_id,
                        library_id: library_id(),
                        sequence: 12,
                        event_kind: "progress".to_string(),
                        status: "running".to_string(),
                        status_label: "Running".to_string(),
                        status_message: "Scan is processing library changes."
                            .to_string(),
                        correlation_id: id(30),
                        idempotency_key: "event-12".to_string(),
                        subject_key: None,
                        current_path: None,
                        occurred_at: ts(12),
                        completed_items: 6,
                        total_items: 10,
                        retrying_items: 0,
                        dead_lettered_items: 0,
                        payload: serde_json::Value::Null,
                    },
                ],
                page: page(2),
                replay: Some(replay.clone()),
            },
            failures: ScanRunFailuresPageResponse {
                scan_id,
                failures: Vec::new(),
                page: page(0),
            },
        });

        assert_eq!(state.selected_events.len(), 2);
        assert_eq!(state.selected_replay, Some(replay));
        assert_eq!(state.selected_run_state, ScanDashboardLoadState::Loaded);
    }

    #[test]
    fn failure_summary_mapping_stays_display_safe() {
        let display = scan_failure_display_text(
            "filesystem_permission",
            "scan.folder_permission_denied",
        );
        assert_eq!(display.label, "Permission issue");
        assert!(display.message.contains("permissions"));
        assert!(!display.message.contains("dead-letter"));

        let fallback = scan_failure_display_text(
            "postgres_dead_letter",
            "debug.internal.error",
        );
        assert_eq!(fallback.label, "Scan item failed");
        assert!(!fallback.message.contains("postgres"));

        let failure = ScanFailureDto {
            scan_id: id(3),
            library_id: library_id(),
            subject_key: "library:/missing".to_string(),
            category: "filesystem_permission".to_string(),
            category_label: display.label,
            message_code: "scan.folder_permission_denied".to_string(),
            message: display.message,
            occurrences: 2,
            first_seen_at: ts(1),
            last_seen_at: ts(2),
            retryable: true,
            debug: None,
        };
        assert_eq!(failure.category_label, "Permission issue");
        assert!(failure.retryable);
    }

    #[test]
    fn terminology_display_mapping_handles_safe_aliases() {
        let finishing = scan_status_display_text("quiescing");
        assert_eq!(finishing.label, "Finishing");

        let canceled = scan_status_display_text("cancelled");
        assert_eq!(canceled.label, "Canceled");
        assert!(is_terminal_status("cancelled"));

        let unknown = scan_status_display_text("postgres_dead_letter");
        assert_eq!(unknown.label, "Scan update");
        assert_eq!(unknown.message, "Scan status changed.");
    }

    #[test]
    fn overview_filters_active_and_keeps_recent_terminal_runs() {
        let scan_id = id(4);
        let terminal_id = id(5);
        let mut state = ScanDashboardState::default();
        state.begin_overview_load(ScanDashboardRefreshReason::UserRequested);
        state.apply_overview(ScanDashboardOverviewPayload {
            health: health(),
            active_runs: vec![
                run(scan_id, "running", 3),
                run(terminal_id, "completed", 4),
            ],
            recent_runs: ScanRunListResponse {
                runs: vec![
                    run(scan_id, "running", 3),
                    run(terminal_id, "completed", 4),
                ],
                page: page(2),
            },
        });

        assert_eq!(state.overview_state, ScanDashboardLoadState::Loaded);
        assert!(!state.overview_refresh_in_flight);
        assert_eq!(state.active_runs.len(), 1);
        assert!(state.terminal_summaries.contains_key(&terminal_id));
        assert_eq!(
            state.last_refresh_reason,
            ScanDashboardRefreshReason::UserRequested
        );
    }

    #[test]
    fn telemetry_refresh_requests_are_deduped_and_rate_limited() {
        let mut state = ScanDashboardState::default();
        let now = Instant::now();

        assert!(state.try_begin_overview_load(
            ScanDashboardRefreshReason::MediaScanEvent,
            now
        ));
        assert!(state.overview_refresh_in_flight);
        assert!(!state.try_begin_overview_load(
            ScanDashboardRefreshReason::MediaScanEvent,
            now + Duration::from_millis(1)
        ));

        state.fail_overview_load("network unavailable");
        assert!(!state.overview_refresh_in_flight);

        let repeated_starts = (0..100)
            .filter(|offset_ms| {
                let request_time = now + Duration::from_millis(10 + *offset_ms);
                let started = state.try_begin_overview_load(
                    ScanDashboardRefreshReason::MediaScanEvent,
                    request_time,
                );
                if started {
                    state.fail_overview_load("done");
                }
                started
            })
            .count();
        assert_eq!(repeated_starts, 0);

        assert!(state.try_begin_overview_load(
            ScanDashboardRefreshReason::MediaScanEvent,
            now + MEDIA_SCAN_EVENT_REFRESH_MIN_INTERVAL
        ));
    }

    #[test]
    fn prompt_refresh_requests_bypass_telemetry_rate_limit() {
        let mut state = ScanDashboardState::default();
        let now = Instant::now();

        assert!(state.try_begin_overview_load(
            ScanDashboardRefreshReason::MediaScanEvent,
            now
        ));
        state.fail_overview_load("network unavailable");
        assert!(!state.try_begin_overview_load(
            ScanDashboardRefreshReason::MediaScanEvent,
            now + Duration::from_millis(100)
        ));

        for reason in [
            ScanDashboardRefreshReason::UserRequested,
            ScanDashboardRefreshReason::CommandAccepted,
            ScanDashboardRefreshReason::RecoveryAccepted,
            ScanDashboardRefreshReason::TerminalProgress,
        ] {
            assert!(
                state.try_begin_overview_load(
                    reason,
                    now + Duration::from_millis(100)
                ),
                "{reason:?} should bypass telemetry rate limiting"
            );
            assert_eq!(state.last_refresh_reason, reason);
            state.fail_overview_load("done");
        }
    }
}
