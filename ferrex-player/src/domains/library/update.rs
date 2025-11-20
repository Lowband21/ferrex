use crate::{
    common::messages::{CrossDomainEvent, DomainMessage, DomainUpdateResult},
    domains::media::library, domains::ui::types::ViewState, domains::ui::view_models::ViewModel,
    state_refactored::State,
};

use super::messages::Message;
use iced::Task;

/// Handle library domain messages
/// Returns a DomainUpdateResult containing both the task and any events to emit
pub fn update_library(state: &mut State, message: Message) -> DomainUpdateResult {
    match message {
        // Core library loading

        //Message::TvShowsLoaded(result) => {
        //    super::tv_loaded::handle_tv_shows_loaded(state, result)
        //}
        Message::RefreshLibrary => {
            let task = super::update_handlers::refresh_library::handle_refresh_library(state);
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        // Library management
        Message::LibrariesLoaded(result) => {
            let task = super::update_handlers::library_loaded::handle_libraries_loaded(state, result);
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        Message::LoadLibraries => {
            log::info!("[Library] LoadLibraries message received - loading libraries from server");
            let task = super::update_handlers::library_loaded::handle_load_libraries(state.server_url.clone());
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        Message::CreateLibrary(library) => {
            let task = super::update_handlers::library_management::handle_create_library(
                state,
                library,
                state.server_url.clone(),
            );
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        Message::LibraryCreated(result) => {
            let task = super::update_handlers::library_management::handle_library_created(state, result);
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        Message::UpdateLibrary(library) => {
            let task = super::update_handlers::library_management::handle_update_library(
                state,
                library,
                state.server_url.clone(),
            );
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        Message::LibraryUpdated(result) => {
            let task = super::update_handlers::library_management::handle_library_updated(state, result);
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        Message::DeleteLibrary(library_id) => {
            let task = super::update_handlers::library_management::handle_delete_library(
                state,
                library_id,
                state.server_url.clone(),
            );
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        Message::LibraryDeleted(result) => {
            let task = super::update_handlers::library_management::handle_library_deleted(state, result);
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        Message::SelectLibrary(library_id) => {
            // This handler returns DomainUpdateResult directly
            super::update_handlers::select_library::handle_select_library(state, library_id)
        }

        Message::LibrarySelected(_library_id, _result) => {
            // Legacy handler removed - using reference-based API
            DomainUpdateResult::task(Task::none())
        }

        Message::ScanLibrary(library_id) => {
            let task = super::update_handlers::scan_updates::handle_scan_library(
                state,
                library_id,
                state.server_url.clone(),
            );
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        // Library form management - using actual handlers
        Message::ShowLibraryForm(library) => {
            let task = super::update_handlers::library_management::handle_show_library_form(state, library);
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        Message::HideLibraryForm => {
            let task = super::update_handlers::library_management::handle_hide_library_form(state);
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        Message::UpdateLibraryFormName(name) => {
            let task = super::update_handlers::library_management::handle_update_libarary_form_name(
                state, name,
            );
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        Message::UpdateLibraryFormType(library_type) => {
            let task = super::update_handlers::library_management::handle_update_library_form_type(
                state,
                library_type,
            );
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        Message::UpdateLibraryFormPaths(paths) => {
            let task = super::update_handlers::library_management::handle_update_library_form_paths(
                state, paths,
            );
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        Message::UpdateLibraryFormScanInterval(interval) => {
            let task = super::update_handlers::library_management::handle_update_library_form_scan_interval(
                state, interval,
            );
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        Message::ToggleLibraryFormEnabled => {
            let task = super::update_handlers::library_management::handle_toggle_library_form_enabled(state);
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        Message::SubmitLibraryForm => {
            let task = super::update_handlers::library_management::handle_submit_library_form(state);
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        // Scanning - duplicate handler removed
        // Already handled above

        Message::ScanStarted(result) => {
            let task = super::update_handlers::scan_updates::handle_scan_started(state, result);
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        Message::ScanProgressUpdate(progress) => {
            let task = super::update_handlers::scan_updates::handle_scan_progress_update(state, progress);
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        Message::ScanCompleted(result) => {
            // Inline handler from update.rs
            state.domains.library.state.scanning = false;

            // NEW: Exit batch mode in MediaStore when scan completes
            if let Ok(mut store) = state.domains.media.state.media_store.write() {
                log::info!("Exiting batch mode in MediaStore - scan completed");
                store.end_batch();
            }
            match result {
                Ok(msg) => {
                    log::info!("Scan completed: {}", msg);
                    // Refresh library after successful scan
                    let task = super::update_handlers::refresh_library::handle_refresh_library(state);
                    DomainUpdateResult::task(task.map(DomainMessage::Library))
                }
                Err(e) => {
                    log::error!("Scan failed: {}", e);
                    state.domains.ui.state.error_message = Some(format!("Scan failed: {}", e));
                    DomainUpdateResult::task(Task::none())
                }
            }
        }

        Message::ClearScanProgress => {
            let task = super::update_handlers::scan_updates::handle_clear_scan_progress(state);
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        Message::ToggleScanProgress => {
            let task = super::update_handlers::scan_updates::handle_toggle_scan_progress(state);
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        Message::CheckActiveScans => {
            // No handler exists for this message
            log::warn!("CheckActiveScans: No handler available");
            DomainUpdateResult::task(Task::none())
        }

        Message::ActiveScansChecked(scans) => {
            let task = super::update_handlers::scan_updates::handle_active_scans_checked(state, scans);
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        // Navigation
        Message::BackToLibrary => {
            // This is handled as part of PlayerMessage::BackToLibrary in update.rs
            // For library domain, we just navigate back to library view
            state.domains.ui.state.view = ViewState::Library;

            // REMOVED: No longer clearing duplicate state fields
            // MediaStore is the single source of truth

            // The scroll position will be restored automatically by the view

            DomainUpdateResult::task(Task::none())
        }

        // Media references - inline handlers from update.rs
        Message::LibraryMediaReferencesLoaded(result) => match result {
            Ok(response) => {
                log::info!("Loaded {} media references", response.media.len());
                
                // Check if any media needs metadata fetching
                let library_id = response.library.id;
                let needs_metadata: Vec<_> = response.media.iter()
                    .filter(|m| {
                        // Check if media needs metadata - typically if it lacks details or has no TMDB ID
                        match m {
                            crate::infrastructure::api_types::MediaReference::Movie(movie) => 
                                movie.tmdb_id == 0 || matches!(movie.details, ferrex_core::MediaDetailsOption::Endpoint(_)),
                            crate::infrastructure::api_types::MediaReference::Series(series) => 
                                series.tmdb_id == 0 || matches!(series.details, ferrex_core::MediaDetailsOption::Endpoint(_)),
                            _ => false
                        }
                    })
                    .cloned()
                    .collect();
                
                // Process the media references (populates MediaStore)
                let tasks = state.process_media_references(response);
                
                // If items need metadata, emit the cross-domain event
                if !needs_metadata.is_empty() {
                    log::info!("Requesting batch metadata fetch for {} items", needs_metadata.len());
                    let library_data = vec![(library_id, needs_metadata)];
                    DomainUpdateResult::with_events(
                        Task::batch(tasks).map(DomainMessage::Library),
                        vec![CrossDomainEvent::RequestBatchMetadataFetch(library_data)]
                    )
                } else {
                    DomainUpdateResult::task(Task::batch(tasks).map(DomainMessage::Library))
                }
            }
            Err(e) => {
                log::error!("Failed to load media references: {}", e);
                state.domains.ui.state.error_message = Some(format!("Failed to load media: {}", e));
                state.loading = false;
                DomainUpdateResult::task(Task::none())
            }
        },

        Message::RefreshCurrentLibrary => {
            let task = super::update_handlers::refresh_library::handle_refresh_library(state);
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        Message::ScanCurrentLibrary => {
            // Scan the currently selected library if one is selected
            if let Some(library_id) = state.domains.library.state.current_library_id {
                log::info!("Scanning library: {}", library_id);
                let task = super::update_handlers::scan_updates::handle_scan_library(
                    state,
                    library_id,
                    state.server_url.clone(),
                );
                DomainUpdateResult::task(task.map(DomainMessage::Library))
            } else {
                log::warn!("No library currently selected to scan");
                DomainUpdateResult::task(Task::none())
            }
        }

        // Media events from server
        Message::MediaDiscovered(references) => {
            let task = crate::domains::media::update_handlers::media_events_library::handle_media_discovered(
                state, references,
            );
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        Message::MediaUpdated(reference) => {
            let task = crate::domains::media::update_handlers::media_events_library::handle_media_updated(
                state, reference,
            );
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        Message::MediaDeleted(id) => {
            let task = crate::domains::media::update_handlers::media_events_library::handle_media_deleted(
                state, id,
            );
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        // Note: _EmitCrossDomainEvent variant has been removed

        // No-op
        Message::NoOp => DomainUpdateResult::task(Task::none()),

        // Batch metadata handling
        Message::MediaDetailsBatch(references) => {
            log::info!(
                "Received batch of {} media references for metadata fetching",
                references.len()
            );
            // This is handled by the batch metadata fetcher
            DomainUpdateResult::task(Task::none())
        }

        Message::BatchMetadataComplete => {
            log::info!("[BatchMetadataFetcher] Complete message received - hiding loading spinner");
            state.loading = false;
            DomainUpdateResult::task(Task::none())
        }

        // View model updates
        Message::RefreshViewModels => {
            // This message is deprecated in Library domain - ViewModels are managed by UI domain
            // The UI domain handles RefreshViewModels to update its own ViewModels
            log::debug!("Library: RefreshViewModels is now handled by UI domain");
            DomainUpdateResult::task(Task::none())
        }

        // TV Shows loading
        Message::TvShowsLoaded(result) => {
            log::info!("TV shows loaded: {:?}", result.as_ref().map(|v| v.len()));
            DomainUpdateResult::task(Task::none())
        }
    }
}
