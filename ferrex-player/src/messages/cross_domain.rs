//! Cross-domain event coordination module
//!
//! This module handles events that require coordination between multiple domains.
//! It acts as a mediator to maintain proper domain boundaries while enabling
//! necessary cross-domain workflows.

use crate::messages::{auth, library, media, ui, CrossDomainEvent, DomainMessage};
use crate::state::State;
use iced::Task;

/// Process cross-domain events and return appropriate domain messages
pub fn handle_event(state: &mut State, event: CrossDomainEvent) -> Task<DomainMessage> {
    log::debug!("[CrossDomain] Processing event: {:?}", event);

    match event {
        // Authentication flow completion
        CrossDomainEvent::AuthenticationComplete => handle_authentication_complete(state),
        
        // Auth command execution requested
        CrossDomainEvent::AuthCommandRequested(command) => {
            log::info!("[CrossDomain] Auth command requested: {}", command.sanitized_display());
            Task::done(DomainMessage::Auth(auth::Message::ExecuteCommand(command)))
        },
        
        // Auth command execution completed
        CrossDomainEvent::AuthCommandCompleted(command, result) => {
            log::info!("[CrossDomain] Auth command completed: {} - {:?}", command.name(), result.is_success());
            
            // Convert auth command result to appropriate settings message
            match command {
                auth::AuthCommand::ChangePassword { .. } => {
                    let settings_result = match result {
                        auth::AuthCommandResult::Success => Ok(()),
                        auth::AuthCommandResult::Error(msg) => Err(msg.clone()),
                    };
                    Task::done(DomainMessage::Settings(crate::messages::settings::Message::PasswordChangeResult(settings_result)))
                },
                auth::AuthCommand::SetDevicePin { .. } | auth::AuthCommand::ChangeDevicePin { .. } => {
                    let settings_result = match result {
                        auth::AuthCommandResult::Success => Ok(()),
                        auth::AuthCommandResult::Error(msg) => Err(msg.clone()),
                    };
                    Task::done(DomainMessage::Settings(crate::messages::settings::Message::PinChangeResult(settings_result)))
                },
                _ => Task::none(),
            }
        },
        
        // Auth configuration changed
        CrossDomainEvent::AuthConfigurationChanged => {
            log::info!("[CrossDomain] Auth configuration changed");
            // Could trigger UI updates, permission refresh, etc.
            Task::none()
        },

        // Database cleared - need to refresh data
        CrossDomainEvent::DatabaseCleared => handle_database_cleared(state),

        // Library refresh requested
        CrossDomainEvent::RequestLibraryRefresh => handle_library_refresh_request(state),

        // User authenticated - store in state
        CrossDomainEvent::UserAuthenticated(user, permissions) => {
            state.is_authenticated = true;
            state.user_permissions = Some(permissions);
            log::info!("[CrossDomain] User {} authenticated", user.username);
            Task::none()
        }

        // User logged out - clear state
        CrossDomainEvent::UserLoggedOut => {
            state.is_authenticated = false;
            state.user_permissions = None;
            log::info!("[CrossDomain] User logged out");
            Task::none()
        }

        // Library selected
        CrossDomainEvent::LibrarySelected(library_id) => {
            state.current_library_id = Some(library_id);
            log::info!("[CrossDomain] Library {} selected", library_id);
            Task::none()
        }

        // Video ready to play - trigger video loading
        CrossDomainEvent::VideoReadyToPlay => {
            log::info!("[CrossDomain] Video ready to play - triggering video load");
            // Create a task that will load the video
            // This needs to be handled in the media domain
            Task::done(DomainMessage::Media(media::Message::_LoadVideo))
        }

        // Navigate to home/library view
        CrossDomainEvent::NavigateHome => {
            log::info!("[CrossDomain] Navigate home requested");
            Task::done(DomainMessage::Ui(ui::Message::NavigateHome))
        }

        // Toggle fullscreen mode
        CrossDomainEvent::MediaToggleFullscreen => {
            log::info!("[CrossDomain] Toggle fullscreen requested");
            Task::done(DomainMessage::Media(media::Message::ToggleFullscreen))
        }

        // Toggle scan progress visibility
        CrossDomainEvent::LibraryToggleScanProgress => {
            log::info!("[CrossDomain] Toggle scan progress requested");
            Task::done(DomainMessage::Library(library::Message::ToggleScanProgress))
        }

        // Play media with ID for tracking
        CrossDomainEvent::MediaPlayWithId(media_file, media_id) => {
            log::info!("[CrossDomain] Play media with ID: {:?}", media_id);
            Task::done(DomainMessage::Media(media::Message::PlayMediaWithId(
                media_file, media_id,
            )))
        }

        // Library management events
        CrossDomainEvent::LibraryShowForm(library) => {
            log::info!("[CrossDomain] Show library form");
            Task::done(DomainMessage::Library(library::Message::ShowLibraryForm(
                library,
            )))
        }
        CrossDomainEvent::LibraryHideForm => {
            log::info!("[CrossDomain] Hide library form");
            Task::done(DomainMessage::Library(library::Message::HideLibraryForm))
        }
        CrossDomainEvent::LibraryScan(library_id) => {
            log::info!("[CrossDomain] Scan library: {}", library_id);
            Task::done(DomainMessage::Library(library::Message::ScanLibrary_(
                library_id,
            )))
        }
        CrossDomainEvent::LibraryDelete(library_id) => {
            log::info!("[CrossDomain] Delete library: {}", library_id);
            Task::done(DomainMessage::Library(library::Message::DeleteLibrary(
                library_id,
            )))
        }
        CrossDomainEvent::LibraryFormUpdateName(name) => {
            log::trace!("[CrossDomain] Update library form name");
            Task::done(DomainMessage::Library(
                library::Message::UpdateLibraryFormName(name),
            ))
        }
        CrossDomainEvent::LibraryFormUpdateType(library_type) => {
            log::trace!("[CrossDomain] Update library form type");
            Task::done(DomainMessage::Library(
                library::Message::UpdateLibraryFormType(library_type),
            ))
        }
        CrossDomainEvent::LibraryFormUpdatePaths(paths) => {
            log::trace!("[CrossDomain] Update library form paths");
            Task::done(DomainMessage::Library(
                library::Message::UpdateLibraryFormPaths(paths),
            ))
        }
        CrossDomainEvent::LibraryFormUpdateScanInterval(interval) => {
            log::trace!("[CrossDomain] Update library form scan interval");
            Task::done(DomainMessage::Library(
                library::Message::UpdateLibraryFormScanInterval(interval),
            ))
        }
        CrossDomainEvent::LibraryFormToggleEnabled => {
            log::trace!("[CrossDomain] Toggle library form enabled");
            Task::done(DomainMessage::Library(
                library::Message::ToggleLibraryFormEnabled,
            ))
        }
        CrossDomainEvent::LibraryFormSubmit => {
            log::info!("[CrossDomain] Submit library form");
            Task::done(DomainMessage::Library(library::Message::SubmitLibraryForm))
        }

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
    log::info!("[CrossDomain] Authentication complete - triggering initial data load");

    let mut tasks = vec![];

    // Load libraries
    tasks.push(Task::done(DomainMessage::Library(
        library::Message::LoadLibraries,
    )));

    // Check for active scans
    tasks.push(Task::done(DomainMessage::Library(
        library::Message::CheckActiveScans,
    )));

    // Additional initialization tasks can be added here

    Task::batch(tasks)
}

/// Handle database cleared - refresh all data
fn handle_database_cleared(state: &State) -> Task<DomainMessage> {
    log::info!("[CrossDomain] Database cleared - refreshing all data");

    // After database is cleared, we need to reload libraries
    Task::done(DomainMessage::Library(library::Message::LoadLibraries))
}

/// Handle library refresh request
fn handle_library_refresh_request(state: &State) -> Task<DomainMessage> {
    log::info!("[CrossDomain] Library refresh requested");

    let mut tasks = vec![];

    // Reload libraries
    tasks.push(Task::done(DomainMessage::Library(
        library::Message::LoadLibraries,
    )));

    // If we have a current library, refresh its content
    if let Some(library_id) = state.current_library_id {
        tasks.push(Task::done(DomainMessage::Library(
            library::Message::RefreshLibrary,
        )));
    }

    Task::batch(tasks)
}

/// Helper to emit cross-domain events from domain handlers
///
/// Domain handlers should use this to emit events rather than
/// trying to send messages to other domains directly.
pub fn emit_event(event: CrossDomainEvent) -> Task<DomainMessage> {
    Task::done(DomainMessage::Event(event))
}
