//! Root-level update function that routes domain messages to appropriate handlers
//!
//! ## Message Routing Architecture
//!
//! This module implements a domain-driven message routing system with two types of messages:
//!
//! ### 1. Direct Domain Messages
//! Messages targeted at a specific domain are routed directly without broadcasting:
//! - `DomainMessage::Auth(_)` → Auth domain handler
//! - `DomainMessage::Library(_)` → Library domain handler
//! - `DomainMessage::Media(_)` → Media domain handler
//! - etc.
//!
//! Direct messages provide:
//! - Type safety: Only valid messages can be sent to each domain
//! - Performance: No unnecessary broadcasting or filtering
//! - Clear intent: Message destination is explicit in the type
//!
//! ### 2. Cross-Domain Events
//! Events that multiple domains need to react to are broadcast via `DomainMessage::Event(_)`:
//! - Authentication state changes
//! - Media playback state changes
//! - Library updates
//! - etc.
//!
//! ### Message Ordering
//! Message ordering is preserved through:
//! - Sequential processing in the update function
//! - Task batching for multiple resulting tasks
//! - FIFO queue semantics in the Iced runtime
//!
//! ### Performance Optimizations
//! - Direct routing avoids unnecessary domain checks
//! - Events from DomainUpdateResult are batched
//! - Profiling tracks message processing time

use crate::common::messages::{CrossDomainEvent, DomainMessage, DomainUpdateResult};
use crate::domains::auth::update::update_auth;
use crate::domains::library::update::update_library;
use crate::domains::media::update::update_media;
use crate::domains::metadata::update::update_metadata;
use crate::domains::player::update::update_player;
use crate::domains::settings::update::update_settings;
use crate::domains::streaming::update::update_streaming;
use crate::domains::ui::update::update_ui;
use crate::domains::user_management::update::update_user_management;
use crate::domains::search::update as search_update;
use crate::infrastructure::profiling::PROFILER;
use crate::state_refactored::State;
use iced::Task;

/// Domain-aware update function that routes messages to appropriate handlers
/// and collects events from DomainUpdateResult for cross-domain communication
pub fn update(state: &mut State, message: DomainMessage) -> Task<DomainMessage> {
    // Add profiling for domain messages
    let message_name = message.name();
    PROFILER.start(&format!("update::{}", message_name));
    
    // Log direct message routing for validation
    #[cfg(debug_assertions)]
    match &message {
        DomainMessage::Event(_) => {
            log::trace!("[Router] Broadcasting cross-domain event");
        }
        _ => {
            log::trace!("[Router] Direct message to {}", message_name);
        }
    }

    // Process the message and collect any events
    let update_result = match message {
        // Route auth messages to the auth domain handler
        DomainMessage::Auth(auth_msg) => {
            update_auth(state, auth_msg)
        }

        // Route library messages to the library domain handler
        DomainMessage::Library(library_msg) => {
            update_library(state, library_msg)
        }

        // Route media messages to the media domain handler
        DomainMessage::Media(media_msg) => {
            update_media(state, media_msg)
        }

        // Route player messages to the player domain handler
        DomainMessage::Player(player_msg) => {
            update_player(&mut state.domains.player.state, player_msg, state.window_size)
        }

        // Route metadata messages to the metadata domain handler
        DomainMessage::Metadata(metadata_msg) => {
            update_metadata(state, metadata_msg)
        }

        // Route UI messages to the UI domain handler
        DomainMessage::Ui(ui_msg) => {
            update_ui(state, ui_msg)
        }

        // Route streaming messages to the streaming domain handler
        DomainMessage::Streaming(streaming_msg) => {
            update_streaming(state, streaming_msg)
        }

        // Route settings messages to the settings domain handler
        DomainMessage::Settings(settings_msg) => {
            update_settings(state, settings_msg)
        }

        // Route user management messages to the user management domain handler
        DomainMessage::UserManagement(user_mgmt_msg) => {
            update_user_management(state, user_mgmt_msg)
        }

        // Route search messages to the search domain handler
        DomainMessage::Search(search_msg) => {
            search_update::update(state, search_msg)
        }

        // Cross-domain messages
        DomainMessage::NoOp => DomainUpdateResult::task(Task::none()),

        DomainMessage::ClearError => {
            state.domains.ui.state.error_message = None;
            DomainUpdateResult::task(Task::none())
        }

        DomainMessage::Event(event) => {
            // Process cross-domain events and trigger appropriate domain actions
            log::info!("[Update] Processing cross-domain event: {:?}", event);
            DomainUpdateResult::task(crate::common::messages::cross_domain::handle_event(state, event))
        }
        
        DomainMessage::Tick => {
            // Periodic tick for UI updates or polling
            DomainUpdateResult::task(Task::none())
        }
    };

    // Process any events that were collected
    let mut tasks = vec![update_result.task];
    
    // If there are events to broadcast, handle them
    for event in update_result.events {
        log::debug!("[Update] Broadcasting event from DomainUpdateResult: {:?}", event);
        tasks.push(crate::common::messages::cross_domain::handle_event(state, event));
    }

    PROFILER.end(&format!("update::{}", message_name));
    
    // Batch all tasks together
    Task::batch(tasks)
}