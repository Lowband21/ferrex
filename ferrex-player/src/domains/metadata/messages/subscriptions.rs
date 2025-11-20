use crate::common::messages::DomainMessage;
use crate::infrastructure::services::api::ApiService;
use crate::state::State;
use iced::Subscription;
use std::sync::Arc;

/// Creates all metadata-related subscriptions
pub fn subscription(state: &State) -> Subscription<DomainMessage> {
    let mut subscriptions = vec![];

    // Batch metadata fetcher no longer uses subscriptions - it emits events directly through Tasks
    // Image loading subscription
    // Get auth token from API service if available
    let auth_token = state
        .domains
        .metadata
        .state
        .api_service
        .as_ref()
        .and_then(|service| {
            // Use block_in_place to get the token synchronously
            let service = service.clone();
            tokio::task::block_in_place(move || {
                tokio::runtime::Handle::current().block_on(async move {
                    service
                        .get_token()
                        .await
                        .map(|token| format!("Bearer {}", token.access_token))
                })
            })
        });

    // Always try to create the subscription - it will handle checking if it's already running
    subscriptions.push(
        super::image_loading_subscription::image_loading(
            Arc::clone(&state.api_service),
            state.server_url.clone(),
            Arc::clone(&state.image_receiver),
            auth_token,
        )
        .map(DomainMessage::Metadata),
    );

    // Periodic cleanup of stale image cache entries
    subscriptions.push(
        super::image_cache_cleanup_subscription::image_cache_cleanup()
            .map(DomainMessage::Metadata),
    );

    Subscription::batch(subscriptions)
}
