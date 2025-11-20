use std::time::Duration;

use crate::domains::library::messages::Message;
use crate::domains::library::server::scan::{start_scan_all_libraries, start_scan_library};
use crate::domains::library::LibraryDomainState;
use crate::infrastructure::services::api::ApiService;
use crate::state_refactored::State;
use ferrex_core::{LibraryID, ScanProgress, ScanStatus};
use iced::Task;
use uuid::Uuid;

pub fn handle_scan_library(
    state: &mut State,
    library_id: LibraryID,
    server_url: String,
) -> Task<Message> {
    log::info!("Starting scan for library: {}", library_id);
    state.domains.library.state.scanning = true;
    state.domains.library.state.scan_progress = None;
    let api_service = state.api_service.clone();
    Task::perform(
        start_scan_library(api_service, library_id, false),
        |result| match result {
            Ok(scan_id) => Message::ScanStarted(scan_id),
            Err(e) => Message::NoOp, //Message::ScanErrored(e.to_string()), // TODO: This is failing silently, we need to handle the error properly with a new message that indicates the failure
        },
    )
}

pub fn handle_scan_all_libraries(state: &mut State) -> Task<Message> {
    state.domains.library.state.scanning = true;
    state.domains.library.state.scan_progress = None;
    let api_service = state.api_service.clone();

    Task::perform(
        start_scan_all_libraries(api_service, false),
        |result| match result {
            Ok(scan_id) => Message::ScanStarted(scan_id),
            Err(e) => Message::NoOp, //Message::ScanErrored(e.to_string()), // TODO: This is failing silently, we need to handle the error properly with a new message that indicates the failure
        },
    )
}

pub fn handle_force_rescan(state: &mut State) -> Task<Message> {
    state.domains.library.state.scanning = true;
    state.domains.library.state.scan_progress = None;
    let api_service = state.api_service.clone();

    Task::perform(
        start_scan_all_libraries(api_service, false),
        |result| match result {
            Ok(scan_id) => Message::ScanStarted(scan_id),
            Err(e) => Message::NoOp, //Message::ScanErrored(e.to_string()), // TODO: This is failing silently, we need to handle the error properly with a new message that indicates the failure
        },
    )
}

pub fn handle_scan_started(state: &mut State, scan_id: Uuid) -> Task<Message> {
    log::info!("Scan started with ID: {}", scan_id);
    state.domains.library.state.active_scan_id = Some(scan_id);
    state.domains.library.state.show_scan_progress = true; // Auto-show progress overlay

    Task::none()
}

pub fn handle_scan_progress_update(state: &mut State, progress: ScanProgress) -> Task<Message> {
    log::info!(
        "Received scan progress update:\n\
        folders scanned: {}/{}\n\
        movies scanned: {}, series scanned: {}, seasons scanned: {}, episodes scanned: {}\n\
        Estimated time remaining: {} seconds",
        progress.folders_scanned,
        progress.folders_to_scan,
        progress.movies_scanned,
        progress.series_scanned,
        progress.seasons_scanned,
        progress.episodes_scanned,
        progress
            .estimated_time_remaining
            .unwrap_or(Duration::ZERO)
            .as_secs()
    );
    log::info!(
        "Scan progress state - show_scan_progress: {}, active_scan_id: {:?}",
        state.domains.library.state.show_scan_progress,
        state.domains.library.state.active_scan_id
    );

    state.domains.library.state.scan_progress = Some(progress.clone());
    log::info!(
        "Set scan_progress to Some - overlay should be visible if show_scan_progress is true"
    );

    // Check if scan is completed
    if progress.status == ScanStatus::Completed
        || progress.status == ScanStatus::Failed
        || progress.status == ScanStatus::Cancelled
    {
        state.domains.library.state.scanning = false;

        if progress.status == ScanStatus::Completed {
            // Refresh library after successful scan
            log::info!("Scan completed successfully, refreshing library");
            // Clear scan progress after a short delay
            Task::batch([
                Task::done(Message::RefreshLibrary),
                Task::perform(
                    async {
                        tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
                    },
                    |_| Message::ClearScanProgress,
                ),
            ])
        } else if progress.status == ScanStatus::Failed {
            // Error handling moved to higher level
            // Clear scan progress after a delay
            Task::perform(
                async {
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                },
                |_| Message::ClearScanProgress,
            )
        } else {
            // Cancelled - clear immediately
            state.domains.library.state.scan_progress = None;
            state.domains.library.state.active_scan_id = None;
            Task::none()
        }
    } else {
        Task::none()
    }
}

pub fn handle_clear_scan_progress(state: &mut State) -> Task<Message> {
    state.domains.library.state.scan_progress = None;
    state.domains.library.state.active_scan_id = None; // Clear active_scan_id when we clear the progress
    state.domains.library.state.show_scan_progress = false;
    Task::none()
}

pub fn handle_toggle_scan_progress(state: &mut State) -> Task<Message> {
    state.domains.library.state.show_scan_progress =
        !state.domains.library.state.show_scan_progress;
    log::info!(
        "Toggled scan progress overlay to: {}, scan_progress exists: {}",
        state.domains.library.state.show_scan_progress,
        state.domains.library.state.scan_progress.is_some()
    );
    Task::none()
}

pub fn handle_active_scans_checked(state: &mut State, scans: Vec<ScanProgress>) -> Task<Message> {
    if let Some(active_scan) = scans
        .into_iter()
        .find(|s| s.status == ScanStatus::Scanning || s.status == ScanStatus::Pending)
    {
        log::info!("Found active scan {}, reconnecting...", active_scan.scan_id);
        state.domains.library.state.active_scan_id = Some(active_scan.scan_id.clone());
        state.domains.library.state.scan_progress = Some(active_scan);
        state.domains.library.state.scanning = true;
        //state.show_scan_progress = true;
    }
    Task::none()
}
