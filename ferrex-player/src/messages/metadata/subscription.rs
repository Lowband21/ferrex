use std::sync::Arc;

use futures::stream;
use iced::Subscription;

use crate::state::State;

use super::Message;

// Batch metadata fetcher subscription
#[derive(Debug, Clone)]
struct BatchMetadataSubscription {
    fetcher: Arc<crate::batch_metadata_fetcher::BatchMetadataFetcher>,
}

impl std::hash::Hash for BatchMetadataSubscription {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // Use a stable hash based on the subscription type
        "batch_metadata_subscription".hash(state);
    }
}

pub fn batch_metadata_subscription(state: &State) -> Subscription<Message> {
    if let Some(fetcher) = &state.batch_metadata_fetcher {
        let subscription_data = BatchMetadataSubscription {
            fetcher: Arc::clone(fetcher),
        };

        Subscription::run_with(subscription_data, batch_metadata_stream)
    } else {
        //log::debug!("[BatchMetadataFetcher] Subscription not active - fetcher not initialized");
        Subscription::none()
    }
}

pub fn batch_metadata_stream(
    subscription: &BatchMetadataSubscription,
) -> impl futures::Stream<Item = Message> {
    use tokio::time::{timeout, Duration};

    let fetcher = Arc::clone(&subscription.fetcher);

    // State: (fetcher, sent_complete, last_batch_time)
    stream::unfold(
        (fetcher, false, tokio::time::Instant::now()),
        |(fetcher, sent_complete, last_batch_time)| async move {
            // Check if processing is complete and we haven't sent the complete message yet
            if !sent_complete && fetcher.is_complete() {
                log::info!(
                    "Batch metadata fetching complete, sending BatchMetadataComplete message"
                );
                return Some((
                    Message::BatchMetadataComplete,
                    (fetcher, true, last_batch_time),
                ));
            }

            // Collect updates for up to 50ms or 100 items, whichever comes first
            let mut batch = Vec::new();
            let batch_start = tokio::time::Instant::now();
            let batch_timeout = Duration::from_millis(50);
            let max_batch_size = 100;

            // First, get any immediately available updates
            let initial_updates = fetcher.get_pending_updates().await;
            batch.extend(initial_updates);

            // If we have some updates, try to collect more within the timeout
            if !batch.is_empty() {
                while batch.len() < max_batch_size {
                    match timeout(
                        batch_timeout.saturating_sub(batch_start.elapsed()),
                        fetcher.get_pending_updates(),
                    )
                    .await
                    {
                        Ok(updates) if !updates.is_empty() => {
                            batch.extend(updates);
                        }
                        _ => break, // Timeout or no more updates
                    }
                }
            } else {
                // No initial updates, wait a bit before checking again
                tokio::time::sleep(Duration::from_millis(100)).await;
            }

            if !batch.is_empty() {
                log::debug!(
                    "Batch metadata subscription collected {} updates in {:?}",
                    batch.len(),
                    batch_start.elapsed()
                );

                // Send as a single batch message
                Some((
                    Message::MediaDetailsBatch(batch),
                    (fetcher, sent_complete, tokio::time::Instant::now()),
                ))
            } else {
                // No updates, just continue
                Some((Message::NoOp, (fetcher, sent_complete, last_batch_time)))
            }
        },
    )
}
