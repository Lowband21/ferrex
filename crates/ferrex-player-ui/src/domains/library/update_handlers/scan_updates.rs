use crate::domains::library::messages::LibraryMessage;
use crate::domains::library::server;
use crate::state::State;
use ferrex_core::player_prelude::{
    LibraryId, ScanProgressEvent, ScanRunMode, ScanSnapshotDto,
};
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

    state
        .domains
        .library
        .state
        .replace_active_scan_snapshots(snapshots);

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
