use super::LibraryMessage;
use ferrex_player_api::services::api::ApiService;
use ferrex_player_library::messages::media_events_subscription::{
    MediaEventsId, media_events_stream,
};
use iced::Subscription;
use std::sync::Arc;

/// Creates an Iced subscription to server-sent events for library media changes.
pub fn media_events(
    server_url: String,
    api_service: Arc<dyn ApiService>,
) -> Subscription<LibraryMessage> {
    Subscription::run_with(
        MediaEventsId::new(server_url, api_service),
        media_events_stream,
    )
}
