use crate::domains::library::LibraryDomainState;
use crate::domains::library::messages::Message;
use crate::domains::library::server::{start_library_scan, start_media_scan};
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
    Task::perform(
        start_library_scan(server_url, library_id, true), // Enable streaming
        |result| match result {
            Ok(scan_id) => Message::ScanStarted(Ok(scan_id)),
            Err(e) => Message::ScanStarted(Err(e.to_string())),
        },
    )
}

pub fn handle_scan_all_libraries(state: &mut State) -> Task<Message> {
    state.domains.library.state.scanning = true;
    state.domains.library.state.scan_progress = None;

    Task::perform(
        start_media_scan(state.server_url.clone(), false, true),
        |result| match result {
            Ok(scan_id) => Message::ScanStarted(Ok(scan_id)),
            Err(e) => Message::ScanStarted(Err(e.to_string())),
        },
    )
}

pub fn handle_force_rescan(state: &mut State) -> Task<Message> {
    state.domains.library.state.scanning = true;
    state.domains.library.state.scan_progress = None;

    Task::perform(
        start_media_scan(state.server_url.clone(), true, true),
        |result| match result {
            Ok(scan_id) => Message::ScanStarted(Ok(scan_id)),
            Err(e) => Message::ScanStarted(Err(e.to_string())),
        },
    )
}

pub fn handle_scan_started(state: &mut State, result: Result<String, String>) -> Task<Message> {
    match result {
        Ok(scan_id) => {
            log::info!("Scan started with ID: {}", scan_id);
            state.domains.library.state.active_scan_id = Some(scan_id);
            state.domains.library.state.show_scan_progress = true; // Auto-show progress overlay

            // NEW: Enter batch mode in MediaStore to prevent sorting during scan
            /*if let Ok(mut store) = state.domains.media.state.media_store.write() {
                log::info!("Entering batch mode in MediaStore for scan");
                store.begin_batch();
            }*/

            Task::none()
        }
        Err(e) => {
            log::error!("Failed to start scan: {}", e);
            state.domains.library.state.scanning = false;
            // Error handling moved to higher level
            Task::none()
        }
    }
}

pub fn handle_scan_progress_update(state: &mut State, progress: ScanProgress) -> Task<Message> {
    log::info!(
        "Received scan progress update: {} files scanned, {} stored, {} metadata fetched",
        progress.scanned_files,
        progress.stored_files,
        progress.metadata_fetched
    );
    log::info!(
        "Scan progress state - show_scan_progress: {}, active_scan_id: {:?}",
        state.domains.library.state.show_scan_progress,
        state.domains.library.state.active_scan_id
    );

    let previous_stored = state
        .domains
        .library
        .state
        .scan_progress
        .as_ref()
        .map(|p| p.stored_files)
        .unwrap_or(0);

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

        /*
        // NEW: Exit batch mode in MediaStore when scan completes
        if let Ok(mut store) = state.domains.library.state.repo_access.write() {
            log::info!(
                "Exiting batch mode in MediaStore - scan complete with status: {:?}",
                progress.status
            );
            store.end_batch();
        } */
        // Don't clear active_scan_id yet - keep it until we clear scan_progress

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
        // If new files were stored, trigger an incremental update
        if progress.stored_files > previous_stored {
            // No longer triggering incremental updates - using SSE events instead
            Task::none()
        } else {
            Task::none()
        }
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
        .find(|s| s.status == ScanStatus::Scanning || s.status == ScanStatus::Processing)
    {
        log::info!("Found active scan {}, reconnecting...", active_scan.scan_id);
        state.domains.library.state.active_scan_id = Some(active_scan.scan_id.clone());
        state.domains.library.state.scan_progress = Some(active_scan);
        state.domains.library.state.scanning = true;
        //state.show_scan_progress = true;
    }
    Task::none()
}
