use crate::domains::library::messages::LibraryMessage;
use crate::domains::library::server;
use crate::state::State;
use ferrex_core::player_prelude::{
    LibraryId, ScanLifecycleStatus, ScanProgressEvent, ScanSnapshotDto,
};
use iced::Task;
use uuid::Uuid;

pub fn handle_scan_library(
    state: &mut State,
    library_id: LibraryId,
) -> Task<LibraryMessage> {
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
                scan_id: response.scan_id,
                correlation_id: response.correlation_id,
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

pub fn apply_active_scan_snapshot(
    state: &mut State,
    snapshots: Vec<ScanSnapshotDto>,
) {
    if snapshots.is_empty() {
        log::debug!("Active scan snapshot list empty");
    } else {
        log::info!(
            "Received {} active scan snapshot(s) from server",
            snapshots.len()
        );
    }

    state.domains.library.state.active_scans.clear();
    for snapshot in snapshots {
        if matches!(
            snapshot.status,
            ScanLifecycleStatus::Completed
                | ScanLifecycleStatus::Failed
                | ScanLifecycleStatus::Canceled
        ) {
            continue;
        }
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
        .latest_progress
        .insert(frame.scan_id, frame.clone());

    if let Some(snapshot) = state
        .domains
        .library
        .state
        .active_scans
        .get_mut(&frame.scan_id)
    {
        snapshot.completed_items = frame.completed_items;
        snapshot.total_items = frame.total_items;
        snapshot.retrying_items =
            frame.retrying_items.unwrap_or(snapshot.retrying_items);
        snapshot.dead_lettered_items = frame
            .dead_lettered_items
            .unwrap_or(snapshot.dead_lettered_items);
        snapshot.current_path = frame.current_path.clone();

        if let Some(mapped) = map_status(&frame.status) {
            snapshot.status = mapped;
        }
    } else {
        log::warn!(
            "Progress frame received for scan {} but no active snapshot is registered",
            frame.scan_id
        );
    }
}

pub fn remove_scan(state: &mut State, scan_id: Uuid) {
    state.domains.library.state.active_scans.remove(&scan_id);
    state.domains.library.state.latest_progress.remove(&scan_id);
    log::info!("Removed scan {} from active tracking", scan_id);
}

fn map_status(status: &str) -> Option<ScanLifecycleStatus> {
    match status {
        "pending" => Some(ScanLifecycleStatus::Pending),
        "running" => Some(ScanLifecycleStatus::Running),
        "paused" => Some(ScanLifecycleStatus::Paused),
        "completed" => Some(ScanLifecycleStatus::Completed),
        "failed" => Some(ScanLifecycleStatus::Failed),
        "canceled" | "cancelled" => Some(ScanLifecycleStatus::Canceled),
        _ => None,
    }
}
