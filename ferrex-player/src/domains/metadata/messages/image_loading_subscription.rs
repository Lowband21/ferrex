use super::Message;
use crate::infra::services::api::ApiService;
use ferrex_core::api::routes::{utils, v1};
use ferrex_core::player_prelude::{ImageSize, ImageType};
use futures::FutureExt;
use futures::stream::FuturesUnordered;
use futures::{StreamExt, stream};
use iced::Subscription;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

/// Creates a subscription for loading images from the server
pub fn image_loading(
    api_service: Arc<dyn ApiService>,
    server_url: String,
    receiver: Arc<Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<()>>>>,
) -> Subscription<Message> {
    // Subscription data that includes both ID and context
    #[derive(Debug, Clone)]
    struct ImageLoaderSubscription {
        id: u64,
        api_service: Arc<dyn ApiService>,
        server_url: String,
        receiver: Arc<Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<()>>>>,
    }

    impl PartialEq for ImageLoaderSubscription {
        fn eq(&self, other: &Self) -> bool {
            self.id == other.id
        }
    }
    impl Eq for ImageLoaderSubscription {}

    impl std::hash::Hash for ImageLoaderSubscription {
        fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
            self.id.hash(state);
        }
    }

    let subscription = ImageLoaderSubscription {
        id: 1, // Static ID for singleton subscription
        api_service,
        server_url,
        receiver,
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
        )
    })
}

// (legacy single-flight stream removed in favor of concurrent version)

// New: concurrent image loader stream with adaptive pacing
fn image_loader_stream_concurrent(
    api_service: Arc<dyn ApiService>,
    server_url: String,
    wake_receiver_arc: Arc<
        Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<()>>>,
    >,
) -> impl futures::Stream<Item = Message> {
    enum ImageLoaderState {
        Running {
            last_spawn_time: Option<std::time::Instant>,
            receiver: Option<tokio::sync::mpsc::UnboundedReceiver<()>>,
            inflight: FuturesUnordered<tokio::task::JoinHandle<Message>>,
        },
        Finished,
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
                                .map(|svc| svc.get().max_concurrent())
                                .unwrap_or(8);

                        // Fill pool with new requests if available
                        loop {
                            if inflight.len() >= desired_concurrency {
                                break;
                            }

                            let request = match crate::infra::service_registry::get_image_service() {
                                Some(image_service) => image_service.get().get_next_request(),
                                _ => None,
                            };
                            let Some(request) = request else { break };

                            if let Some(image_service) = crate::infra::service_registry::get_image_service() {
                                image_service.get().mark_loading(&request);
                            }

                            let (cancel_tx, cancel_rx) =
                                tokio::sync::oneshot::channel::<()>();
                            if let Some(image_service) =
                                crate::infra::service_registry::get_image_service()
                            {
                                image_service
                                    .get()
                                    .register_inflight_cancel(&request, cancel_tx);
                            } else {
                                drop(cancel_tx);
                            }

                            let api = Arc::clone(&api_service);
                            let srv = server_url.clone();
                            let request_for_fetch = request.clone();
                            let request_for_cancel = request.clone();
                            let task = tokio::spawn(async move {
                                let fetch = async move {
                                    let mut buf = Uuid::encode_buffer();
                                    // Build media type/id mapping (including Person)
                                    let (media_type, id) =
                                        match &request_for_fetch.image_type {
                                            ImageType::Movie => (
                                                "movie",
                                                request_for_fetch
                                                    .media_id
                                                    .hyphenated()
                                                    .encode_lower(&mut buf),
                                            ),
                                            ImageType::Series => (
                                                "series",
                                                request_for_fetch
                                                    .media_id
                                                    .hyphenated()
                                                    .encode_lower(&mut buf),
                                            ),
                                            ImageType::Season => (
                                                "season",
                                                request_for_fetch
                                                    .media_id
                                                    .hyphenated()
                                                    .encode_lower(&mut buf),
                                            ),
                                            ImageType::Episode => (
                                                "episode",
                                                request_for_fetch
                                                    .media_id
                                                    .hyphenated()
                                                    .encode_lower(&mut buf),
                                            ),
                                            ImageType::Person => (
                                                "person",
                                                request_for_fetch
                                                    .media_id
                                                    .hyphenated()
                                                    .encode_lower(&mut buf),
                                            ),
                                        };

                                    let size = request_for_fetch.size;
                                    let category = match size {
                                        ImageSize::Poster => "poster",
                                        ImageSize::Backdrop => "backdrop",
                                        ImageSize::Thumbnail => "thumbnail",
                                        ImageSize::Full => "poster",
                                        ImageSize::Profile => "cast",
                                    };

                                    if srv.is_empty() {
                                        return Message::UnifiedImageLoadFailed(
                                            request_for_fetch,
                                            "Server URL is empty".to_string(),
                                        );
                                    }

                                    // Target on-screen widget dimensions so the server returns
                                    // the nearest TMDB bucket >= requested width, and we then
                                    // resize exactly to the widget size before uploading to the GPU.
                                    // This keeps textures aligned with UI widget dimensions.
                                    let (target_w, target_h): (u32, u32) = {
                                        match (
                                            request_for_fetch.image_type,
                                            size,
                                        ) {
                                            // Episode stills are 16:9 even when requested as Thumbnail
                                            (
                                                ImageType::Episode,
                                                ImageSize::Thumbnail,
                                            ) => (400, 225),
                                            // Default mappings
                                            (_, ImageSize::Thumbnail) => {
                                                (150, 225)
                                            } // small cards (2:3)
                                            (_, ImageSize::Poster) => {
                                                (185, 278)
                                            } // standard posters (2:3)
                                            (_, ImageSize::Full) => (300, 450), // hero/detail posters (2:3)
                                            (_, ImageSize::Backdrop) => {
                                                (400, 225)
                                            } // wide cards / stills (16:9)
                                            (_, ImageSize::Profile) => {
                                                (120, 180)
                                            } // cast/profile thumbs (2:3)
                                        }
                                    };

                                    let index_str = request_for_fetch
                                        .image_index
                                        .to_string();
                                    let path = utils::replace_params(
                                        v1::images::SERVE,
                                        &[
                                            ("{type}", media_type),
                                            ("{id}", id),
                                            ("{category}", category),
                                            ("{index}", index_str.as_str()),
                                        ],
                                    );

                                    // Ask server for the closest variant at or above target width.
                                    // Server maps `w` to recognized TMDB sizes and falls back.
                                    let width_param = target_w.to_string();
                                    let result = api.as_ref().get_bytes(
                                        &path,
                                        Some(("w", &width_param)),
                                    );
                                    match result.await {
                                        Ok(bytes) => {
                                            let byte_len = bytes.len();
                                            if byte_len == 0 {
                                                let msg = format!(
                                                    "Empty image body for path {}",
                                                    path
                                                );
                                                log::error!("{}", msg);
                                                let full_url = format!(
                                                    "{}{}?w={}",
                                                    srv, path, target_w
                                                );
                                                crate::infra::image_log::log_fetch_failure_once(
                                                    request_for_fetch.media_id,
                                                    category,
                                                    size,
                                                    target_w,
                                                    &full_url,
                                                    &msg,
                                                );
                                                return Message::UnifiedImageLoadFailed(
                                                    request_for_fetch,
                                                    msg,
                                                );
                                            }

                                            // Decode and resize to exact widget dimensions before creating the handle.
                                            // Treat any decode failure as fatal to avoid uploading corrupt/partial bytes.
                                            let decoded =
                                                match ::image::load_from_memory(
                                                    &bytes,
                                                ) {
                                                    Ok(img) => img,
                                                    Err(e) => {
                                                        let msg = format!(
                                                            "Image decode failed for path {}: {}",
                                                            path, e
                                                        );
                                                        log::error!("{}", msg);
                                                        let full_url = format!(
                                                            "{}{}?w={}",
                                                            srv, path, target_w
                                                        );
                                                        crate::infra::image_log::log_fetch_failure_once(
                                                        request_for_fetch.media_id,
                                                        category,
                                                        size,
                                                        target_w,
                                                        &full_url,
                                                        &format!("{}", e),
                                                    );
                                                        return Message::UnifiedImageLoadFailed(
                                                        request_for_fetch,
                                                        msg,
                                                    );
                                                    }
                                                };

                                            let resized = decoded.resize_exact(
                                                target_w,
                                                target_h,
                                                ::image::imageops::FilterType::Triangle,
                                            );
                                            let rgba = resized.to_rgba8();
                                            let raw = rgba.into_raw();
                                            let expected = (target_w as usize)
                                                * (target_h as usize)
                                                * 4usize;
                                            if raw.len() != expected {
                                                let msg = format!(
                                                    "Decoded size mismatch for {}: got {} bytes, expected {}",
                                                    path,
                                                    raw.len(),
                                                    expected
                                                );
                                                log::error!("{}", msg);
                                                let full_url = format!(
                                                    "{}{}?w={}",
                                                    srv, path, target_w
                                                );
                                                crate::infra::image_log::log_fetch_failure_once(
                                                    request_for_fetch.media_id,
                                                    category,
                                                    size,
                                                    target_w,
                                                    &full_url,
                                                    &msg,
                                                );
                                                return Message::UnifiedImageLoadFailed(
                                                    request_for_fetch,
                                                    msg,
                                                );
                                            }

                                            let handle =
                                                iced::widget::image::Handle::from_rgba(
                                                    target_w,
                                                    target_h,
                                                    raw,
                                                );

                                            // Temporary diagnostics: log one-time successful fetch
                                            let full_url = format!(
                                                "{}{}?w={}",
                                                srv, path, target_w
                                            );
                                            crate::infra::image_log::log_fetch_once(
                                                request_for_fetch.media_id,
                                                category,
                                                size,
                                                target_w,
                                                &full_url,
                                                byte_len,
                                            );
                                            Message::UnifiedImageLoaded(
                                                request_for_fetch,
                                                handle,
                                            )
                                        }
                                        Err(e) => {
                                            let msg = format!(
                                                "Image download failed with path: {}\n Error: {}",
                                                path, e
                                            );
                                            log::error!("{}", msg);
                                            // Temporary diagnostics: log one-time fetch failure
                                            let full_url = format!(
                                                "{}{}?w={}",
                                                srv, path, target_w
                                            );
                                            crate::infra::image_log::log_fetch_failure_once(
                                                request_for_fetch.media_id,
                                                category,
                                                size,
                                                target_w,
                                                &full_url,
                                                &format!("{}", e),
                                            );
                                            Message::UnifiedImageLoadFailed(
                                                request_for_fetch,
                                                msg,
                                            )
                                        }
                                    }
                                };

                                tokio::select! {
                                    _ = cancel_rx => {
                                        if let Some(image_service) = crate::infra::service_registry::get_image_service() {
                                            image_service.get().mark_cancelled(&request_for_cancel);
                                        }
                                        Message::UnifiedImageCancelled(request_for_cancel)
                                    }
                                    msg = fetch => msg,
                                }
                            });

                            inflight.push(task);

                            // Adaptive pacing: relax gap when queue is short (<=2)
                            const MIN_GAP_MS_DEFAULT: u64 = 12;
                            let queue_len = crate::infra::service_registry::get_image_service()
                                .map(|svc| svc.get().queue_len())
                                .unwrap_or(0);
                            let gap_ms = if queue_len <= 2 {
                                0
                            } else {
                                MIN_GAP_MS_DEFAULT
                            };
                            if gap_ms > 0 {
                                tokio::time::sleep(
                                    std::time::Duration::from_millis(gap_ms),
                                )
                                .await;
                            }
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
                                    Message::NoOp
                                }
                            };
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
                        if receiver.is_none() {
                            if let Ok(mut guard) = wake_receiver_arc.lock() {
                                if let Some(rx) = guard.take() {
                                    receiver = Some(rx);
                                }
                            }
                        }
                        if let Some(ref mut rx) = receiver {
                            let _ = rx.recv().await;
                        } else {
                            tokio::time::sleep(
                                std::time::Duration::from_millis(250),
                            )
                            .await;
                        }

                        Some((
                            Message::NoOp,
                            ImageLoaderState::Running {
                                last_spawn_time,
                                receiver,
                                inflight,
                            },
                        ))
                    }
                    ImageLoaderState::Finished => None,
                }
            }
        },
    )
}
