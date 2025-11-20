//! Cross-domain event coordination module
//!
//! This module handles events that require coordination between multiple domains.
//! It acts as a mediator to maintain proper domain boundaries while enabling
//! necessary cross-domain workflows.

use crate::{
    common::messages::{CrossDomainEvent, DomainMessage},
    domains::{
        auth, library, player,
        ui::{self, scroll_manager::ScrollStateExt},
    },
    state::State,
};
use iced::Task;

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn handle_event(
    state: &mut State,
    event: CrossDomainEvent,
) -> Task<DomainMessage> {
    log::debug!("[CrossDomain] Processing event: {:?}", event);

    match event {
        // Authentication flow completion
        CrossDomainEvent::AuthenticationComplete => {
            handle_authentication_complete(state)
        }

        // Auth command execution requested
        CrossDomainEvent::AuthCommandRequested(command) => {
            log::info!(
                "[CrossDomain] Auth command requested: {}",
                command.sanitized_display()
            );
            Task::done(DomainMessage::Auth(
                auth::messages::Message::ExecuteCommand(command),
            ))
        }

        // Auth command execution completed
        CrossDomainEvent::AuthCommandCompleted(command, result) => {
            log::info!(
                "[CrossDomain] Auth command completed: {} - {:?}",
                command.name(),
                result.is_success()
            );

            // Convert auth command result to appropriate settings message
            match command {
                auth::messages::AuthCommand::ChangePassword { .. } => {
                    let settings_result = match result {
                        auth::messages::AuthCommandResult::Success => Ok(()),
                        auth::messages::AuthCommandResult::Error(msg) => {
                            Err(msg.clone())
                        }
                    };
                    Task::done(DomainMessage::Settings(
                        crate::domains::settings::messages::Message::PasswordChangeResult(
                            settings_result,
                        ),
                    ))
                }
                auth::messages::AuthCommand::SetUserPin { .. }
                | auth::messages::AuthCommand::ChangeUserPin { .. } => {
                    let settings_result = match result {
                        auth::messages::AuthCommandResult::Success => Ok(()),
                        auth::messages::AuthCommandResult::Error(msg) => {
                            Err(msg.clone())
                        }
                    };
                    Task::done(DomainMessage::Settings(
                        crate::domains::settings::messages::Message::PinChangeResult(
                            settings_result,
                        ),
                    ))
                }
                _ => Task::none(),
            }
        }

        // Auth configuration changed
        CrossDomainEvent::AuthConfigurationChanged => {
            log::info!("[CrossDomain] Auth configuration changed");
            // Could trigger UI updates, permission refresh, etc.
            Task::none()
        }

        // Database cleared - need to refresh data
        CrossDomainEvent::DatabaseCleared => handle_database_cleared(state),

        // Library refresh requested
        CrossDomainEvent::RequestLibraryRefresh => {
            // Also notify UI to refresh ViewModels
            // [MediaStoreNotifier] RefreshViewModels no longer needed here
            handle_library_refresh_request(state)
        }

        // User authenticated - store in state
        CrossDomainEvent::UserAuthenticated(user, permissions) => {
            state.is_authenticated = true;
            state.domains.auth.state.user_permissions = Some(permissions);
            log::info!("[CrossDomain] User {} authenticated", user.username);
            Task::none()
        }

        // User logged out - clear state
        CrossDomainEvent::UserLoggedOut => {
            state.is_authenticated = false;
            state.domains.auth.state.user_permissions = None;
            log::info!(
                "[CrossDomain] User logged out - emitting cleanup events"
            );

            // Clear all sensitive data
            Task::batch(vec![
                Task::done(DomainMessage::Event(
                    CrossDomainEvent::ClearMediaStore,
                )),
                Task::done(DomainMessage::Event(
                    CrossDomainEvent::ClearLibraries,
                )),
                Task::done(DomainMessage::Event(
                    CrossDomainEvent::ClearCurrentShowData,
                )),
            ])
        }

        // Library events
        CrossDomainEvent::LibrarySelected(library_id) => {
            state.domains.library.state.current_library_id = Some(library_id);

            // Restore scroll state for the new library context
            let scroll_task =
                state.restore_library_scroll_state(Some(library_id));

            Task::batch(vec![
                scroll_task,
                Task::done(DomainMessage::Library(
                    library::messages::Message::SelectLibrary(Some(library_id)),
                )),
                // Notify UI to update view models
                // [MediaStoreNotifier] RefreshViewModels no longer needed here
            ])
        }
        CrossDomainEvent::LibrarySelectAll => {
            state.domains.library.state.current_library_id = None;

            // Restore scroll state for the global context (all libraries)
            let scroll_task = state.restore_library_scroll_state(None);

            // Broadcast the event to all domains (e.g., UI switches to Curated)
            let broadcast = state
                .domains
                .handle_event(CrossDomainEvent::LibrarySelectAll);

            Task::batch(vec![
                scroll_task,
                broadcast,
                // Update the library domain directly without re-emitting events
                Task::done(DomainMessage::Library(
                    library::messages::Message::SelectLibrary(None),
                )),
            ])
        }
        CrossDomainEvent::LibraryChanged(library_id) => {
            log::info!("[CrossDomain] Library changed to: {}", library_id);
            // Notify all domains about library change
            state
                .domains
                .handle_event(CrossDomainEvent::LibraryChanged(library_id))
        }
        CrossDomainEvent::SeriesChildrenChanged(series_id) => {
            // Forward to domains (UI domain will invalidate caches/refresh)
            state
                .domains
                .handle_event(CrossDomainEvent::SeriesChildrenChanged(
                    series_id,
                ))
        }
        CrossDomainEvent::SeasonChildrenChanged(season_id) => {
            // Forward to domains (UI domain will invalidate caches/refresh)
            state
                .domains
                .handle_event(CrossDomainEvent::SeasonChildrenChanged(
                    season_id,
                ))
        }
        CrossDomainEvent::LibraryUpdated => {
            log::info!("[CrossDomain] Library updated");
            Task::none()
        }
        // Media events
        CrossDomainEvent::MediaListChanged => {
            log::info!("[CrossDomain] Media list changed");
            // UI ViewModels notified via MediaStoreNotifier pattern
            Task::none()
        }
        CrossDomainEvent::MediaLoaded => {
            log::info!(
                "[CrossDomain] Media loaded - running search calibration"
            );
            // Run search calibration after media is loaded
            Task::perform(
                async move {
                    // Small delay to ensure media store is fully populated
                    tokio::time::sleep(tokio::time::Duration::from_millis(500))
                        .await;
                },
                |_| {
                    DomainMessage::Search(crate::domains::search::messages::Message::RunCalibration)
                },
            )
        }

        // Toggle fullscreen mode
        CrossDomainEvent::MediaToggleFullscreen => {
            log::info!("[CrossDomain] Toggle fullscreen requested");
            Task::done(DomainMessage::Player(
                crate::domains::player::messages::Message::ToggleFullscreen,
            ))
        }

        // Play media with ID
        CrossDomainEvent::MediaPlayWithId(media_file, media_id) => {
            log::info!("[CrossDomain] Play media with ID: {:?}", media_id);

            // Derive resume position from media domain watch state (resume-at-last-position by default)
            let mut resume_opt: Option<f32> = None;
            let mut watch_duration_hint: Option<f64> = None;
            if let Some(watch_state) =
                &state.domains.media.state.user_watch_state
                && let Some(item) =
                    watch_state.get_by_media_id(media_id.as_uuid())
            {
                // Use the last known position from watch state
                if item.position > 0.0 && item.duration > 0.0 {
                    resume_opt = Some(item.position);
                }

                if item.duration > 0.0 {
                    watch_duration_hint = Some(item.duration as f64);
                }
            }

            // Prefer metadata duration when available, fall back to watch cache hint
            let metadata_duration_hint = media_file
                .media_file_metadata
                .as_ref()
                .and_then(|meta| meta.duration)
                .filter(|d| *d > 0.0);

            let duration_hint = watch_duration_hint.or(metadata_duration_hint);

            // Seed player state with progress hints so UI can update immediately
            state.domains.player.state.last_valid_position =
                resume_opt.map(|pos| pos as f64).unwrap_or(0.0);
            state.domains.player.state.last_valid_duration =
                duration_hint.unwrap_or(0.0);

            // Store resume position so the player picks it up during PlayMediaWithId
            state.domains.media.state.pending_resume_position = resume_opt;
            // Also prime the player domain for immediate seek during load
            state.domains.player.state.pending_resume_position = resume_opt;

            Task::done(DomainMessage::Player(
                crate::domains::player::messages::Message::PlayMediaWithId(
                    media_file, media_id,
                ),
            ))
        }

        // // Legacy transcoding events (deprecated)
        // CrossDomainEvent::RequestTranscoding(_)
        // | CrossDomainEvent::TranscodingReady(_) => {
        //     log::warn!(
        //         "[CrossDomain] Legacy transcoding event received - ignoring"
        //     );
        //     Task::none()
        // }

        // Window management events
        CrossDomainEvent::HideWindow => {
            log::info!("[CrossDomain] Hide window requested");
            iced::window::latest().and_then(|id| {
                log::info!("Hiding window with id: {:?}", id);
                //iced::window::set_mode(id, iced::window::Mode::Fullscreen)
                iced::window::minimize(id, true)
            })
        }

        CrossDomainEvent::RestoreWindow(fullscreen) => {
            log::info!(
                "[CrossDomain] Restore window requested (fullscreen: {})",
                fullscreen
            );
            let minimize_task = iced::window::latest().and_then(|id| {
                log::info!("Hiding window with id: {:?}", id);
                //iced::window::set_mode(id, iced::window::Mode::Fullscreen)
                iced::window::minimize(id, true)
            });

            let mode = if fullscreen {
                iced::window::Mode::Fullscreen
            } else {
                iced::window::Mode::Windowed
            };
            let restore_task = iced::window::latest().and_then(move |id| {
                log::info!("Re storing window {:?} to mode: {:?}", id, mode);
                iced::window::set_mode(id, mode)
            });
            Task::batch(vec![minimize_task, restore_task])
        }

        CrossDomainEvent::SetWindowMode(mode) => {
            log::info!("[CrossDomain] Set window mode: {:?}", mode);
            iced::window::latest().and_then(move |id| {
                log::info!("Setting window {:?} to mode: {:?}", id, mode);
                iced::window::set_mode(id, mode)
            })
        }

        // Media playback events
        CrossDomainEvent::MediaStartedPlaying(media_file) => {
            log::info!("[CrossDomain] Media started playing");
            Task::done(DomainMessage::Player(
                player::messages::Message::PlayMedia(media_file),
            ))
        }

        CrossDomainEvent::MediaStopped => {
            log::info!("[CrossDomain] Media stopped");
            Task::none()
        }

        CrossDomainEvent::MediaPaused => {
            log::info!("[CrossDomain] Media paused");
            Task::none()
        }

        // Cleanup events - handled directly by domains via their event handlers
        CrossDomainEvent::ClearMediaStore
        | CrossDomainEvent::ClearLibraries
        | CrossDomainEvent::ClearCurrentShowData => {
            log::info!(
                "[CrossDomain] Cleanup event {:?} - handled by domain event handlers",
                event
            );
            // Special handling for ClearLibraries to reset current_library_id
            if matches!(event, CrossDomainEvent::ClearLibraries) {
                state.domains.library.state.current_library_id = None;
            }
            Task::none()
        }

        // Metadata events
        CrossDomainEvent::BatchMetadataReady(items) => {
            log::info!(
                "[CrossDomain] Batch metadata ready: {} items",
                items.len()
            );

            // Log details about what we received
            let movies_with_details = 0;
            let series_with_details = 0;
            let still_need_fetch = 0;

            /*
            for item in &items {
                match item {
                    crate::infra::api_types::Media::Movie(movie) => {
                        if crate::infra::api_types::needs_details_fetch(&movie.details) {
                            still_need_fetch += 1;
                            log::debug!(
                                "Movie {} still has Endpoint, not Details",
                                movie.title.as_str()
                            );
                        } else {
                            movies_with_details += 1;
                            log::debug!("Movie {} has full Details", movie.title.as_str());
                        }
                    }
                    crate::infra::api_types::Media::Series(series) => {
                        if crate::infra::api_types::needs_details_fetch(&series.details) {
                            still_need_fetch += 1;
                            log::debug!(
                                "Series {} still has Endpoint, not Details",
                                series.title.as_str()
                            );
                        } else {
                            series_with_details += 1;
                            log::debug!("Series {} has full Details", series.title.as_str());
                        }
                    }
                    _ => {}
                }
            } */

            log::info!(
                "[CrossDomain] Batch contains: {} movies with details, {} series with details, {} still need fetch",
                movies_with_details,
                series_with_details,
                still_need_fetch
            );

            // Update MediaStore directly - don't create a task that can be re-executed
            //let media_store = Arc::clone(&state.domains.media.state.media_store);
            //let items_clone = items.clone();

            /*
            // Spawn processing on background thread directly
            tokio::spawn(async move {
                let coordinator = crate::domains::media::store::BatchCoordinator::new(media_store);

                // Process as metadata batch (not initial load)
                match coordinator.process_metadata_batch(items_clone).await {
                    Ok(_) => {
                        log::info!("[CrossDomain] Batch metadata processing completed");
                    }
                    Err(e) => {
                        log::error!("[CrossDomain] Failed to process metadata batch: {}", e);
                    }
                }
            });*/

            // Return Task::none() - processing happens in the background
            Task::none()
        }

        CrossDomainEvent::RequestBatchMetadataFetch(libraries_data) => {
            log::info!(
                "[CrossDomain] Batch metadata fetch requested for {} libraries",
                libraries_data.len()
            );
            // Forward to metadata domain to handle the batch fetching
            //Task::done(DomainMessage::Metadata(
            //    crate::domains::metadata::messages::Message::FetchBatchMetadata(libraries_data),
            //))
            Task::none()
        }

        // Search-related events
        CrossDomainEvent::SearchInProgress(is_searching) => {
            log::debug!("[CrossDomain] Search in progress: {}", is_searching);
            // UI might want to show a loading indicator
            Task::none()
        }

        CrossDomainEvent::NavigateToMedia(media_ref) => {
            log::info!("[CrossDomain] Navigate to media requested");
            // Convert to appropriate UI navigation message based on media type
            use crate::infra::api_types::Media;

            let ui_message = match media_ref {
                Media::Movie(movie) => {
                    ui::messages::Message::ViewMovieDetails(movie.id)
                }
                Media::Series(series) => {
                    ui::messages::Message::ViewTvShow(series.id)
                }
                Media::Season(season) => ui::messages::Message::ViewSeason(
                    season.series_id,
                    season.id,
                ),
                Media::Episode(episode) => {
                    ui::messages::Message::ViewEpisode(episode.id)
                }
            };

            Task::done(DomainMessage::Ui(ui_message))
        }

        CrossDomainEvent::RequestMediaDetails(_media_ref) => {
            log::debug!("[CrossDomain] Media details requested");
            todo!();
        }

        CrossDomainEvent::NoOp => Task::none(),

        // Other events that don't require special handling yet
        _ => {
            log::trace!(
                "[CrossDomain] Event {:?} - no special handling needed",
                event
            );
            Task::none()
        }
    }
}

/// Handle authentication completion - trigger initial data loading
fn handle_authentication_complete(state: &State) -> Task<DomainMessage> {
    log::info!(
        "[CrossDomain] AuthenticationComplete event received - triggering initial data load"
    );

    // Guard against duplicate library loading
    let accessor = &state.domains.library.state.repo_accessor;
    if let Ok(library_count) = accessor.library_count()
        && library_count > 0
    {
        log::info!(
            "[CrossDomain] {} libraries already loaded, skipping duplicate load",
            library_count
        );
        return Task::none();
    }

    // Also guard based on the library domain's load state
    {
        use crate::domains::library::LibrariesLoadState;
        match state.domains.library.state.load_state {
            LibrariesLoadState::InProgress
            | LibrariesLoadState::Succeeded { .. } => {
                log::info!(
                    "[CrossDomain] Library load is in-progress or already succeeded; skipping duplicate trigger"
                );
                return Task::none();
            }
            LibrariesLoadState::NotStarted | LibrariesLoadState::Failed { .. } => {}
        }
    }

    let mut tasks = vec![];

    // Load libraries
    log::info!("[CrossDomain] Creating LoadLibraries task (first time only)");
    tasks.push(Task::done(DomainMessage::Library(
        library::messages::Message::LoadLibraries,
    )));

    // Check for active scans
    log::info!("[CrossDomain] Creating CheckActiveScans task");
    tasks.push(Task::done(DomainMessage::Library(
        library::messages::Message::FetchActiveScans,
    )));

    // Additional initialization tasks can be added here
    log::info!(
        "[CrossDomain] Batching {} tasks for post-auth initialization",
        tasks.len()
    );

    Task::batch(tasks)
}

/// Handle database cleared - refresh all data
fn handle_database_cleared(_state: &State) -> Task<DomainMessage> {
    log::info!("[CrossDomain] Database cleared - refreshing all data");

    // After database is cleared, we need to reload libraries
    Task::done(DomainMessage::Library(
        library::messages::Message::LoadLibraries,
    ))
}

/// Handle library refresh request
fn handle_library_refresh_request(state: &State) -> Task<DomainMessage> {
    log::info!("[CrossDomain] Library refresh requested");

    let mut tasks = vec![];

    if !state.domains.library.state.repo_accessor.is_initialized() {
        use crate::domains::library::LibrariesLoadState;
        match state.domains.library.state.load_state {
            LibrariesLoadState::NotStarted | LibrariesLoadState::Failed { .. } => {
                // Reload libraries
                tasks.push(Task::done(DomainMessage::Library(
                    library::messages::Message::LoadLibraries,
                )));
            }
            LibrariesLoadState::InProgress
            | LibrariesLoadState::Succeeded { .. } => {}
        }
    }

    // If we have a current library, refresh its content
    if let Some(_library_id) = state.domains.library.state.current_library_id {
        tasks.push(Task::done(DomainMessage::Library(
            library::messages::Message::RefreshLibrary,
        )));
    }

    // UI domain will be notified separately via its own event handler
    // We can't call handle_event here because state is immutable

    Task::batch(tasks)
}

/// Helper to emit cross-domain events from domain handlers
///
/// Domain handlers should use this to emit events rather than
/// trying to send messages to other domains directly.
pub fn emit_event(event: CrossDomainEvent) -> Task<DomainMessage> {
    Task::done(DomainMessage::Event(event))
}
