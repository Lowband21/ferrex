#[cfg(feature = "demo")]
use crate::infra::api_types::DemoStatus;
use crate::{
    common::{
        focus::{FocusArea, FocusMessage},
        messages::{DomainMessage, DomainUpdateResult},
        task::into_iced_task,
    },
    domains::{
        library::update_handlers::{
            handle_fetch_movie_reference_batch, handle_fetch_series_bundle,
            handle_scan_library,
        },
        ui::{
            tabs::{TabId, TabState},
            update_handlers::{
                emit_initial_all_tab_snapshots_combined, init_all_tab_view,
            },
        },
    },
    infra::api_types::Media,
    state::State,
};

use super::messages::LibraryMessage;
use crate::domains::auth::types::AuthenticationFlow;
use iced::Task;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use ferrex_core::player_prelude::{Library, LibraryId};
#[cfg(feature = "demo")]
use ferrex_model::library::LibraryType;
use ferrex_player_api::services::api::ApiService;
use ferrex_player_foundation::{
    domain::DomainTask, repository::RepositoryResult,
};
use ferrex_player_library::{
    LibraryDomainState,
    types::LibrariesBootstrapPayload,
    update::{
        LibraryUpdateContext, update_library as update_library_state_machine,
    },
    update_handlers::library_loaded::LibrariesLoadedContext,
};

impl LibrariesLoadedContext for State {
    fn library_state_mut(&mut self) -> &mut LibraryDomainState {
        &mut self.domains.library.state
    }

    fn install_libraries_bootstrap(
        &mut self,
        payload: LibrariesBootstrapPayload,
    ) -> RepositoryResult<()> {
        super::update_handlers::library_loaded::install_libraries_bootstrap_payload(
            self, payload,
        )
    }

    fn mark_loading(&mut self, loading: bool) {
        self.loading = loading;
    }

    fn session_user_id(&self) -> Option<uuid::Uuid> {
        match &self.domains.auth.state.auth_flow {
            AuthenticationFlow::Authenticated { user, .. } => Some(user.id),
            _ => None,
        }
    }

    fn server_url(&self) -> &str {
        &self.server_url
    }
}

impl LibraryUpdateContext for State {
    type AppMessage = DomainMessage;

    fn library_message(message: LibraryMessage) -> Self::AppMessage {
        DomainMessage::Library(message)
    }

    fn is_authenticated(&self) -> bool {
        self.is_authenticated
    }

    fn api_service(&self) -> Arc<dyn ApiService> {
        Arc::clone(&self.api_service)
    }

    fn set_libraries_for_navigation(&mut self, libraries: &[Library]) {
        self.domains.library.state.libraries = libraries.to_vec();
        self.tab_manager.set_libraries(libraries);
    }

    fn fetch_libraries_bootstrap_task(
        &self,
        api_service: Arc<dyn ApiService>,
        libraries: Vec<Library>,
    ) -> DomainTask<Self::AppMessage> {
        let media_repo_cache = self.disk_media_repo_cache.clone();
        DomainTask::perform(
            super::update_handlers::library_loaded::fetch_libraries_bootstrap(
                api_service,
                media_repo_cache,
                libraries,
            ),
            |result| {
                DomainMessage::Library(LibraryMessage::LibrariesLoaded(
                    result.map_err(|err| format!("{err:#}")),
                ))
            },
        )
    }
}

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
    message: LibraryMessage,
) -> DomainUpdateResult {
    match message {
        LibraryMessage::RefreshLibrary => {
            let task =
                super::update_handlers::refresh_library::handle_refresh_library(
                    state,
                );
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        // Library management
        message @ (LibraryMessage::LibrariesLoaded(_)
        | LibraryMessage::LibrariesListLoaded(_)
        | LibraryMessage::LoadLibraries) => {
            let result = update_library_state_machine(state, message);
            DomainUpdateResult::task(into_iced_task(result.task))
        }

        LibraryMessage::CreateLibrary {
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

        LibraryMessage::LibraryCreated(result) => {
            let task =
                super::update_handlers::library_management::handle_library_created(state, result);
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        LibraryMessage::UpdateLibrary(library) => {
            let task = super::update_handlers::library_management::handle_update_library(
                state,
                library,
                state.server_url.clone(),
            );
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        LibraryMessage::LibraryUpdated(result) => {
            let task =
                super::update_handlers::library_management::handle_library_updated(state, result);
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        LibraryMessage::DeleteLibrary(library_id) => {
            let task = super::update_handlers::library_management::handle_delete_library(
                state,
                library_id,
                state.server_url.clone(),
            );
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        LibraryMessage::LibraryDeleted(result) => {
            let task =
                super::update_handlers::library_management::handle_library_deleted(state, result);
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        LibraryMessage::SelectLibrary(library_id) => {
            // This handler returns DomainUpdateResult directly
            super::update_handlers::select_library::handle_select_library(
                state, library_id,
            )
        }

        LibraryMessage::LibrarySelected(_library_id, _result) => {
            // Legacy handler removed - using reference-based API
            DomainUpdateResult::task(Task::none())
        }

        LibraryMessage::ScanLibrary(library_id) => {
            let task = handle_scan_library(state, library_id);
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        LibraryMessage::FetchMovieBatch {
            library_id,
            batch_id,
        } => {
            let task = handle_fetch_movie_reference_batch(
                state.api_service.clone(),
                library_id,
                batch_id,
            );
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        LibraryMessage::FetchSeriesBundle {
            library_id,
            series_id,
        } => {
            let task = handle_fetch_series_bundle(
                state.api_service.clone(),
                library_id,
                series_id,
            );
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        LibraryMessage::MovieBatchLoaded {
            library_id,
            batch_id,
            result,
        } => {
            let bytes = match result {
                Ok(bytes) => bytes,
                Err(err) => {
                    log::warn!(
                        "[Library] Movie batch load failed: library {} batch {} err={}",
                        library_id,
                        batch_id,
                        err
                    );
                    return DomainUpdateResult::task(Task::none());
                }
            };

            log::info!(
                "[Library] Movie batch loaded: library {} batch {} bytes={}",
                library_id,
                batch_id,
                bytes.len()
            );

            let outcome = match state
                .domains
                .library
                .state
                .repo_accessor
                .install_movie_reference_batch(library_id, batch_id, bytes)
            {
                Ok(outcome) => outcome,
                Err(err) => {
                    log::error!(
                        "[Library] Failed to install movie batch: library {} batch {} err={}",
                        library_id,
                        batch_id,
                        err
                    );
                    return DomainUpdateResult::task(Task::none());
                }
            };

            for movie_id in outcome.movie_ids.iter() {
                state.domains.ui.state.movie_yoke_cache.remove(movie_id);
            }

            log::info!(
                "[Library] Installed movie batch: library {} batch {} movies={} pruned_runtime={}",
                library_id,
                batch_id,
                outcome.movies_indexed,
                outcome.movies_replaced_from_runtime_overlay
            );

            refresh_tabs_for_libraries(state, &HashSet::from([library_id]));
            DomainUpdateResult::task(Task::none())
        }

        LibraryMessage::SeriesBundleLoaded {
            library_id,
            series_id,
            result,
        } => {
            let bytes = match result {
                Ok(bytes) => {
                    log::debug!(
                        "[Library] Series bundle loaded {} bytes",
                        bytes.len()
                    );
                    bytes
                }
                Err(err) => {
                    log::warn!(
                        "[Library] Series bundle load failed: library {} series {} err={}",
                        library_id,
                        series_id,
                        err
                    );
                    return DomainUpdateResult::task(Task::none());
                }
            };

            log::info!(
                "[Library] Series bundle loaded: library {} series {} bytes={}",
                library_id,
                series_id,
                bytes.len()
            );

            let outcome = match state
                .domains
                .library
                .state
                .repo_accessor
                .install_series_bundle(library_id, series_id, bytes)
            {
                Ok(outcome) => outcome,
                Err(err) => {
                    log::error!(
                        "[Library] Failed to install series bundle: library {} series {} err={}",
                        library_id,
                        series_id,
                        err
                    );
                    return DomainUpdateResult::task(Task::none());
                }
            };
            state
                .domains
                .ui
                .state
                .series_yoke_cache
                .remove(&outcome.series_id);

            for season_id in outcome.season_ids.iter() {
                state.domains.ui.state.series_yoke_cache.remove(season_id);
            }
            for episode_id in outcome.episode_ids.iter() {
                state.domains.ui.state.series_yoke_cache.remove(episode_id);
            }

            if let Err(err) = state
                .domains
                .library
                .state
                .repo_accessor
                .mark_episode_len_dirty(&library_id)
            {
                log::warn!(
                    "[Library] Failed to mark episode length cache dirty: library {} err={}",
                    library_id,
                    err
                );
            }

            log::info!(
                "[Library] Installed series bundle: library {} series {} seasons={} episodes={} pruned_runtime={}",
                library_id,
                series_id,
                outcome.seasons_indexed,
                outcome.episodes_indexed,
                outcome.items_replaced_from_runtime_overlay,
            );

            refresh_tabs_for_libraries(state, &HashSet::from([library_id]));
            DomainUpdateResult::task(Task::none())
        }

        LibraryMessage::PauseScan {
            library_id,
            scan_id,
        } => {
            let task = super::update_handlers::scan_updates::handle_pause_scan(
                state, library_id, scan_id,
            );
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        LibraryMessage::ResumeScan {
            library_id,
            scan_id,
        } => {
            let task = super::update_handlers::scan_updates::handle_resume_scan(
                state, library_id, scan_id,
            );
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        LibraryMessage::CancelScan {
            library_id,
            scan_id,
        } => {
            let task = super::update_handlers::scan_updates::handle_cancel_scan(
                state, library_id, scan_id,
            );
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        #[cfg(feature = "demo")]
        LibraryMessage::FetchDemoStatus => {
            let task = super::update_handlers::handle_fetch_demo_status(state);
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        #[cfg(feature = "demo")]
        LibraryMessage::DemoStatusLoaded(result) => {
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
        LibraryMessage::ApplyDemoSizing(request) => {
            let task = super::update_handlers::handle_apply_demo_sizing(
                state, request,
            );
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        #[cfg(feature = "demo")]
        LibraryMessage::DemoSizingApplied(result) => {
            state.domains.library.state.demo_controls.is_updating = false;
            match result {
                Ok(status) => {
                    // Update UI/control state from returned status
                    apply_demo_status(state, status.clone());

                    // Demo sizing uses incremental server-side scanning; avoid
                    // triggering a full rescan from the client.
                    DomainUpdateResult::task(Task::none())
                }
                Err(err) => {
                    state.domains.library.state.demo_controls.error = Some(err);
                    DomainUpdateResult::task(Task::none())
                }
            }
        }

        LibraryMessage::FetchScanMetrics => {
            let task =
                super::update_handlers::scan_updates::handle_fetch_scan_metrics(
                    state,
                );
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        LibraryMessage::ScanMetricsLoaded(result) => {
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

        LibraryMessage::FetchScanConfig => {
            let task =
                super::update_handlers::scan_updates::handle_fetch_scan_config(
                    state,
                );
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        LibraryMessage::ScanConfigLoaded(result) => {
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

        LibraryMessage::ResetLibrary(library_id) => {
            let task =
                super::update_handlers::library_management::handle_reset_library(state, library_id);
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        LibraryMessage::ResetLibraryDone(result) => {
            if let Err(err) = result {
                state.domains.ui.state.error_message =
                    Some(format!("Library reset failed: {}", err));
            } else {
                // Refresh libraries and active scans after fresh rescan
                let fetch =
                    super::update_handlers::library_loaded::fetch_libraries(
                        state.api_service.clone(),
                        state.disk_media_repo_cache.clone(),
                    );
                return DomainUpdateResult::task(
                    Task::perform(fetch, |res| {
                        LibraryMessage::LibrariesLoaded(
                            res.map_err(|e| format!("{:#}", e)),
                        )
                    })
                    .map(DomainMessage::Library),
                );
            }
            DomainUpdateResult::task(Task::none())
        }

        // Library form management - using actual handlers
        LibraryMessage::ShowLibraryForm(library) => {
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

        LibraryMessage::HideLibraryForm => {
            let task = super::update_handlers::library_management::handle_hide_library_form(state);
            let clear_task =
                Task::done(DomainMessage::Focus(FocusMessage::Clear));
            DomainUpdateResult::task(Task::batch(vec![
                task.map(DomainMessage::Library),
                clear_task,
            ]))
        }

        LibraryMessage::UpdateLibraryFormName(name) => {
            let task = super::update_handlers::library_management::handle_update_libarary_form_name(
                state, name,
            );
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        LibraryMessage::UpdateLibraryFormType(library_type) => {
            let task = super::update_handlers::library_management::handle_update_library_form_type(
                state,
                library_type,
            );
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        LibraryMessage::UpdateLibraryFormPaths(paths) => {
            let task = super::update_handlers::library_management::handle_update_library_form_paths(
                state, paths,
            );
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        LibraryMessage::UpdateLibraryFormScanInterval(interval) => {
            let task = super::update_handlers::library_management::handle_update_library_form_scan_interval(
                state, interval,
            );
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        LibraryMessage::ToggleLibraryFormEnabled => {
            let task =
                super::update_handlers::library_management::handle_toggle_library_form_enabled(
                    state,
                );
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        LibraryMessage::ToggleLibraryFormAutoScan => {
            let task = super::update_handlers::library_management::handle_toggle_library_form_auto_scan(
                state,
            );
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        LibraryMessage::ToggleLibraryFormWatchForChanges => {
            let task = super::update_handlers::library_management::handle_toggle_library_form_watch_for_changes(
                state,
            );
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        LibraryMessage::ToggleLibraryFormStartScan => {
            let task =
                super::update_handlers::library_management::handle_toggle_library_form_start_scan(
                    state,
                );
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        LibraryMessage::SubmitLibraryForm => {
            let task =
                super::update_handlers::library_management::handle_submit_library_form(state);
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        LibraryMessage::MediaRootBrowser(inner) => {
            let task = super::update_handlers::media_root_browser::update(
                state, inner,
            );
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        // Scanning - duplicate handler removed
        // Already handled above
        LibraryMessage::ScanStarted {
            library_id,
            response,
        } => {
            log::info!(
                "Scan start accepted: library={}, scan={}, correlation={}, mode={}, status={}, disposition={:?}",
                library_id,
                response.scan_id,
                response.correlation_id,
                response.mode.as_str(),
                response.status.as_str(),
                response.disposition
            );

            state
                .domains
                .library
                .state
                .apply_scan_start_response(library_id, &response);

            let task =
                super::update_handlers::scan_updates::handle_fetch_active_scans(
                    state,
                );
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        LibraryMessage::FetchActiveScans => {
            let task =
                super::update_handlers::scan_updates::handle_fetch_active_scans(
                    state,
                );
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        LibraryMessage::ActiveScansUpdated(snapshots) => {
            super::update_handlers::scan_updates::apply_active_scan_snapshot(
                state, snapshots,
            );
            DomainUpdateResult::task(Task::none())
        }

        LibraryMessage::ScanProgressFrame(frame) => {
            let status = frame.status.clone();
            super::update_handlers::scan_updates::apply_scan_progress_frame(
                state, frame,
            );

            match status.as_str() {
                "completed" => {
                    if !super::update_handlers::scan_updates::progress_frame_has_recovery_affordance(
                        &frame,
                    ) {
                        super::update_handlers::scan_updates::remove_scan(
                            state,
                            frame.scan_id,
                        );
                    }
                    let refresh_task =
                        super::update_handlers::refresh_library::handle_refresh_library(state);
                    DomainUpdateResult::task(
                        refresh_task.map(DomainMessage::Library),
                    )
                }
                "failed"
                | "failed_needs_attention"
                | "needs_attention"
                | "skipped" => {
                    if !super::update_handlers::scan_updates::progress_frame_has_recovery_affordance(
                        &frame,
                    ) {
                        super::update_handlers::scan_updates::remove_scan(
                            state,
                            frame.scan_id,
                        );
                    }
                    DomainUpdateResult::task(Task::none())
                }
                "canceled" | "cancelled" => {
                    super::update_handlers::scan_updates::remove_scan(
                        state,
                        frame.scan_id,
                    );
                    DomainUpdateResult::task(Task::none())
                }
                _ => DomainUpdateResult::task(Task::none()),
            }
        }

        LibraryMessage::ScanCommandFailed { library_id, error } => {
            if let Some(id) = library_id {
                state.domains.library.state.finish_scan_start(
                    id,
                    ferrex_core::player_prelude::ScanRunMode::Manual,
                );
                log::error!("Scan command failed for {}: {}", id, error);
            } else {
                log::error!("Scan command failed: {}", error);
            }
            state.domains.ui.state.error_message = Some(error);
            DomainUpdateResult::task(Task::none())
        }

        // Media references - inline handlers from update.rs
        LibraryMessage::LibraryMediasLoaded(result) => match result {
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

        LibraryMessage::RefreshCurrentLibrary => {
            let task =
                super::update_handlers::refresh_library::handle_refresh_library(
                    state,
                );
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        LibraryMessage::ScanCurrentLibrary => {
            // Scan the currently selected library if one is selected
            if let Some(library_id) = state.domains.ui.state.scope.lib_id() {
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
        LibraryMessage::MediaDiscovered(references) => {
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

        LibraryMessage::MediaUpdated(media) => {
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

        LibraryMessage::MediaDeleted(id) => {
            let mut touched_libraries: HashSet<LibraryId> = HashSet::new();

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

        // No-op
        LibraryMessage::NoOp => DomainUpdateResult::task(Task::none()),
    }
}

fn media_library_id(media: &Media) -> Option<LibraryId> {
    match media {
        Media::Movie(movie) => Some(movie.library_id),
        Media::Series(series) => Some(series.library_id),
        Media::Season(season) => Some(season.library_id),
        Media::Episode(episode) => Some(episode.library_id),
    }
}

// fn image_request_for_media(media: &Media) -> Option<ImageRequest> {
//     match media {
//         Media::Movie(movie) => Some(
//             ImageRequest::new(
//                 movie.id.to_uuid(),
//                 ImageSize::poster(),
//                 MediaType::Movie,
//             )
//             .with_priority(Priority::Visible)
//             .with_index(0),
//         ),
//         Media::Series(series) => Some(
//             ImageRequest::new(
//                 series.id.to_uuid(),
//                 ImageSize::poster(),
//                 MediaType::Series,
//             )
//             .with_priority(Priority::Visible)
//             .with_index(0),
//         ),
//         Media::Season(season) => Some(
//             ImageRequest::new(
//                 season.id.to_uuid(),
//                 ImageSize::poster(),
//                 MediaType::Season,
//             )
//             .with_priority(Priority::Visible)
//             .with_index(0),
//         ),
//         Media::Episode(episode) => Some(
//             ImageRequest::new(
//                 *episode.id.as_uuid(),
//                 ImageSize::thumbnail(),
//                 MediaType::Episode,
//             )
//             .with_priority(Priority::Visible)
//             .with_index(0),
//         ),
//     }
// }

fn refresh_tabs_for_libraries(
    state: &mut State,
    libraries: &HashSet<LibraryId>,
) -> bool {
    if libraries.is_empty() {
        return false;
    }

    let active_tab = state.tab_manager.active_tab_id();
    let mut active_needs_refresh = false;

    for library_id in libraries {
        let tab_id = TabId::Library(*library_id);

        // Ensure the tab exists so downstream All-tab carousel helpers can read
        // its cached IDs.
        let tab = state.tab_manager.get_or_create_tab(tab_id);
        if let TabState::Library(tab_state) = tab {
            tab_state.mark_needs_refresh();
            tab_state.refresh_from_repo();
        }
    }

    // Refreshing the "Home" (All) view is not driven by TabManager::refresh_active_tab.
    // Instead, the All view's carousels are wired through UI-level helpers that
    // re-sync per-library carousels from each library tab's cached IDs and emit
    // demand snapshots for poster loading.
    if active_tab == TabId::Home {
        init_all_tab_view(state);
        emit_initial_all_tab_snapshots_combined(state);
        active_needs_refresh = true;
    } else if libraries.iter().any(|id| TabId::Library(*id) == active_tab) {
        active_needs_refresh = true;
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
    additions: &HashMap<LibraryId, Vec<Media>>,
) -> HashSet<LibraryId> {
    if additions.is_empty() {
        return HashSet::new();
    }

    let active_tab = state.tab_manager.active_tab_id();
    let mut inline_updated: HashSet<LibraryId> = HashSet::new();

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
    libraries: &HashSet<LibraryId>,
    inline_updated: &HashSet<LibraryId>,
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

#[cfg(test)]
mod tests {
    use super::*;
    use ferrex_core::player_prelude::{
        ScanCommandAcceptedResponse, ScanLifecycleStatus, ScanRunMode,
        ScanStartDisposition,
    };

    #[tokio::test(flavor = "current_thread")]
    async fn double_click_scan_started_reuses_one_active_scan_entry() {
        let mut state = State::new("http://localhost:3000".to_string());
        let library_id = LibraryId(uuid::Uuid::now_v7());
        let scan_id = uuid::Uuid::now_v7();
        let correlation_id = uuid::Uuid::now_v7();
        let response = ScanCommandAcceptedResponse {
            scan_id,
            correlation_id,
            status: ScanLifecycleStatus::Pending,
            mode: ScanRunMode::Manual,
            idempotency_key: format!("scan-{scan_id}"),
            run_key: ScanRunMode::Manual.run_key(library_id),
            disposition: ScanStartDisposition::Created,
        };

        for _ in 0..2 {
            let _ = update_library(
                &mut state,
                LibraryMessage::ScanStarted {
                    library_id,
                    response: response.clone(),
                },
            );
        }

        assert_eq!(state.domains.library.state.active_scans.len(), 1);
        let snapshot = state
            .domains
            .library
            .state
            .active_scans
            .get(&scan_id)
            .expect("scan remains tracked once");
        assert_eq!(snapshot.library_id, library_id);
        assert_eq!(snapshot.correlation_id, correlation_id);
        assert_eq!(snapshot.mode, ScanRunMode::Manual);
        assert_eq!(snapshot.run_key, ScanRunMode::Manual.run_key(library_id));
    }
}
