use super::Message;
use futures::stream;
use iced::Subscription;
use std::sync::{Arc, Mutex};

/// Creates a subscription for loading images from the server
pub fn image_loading(
    server_url: String,
    receiver: Arc<Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<()>>>>,
    auth_token: Option<String>,
) -> Subscription<Message> {
    // Subscription data that includes both ID and context
    #[derive(Debug, Clone)]
    struct ImageLoaderSubscription {
        id: u64,
        server_url: String,
        receiver: Arc<Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<()>>>>,
        auth_token: Option<String>,
    }

    impl std::hash::Hash for ImageLoaderSubscription {
        fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
            self.id.hash(state);
        }
    }

    let subscription = ImageLoaderSubscription {
        id: 1, // Static ID for singleton subscription
        server_url,
        receiver,
        auth_token,
    };

    Subscription::run_with(subscription, |sub| {
        image_loader_stream(
            sub.server_url.clone(),
            Arc::clone(&sub.receiver),
            sub.auth_token.clone(),
        )
    })
}

// Image loader stream function
fn image_loader_stream(
    server_url: String,
    receiver: Arc<Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<()>>>>,
    auth_token: Option<String>,
) -> impl futures::Stream<Item = Message> {
    enum ImageLoaderState {
        Running(Option<std::time::Instant>), // Track last request time
        Finished,
    }

    log::debug!("image_loader_stream: Creating stream");

    stream::unfold(ImageLoaderState::Running(None), move |state| {
        let server_url = server_url.clone();
        let receiver = Arc::clone(&receiver);
        let auth_token = auth_token.clone();
        async move {
            match state {
                ImageLoaderState::Running(last_request_time) => {
                    // Throttle to one request every 25ms
                    const THROTTLE_MS: u64 = 25;

                    // Check if we need to wait before processing the next request
                    if let Some(last_time) = last_request_time {
                        let elapsed = last_time.elapsed().as_millis() as u64;
                        if elapsed < THROTTLE_MS {
                            let wait_time = THROTTLE_MS - elapsed;
                            tokio::time::sleep(std::time::Duration::from_millis(wait_time)).await;
                        }
                    }
                    // Try to receive a wake-up signal
                    let wake_up = match receiver.try_lock() {
                        Ok(mut guard) => {
                            if let Some(ref mut rx) = *guard {
                                // Use try_recv to avoid blocking
                                match rx.try_recv() {
                                    Ok(_) => true, // Got wake-up signal
                                    Err(tokio::sync::mpsc::error::TryRecvError::Empty) => false,
                                    Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                                        log::info!("Image receiver channel closed");
                                        drop(guard);
                                        return Some((Message::NoOp, ImageLoaderState::Finished));
                                    }
                                }
                            } else {
                                log::debug!("No receiver available in mutex");
                                drop(guard);
                                return Some((Message::NoOp, ImageLoaderState::Finished));
                            }
                        }
                        Err(_) => {
                            log::debug!("Could not lock receiver mutex, will retry");
                            false
                        }
                    };

                    // Get the next request from the priority queue
                    // We check for work regardless of wake-up signal to process any queued items
                    let request = if let Some(image_service) =
                        crate::infrastructure::service_registry::get_image_service()
                    {
                        image_service.get().get_next_request()
                    } else {
                        None
                    };

                    if let Some(request) = request {
                        // Mark the image as loading in the service
                        if let Some(image_service) =
                            crate::infrastructure::service_registry::get_image_service()
                        {
                            image_service.get().mark_loading(&request);
                        }

                        // Download the image
                        let (media_type, id) = match &request.media_id {
                            ferrex_core::api_types::MediaId::Movie(id) => ("movie", id.as_str()),
                            ferrex_core::api_types::MediaId::Series(id) => ("series", id.as_str()),
                            ferrex_core::api_types::MediaId::Season(id) => ("season", id.as_str()),
                            ferrex_core::api_types::MediaId::Episode(id) => {
                                ("episode", id.as_str())
                            }
                            ferrex_core::api_types::MediaId::Person(id) => ("person", id.as_str()),
                        };

                        let category = match request.size {
                            crate::domains::metadata::image_types::ImageSize::Poster => "poster",
                            crate::domains::metadata::image_types::ImageSize::Backdrop => {
                                "backdrop"
                            }
                            crate::domains::metadata::image_types::ImageSize::Thumbnail => {
                                // Episodes use "still" images, not posters
                                match &request.media_id {
                                    ferrex_core::api_types::MediaId::Episode(_) => "still",
                                    _ => "poster",
                                }
                            }
                            crate::domains::metadata::image_types::ImageSize::Full => "poster", // Use poster for full size too
                            crate::domains::metadata::image_types::ImageSize::Profile => "profile", // Person profile images
                        };

                        // Debug logging for episode image requests
                        if matches!(
                            &request.media_id,
                            ferrex_core::api_types::MediaId::Episode(_)
                        ) {
                            log::debug!(
                                    "Episode image request: size={:?}, category={}, media_type={}, id={}",
                                    request.size, category, media_type, id
                                );
                        }

                        // Server uses /images/{type}/{id}/{category}/{index} pattern
                        // For now, always use index 0 (first image)
                        if server_url.is_empty() {
                            log::error!("Server URL is empty! Cannot download images.");
                            return Some((
                                Message::UnifiedImageLoadFailed(
                                    request,
                                    "Server URL is empty".to_string(),
                                ),
                                ImageLoaderState::Running(last_request_time),
                            ));
                        }

                        // Determine the appropriate size parameter based on the request
                        let size_param = match request.size {
                            crate::domains::metadata::image_types::ImageSize::Backdrop => {
                                "?size=original"
                            } // Original quality backdrop
                            crate::domains::metadata::image_types::ImageSize::Thumbnail => {
                                "?size=w185"
                            } // Small thumbnail
                            crate::domains::metadata::image_types::ImageSize::Poster => {
                                "?size=w500"
                            } // Medium poster
                            crate::domains::metadata::image_types::ImageSize::Full => {
                                "?size=original"
                            } // Original size
                            crate::domains::metadata::image_types::ImageSize::Profile => {
                                "?size=w185"
                            } // Profile size
                        };

                        let url = if server_url.ends_with('/') {
                            format!(
                                "{}images/{}/{}/{}/0{}",
                                server_url, media_type, id, category, size_param
                            )
                        } else {
                            format!(
                                "{}/images/{}/{}/{}/0{}",
                                server_url, media_type, id, category, size_param
                            )
                        };

                        // Create request with auth header if available
                        let client = reqwest::Client::new();
                        let mut request_builder = client.get(&url);

                        if let Some(ref token) = auth_token {
                            request_builder = request_builder.header("Authorization", token);
                        }

                        // Download with timeout
                        let download_result = tokio::time::timeout(
                            std::time::Duration::from_secs(10),
                            request_builder.send(),
                        )
                        .await;

                        let message = match download_result {
                            Ok(Ok(response)) if response.status().is_success() => {
                                match response.bytes().await {
                                    Ok(bytes) => {
                                        let handle =
                                            iced::widget::image::Handle::from_bytes(bytes.to_vec());
                                        Message::UnifiedImageLoaded(request, handle)
                                    }
                                    Err(e) => {
                                        log::error!("Failed to read image bytes: {}", e);
                                        Message::UnifiedImageLoadFailed(request, e.to_string())
                                    }
                                }
                            }
                            Ok(Ok(response)) => {
                                let error =
                                    format!("Server returned {}: {}", response.status(), url);
                                log::error!("{}", error);
                                Message::UnifiedImageLoadFailed(request, error)
                            }
                            Ok(Err(e)) => {
                                log::error!("Failed to fetch image: {}", e);
                                Message::UnifiedImageLoadFailed(request, e.to_string())
                            }
                            Err(_) => {
                                log::error!("Image download timed out: {}", url);
                                Message::UnifiedImageLoadFailed(
                                    request,
                                    "Download timeout".to_string(),
                                )
                            }
                        };

                        Some((
                            message,
                            ImageLoaderState::Running(Some(std::time::Instant::now())),
                        ))
                    } else {
                        // No request available, sleep briefly
                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                        Some((Message::NoOp, ImageLoaderState::Running(last_request_time)))
                    }
                }
                ImageLoaderState::Finished => {
                    // Subscription is finishing, receiver should already be returned
                    None
                }
            }
        }
    })
}
