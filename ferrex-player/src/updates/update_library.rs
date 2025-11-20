use crate::{
    media_library,
    messages::library,
    state::{State, ViewMode, ViewState},
};
use iced::Task;

/// Handle library domain messages
pub fn update_library(state: &mut State, message: library::Message) -> Task<library::Message> {
    match message {
        // Core library loading
        library::Message::LibraryLoaded(result) => {
            // Legacy message - no handler exists
            log::warn!("LibraryLoaded: Legacy message received - no handler available");
            Task::none()
        }

        library::Message::MoviesLoaded(result) => {
            // Legacy message - no handler exists
            log::warn!("MoviesLoaded: Legacy message received - no handler available");
            Task::none()
        }

        //library::Message::TvShowsLoaded(result) => {
        //    super::tv_loaded::handle_tv_shows_loaded(state, result)
        //}
        library::Message::RefreshLibrary => super::refresh_library::handle_refresh_library(state),

        // Library management
        library::Message::LibrariesLoaded(result) => {
            super::library_loaded::handle_libraries_loaded(state, result)
        }

        library::Message::LoadLibraries => {
            // handle_load_libraries returns Task<DomainMessage>, convert to Task<library::Message>
            super::library_loaded::handle_load_libraries(state).map(|domain_msg| match domain_msg {
                crate::messages::DomainMessage::Library(lib_msg) => lib_msg,
                _ => {
                    log::warn!("Unexpected domain message from handle_load_libraries");
                    library::Message::NoOp
                }
            })
        }

        library::Message::CreateLibrary(library) => {
            super::library_management::handle_create_library(state, library)
        }

        library::Message::LibraryCreated(result) => {
            super::library_management::handle_library_created(state, result)
        }

        library::Message::UpdateLibrary(library) => {
            super::library_management::handle_update_library(state, library)
        }

        library::Message::LibraryUpdated(result) => {
            super::library_management::handle_library_updated(state, result)
        }

        library::Message::DeleteLibrary(library_id) => {
            super::library_management::handle_delete_library(state, library_id)
        }

        library::Message::LibraryDeleted(result) => {
            super::library_management::handle_library_deleted(state, result)
        }

        library::Message::SelectLibrary(library_id) => {
            super::select_library::handle_select_library(state, library_id)
        }

        library::Message::LibrarySelected(_library_id, _result) => {
            // Legacy handler removed - using reference-based API
            Task::none()
        }

        library::Message::ScanLibrary_(library_id) => {
            super::scan_updates::handle_scan_library(state, library_id)
        }

        // Library form management - using actual handlers
        library::Message::ShowLibraryForm(library) => {
            super::library_management::handle_show_library_form(state, library)
        }

        library::Message::HideLibraryForm => {
            super::library_management::handle_hide_library_form(state)
        }

        library::Message::UpdateLibraryFormName(name) => {
            super::library_management::handle_update_libarary_form_name(state, name)
        }

        library::Message::UpdateLibraryFormType(library_type) => {
            super::library_management::handle_update_library_form_type(state, library_type)
        }

        library::Message::UpdateLibraryFormPaths(paths) => {
            super::library_management::handle_update_library_form_paths(state, paths)
        }

        library::Message::UpdateLibraryFormScanInterval(interval) => {
            super::library_management::handle_update_library_form_scan_interval(state, interval)
        }

        library::Message::ToggleLibraryFormEnabled => {
            super::library_management::handle_toggle_library_form_enabled(state)
        }

        library::Message::SubmitLibraryForm => {
            super::library_management::handle_submit_library_form(state)
        }

        // Scanning
        library::Message::ScanLibrary => super::scan_updates::handle_scan_all_libraries(state),

        library::Message::ScanStarted(result) => {
            super::scan_updates::handle_scan_started(state, result)
        }

        library::Message::ScanProgressUpdate(progress) => {
            super::scan_updates::handle_scan_progress_update(state, progress)
        }

        library::Message::ScanCompleted(result) => {
            // Inline handler from update.rs
            state.scanning = false;
            match result {
                Ok(msg) => {
                    log::info!("Scan completed: {}", msg);
                    // Refresh library after successful scan
                    super::refresh_library::handle_refresh_library(state)
                }
                Err(e) => {
                    log::error!("Scan failed: {}", e);
                    state.error_message = Some(format!("Scan failed: {}", e));
                    Task::none()
                }
            }
        }

        library::Message::ClearScanProgress => {
            super::scan_updates::handle_clear_scan_progress(state)
        }

        library::Message::ToggleScanProgress => {
            super::scan_updates::handle_toggle_scan_progress(state)
        }

        library::Message::CheckActiveScans => {
            // No handler exists for this message
            log::warn!("CheckActiveScans: No handler available");
            Task::none()
        }

        library::Message::ActiveScansChecked(scans) => {
            super::scan_updates::handle_active_scans_checked(state, scans)
        }

        // Navigation
        library::Message::BackToLibrary => {
            // This is handled as part of PlayerMessage::BackToLibrary in update.rs
            // For library domain, we just navigate back to library view
            state.view = ViewState::Library;
            state.view_mode = crate::state::ViewMode::All;

            // Clear detail view data
            state.current_show_seasons.clear();
            state.current_season_episodes.clear();

            // The scroll position will be restored automatically by the view

            Task::none()
        }

        // Media references - inline handlers from update.rs
        library::Message::LibraryMediaReferencesLoaded(result) => {
            match result {
                Ok(response) => {
                    log::info!("Loaded {} media references", response.media.len());
                    let mut tasks = state.process_media_references(response);

                    // If we're in "All" view with no current library, aggregate after loading
                    if state.current_library_id.is_none()
                        && state.view_mode == crate::state::ViewMode::All
                    {
                        log::info!("In All view mode, triggering aggregation after library load");
                        // Add a task to aggregate all cached libraries
                        tasks.push(Task::perform(async {}, |_| {
                            library::Message::AggregateAllLibraries
                        }));
                    }

                    Task::batch(tasks)
                }
                Err(e) => {
                    log::error!("Failed to load media references: {}", e);
                    state.error_message = Some(format!("Failed to load media: {}", e));
                    state.loading = false;
                    Task::none()
                }
            }
        }

        library::Message::AllLibrariesLoaded(results) => {
            log::info!(
                "All libraries loaded in parallel: {} results",
                results.len()
            );
            let mut all_tasks = Vec::new();
            let mut failed_count = 0;

            // Process each library result
            for (library_id, result) in results {
                match result {
                    Ok(response) => {
                        log::info!(
                            "Library {} loaded successfully with {} items",
                            library_id,
                            response.media.len()
                        );
                        let tasks = state.process_media_references(response);
                        all_tasks.extend(tasks);
                    }
                    Err(e) => {
                        log::error!("Failed to load library {}: {}", library_id, e);
                        failed_count += 1;
                    }
                }
            }

            // After processing all libraries, aggregate them
            if failed_count == 0 {
                log::info!("All libraries loaded successfully, triggering aggregation");
                all_tasks.push(Task::perform(async {}, |_| {
                    library::Message::AggregateAllLibraries
                }));
            } else {
                log::warn!(
                    "{} libraries failed to load, still aggregating successful ones",
                    failed_count
                );
                all_tasks.push(Task::perform(async {}, |_| {
                    library::Message::AggregateAllLibraries
                }));
            }

            Task::batch(all_tasks)
        }

        library::Message::RefreshCurrentLibrary => {
            super::refresh_library::handle_refresh_library(state)
        }

        library::Message::ScanCurrentLibrary => {
            // TODO: Implement library-specific scanning
            log::info!("Scan current library requested");
            Task::none()
        }

        // Library aggregation
        library::Message::AggregateAllLibraries => {
            super::media_organization::handle_aggregate_all_libraries(state)
        }

        // Media events from server
        library::Message::MediaDiscovered(references) => {
            super::media_events_library::handle_media_discovered(state, references)
        }

        library::Message::MediaUpdated(reference) => {
            super::media_events_library::handle_media_updated(state, reference)
        }

        library::Message::MediaDeleted(id) => {
            super::media_events_library::handle_media_deleted(state, id)
        }

        // Internal cross-domain coordination
        library::Message::_EmitCrossDomainEvent(_) => {
            // This should be handled by the main update loop, not here
            log::warn!("_EmitCrossDomainEvent should be handled by main update loop");
            Task::none()
        }

        // No-op
        library::Message::NoOp => Task::none(),

        // Batch metadata handling
        library::Message::MediaDetailsBatch(references) => {
            log::info!(
                "Received batch of {} media references for metadata fetching",
                references.len()
            );
            // This is handled by the batch metadata fetcher
            Task::none()
        }

        library::Message::BatchMetadataComplete => {
            log::info!("[BatchMetadataFetcher] Complete message received - hiding loading spinner");
            state.loading = false;
            Task::none()
        }

        // View model updates
        library::Message::RefreshViewModels => {
            log::debug!("RefreshViewModels requested");
            Task::none()
        }

        // TV Shows loading
        library::Message::TvShowsLoaded(result) => {
            log::info!("TV shows loaded: {:?}", result.as_ref().map(|v| v.len()));
            Task::none()
        }
    }
}
