use crate::messages::DomainMessage;
use crate::state::State;
use iced::Subscription;
use std::sync::Arc;

/// Creates all metadata-related subscriptions
pub fn subscription(state: &State) -> Subscription<DomainMessage> {
    let mut subscriptions = vec![];

    // Batch metadata fetcher subscription
    subscriptions
        .push(super::subscription::batch_metadata_subscription(state).map(DomainMessage::Metadata));

    // Image loading subscription
    // Get auth token from API client if available
    let auth_token = state.api_client.as_ref().and_then(|client| {
        // Use block_in_place to get the token synchronously
        tokio::task::block_in_place(move || {
            tokio::runtime::Handle::current()
                .block_on(async move { client.get_auth_header().await })
        })
    });

    // Always try to create the subscription - it will handle checking if it's already running
    subscriptions.push(
        super::image_loading_subscription::image_loading(
            state.server_url.clone(),
            Arc::clone(&state.image_receiver),
            auth_token,
        )
        .map(DomainMessage::Metadata),
    );

    // Periodic cleanup of stale image cache entries
    subscriptions.push(
        super::image_cache_cleanup_subscription::image_cache_cleanup().map(DomainMessage::Metadata),
    );

    Subscription::batch(subscriptions)
}
