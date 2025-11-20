use crate::common::messages::DomainMessage;
use crate::state::State;
use iced::Subscription;
use std::sync::Arc;

/// Creates all metadata-related subscriptions
pub fn subscription(state: &State) -> Subscription<DomainMessage> {
    Subscription::batch(vec![
        super::image_loading_subscription::image_loading(
            Arc::clone(&state.api_service),
            state.server_url.clone(),
            Arc::clone(&state.image_receiver),
        )
        .map(DomainMessage::Metadata),
        super::image_cache_cleanup_subscription::image_cache_cleanup()
            .map(DomainMessage::Metadata),
    ])
}
