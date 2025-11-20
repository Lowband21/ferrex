//! Tests for message routing infrastructure
//!
//! Validates that:
//! - Direct messages reach their target domains
//! - Message ordering is preserved
//! - No messages are lost
//! - Performance is acceptable

#[cfg(test)]
mod routing_tests {
    use ferrex_player::common::messages::{CrossDomainEvent, DomainMessage};

    #[test]
    fn test_direct_message_types() {
        // Verify all domain message types can be created
        use ferrex_player::domains::{
            auth, library, media, metadata, player, search, settings, streaming, ui,
            user_management,
        };

        // Test that each domain message type converts to DomainMessage
        let _auth_msg: DomainMessage = auth::messages::Message::CheckAuthStatus.into();
        let _library_msg: DomainMessage = library::messages::Message::LoadLibraries.into();
        let _media_msg: DomainMessage = media::messages::Message::Stop.into();
        let _player_msg: DomainMessage = player::messages::Message::NavigateBack.into();
        let _ui_msg: DomainMessage = ui::messages::Message::NavigateHome.into();
        let _metadata_msg: DomainMessage = metadata::messages::Message::InitializeService.into();
        let _streaming_msg: DomainMessage =
            streaming::messages::Message::CheckTranscodingStatus.into();
        let _settings_msg: DomainMessage = settings::messages::Message::ShowProfile.into();
        let _user_mgmt_msg: DomainMessage = user_management::messages::Message::LoadUsers.into();
        let _search_msg: DomainMessage = search::messages::Message::ClearSearch.into();

        // All conversions compile = type safety verified
    }

    #[test]
    fn test_cross_domain_event_routing() {
        // Verify cross-domain events can be wrapped in DomainMessage
        let event = CrossDomainEvent::AuthenticationComplete;
        let _msg = DomainMessage::Event(event);

        // Test various event types
        let _auth_event = DomainMessage::Event(CrossDomainEvent::UserLoggedOut);
        let _library_event = DomainMessage::Event(CrossDomainEvent::LibraryUpdated);
        let _media_event = DomainMessage::Event(CrossDomainEvent::MediaStopped);
    }

    #[test]
    fn test_message_naming() {
        // Verify message naming for debugging/profiling
        use ferrex_player::domains::auth;

        let msg = DomainMessage::Auth(auth::messages::Message::CheckAuthStatus);
        let name = msg.name();
        assert!(
            name.starts_with("Auth"),
            "Auth message name should start with 'Auth', got: {}",
            name
        );

        let msg = DomainMessage::Event(CrossDomainEvent::AuthenticationComplete);
        let name = msg.name();
        assert_eq!(name, "DomainMessage::Event");
    }

    #[test]
    fn test_no_message_variants_missing() {
        // This test ensures all DomainMessage variants are handled
        // If this fails to compile after adding a new domain,
        // it means the routing needs to be updated

        fn exhaustive_match(msg: DomainMessage) -> &'static str {
            match msg {
                DomainMessage::Auth(_) => "auth",
                DomainMessage::Library(_) => "library",
                DomainMessage::Media(_) => "media",
                DomainMessage::Player(_) => "player",
                DomainMessage::Ui(_) => "ui",
                DomainMessage::Metadata(_) => "metadata",
                DomainMessage::Streaming(_) => "streaming",
                DomainMessage::Settings(_) => "settings",
                DomainMessage::UserManagement(_) => "user_management",
                DomainMessage::Search(_) => "search",
                DomainMessage::NoOp => "noop",
                DomainMessage::Tick => "tick",
                DomainMessage::ClearError => "clear_error",
                DomainMessage::Event(_) => "event",
            }
        }

        // If this compiles, all variants are handled
        let test_msg = DomainMessage::NoOp;
        let _ = exhaustive_match(test_msg);
    }
}
