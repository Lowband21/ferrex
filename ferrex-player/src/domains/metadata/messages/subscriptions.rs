use crate::common::messages::DomainMessage;
use crate::state::State;
use iced::Subscription;
use std::sync::Arc;

/// Creates all metadata-related subscriptions
pub fn subscription(state: &State) -> Subscription<DomainMessage> {
    let mut subs = Vec::new();

    if !state.server_url.is_empty() {
        subs.push(
            super::image_events_subscription::image_events(
                state.server_url.clone(),
                Arc::clone(&state.api_service),
            )
            .map(DomainMessage::Metadata),
        );
    }

    subs.push(
        super::image_loading_subscription::image_loading(
            Arc::clone(&state.api_service),
            state.server_url.clone(),
            Arc::clone(&state.image_receiver),
            state.disk_image_cache.clone(),
        )
        .map(DomainMessage::Metadata),
    );
    subs.push(
        super::image_cache_cleanup_subscription::image_cache_cleanup(
            Arc::clone(&state.image_service),
            state.disk_image_cache.clone(),
        )
        .map(DomainMessage::Metadata),
    );

    Subscription::batch(subs)
}
