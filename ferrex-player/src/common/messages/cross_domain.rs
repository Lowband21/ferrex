//! Cross-domain event coordination module
//!
//! This module handles events that require coordination between multiple domains.
//! It acts as a mediator to maintain proper domain boundaries while enabling
//! necessary cross-domain workflows.

use crate::common::messages::{CrossDomainEvent, DomainMessage};
use crate::domains::{auth, library, media, ui};
use crate::domains::ui::scroll_manager::ScrollStateExt;
use crate::state_refactored::State;
use iced::Task;
use std::sync::Arc;

pub fn handle_event(state: &mut State, event: CrossDomainEvent) -> Task<DomainMessage> {
    log::debug!("[CrossDomain] Processing event: {:?}", event);

    match event {
        // Authentication flow completion
        CrossDomainEvent::AuthenticationComplete => handle_authentication_complete(state),

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
                        auth::messages::AuthCommandResult::Error(msg) => Err(msg.clone()),
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
                        auth::messages::AuthCommandResult::Error(msg) => Err(msg.clone()),
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
            let library_tasks = handle_library_refresh_request(state);
            // Also notify UI to refresh ViewModels
            // [MediaStoreNotifier] RefreshViewModels no longer needed here
            library_tasks
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
            log::info!("[CrossDomain] User logged out - emitting cleanup events");

            // Clear all sensitive data
            Task::batch(vec![
                Task::done(DomainMessage::Event(CrossDomainEvent::ClearMediaStore)),
                Task::done(DomainMessage::Event(CrossDomainEvent::ClearLibraries)),
                Task::done(DomainMessage::Event(CrossDomainEvent::ClearCurrentShowData)),
            ])
        }

        // Library events
        CrossDomainEvent::LibrarySelected(library_id) => {
            state.domains.library.state.current_library_id = Some(library_id);
            
            // Restore scroll state for the new library context
            state.restore_library_scroll_state(Some(library_id));
            
            Task::batch(vec![
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
            state.restore_library_scroll_state(None);
            
            Task::batch(vec![
                Task::done(DomainMessage::Library(
                    library::messages::Message::SelectLibrary(None),
                )),
                // Notify UI to update view models
                // [MediaStoreNotifier] RefreshViewModels no longer needed here
            ])
        }
        CrossDomainEvent::LibraryChanged(library_id) => {
            log::info!("[CrossDomain] Library changed to: {}", library_id);
            // Notify all domains about library change
            state
                .domains
                .handle_event(CrossDomainEvent::LibraryChanged(library_id))
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
            log::info!("[CrossDomain] Media loaded - running search calibration");
            // Run search calibration after media is loaded
            Task::perform(
                async move {
                    // Small delay to ensure media store is fully populated
                    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                },
                |_| DomainMessage::Search(
                    crate::domains::search::messages::Message::RunCalibration
                )
            )
        }

        // Toggle fullscreen mode
        CrossDomainEvent::MediaToggleFullscreen => {
            log::info!("[CrossDomain] Toggle fullscreen requested");
            Task::done(DomainMessage::Ui(ui::messages::Message::ToggleFullscreen))
        }

        // Play media with ID
        CrossDomainEvent::MediaPlayWithId(media_file, media_id) => {
            log::info!("[CrossDomain] Play media with ID: {:?}", media_id);
            Task::done(DomainMessage::Media(
                media::messages::Message::PlayMediaWithId(media_file, media_id),
            ))
        }

        // Legacy transcoding events (deprecated)
        CrossDomainEvent::RequestTranscoding(_) | CrossDomainEvent::TranscodingReady(_) => {
            log::warn!("[CrossDomain] Legacy transcoding event received - ignoring");
            Task::none()
        }

        // Media playback events
        CrossDomainEvent::MediaStartedPlaying(media_file) => {
            log::info!("[CrossDomain] Media started playing");
            Task::done(DomainMessage::Media(media::messages::Message::PlayMedia(
                media_file,
            )))
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
            log::info!("[CrossDomain] Cleanup event {:?} - handled by domain event handlers", event);
            // Special handling for ClearLibraries to reset current_library_id
            if matches!(event, CrossDomainEvent::ClearLibraries) {
                state.domains.library.state.current_library_id = None;
            }
            Task::none()
        }

        // Metadata events
        CrossDomainEvent::BatchMetadataReady(items) => {
            log::info!("[CrossDomain] Batch metadata ready: {} items", items.len());

            // Log details about what we received
            let mut movies_with_details = 0;
            let mut series_with_details = 0;
            let mut still_need_fetch = 0;

            for item in &items {
                match item {
                    crate::infrastructure::api_types::MediaReference::Movie(movie) => {
                        if crate::infrastructure::api_types::needs_details_fetch(&movie.details) {
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
                    crate::infrastructure::api_types::MediaReference::Series(series) => {
                        if crate::infrastructure::api_types::needs_details_fetch(&series.details) {
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
            }

            log::info!(
                "[CrossDomain] Batch contains: {} movies with details, {} series with details, {} still need fetch",
                movies_with_details, series_with_details, still_need_fetch
            );

            // Update MediaStore with the fetched metadata
            // Note: We process metadata updates without batch mode to avoid conflicts
            // when multiple batches are processed simultaneously
            log::info!(
                "[CrossDomain] Updating MediaStore with {} items (no batch mode)",
                items.len()
            );

            // Update MediaStore directly - don't create a task that can be re-executed
            let media_store = Arc::clone(&state.domains.media.state.media_store);
            let items_clone = items.clone();

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
            });

            // Return Task::none() - processing happens in the background
            Task::none()
        }

        CrossDomainEvent::RequestBatchMetadataFetch(libraries_data) => {
            log::info!(
                "[CrossDomain] Batch metadata fetch requested for {} libraries",
                libraries_data.len()
            );
            // Forward to metadata domain to handle the batch fetching
            Task::done(DomainMessage::Metadata(
                crate::domains::metadata::messages::Message::FetchBatchMetadata(libraries_data),
            ))
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
            use crate::infrastructure::api_types::MediaReference;

            let ui_message = match media_ref {
                MediaReference::Movie(movie) => ui::messages::Message::ViewMovieDetails(movie),
                MediaReference::Series(series) => ui::messages::Message::ViewTvShow(series.id),
                MediaReference::Season(season) => {
                    ui::messages::Message::ViewSeason(season.series_id, season.id)
                }
                MediaReference::Episode(episode) => ui::messages::Message::ViewEpisode(episode.id),
            };

            Task::done(DomainMessage::Ui(ui_message))
        }

        CrossDomainEvent::RequestMediaDetails(media_ref) => {
            log::debug!("[CrossDomain] Media details requested");
            // This could trigger metadata fetching if needed
            Task::none()
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
    log::info!("[CrossDomain] AuthenticationComplete event received - triggering initial data load");

    // Guard against duplicate library loading
    if !state.domains.library.state.libraries.is_empty() {
        log::info!("[CrossDomain] Libraries already loaded, skipping duplicate load");
        return Task::none();
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
        library::messages::Message::CheckActiveScans,
    )));

    // Additional initialization tasks can be added here
    log::info!("[CrossDomain] Batching {} tasks for post-auth initialization", tasks.len());

    Task::batch(tasks)
}

/// Handle database cleared - refresh all data
fn handle_database_cleared(state: &State) -> Task<DomainMessage> {
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

    // Reload libraries
    tasks.push(Task::done(DomainMessage::Library(
        library::messages::Message::LoadLibraries,
    )));

    // If we have a current library, refresh its content
    if let Some(library_id) = state.domains.library.state.current_library_id {
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
