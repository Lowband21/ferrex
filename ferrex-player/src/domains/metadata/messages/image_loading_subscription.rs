use super::MetadataMessage;

use crate::infra::{
    cache::PlayerDiskImageCache,
    constants::image::IMAGE_PENDING_RETRY_DELAY,
    services::api::{ApiService, ImageFetchResult},
};

use ferrex_core::api::routes::v1;

use ferrex_model::image::ImageQuery;
use futures::{
    StreamExt,
    stream::{self, FuturesUnordered},
};

use iced::Subscription;
use std::sync::{Arc, Mutex};

/// Creates a subscription for loading images from the server
pub(crate) fn image_loading(
    api_service: Arc<dyn ApiService>,
    server_url: String,
    receiver: Arc<Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<()>>>>,
    disk_cache: Option<Arc<PlayerDiskImageCache>>,
) -> Subscription<MetadataMessage> {
    // Subscription data that includes both ID and context
    #[derive(Debug, Clone)]
    struct ImageLoaderSubscription {
        id: u64,
        api_service: Arc<dyn ApiService>,
        server_url: String,
        receiver: Arc<Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<()>>>>,
        disk_cache: Option<Arc<PlayerDiskImageCache>>,
    }

    impl PartialEq for ImageLoaderSubscription {
        fn eq(&self, other: &Self) -> bool {
            self.id == other.id && self.server_url == other.server_url
        }
    }
    impl Eq for ImageLoaderSubscription {}

    impl std::hash::Hash for ImageLoaderSubscription {
        fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
            self.id.hash(state);
            self.server_url.hash(state);
        }
    }

    let subscription = ImageLoaderSubscription {
        id: 1, // Static ID for singleton subscription
        api_service,
        server_url,
        receiver,
        disk_cache,
    };

    Subscription::run_with(subscription, |sub| {
        #[cfg(any(
            feature = "profile-with-puffin",
            feature = "profile-with-tracy",
            feature = "profile-with-tracing"
        ))]
        profiling::register_thread!("Image Loader Stream");
        #[cfg(any(
            feature = "profile-with-puffin",
            feature = "profile-with-tracy",
            feature = "profile-with-tracing"
        ))]
        profiling::scope!("ImageLoaderSubscription");
        image_loader_stream_concurrent(
            Arc::clone(&sub.api_service),
            sub.server_url.clone(),
            Arc::clone(&sub.receiver),
            sub.disk_cache.clone(),
        )
    })
}

fn image_loader_stream_concurrent(
    api_service: Arc<dyn ApiService>,
    server_url: String,
    wake_receiver_arc: Arc<
        Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<()>>>,
    >,
    disk_cache: Option<Arc<PlayerDiskImageCache>>,
) -> impl futures::Stream<Item = MetadataMessage> {
    enum ImageLoaderState {
        Running {
            last_spawn_time: Option<std::time::Instant>,
            receiver: Option<tokio::sync::mpsc::UnboundedReceiver<()>>,
            inflight:
                FuturesUnordered<tokio::task::JoinHandle<MetadataMessage>>,
        },
    }

    log::debug!("image_loader_stream (concurrent): Creating stream");

    stream::unfold(
        ImageLoaderState::Running {
            last_spawn_time: None,
            receiver: None,
            inflight: FuturesUnordered::new(),
        },
        move |state| {
            let server_url = server_url.clone();
            let api_service = Arc::clone(&api_service);
            let wake_receiver_arc = Arc::clone(&wake_receiver_arc);
            let disk_cache = disk_cache.clone();
            async move {
                match state {
                    ImageLoaderState::Running {
                        mut last_spawn_time,
                        mut receiver,
                        mut inflight,
                    } => {
                        // Keep a pool of concurrent fetch tasks. Tie to the
                        // UnifiedImageService cap so this stream respects the
                        // configured limit.
                        let desired_concurrency: usize =
                            crate::infra::service_registry::get_image_service()
                                .map(|svc| svc.max_concurrent())
                                .unwrap_or(8);

                        // Fill pool with new requests if available
                        loop {
                            if inflight.len() >= desired_concurrency {
                                break;
                            }

                            let request = match crate::infra::service_registry::get_image_service() {
                                Some(image_service) => image_service.get_next_request(),
                                _ => None,
                            };
                            let Some(request) = request else { break };

                            if let Some(image_service) = crate::infra::service_registry::get_image_service() {
                                image_service.mark_loading(&request);
                            }

                            let (cancel_tx, cancel_rx) =
                                tokio::sync::oneshot::channel::<()>();
                            if let Some(image_service) =
                                crate::infra::service_registry::get_image_service()
                            {
                                image_service
                                    .register_inflight_cancel(&request, cancel_tx);
                            } else {
                                drop(cancel_tx);
                            }

                            let api = Arc::clone(&api_service);
                            let srv = server_url.clone();
                            let disk_cache = disk_cache.clone();
                            let request_for_fetch = request.clone();
                            let request_for_cancel = request.clone();
                            let task = tokio::spawn(async move {
                                let fetch = async move {
                                    if let Some(cache) = disk_cache.as_ref()
                                        && let Some(bytes) = cache
                                            .read_bytes(&request_for_fetch)
                                            .await
                                    {
                                        let (handle, estimated_bytes) =
                                            crate::infra::cache::handle_from_encoded_bytes(
                                                &request_for_fetch,
                                                bytes,
                                            );
                                        return MetadataMessage::UnifiedImageLoaded(
                                            request_for_fetch,
                                            handle,
                                            estimated_bytes,
                                        );
                                    }

                                    if srv.is_empty() {
                                        return MetadataMessage::UnifiedImageLoadFailed(
                                            request_for_fetch,
                                            "Server URL is empty".to_string(),
                                        );
                                    }

                                    let image_query = ImageQuery {
                                        iid: request_for_fetch.iid,
                                        imz: request_for_fetch.size,
                                    };

                                    let result = api.as_ref().get_image(
                                        v1::images::SERVE,
                                        image_query,
                                    );

                                    let full_url = format!(
                                        "{}{}",
                                        srv,
                                        v1::images::SERVE,
                                    );

                                    match result.await {
                                        Ok(ImageFetchResult::Ready(bytes)) => {
                                            let byte_len = bytes.len();
                                            if byte_len == 0 {
                                                let msg = format!(
                                                    "Image request failed with url {} and ImageQuery {:#?}",
                                                    full_url, image_query
                                                );

                                                crate::infra::image_log::log_fetch_failure_once(
                                                    image_query,
                                                    &full_url,
                                                );
                                                return MetadataMessage::UnifiedImageLoadFailed(
                                                    request_for_fetch,
                                                    msg,
                                                );
                                            }

                                            // Quick header sniff: reject bodies that do not look like common image formats
                                            // to avoid attempting to decode corrupt/partial data.
                                            fn looks_like_supported_image(
                                                b: &[u8],
                                            ) -> bool
                                            {
                                                // JPEG: FF D8 FF
                                                if b.len() >= 3
                                                    && b[0] == 0xFF
                                                    && b[1] == 0xD8
                                                    && b[2] == 0xFF
                                                {
                                                    return true;
                                                }
                                                // PNG: 89 50 4E 47 0D 0A 1A 0A
                                                if b.len() >= 8
                                                    && b[0..8]
                                                        == [
                                                            0x89, 0x50, 0x4E,
                                                            0x47, 0x0D, 0x0A,
                                                            0x1A, 0x0A,
                                                        ]
                                                {
                                                    return true;
                                                }
                                                // WebP: RIFF....WEBP
                                                if b.len() >= 12
                                                    && &b[0..4] == b"RIFF"
                                                    && &b[8..12] == b"WEBP"
                                                {
                                                    return true;
                                                }
                                                // AVIF (ISOBMFF): ftyp + avif/avis brand
                                                if b.len() >= 12
                                                    && &b[4..8] == b"ftyp"
                                                    && (&b[8..12] == b"avif"
                                                        || &b[8..12] == b"avis")
                                                {
                                                    return true;
                                                }
                                                false
                                            }

                                            if !looks_like_supported_image(
                                                &bytes,
                                            ) {
                                                let msg = format!(
                                                    "Response does not look like a supported image for path {} ({} bytes)",
                                                    full_url, byte_len
                                                );
                                                log::error!("{}", msg);

                                                crate::infra::image_log::log_fetch_failure_once(
                                                    image_query,
                                                    &full_url,
                                                );

                                                return MetadataMessage::UnifiedImageLoadFailed(request_for_fetch, msg);
                                            }

                                            if let Some(cache) =
                                                disk_cache.as_ref()
                                            {
                                                cache
                                                    .write_bytes(
                                                        &request_for_fetch,
                                                        &bytes,
                                                    )
                                                    .await;
                                            }

                                            let (handle, estimated_bytes) =
                                                crate::infra::cache::handle_from_encoded_bytes(
                                                    &request_for_fetch,
                                                    bytes,
                                                );
                                            MetadataMessage::UnifiedImageLoaded(
                                                request_for_fetch,
                                                handle,
                                                estimated_bytes,
                                            )
                                        }
                                        Ok(ImageFetchResult::Pending {
                                            retry_after,
                                        }) => {
                                            let delay = retry_after.unwrap_or(
                                                IMAGE_PENDING_RETRY_DELAY,
                                            );
                                            if let Some(image_service) =
                                                crate::infra::service_registry::get_image_service()
                                            {
                                                image_service
                                                    .mark_pending(&request_for_fetch);
                                            }

                                            let retry_request =
                                                request_for_fetch.clone();
                                            tokio::spawn(async move {
                                                tokio::time::sleep(delay).await;
                                                if let Some(
                                                    image_service,
                                                ) =
                                                    crate::infra::service_registry::get_image_service()
                                                {
                                                    image_service
                                                        .request_image(
                                                            retry_request,
                                                        );
                                                }
                                            });

                                            log::debug!(
                                                "Image pending (iid={}, size={:?}); retrying in {:?}",
                                                request_for_fetch.iid,
                                                request_for_fetch.size,
                                                delay
                                            );

                                            MetadataMessage::NoOp
                                        }
                                        Err(e) => {
                                            let msg = format!(
                                                "Image download failed with path: {}\n Error: {}",
                                                full_url, e
                                            );
                                            log::error!("{}", msg);

                                            crate::infra::image_log::log_fetch_failure_once(
                                                            image_query,
                                                            &full_url,
                                                        );
                                            MetadataMessage::UnifiedImageLoadFailed(
                                                request_for_fetch,
                                                msg,
                                            )
                                        }
                                    }
                                };

                                tokio::select! {
                                    _ = cancel_rx => {
                                        if let Some(image_service) = crate::infra::service_registry::get_image_service() {
                                            image_service.mark_cancelled(&request_for_cancel);
                                        }
                                        MetadataMessage::UnifiedImageCancelled(request_for_cancel)
                                    }
                                    msg = fetch => msg,
                                }
                            });

                            inflight.push(task);

                            // No artificial spawn gap - the concurrent limit already
                            // prevents overwhelming the server, and gaps add cumulative
                            // latency for large queues
                            last_spawn_time = Some(std::time::Instant::now());
                        }

                        // If any task finished, yield its message
                        if let Some(result) = inflight.next().await {
                            let msg = match result {
                                Ok(m) => m,
                                Err(e) => {
                                    log::error!(
                                        "Image fetch task join error: {}",
                                        e
                                    );
                                    MetadataMessage::NoOp
                                }
                            };

                            // Periodically adapt concurrency based on observed latency
                            // (runs roughly every 8 completions to avoid overhead)
                            static ADAPT_COUNTER: std::sync::atomic::AtomicU32 =
                                std::sync::atomic::AtomicU32::new(0);
                            if ADAPT_COUNTER.fetch_add(
                                1,
                                std::sync::atomic::Ordering::Relaxed,
                            ).is_multiple_of(8)
                                && let Some(svc) =
                                    crate::infra::service_registry::get_image_service()
                                {
                                    svc.adapt_concurrency();
                                }

                            return Some((
                                msg,
                                ImageLoaderState::Running {
                                    last_spawn_time,
                                    receiver,
                                    inflight,
                                },
                            ));
                        }

                        // No tasks: wait for wake-up
                        if receiver.is_none()
                            && let Ok(mut guard) = wake_receiver_arc.lock()
                            && let Some(res) = guard.take()
                        {
                            receiver = Some(res);
                        }
                        if let Some(ref mut res) = receiver {
                            let _ = res.recv().await;
                        }

                        Some((
                            MetadataMessage::NoOp,
                            ImageLoaderState::Running {
                                last_spawn_time,
                                receiver,
                                inflight,
                            },
                        ))
                    }
                }
            }
        },
    )
}
