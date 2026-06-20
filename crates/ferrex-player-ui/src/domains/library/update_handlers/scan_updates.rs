use crate::domains::library::messages::LibraryMessage;
use crate::domains::library::server;
use crate::state::State;
use ferrex_core::player_prelude::{
    LibraryId, ScanLifecycleStatus, ScanPathReasonCategory,
    ScanPathReasonDetail, ScanProgressEvent, ScanRecoveryRequest, ScanRunMode,
    ScanSnapshotDto,
};
use ferrex_player_library::scan_dashboard::ScanDashboardRefreshReason;
use iced::Task;
use uuid::Uuid;

pub fn handle_scan_library(
    state: &mut State,
    library_id: LibraryId,
) -> Task<LibraryMessage> {
    if state
        .domains
        .library
        .state
        .active_scan_by_library_mode(library_id, ScanRunMode::Manual)
        .is_some()
    {
        log::info!(
            "Scan request ignored for library {} because a manual scan is already active",
            library_id
        );
        return Task::done(LibraryMessage::FetchActiveScans);
    }

    if !state
        .domains
        .library
        .state
        .begin_scan_start(library_id, ScanRunMode::Manual)
    {
        log::debug!(
            "Scan request ignored for library {} because a manual scan start is already pending",
            library_id
        );
        return Task::none();
    }

    let api_service = state.api_service.clone();
    Task::perform(
        async move {
            server::scan::start_library_scan(api_service, library_id, None)
                .await
                .map_err(|e| e.to_string())
        },
        move |result| match result {
            Ok(response) => LibraryMessage::ScanStarted {
                library_id,
                response,
            },
            Err(error) => LibraryMessage::ScanCommandFailed {
                library_id: Some(library_id),
                error,
            },
        },
    )
}

pub fn handle_pause_scan(
    state: &mut State,
    library_id: LibraryId,
    scan_id: Uuid,
) -> Task<LibraryMessage> {
    let api_service = state.api_service.clone();
    Task::perform(
        async move {
            server::scan::pause_library_scan(api_service, library_id, scan_id)
                .await
                .map_err(|e| e.to_string())
        },
        move |result| match result {
            Ok(_) => LibraryMessage::FetchActiveScans,
            Err(error) => LibraryMessage::ScanCommandFailed {
                library_id: Some(library_id),
                error,
            },
        },
    )
}

pub fn handle_resume_scan(
    state: &mut State,
    library_id: LibraryId,
    scan_id: Uuid,
) -> Task<LibraryMessage> {
    let api_service = state.api_service.clone();
    Task::perform(
        async move {
            server::scan::resume_library_scan(api_service, library_id, scan_id)
                .await
                .map_err(|e| e.to_string())
        },
        move |result| match result {
            Ok(_) => LibraryMessage::FetchActiveScans,
            Err(error) => LibraryMessage::ScanCommandFailed {
                library_id: Some(library_id),
                error,
            },
        },
    )
}

pub fn handle_cancel_scan(
    state: &mut State,
    library_id: LibraryId,
    scan_id: Uuid,
) -> Task<LibraryMessage> {
    let api_service = state.api_service.clone();
    Task::perform(
        async move {
            server::scan::cancel_library_scan(api_service, library_id, scan_id)
                .await
                .map_err(|e| e.to_string())
        },
        move |result| match result {
            Ok(_) => LibraryMessage::FetchActiveScans,
            Err(error) => LibraryMessage::ScanCommandFailed {
                library_id: Some(library_id),
                error,
            },
        },
    )
}

pub fn handle_fetch_active_scans(state: &mut State) -> Task<LibraryMessage> {
    let api_service = state.api_service.clone();
    Task::perform(
        async move {
            server::scan::fetch_active_scans(api_service)
                .await
                .map_err(|e| e.to_string())
        },
        |result| match result {
            Ok(scans) => LibraryMessage::ActiveScansUpdated(scans),
            Err(error) => LibraryMessage::ScanCommandFailed {
                library_id: None,
                error,
            },
        },
    )
}

pub fn handle_fetch_scan_metrics(state: &mut State) -> Task<LibraryMessage> {
    let api = state.api_service.clone();
    Task::perform(
        async move { api.fetch_scan_metrics().await.map_err(|e| e.to_string()) },
        |result| match result {
            Ok(metrics) => LibraryMessage::ScanMetricsLoaded(Ok(metrics)),
            Err(err) => LibraryMessage::ScanMetricsLoaded(Err(err)),
        },
    )
}

pub fn handle_fetch_scan_config(state: &mut State) -> Task<LibraryMessage> {
    let api = state.api_service.clone();
    Task::perform(
        async move { api.fetch_scan_config().await.map_err(|e| e.to_string()) },
        |result| match result {
            Ok(cfg) => LibraryMessage::ScanConfigLoaded(Ok(cfg)),
            Err(err) => LibraryMessage::ScanConfigLoaded(Err(err)),
        },
    )
}

pub fn handle_refresh_scan_dashboard(
    state: &mut State,
    reason: ScanDashboardRefreshReason,
) -> Task<LibraryMessage> {
    state
        .domains
        .library
        .state
        .scan_dashboard
        .begin_overview_load(reason);
    let api = state.api_service.clone();
    Task::perform(
        async move {
            server::scan::fetch_scan_dashboard_overview(api)
                .await
                .map_err(|e| e.to_string())
        },
        LibraryMessage::ScanDashboardOverviewLoaded,
    )
}

pub fn handle_select_scan_dashboard_run(
    state: &mut State,
    scan_id: Uuid,
) -> Task<LibraryMessage> {
    state
        .domains
        .library
        .state
        .scan_dashboard
        .begin_run_load(scan_id);
    handle_fetch_scan_dashboard_run(state, scan_id, true)
}

pub fn handle_refresh_scan_dashboard_run(
    state: &mut State,
    scan_id: Uuid,
) -> Task<LibraryMessage> {
    if state.domains.library.state.scan_dashboard.selected_run_id
        == Some(scan_id)
    {
        state
            .domains
            .library
            .state
            .scan_dashboard
            .begin_run_load(scan_id);
    }
    handle_fetch_scan_dashboard_run(state, scan_id, false)
}

fn handle_fetch_scan_dashboard_run(
    state: &State,
    scan_id: Uuid,
    select: bool,
) -> Task<LibraryMessage> {
    let api = state.api_service.clone();
    Task::perform(
        async move {
            server::scan::fetch_scan_dashboard_run(api, scan_id)
                .await
                .map_err(|e| e.to_string())
        },
        move |result| LibraryMessage::ScanDashboardRunLoaded {
            scan_id,
            select,
            result,
        },
    )
}

pub fn handle_recover_scan_path(
    state: &mut State,
    request: ScanRecoveryRequest,
) -> Task<LibraryMessage> {
    let api = state.api_service.clone();
    Task::perform(
        async move {
            server::scan::recover_scan_path(api, request)
                .await
                .map_err(|e| e.to_string())
        },
        LibraryMessage::ScanRecoveryCompleted,
    )
}

pub fn apply_active_scan_snapshot(
    state: &mut State,
    snapshots: Vec<ScanSnapshotDto>,
) {
    state
        .domains
        .library
        .state
        .scan_dashboard
        .apply_active_snapshots(&snapshots);

    if snapshots.is_empty() {
        log::debug!("Active scan snapshot list empty");
    } else {
        log::info!(
            "Received {} active scan snapshot(s) from server",
            snapshots.len()
        );
    }

    let recoverable_terminal_snapshots: Vec<_> = snapshots
        .iter()
        .filter(|snapshot| {
            !snapshot.status.is_active() && should_track_scan_snapshot(snapshot)
        })
        .cloned()
        .collect();

    state
        .domains
        .library
        .state
        .replace_active_scan_snapshots(snapshots);

    for snapshot in recoverable_terminal_snapshots {
        state
            .domains
            .library
            .state
            .active_scans
            .insert(snapshot.scan_id, snapshot);
    }

    if state.domains.library.state.active_scans.is_empty() {
        log::debug!("No running scans after filtering terminal statuses");
    }
}

pub fn apply_scan_progress_frame(state: &mut State, frame: ScanProgressEvent) {
    log::debug!(
        "Scan progress frame received: scan={}, seq={}, status={}, completed={}/{}",
        frame.scan_id,
        frame.sequence,
        frame.status,
        frame.completed_items,
        frame.total_items
    );

    state
        .domains
        .library
        .state
        .scan_dashboard
        .apply_progress_frame(&frame);

    if !state
        .domains
        .library
        .state
        .apply_scan_progress_frame(frame.clone())
    {
        log::warn!(
            "Progress frame received for scan {} but no active snapshot is registered",
            frame.scan_id
        );
    }
}

pub fn remove_scan(state: &mut State, scan_id: Uuid) {
    state.domains.library.state.remove_active_scan(scan_id);
    log::info!("Removed scan {} from active tracking", scan_id);
}

pub(crate) fn should_track_scan_snapshot(snapshot: &ScanSnapshotDto) -> bool {
    match snapshot.status {
        ScanLifecycleStatus::Completed | ScanLifecycleStatus::Failed => {
            snapshot_has_recovery_affordance(snapshot)
        }
        ScanLifecycleStatus::Canceled => false,
        ScanLifecycleStatus::Pending
        | ScanLifecycleStatus::Running
        | ScanLifecycleStatus::Paused => true,
    }
}

pub(crate) fn snapshot_has_recovery_affordance(
    snapshot: &ScanSnapshotDto,
) -> bool {
    scan_has_recovery_affordance(
        snapshot.needs_attention_items,
        snapshot.failed_items,
        snapshot.skipped_items,
        &snapshot.reason_details,
    ) || (matches!(snapshot.status, ScanLifecycleStatus::Failed)
        && snapshot.skipped_items > 0)
}

pub(crate) fn progress_frame_has_recovery_affordance(
    frame: &ScanProgressEvent,
) -> bool {
    scan_has_recovery_affordance(
        frame.needs_attention_items,
        frame.failed_items,
        frame.skipped_items,
        &frame.reason_details,
    ) || matches!(
        frame.status.as_str(),
        "failed_needs_attention" | "needs_attention" | "skipped"
    )
}

fn scan_has_recovery_affordance(
    needs_attention_items: u64,
    failed_items: u64,
    skipped_items: u64,
    reason_details: &[ScanPathReasonDetail],
) -> bool {
    needs_attention_items > 0
        || failed_items > 0
        || (skipped_items > 0
            && reason_details.iter().any(reason_detail_needs_rescan))
        || reason_details.iter().any(reason_detail_needs_rescan)
}

fn reason_detail_needs_rescan(detail: &ScanPathReasonDetail) -> bool {
    match detail.category {
        ScanPathReasonCategory::NeedsAttention => true,
        ScanPathReasonCategory::Skipped => matches!(
            detail.reason_code.as_str(),
            "path_missing"
                | "no_supported_media_found"
                | "unsupported_media_layout"
                | "skipped"
        ),
        ScanPathReasonCategory::KnownUnchanged
        | ScanPathReasonCategory::Retrying => false,
    }
}

#[cfg(test)]
fn map_status(status: &str) -> Option<ScanLifecycleStatus> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, Utc};
    use ferrex_core::player_prelude::ScanStageLatencySummary;

    fn fixed_time() -> DateTime<Utc> {
        DateTime::<Utc>::from_timestamp(1_700_000_000, 0)
            .expect("valid fixed timestamp")
    }

    fn reason_detail(
        category: ScanPathReasonCategory,
        reason_code: &str,
    ) -> ScanPathReasonDetail {
        ScanPathReasonDetail {
            category,
            reason_code: reason_code.to_string(),
            message: Some(
                "Review this path and rescan when it is ready".into(),
            ),
            path: Some("/media/missing".into()),
            path_key: None,
            retryable: false,
            action_hint: Some("rescan_library".into()),
        }
    }

    fn snapshot(status: ScanLifecycleStatus) -> ScanSnapshotDto {
        let scan_id = Uuid::now_v7();
        let library_id = LibraryId::new();
        ScanSnapshotDto {
            scan_id,
            library_id,
            status,
            mode: ScanRunMode::Manual,
            completed_items: 0,
            total_items: 3,
            validated_items: 0,
            known_unchanged_items: 0,
            skipped_items: 0,
            failed_items: 0,
            needs_attention_items: 0,
            retrying_items: 0,
            correlation_id: scan_id,
            idempotency_key: "scan:test:1".into(),
            run_key: ScanRunMode::Manual.run_key(library_id),
            disposition: None,
            current_path: None,
            started_at: fixed_time(),
            terminal_at: None,
            sequence: 1,
            reason_details: Vec::new(),
        }
    }

    fn progress_frame(
        scan_id: Uuid,
        library_id: LibraryId,
    ) -> ScanProgressEvent {
        ScanProgressEvent {
            version: "2".into(),
            scan_id,
            library_id,
            status: "failed_needs_attention".into(),
            completed_items: 2,
            total_items: 4,
            validated_items: 1,
            known_unchanged_items: 1,
            skipped_items: 1,
            failed_items: 1,
            needs_attention_items: 1,
            retrying_items: 0,
            sequence: 7,
            current_path: Some("/media/missing".into()),
            path_key: None,
            p95_stage_latencies_ms: ScanStageLatencySummary {
                scan: 10,
                analyze: 20,
                index: 30,
            },
            correlation_id: scan_id,
            idempotency_key: "scan:test:7".into(),
            emitted_at: fixed_time(),
            terminal_at: Some(fixed_time()),
            reason_details: vec![reason_detail(
                ScanPathReasonCategory::NeedsAttention,
                "path_missing",
            )],
        }
    }

    #[test]
    fn maps_progress_statuses_to_lifecycle_states() {
        assert_eq!(
            map_status("processing"),
            Some(ScanLifecycleStatus::Running)
        );
        assert_eq!(map_status("quiescing"), Some(ScanLifecycleStatus::Running));
        assert_eq!(
            map_status("failed_needs_attention"),
            Some(ScanLifecycleStatus::Failed)
        );
        assert_eq!(map_status("skipped"), Some(ScanLifecycleStatus::Failed));
    }

    #[tokio::test]
    async fn active_snapshot_keeps_recoverable_terminal_scans_only() {
        let mut recoverable = snapshot(ScanLifecycleStatus::Failed);
        recoverable.needs_attention_items = 1;
        recoverable.failed_items = 1;
        recoverable.reason_details = vec![reason_detail(
            ScanPathReasonCategory::NeedsAttention,
            "path_missing",
        )];

        let mut no_media = snapshot(ScanLifecycleStatus::Completed);
        no_media.skipped_items = 1;
        no_media.reason_details = vec![reason_detail(
            ScanPathReasonCategory::Skipped,
            "no_supported_media_found",
        )];

        let plain_failed = snapshot(ScanLifecycleStatus::Failed);
        let canceled = snapshot(ScanLifecycleStatus::Canceled);

        let mut state = State::new("http://localhost:3000".into());
        apply_active_scan_snapshot(
            &mut state,
            vec![
                recoverable.clone(),
                no_media.clone(),
                plain_failed,
                canceled,
            ],
        );

        assert!(
            state
                .domains
                .library
                .state
                .active_scans
                .contains_key(&recoverable.scan_id)
        );
        assert!(
            state
                .domains
                .library
                .state
                .active_scans
                .contains_key(&no_media.scan_id)
        );
        assert_eq!(state.domains.library.state.active_scans.len(), 2);
    }

    #[tokio::test]
    async fn progress_frame_maps_safe_counters_and_reason_details() {
        let mut state = State::new("http://localhost:3000".into());
        let base = snapshot(ScanLifecycleStatus::Running);
        let scan_id = base.scan_id;
        let library_id = base.library_id;
        state
            .domains
            .library
            .state
            .active_scans
            .insert(scan_id, base);

        let frame = progress_frame(scan_id, library_id);
        apply_scan_progress_frame(&mut state, frame.clone());

        let updated = state
            .domains
            .library
            .state
            .active_scans
            .get(&scan_id)
            .expect("scan remains tracked");
        assert_eq!(updated.status, ScanLifecycleStatus::Failed);
        assert_eq!(updated.validated_items, frame.validated_items);
        assert_eq!(updated.known_unchanged_items, frame.known_unchanged_items);
        assert_eq!(updated.skipped_items, frame.skipped_items);
        assert_eq!(updated.failed_items, frame.failed_items);
        assert_eq!(updated.needs_attention_items, frame.needs_attention_items);
        assert_eq!(updated.retrying_items, frame.retrying_items);
        assert_eq!(updated.reason_details, frame.reason_details);
        assert!(progress_frame_has_recovery_affordance(&frame));
    }
}
