#[cfg(feature = "demo")]
use crate::infra::api_types::DemoStatus;
use crate::{
    common::{
        focus::{FocusArea, FocusMessage},
        messages::{DomainMessage, DomainUpdateResult},
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
use crate::domains::library::LibrariesLoadState;
use iced::Task;
use std::collections::{HashMap, HashSet};

use ferrex_core::player_prelude::{
    LibraryId, ScanLifecycleStatus, ScanSnapshotDto,
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
        LibraryMessage::LibrariesLoaded(result) => {
            let task =
                super::update_handlers::library_loaded::handle_libraries_loaded(
                    state, result,
                );
            DomainUpdateResult::task(task.map(DomainMessage::Library))
        }

        LibraryMessage::LibrariesListLoaded(result) => match result {
            Ok(libraries) => {
                // Update UI navigation state immediately so the header can render
                // library tabs without waiting for the full media cache bootstrap.
                state.domains.library.state.libraries = libraries.clone();
                state.tab_manager.set_libraries(&libraries);

                let api_service = state.api_service.clone();
                let cache = state.disk_media_repo_cache.clone();
                let libraries_for_bootstrap = libraries.clone();

                let task = Task::perform(
                    super::update_handlers::library_loaded::fetch_libraries_bootstrap(
                        api_service,
                        cache,
                        libraries_for_bootstrap,
                    ),
                    |result| {
                        LibraryMessage::LibrariesLoaded(
                            result.map_err(|e| format!("{:#}", e)),
                        )
                    },
                );

                DomainUpdateResult::task(task.map(DomainMessage::Library))
            }
            Err(e) => {
                log::error!(
                    "[Library] Failed to fetch libraries list (server_url={}): {}",
                    state.server_url,
                    e
                );
                state.domains.library.state.load_state =
                    LibrariesLoadState::Failed { last_error: e };
                state.loading = false;
                DomainUpdateResult::task(Task::none())
            }
        },

        LibraryMessage::LoadLibraries => {
            // Auth gating: avoid starting a fetch if we're not authenticated yet.
            if !state.is_authenticated {
                log::info!(
                    "[Library] Ignoring LoadLibraries: user not authenticated yet"
                );
                return DomainUpdateResult::task(Task::none());
            }

            // Determine current session identity
            let current_user_id = match &state.domains.auth.state.auth_flow {
                AuthenticationFlow::Authenticated { user, .. } => Some(user.id),
                _ => None,
            };
            let current_server = state.server_url.clone();

            // Act based on current load state
            let task = match &state.domains.library.state.load_state {
                LibrariesLoadState::NotStarted => {
                    state.domains.library.state.load_state =
                        LibrariesLoadState::InProgress;
                    Task::perform(
                        super::update_handlers::library_loaded::fetch_libraries_list(
                            state.api_service.clone(),
                        ),
                        |result| {
                            LibraryMessage::LibrariesListLoaded(
                                result.map_err(|e| format!("{:#}", e)),
                            )
                        },
                    )
                }
                LibrariesLoadState::Failed { .. } => {
                    // Allow retry after failure
                    log::info!("[Library] Retrying library load after failure");
                    state.domains.library.state.load_state =
                        LibrariesLoadState::InProgress;
                    Task::perform(
                        super::update_handlers::library_loaded::fetch_libraries_list(
                            state.api_service.clone(),
                        ),
                        |result| {
                            LibraryMessage::LibrariesListLoaded(
                                result.map_err(|e| format!("{:#}", e)),
                            )
                        },
                    )
                }
                LibrariesLoadState::InProgress => {
                    // Idempotent: no-op while a fetch is already in flight
                    log::debug!(
                        "[Library] LoadLibraries ignored: load already in progress"
                    );
                    Task::none()
                }
                LibrariesLoadState::Succeeded {
                    user_id,
                    server_url,
                } => {
                    // If session changed (user or server), re-load; else no-op
                    let same_user = user_id.is_some()
                        && current_user_id.is_some()
                        && user_id == &current_user_id;
                    let same_server = *server_url == current_server;
                    if same_user && same_server {
                        log::debug!(
                            "[Library] LoadLibraries ignored: libraries already loaded for this session"
                        );
                        Task::none()
                    } else {
                        log::info!(
                            "[Library] Session changed (user or server); reloading libraries"
                        );
                        state.domains.library.state.load_state =
                            LibrariesLoadState::InProgress;
                        Task::perform(
                            super::update_handlers::library_loaded::fetch_libraries_list(
                                state.api_service.clone(),
                            ),
                            |result| {
                                LibraryMessage::LibrariesListLoaded(
                                    result.map_err(|e| format!("{:#}", e)),
                                )
                            },
                        )
                    }
                }
            };

            log::info!("LoadLibraries message received");
            DomainUpdateResult::task(task.map(DomainMessage::Library))
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

        LibraryMessage::ScanCommandFailed { library_id, error } => {
            if let Some(id) = library_id {
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
