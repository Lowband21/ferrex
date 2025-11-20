use super::MetadataMessage;
use futures::stream;
use iced::Subscription;

/// Creates a subscription that periodically cleans up stale image cache entries
pub fn image_cache_cleanup() -> Subscription<MetadataMessage> {
    #[derive(Debug, Clone, Hash)]
    struct ImageCacheCleanupId;

    Subscription::run_with(ImageCacheCleanupId, |_| {
        stream::unfold(tokio::time::Instant::now(), |last_cleanup| async move {
            // Wait 5 minutes between cleanups
            let next_cleanup =
                last_cleanup + std::time::Duration::from_secs(300);
            tokio::time::sleep_until(next_cleanup).await;

            // Perform cleanup
            if let Some(image_service) =
                crate::infra::service_registry::get_image_service()
            {
                // Clean up entries older than 30 minutes
                image_service.get().cleanup_stale_entries(
                    std::time::Duration::from_secs(1800),
                );
                log::debug!("Cleaned up stale image cache entries");
            }

            Some((MetadataMessage::NoOp, tokio::time::Instant::now()))
        })
    })
}
