//! Domain modules for the Ferrex player
//!
//! This module organizes the application into domain-driven modules,
//! breaking up the monolithic state and update logic into focused,
//! testable domains.

pub mod auth;
pub mod library;
pub mod media;
pub mod metadata;
pub mod player;
pub mod search;
pub mod settings;
pub mod streaming;
pub mod ui;
pub mod user_management;

use crate::common::messages::{
    CrossDomainEvent, DomainMessage, DomainUpdateResult,
};
use iced::Task;

/// Domain trait that all domains must implement
/// Provides a unified interface for updating domain state
pub trait Domain {
    /// The message type for this domain
    type Message;

    /// Update the domain state based on a message
    /// Returns a DomainUpdateResult containing a task and events to emit
    fn update(&mut self, message: Self::Message) -> DomainUpdateResult;

    /// Handle a cross-domain event
    /// Returns a Task that will produce domain messages
    fn handle_event(&mut self, event: &CrossDomainEvent)
    -> Task<DomainMessage>;
}

/// Domain registry that manages all domain states
#[derive(Debug)]
pub struct DomainRegistry {
    pub auth: auth::AuthDomain,
    pub library: library::LibraryDomain,
    pub media: media::MediaDomain,
    pub metadata: metadata::MetadataDomain,
    pub player: player::PlayerDomain,
    pub settings: settings::SettingsDomain,
    pub streaming: streaming::StreamingDomain,
    pub ui: ui::UIDomain,
    pub user_management: user_management::UserManagementDomain,
    pub search: search::SearchDomain,
}

impl DomainRegistry {
    /// Handle a cross-domain event by notifying all relevant domains
    pub fn handle_event(
        &mut self,
        event: CrossDomainEvent,
    ) -> Task<DomainMessage> {
        // Each domain can react to cross-domain events
        let tasks = vec![
            self.auth.handle_event(&event),
            self.library.handle_event(&event),
            self.media.handle_event(&event),
            self.metadata.handle_event(&event),
            self.player.handle_event(&event),
            self.settings.handle_event(&event),
            self.streaming.handle_event(&event),
            self.ui.handle_event(&event),
            self.user_management.handle_event(&event),
            self.search.handle_event(&event),
        ];

        Task::batch(tasks)
    }
}
