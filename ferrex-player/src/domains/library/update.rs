#[cfg(feature = "demo")]
use crate::infra::api_types::DemoStatus;
use crate::{
    common::focus::{FocusArea, FocusMessage},
    common::messages::{DomainMessage, DomainUpdateResult},
    domains::{
        library::update_handlers::fetch_libraries,
        ui::tabs::{TabId, TabState},
    },
    infra::api_types::Media,
    state::State,
};

use super::messages::Message;
use iced::Task;
use std::collections::{HashMap, HashSet};

use ferrex_core::player_prelude::{
    ImageRequest, ImageSize, ImageType, LibraryID, MediaIDLike, MediaOps,
    Priority, ScanLifecycleStatus, ScanSnapshotDto,
};
#[cfg(feature = "demo")]
use ferrex_model::library::LibraryType;

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn update_library(
    state: &mut State,
    message: Message,
) -> DomainUpdateResult {
    match message {
        // Core library loading

        //Message::TvShowsLoaded(result) => {
        //    super::tv_loaded::handle_tv_shows_loaded(state, result)
        //}
        Message::RefreshLibrary => {
            let task =
                super::update_handlers::refresh_library::handle_refresh_library(
                    state,
                );
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        // Library management
        Message::LibrariesLoaded(result) => {
            let task =
                super::update_handlers::library_loaded::handle_libraries_loaded(
                    state, result,
                );
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        Message::LoadLibraries => {
            let task = if !state.domains.library.state.initial_library_fetch {
                state.domains.library.state.initial_library_fetch = true;
                Task::perform(
                    fetch_libraries(state.api_service.clone()),
                    |result| {
                        Message::LibrariesLoaded(
                            result.map_err(|e| e.to_string()),
                        )
                    },
                )
            } else {
                log::warn!(
                    "The libraries are already loaded, why is another attempt being made?"
                );
                Task::none()
            };
            log::info!(
                "LoadLibraries message received - loading libraries from server"
            );
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        Message::CreateLibrary {
            library,
            start_scan,
        } => {
            let task = super::update_handlers::library_management::handle_create_library(
                state,
                library,
                start_scan,
                state.server_url.clone(),
            );
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        Message::LibraryCreated(result) => {
            let task =
                super::update_handlers::library_management::handle_library_created(state, result);
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
            let task =
                super::update_handlers::library_management::handle_library_updated(state, result);
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
            let task =
                super::update_handlers::library_management::handle_library_deleted(state, result);
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        Message::SelectLibrary(library_id) => {
            // This handler returns DomainUpdateResult directly
            super::update_handlers::select_library::handle_select_library(
                state, library_id,
            )
        }

        Message::LibrarySelected(_library_id, _result) => {
            // Legacy handler removed - using reference-based API
            DomainUpdateResult::task(Task::none())
        }

        Message::ScanLibrary(library_id) => {
            let task =
                super::update_handlers::scan_updates::handle_scan_library(
                    state, library_id,
                );
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        Message::PauseScan {
            library_id,
            scan_id,
        } => {
            let task = super::update_handlers::scan_updates::handle_pause_scan(
                state, library_id, scan_id,
            );
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        Message::ResumeScan {
            library_id,
            scan_id,
        } => {
            let task = super::update_handlers::scan_updates::handle_resume_scan(
                state, library_id, scan_id,
            );
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        Message::CancelScan {
            library_id,
            scan_id,
        } => {
            let task = super::update_handlers::scan_updates::handle_cancel_scan(
                state, library_id, scan_id,
            );
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        #[cfg(feature = "demo")]
        Message::FetchDemoStatus => {
            let task = super::update_handlers::handle_fetch_demo_status(state);
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        #[cfg(feature = "demo")]
        Message::DemoStatusLoaded(result) => {
            state.domains.library.state.demo_controls.is_loading = false;
            match result {
                Ok(status) => apply_demo_status(state, status),
                Err(err) => {
                    state.domains.library.state.demo_controls.error = Some(err);
                }
            }
            DomainUpdateResult::task(Task::none())
        }

        #[cfg(feature = "demo")]
        Message::ApplyDemoSizing(request) => {
            let task = super::update_handlers::handle_apply_demo_sizing(
                state, request,
            );
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        #[cfg(feature = "demo")]
        Message::DemoSizingApplied(result) => {
            state.domains.library.state.demo_controls.is_updating = false;
            match result {
                Ok(status) => {
                    // Update UI/control state from returned status
                    apply_demo_status(state, status.clone());

                    // After resetting demo data, trigger fresh scans for all
                    // registered demo libraries so the user can explore the
                    // new seed without restarting the server.
                    let mut tasks: Vec<Task<Message>> = Vec::new();
                    for lib in status.libraries {
                        tasks.push(
                            super::update_handlers::scan_updates::handle_scan_library(
                                state,
                                lib.library_id,
                            ),
                        );
                    }

                    if tasks.is_empty() {
                        DomainUpdateResult::task(Task::none())
                    } else {
                        DomainUpdateResult::task(
                            Task::batch(tasks).map(DomainMessage::Library),
                        )
                    }
                }
                Err(err) => {
                    state.domains.library.state.demo_controls.error = Some(err);
                    DomainUpdateResult::task(Task::none())
                }
            }
        }

        Message::FetchScanMetrics => {
            let task =
                super::update_handlers::scan_updates::handle_fetch_scan_metrics(
                    state,
                );
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        Message::ScanMetricsLoaded(result) => {
            match result {
                Ok(metrics) => {
                    state.domains.library.state.scan_metrics = Some(metrics);
                }
                Err(err) => {
                    log::warn!("Failed to fetch scan metrics: {}", err);
                }
            }
            DomainUpdateResult::task(Task::none())
        }

        Message::FetchScanConfig => {
            let task =
                super::update_handlers::scan_updates::handle_fetch_scan_config(
                    state,
                );
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        Message::ScanConfigLoaded(result) => {
            match result {
                Ok(cfg) => {
                    state.domains.library.state.scan_config = Some(cfg);
                }
                Err(err) => {
                    log::warn!("Failed to fetch scan config: {}", err);
                }
            }
            DomainUpdateResult::task(Task::none())
        }

        Message::ResetLibrary(library_id) => {
            let task =
                super::update_handlers::library_management::handle_reset_library(state, library_id);
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        Message::ResetLibraryDone(result) => {
            if let Err(err) = result {
                state.domains.ui.state.error_message =
                    Some(format!("Library reset failed: {}", err));
            } else {
                // Refresh libraries and active scans after fresh rescan
                let fetch =
                    super::update_handlers::library_loaded::fetch_libraries(
                        state.api_service.clone(),
                    );
                return DomainUpdateResult::task(
                    Task::perform(fetch, |res| {
                        Message::LibrariesLoaded(res.map_err(|e| e.to_string()))
                    })
                    .map(DomainMessage::Library),
                );
            }
            DomainUpdateResult::task(Task::none())
        }

        // Library form management - using actual handlers
        Message::ShowLibraryForm(library) => {
            let task = super::update_handlers::library_management::handle_show_library_form(
                state, library,
            );
            let focus_task =
                if state.domains.library.state.library_form_data.is_some() {
                    Task::done(DomainMessage::Focus(FocusMessage::Activate(
                        FocusArea::LibraryForm,
                    )))
                } else {
                    Task::none()
                };

            DomainUpdateResult::task(Task::batch(vec![
                task.map(DomainMessage::Library),
                focus_task,
            ]))
        }

        Message::HideLibraryForm => {
            let task = super::update_handlers::library_management::handle_hide_library_form(state);
            let clear_task =
                Task::done(DomainMessage::Focus(FocusMessage::Clear));
            DomainUpdateResult::task(Task::batch(vec![
                task.map(DomainMessage::Library),
                clear_task,
            ]))
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
            let task =
                super::update_handlers::library_management::handle_toggle_library_form_enabled(
                    state,
                );
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        Message::ToggleLibraryFormStartScan => {
            let task =
                super::update_handlers::library_management::handle_toggle_library_form_start_scan(
                    state,
                );
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        Message::SubmitLibraryForm => {
            let task =
                super::update_handlers::library_management::handle_submit_library_form(state);
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        Message::MediaRootBrowser(inner) => {
            let task = super::update_handlers::media_root_browser::update(
                state, inner,
            );
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        // Scanning - duplicate handler removed
        // Already handled above
        Message::ScanStarted {
            library_id,
            scan_id,
            correlation_id,
        } => {
            log::info!(
                "Scan started: library={}, scan={}, correlation={}",
                library_id,
                scan_id,
                correlation_id
            );

            state.domains.library.state.active_scans.insert(
                scan_id,
                ScanSnapshotDto {
                    scan_id,
                    library_id,
                    status: ScanLifecycleStatus::Running,
                    completed_items: 0,
                    total_items: 0,
                    retrying_items: 0,
                    dead_lettered_items: 0,
                    correlation_id,
                    idempotency_key: String::new(),
                    current_path: None,
                    started_at: chrono::Utc::now(),
                    terminal_at: None,
                    sequence: 0,
                },
            );

            let task =
                super::update_handlers::scan_updates::handle_fetch_active_scans(
                    state,
                );
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        Message::FetchActiveScans => {
            let task =
                super::update_handlers::scan_updates::handle_fetch_active_scans(
                    state,
                );
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        Message::ActiveScansUpdated(snapshots) => {
            super::update_handlers::scan_updates::apply_active_scan_snapshot(
                state, snapshots,
            );
            DomainUpdateResult::task(Task::none())
        }

        Message::ScanProgressFrame(frame) => {
            let status = frame.status.clone();
            super::update_handlers::scan_updates::apply_scan_progress_frame(
                state,
                frame.clone(),
            );

            match status.as_str() {
                "completed" => {
                    super::update_handlers::scan_updates::remove_scan(
                        state,
                        frame.scan_id,
                    );
                    let refresh_task =
                        super::update_handlers::refresh_library::handle_refresh_library(state);
                    DomainUpdateResult::task(
                        refresh_task.map(DomainMessage::Library),
                    )
                }
                "failed" | "canceled" => {
                    super::update_handlers::scan_updates::remove_scan(
                        state,
                        frame.scan_id,
                    );
                    DomainUpdateResult::task(Task::none())
                }
                _ => DomainUpdateResult::task(Task::none()),
            }
        }

        Message::ScanCommandFailed { library_id, error } => {
            if let Some(id) = library_id {
                log::error!("Scan command failed for {}: {}", id, error);
            } else {
                log::error!("Scan command failed: {}", error);
            }
            state.domains.ui.state.error_message = Some(error);
            DomainUpdateResult::task(Task::none())
        }

        // Media references - inline handlers from update.rs
        Message::LibraryMediasLoaded(result) => match result {
            Ok(response) => {
                log::info!("Loaded {} media references", response.media.len());

                /*
                // Check if any media needs metadata fetching
                let library_id = response.library.id;
                let needs_metadata: Vec<_> = response
                    .media
                    .iter()
                    .filter(|m| {
                        // Check if media needs metadata - typically if it lacks details or has no TMDB ID
                        match m {
                            crate::infra::api_types::Media::Movie(movie) => {
                                movie.tmdb_id == 0
                                    || matches!(
                                        movie.details,
                                        MediaDetailsOption::Endpoint(_)
                                    )
                            }
                            crate::infra::api_types::Media::Series(series) => {
                                series.tmdb_id == 0
                                    || matches!(
                                        series.details,
                                        MediaDetailsOption::Endpoint(_)
                                    )
                            }
                            _ => false,
                        }
                    })
                    .cloned()
                    .collect(); */

                // Process the media references (populates MediaStore)
                //let tasks = state.process_media_references(response);

                // Refresh the All tab after MediaStore is populated
                // This ensures content is visible on startup with poster placeholders
                state.tab_manager.refresh_active_tab();
                //state.all_view_model.refresh_from_store();
                log::info!(
                    "Refreshed All tab after loading media references - UI should display immediately"
                );

                /*
                // If items need metadata, emit batch metadata fetch event
                // This will fetch metadata in the background without blocking UI
                if !needs_metadata.is_empty() {
                    log::info!(
                        "Requesting batch metadata fetch for {} items (non-blocking)",
                        needs_metadata.len()
                    );
                    let library_data = vec![(library_id, needs_metadata)];
                    DomainUpdateResult::with_events(
                        Task::batch(tasks).map(DomainMessage::Library),
                        vec![CrossDomainEvent::RequestBatchMetadataFetch(library_data)],
                    )
                } else {
                    DomainUpdateResult::task(Task::batch(tasks).map(DomainMessage::Library))
                } */
                //DomainUpdateResult::task(Task::batch(tasks).map(DomainMessage::Library))
                DomainUpdateResult::task(Task::none())
            }
            Err(e) => {
                log::error!("Failed to load media references: {}", e);
                state.domains.ui.state.error_message =
                    Some(format!("Failed to load media: {}", e));
                state.loading = false;
                DomainUpdateResult::task(Task::none())
            }
        },

        Message::RefreshCurrentLibrary => {
            let task =
                super::update_handlers::refresh_library::handle_refresh_library(
                    state,
                );
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        Message::ScanCurrentLibrary => {
            // Scan the currently selected library if one is selected
            if let Some(library_id) =
                state.domains.library.state.current_library_id
            {
                log::info!("Scanning library: {}", library_id);
                let task =
                    super::update_handlers::scan_updates::handle_scan_library(
                        state, library_id,
                    );
                DomainUpdateResult::task(task.map(DomainMessage::Library))
            } else {
                log::warn!("No library currently selected to scan");
                DomainUpdateResult::task(Task::none())
            }
        }

        // Media events from server
        Message::MediaDiscovered(references) => {
            use super::update_handlers::media_events::{
                apply_media_discovered, build_children_changed_events,
            };

            let outcome = apply_media_discovered(state, references);

            // Inline additions only for Movies/Series in the active grid
            let inline_updated = apply_discovered_media_to_tabs(
                state,
                &outcome.inline_additions,
            );

            mark_tabs_after_media_changes(
                state,
                &outcome.touched_libraries,
                &inline_updated,
            );

            // Build targeted UI events for series/season children
            let ui_events = build_children_changed_events(
                &outcome.affected_series,
                &outcome.affected_seasons,
            );

            DomainUpdateResult::with_events(Task::none(), ui_events)
        }

        Message::MediaUpdated(media) => {
            use super::update_handlers::media_events::{
                apply_media_updated, build_children_changed_events,
            };

            // Apply update to repo and collect affected parents
            let outcome = apply_media_updated(state, media);

            refresh_tabs_for_libraries(state, &outcome.touched_libraries);

            // Build targeted UI events for series/season children
            let ui_events = build_children_changed_events(
                &outcome.affected_series,
                &outcome.affected_seasons,
            );

            DomainUpdateResult::with_events(Task::none(), ui_events)
        }

        Message::MediaDeleted(id) => {
            let mut touched_libraries: HashSet<LibraryID> = HashSet::new();

            let library_for_refresh =
                match state.domains.library.state.repo_accessor.get(&id) {
                    Ok(media) => media_library_id(&media),
                    Err(err) => {
                        log::warn!(
                            "Failed to resolve media {} before deletion: {}",
                            id,
                            err
                        );
                        None
                    }
                };

            match state.domains.library.state.repo_accessor.delete(&id) {
                Ok(()) => {
                    if let Some(lib_id) = library_for_refresh {
                        touched_libraries.insert(lib_id);
                    }
                }
                Err(err) => {
                    log::error!("Failed to delete media {}: {}", id, err);
                }
            }

            refresh_tabs_for_libraries(state, &touched_libraries);
            DomainUpdateResult::task(Task::none())
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
            log::info!(
                "[BatchMetadataFetcher] Complete message received - hiding loading spinner"
            );
            state.loading = false;
            DomainUpdateResult::task(Task::none())
        }

        // View model updates
        Message::RefreshViewModels => {
            // This message is deprecated in Library domain - ViewModels are managed by UI domain
            // The UI domain handles RefreshViewModels to update its own ViewModels
            log::debug!(
                "Library: RefreshViewModels is now handled by UI domain"
            );
            DomainUpdateResult::task(Task::none())
        } // TV Shows loading
          //Message::TvShowsLoaded(result) => {
          //    log::info!("TV shows loaded: {:?}", result.as_ref().map(|v| v.len()));
          //    DomainUpdateResult::task(Task::none())
          //}
    }
}

fn media_library_id(media: &Media) -> Option<LibraryID> {
    match media {
        Media::Movie(movie) => Some(movie.library_id),
        Media::Series(series) => Some(series.library_id),
        Media::Season(season) => Some(season.library_id),
        Media::Episode(episode) => Some(episode.library_id),
    }
}

fn image_request_for_media(media: &Media) -> Option<ImageRequest> {
    match media {
        Media::Movie(movie) => Some(
            ImageRequest::new(
                movie.id.to_uuid(),
                ImageSize::Poster,
                ImageType::Movie,
            )
            .with_priority(Priority::Visible)
            .with_index(0),
        ),
        Media::Series(series) => Some(
            ImageRequest::new(
                series.id.to_uuid(),
                ImageSize::Poster,
                ImageType::Series,
            )
            .with_priority(Priority::Visible)
            .with_index(0),
        ),
        Media::Season(season) => Some(
            ImageRequest::new(
                season.id.to_uuid(),
                ImageSize::Poster,
                ImageType::Season,
            )
            .with_priority(Priority::Visible)
            .with_index(0),
        ),
        Media::Episode(episode) => Some(
            ImageRequest::new(
                *episode.id.as_uuid(),
                ImageSize::Thumbnail,
                ImageType::Episode,
            )
            .with_priority(Priority::Visible)
            .with_index(0),
        ),
    }
}

fn refresh_tabs_for_libraries(
    state: &mut State,
    libraries: &HashSet<LibraryID>,
) -> bool {
    if libraries.is_empty() {
        return false;
    }

    let active_tab = state.tab_manager.active_tab_id();
    let mut active_needs_refresh = false;

    for library_id in libraries {
        let tab_id = TabId::Library(*library_id);
        state.tab_manager.mark_tab_needs_refresh(tab_id);
        if active_tab == tab_id {
            active_needs_refresh = true;
        }
    }

    if active_needs_refresh {
        state.tab_manager.refresh_active_tab();
    }

    active_needs_refresh
}

#[cfg(feature = "demo")]
fn apply_demo_status(state: &mut State, status: DemoStatus) {
    let ctrl = &mut state.domains.library.state.demo_controls;
    let was_updating = ctrl.is_updating;
    ctrl.is_loading = false;
    ctrl.is_updating = false;
    ctrl.error = None;

    ctrl.demo_library_ids = status
        .libraries
        .iter()
        .map(|library| library.library_id)
        .collect();

    ctrl.demo_root = Some(status.root.clone());
    ctrl.demo_username = Some(status.username.clone());

    ctrl.movies_current = status
        .libraries
        .iter()
        .find(|library| matches!(library.library_type, LibraryType::Movies))
        .map(|library| library.primary_item_count);

    ctrl.series_current = status
        .libraries
        .iter()
        .find(|library| matches!(library.library_type, LibraryType::Series))
        .map(|library| library.primary_item_count);

    if was_updating || ctrl.movies_input.trim().is_empty() {
        ctrl.movies_input = ctrl
            .movies_current
            .map(|value| value.to_string())
            .unwrap_or_default();
    }

    if was_updating || ctrl.series_input.trim().is_empty() {
        ctrl.series_input = ctrl
            .series_current
            .map(|value| value.to_string())
            .unwrap_or_default();
    }
}

fn apply_discovered_media_to_tabs(
    state: &mut State,
    additions: &HashMap<LibraryID, Vec<Media>>,
) -> HashSet<LibraryID> {
    if additions.is_empty() {
        return HashSet::new();
    }

    let active_tab = state.tab_manager.active_tab_id();
    let mut inline_updated: HashSet<LibraryID> = HashSet::new();

    for (library_id, media_items) in additions {
        let tab_id = TabId::Library(*library_id);
        if tab_id != active_tab {
            continue;
        }

        if let Some(TabState::Library(tab_state)) =
            state.tab_manager.get_tab_mut(tab_id)
        {
            let mut inserted_any = false;
            for media in media_items {
                if tab_state.insert_media_reference(media) {
                    inserted_any = true;
                }
            }
            if inserted_any {
                inline_updated.insert(*library_id);
            }
        }
    }

    inline_updated
}

fn mark_tabs_after_media_changes(
    state: &mut State,
    libraries: &HashSet<LibraryID>,
    inline_updated: &HashSet<LibraryID>,
) -> bool {
    if libraries.is_empty() {
        return false;
    }

    let active_tab = state.tab_manager.active_tab_id();
    let mut active_needs_refresh = false;

    for library_id in libraries {
        let tab_id = TabId::Library(*library_id);
        let skip_active_refresh =
            inline_updated.contains(library_id) && active_tab == tab_id;

        if skip_active_refresh {
            continue;
        }

        state.tab_manager.mark_tab_needs_refresh(tab_id);
        if active_tab == tab_id {
            active_needs_refresh = true;
        }
    }

    if active_needs_refresh {
        state.tab_manager.refresh_active_tab();
    }

    active_needs_refresh
}
