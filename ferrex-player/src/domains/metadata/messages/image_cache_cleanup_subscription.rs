use super::MetadataMessage;
use crate::domains::metadata::image_service::UnifiedImageService;
use crate::infra::cache::PlayerDiskImageCache;
use futures::{StreamExt, stream};
use iced::Subscription;
use std::sync::Arc;

/// Creates a subscription that periodically cleans up stale image cache entries
pub fn image_cache_cleanup(
    image_service: Arc<UnifiedImageService>,
    disk_cache: Option<Arc<PlayerDiskImageCache>>,
) -> Subscription<MetadataMessage> {
    #[derive(Debug, Clone)]
    struct ImageCacheCleanupSubscription {
        id: u64,
        image_service: Arc<UnifiedImageService>,
        disk_cache: Option<Arc<PlayerDiskImageCache>>,
    }

    impl PartialEq for ImageCacheCleanupSubscription {
        fn eq(&self, other: &Self) -> bool {
            self.id == other.id
        }
    }
    impl Eq for ImageCacheCleanupSubscription {}

    impl std::hash::Hash for ImageCacheCleanupSubscription {
        fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
            self.id.hash(state);
        }
    }

    fn build_stream(
        sub: &ImageCacheCleanupSubscription,
    ) -> futures::stream::BoxStream<'static, MetadataMessage> {
        let image_service = sub.image_service.clone();
        let disk_cache = sub.disk_cache.clone();

        stream::unfold((tokio::time::Instant::now(), 0u32), move |state| {
            let image_service = image_service.clone();
            let disk_cache = disk_cache.clone();
            async move {
                let (last_cleanup, ticks) = state;

                // Wait 5 minutes between cleanups.
                let next_cleanup =
                    last_cleanup + std::time::Duration::from_secs(300);
                tokio::time::sleep_until(next_cleanup).await;

                // RAM cleanup:
                // - evict stale non-loaded states older than 30 minutes
                // - enforce RAM budget for loaded images (LRU-ish)
                image_service.cleanup_stale_entries(
                    std::time::Duration::from_secs(1800),
                );

                // Disk cleanup: run every 60 minutes (12 * 5min ticks).
                if ticks.is_multiple_of(12)
                    && let Some(cache) = disk_cache.as_ref()
                {
                    cache.cleanup_once().await;
                }

                log::debug!("Cleaned up image caches (ram + disk)");

                Some((
                    MetadataMessage::NoOp,
                    (tokio::time::Instant::now(), ticks.wrapping_add(1)),
                ))
            }
        })
        .boxed()
    }

    Subscription::run_with(
        ImageCacheCleanupSubscription {
            id: 1,
            image_service,
            disk_cache,
        },
        build_stream,
    )
}
