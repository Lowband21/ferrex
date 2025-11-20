use super::Message;
use crate::infrastructure::{adapters::ApiClientAdapter, services::api::ApiService};
use ferrex_core::api_routes::{utils, v1};
use ferrex_core::player_prelude::{ImageSize, ImageType};
use futures::stream;
use iced::Subscription;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

/// Creates a subscription for loading images from the server
pub fn image_loading(
    api_service: Arc<ApiClientAdapter>,
    server_url: String,
    receiver: Arc<Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<()>>>>,
    auth_token: Option<String>,
) -> Subscription<Message> {
    // Subscription data that includes both ID and context
    #[derive(Debug, Clone)]
    struct ImageLoaderSubscription {
        id: u64,
        api_service: Arc<ApiClientAdapter>,
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
        api_service,
        server_url,
        receiver,
        auth_token,
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
        image_loader_stream(
            Arc::clone(&sub.api_service),
            sub.server_url.clone(),
            Arc::clone(&sub.receiver),
            sub.auth_token.clone(),
        )
    })
}

// Image loader stream function
fn image_loader_stream(
    api_service: Arc<ApiClientAdapter>,
    server_url: String,
    wake_receiver_arc: Arc<Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<()>>>>,
    auth_token: Option<String>,
) -> impl futures::Stream<Item = Message> {
    enum ImageLoaderState {
        Running {
            last_request_time: Option<std::time::Instant>,
            receiver: Option<tokio::sync::mpsc::UnboundedReceiver<()>>,
        },
        Finished,
    }

    log::debug!("image_loader_stream: Creating stream");

    stream::unfold(
        ImageLoaderState::Running {
            last_request_time: None,
            receiver: None,
        },
        move |state| {
            let server_url = server_url.clone();
            let auth_token = auth_token.clone();
            let api_service = Arc::clone(&api_service);
            let wake_receiver_arc = Arc::clone(&wake_receiver_arc);
            async move {
                match state {
                    ImageLoaderState::Running {
                        last_request_time,
                        mut receiver,
                    } => {
                        // Try to pull a request from the queue first
                        let request =
                            match crate::infrastructure::service_registry::get_image_service() {
                                Some(image_service) => image_service.get().get_next_request(),
                                _ => None,
                            };

                        if let Some(request) = request {
                            // Mark the image as loading in the service
                            if let Some(image_service) =
                                crate::infrastructure::service_registry::get_image_service()
                            {
                                image_service.get().mark_loading(&request);
                            }

                            let mut buf = Uuid::encode_buffer();

                            let image_type = request.image_type;
                            let size = request.size;

                            // Download the image
                            let (media_type, id) = match &image_type {
                                ImageType::Movie => (
                                    "movie",
                                    request.media_id.hyphenated().encode_lower(&mut buf),
                                ),
                                ImageType::Series => (
                                    "series",
                                    request.media_id.hyphenated().encode_lower(&mut buf),
                                ),
                                ImageType::Season => (
                                    "season",
                                    request.media_id.hyphenated().encode_lower(&mut buf),
                                ),
                                ImageType::Episode => (
                                    "episode",
                                    request.media_id.hyphenated().encode_lower(&mut buf),
                                ),
                                ImageType::Person => (
                                    "person",
                                    request.media_id.hyphenated().encode_lower(&mut buf),
                                ),
                            };

                            let category = match size {
                                ImageSize::Poster => "poster",
                                ImageSize::Backdrop => "backdrop",
                                ImageSize::Thumbnail => "thumbnail",
                                ImageSize::Full => "poster", // Hero poster variant
                                ImageSize::Profile => "cast", // Person profile images
                            };

                            // Server uses /images/{type}/{id}/{category}/{index}
                            if server_url.is_empty() {
                                log::error!("Server URL is empty! Cannot download images.");
                                return Some((
                                    Message::UnifiedImageLoadFailed(
                                        request,
                                        "Server URL is empty".to_string(),
                                    ),
                                    ImageLoaderState::Running {
                                        last_request_time,
                                        receiver,
                                    },
                                ));
                            }

                            let size_param = match size {
                                ImageSize::Backdrop => "original",
                                ImageSize::Thumbnail => "w185",
                                ImageSize::Poster => "w300",
                                ImageSize::Full => "w500",
                                ImageSize::Profile => "w185",
                            };

                            let index_str = request.image_index.to_string();
                            let path = utils::replace_params(
                                v1::images::SERVE,
                                &[
                                    ("{type}", media_type),
                                    ("{id}", id),
                                    ("{category}", category),
                                    ("{index}", index_str.as_str()),
                                ],
                            );

                            let result = api_service
                                .as_ref()
                                .get_bytes(&path, Some(("size", size_param)));

                            let message = match result.await {
                                Ok(bytes) => {
                                    let handle = iced::widget::image::Handle::from_bytes(bytes);
                                    Message::UnifiedImageLoaded(request, handle)
                                }
                                Err(e) => {
                                    let msg = format!(
                                        "Image download failed with path: {}\n Error: {}",
                                        path, e
                                    );
                                    log::error!("{}", msg);
                                    Message::UnifiedImageLoadFailed(request, msg)
                                }
                            };

                            Some((
                                message,
                                ImageLoaderState::Running {
                                    last_request_time: Some(std::time::Instant::now()),
                                    receiver,
                                },
                            ))
                        } else {
                            // No work: wait for a wake-up signal to avoid busy loop
                            // Acquire the receiver once and keep it in state
                            if receiver.is_none()
                                && let Ok(mut guard) = wake_receiver_arc.lock()
                                && let Some(rx) = guard.take()
                            {
                                receiver = Some(rx);
                            }

                            if let Some(ref mut rx) = receiver {
                                let _ = rx.recv().await; // Wake on new work
                            } else {
                                // Fallback: avoid tight loop if no receiver available
                                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                            }

                            Some((
                                Message::NoOp,
                                ImageLoaderState::Running {
                                    last_request_time,
                                    receiver,
                                },
                            ))
                        }
                    }
                    ImageLoaderState::Finished => None,
                }
            }
        },
    )
}
