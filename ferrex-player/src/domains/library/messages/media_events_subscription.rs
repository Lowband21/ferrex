use crate::domains::library::messages::Message;
use crate::infrastructure::{
    adapters::ApiClientAdapter,
    api_types::{Media, MediaID},
    services::api::ApiService,
};
use ferrex_core::api_routes::v1;
use ferrex_core::{MediaEvent, MediaIDLike};
use futures::StreamExt;
use futures::stream::{self, BoxStream};
use iced::Subscription;
use tokio::sync::mpsc;
use uuid::Uuid;

use std::hash::{Hash, Hasher};
use std::sync::Arc;

#[derive(Debug, Clone)]
struct MediaEventsId {
    server_url: String,
    api: Arc<ApiClientAdapter>,
}

impl PartialEq for MediaEventsId {
    fn eq(&self, other: &Self) -> bool {
        self.server_url == other.server_url && Arc::ptr_eq(&self.api, &other.api)
    }
}

impl Eq for MediaEventsId {}

impl Hash for MediaEventsId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.server_url.hash(state);
        Arc::as_ptr(&self.api).hash(state);
    }
}

/// Creates a subscription to server-sent events for library media changes
pub fn media_events(
    server_url: String,
    api_service: Arc<ApiClientAdapter>,
) -> Subscription<Message> {
    Subscription::run_with(
        MediaEventsId {
            server_url: server_url.clone(),
            api: Arc::clone(&api_service),
        },
        build_media_subscription_stream,
    )
}

fn build_media_subscription_stream(id: &MediaEventsId) -> BoxStream<'static, Message> {
    let server_url = id.server_url.clone();
    let api = Arc::clone(&id.api);
    Box::pin(stream::unfold(
        MediaEventState::new(server_url.to_owned(), api),
        |mut state| async move {
            match state.next_event().await {
                Some(message) => Some((message, state)),
                None => None,
            }
        },
    ))
}

/// Internal event type for channel communication
#[derive(Debug)]
enum MediaSseEvent {
    Open,
    Message(eventsource_stream::Event),
    Error(String),
    Closed,
}

/// State machine for media events SSE subscription
struct MediaEventState {
    server_url: String,
    event_receiver: Option<mpsc::UnboundedReceiver<MediaSseEvent>>,
    task_handle: Option<tokio::task::JoinHandle<()>>,
    retry_count: u32,
    max_retries: u32,
    api_service: Arc<ApiClientAdapter>,
}

impl MediaEventState {
    fn new(server_url: String, api_service: Arc<ApiClientAdapter>) -> Self {
        Self {
            server_url,
            event_receiver: None,
            task_handle: None,
            retry_count: 0,
            max_retries: 10,
            api_service,
        }
    }

    async fn next_event(&mut self) -> Option<Message> {
        loop {
            // Create event source if needed
            if self.event_receiver.is_none() {
                self.create_event_source().await;
            }

            // Try to get next event from channel
            if let Some(receiver) = &mut self.event_receiver {
                match receiver.recv().await {
                    Some(MediaSseEvent::Open) => {
                        log::info!("Library media events SSE connection opened");
                        self.retry_count = 0;
                        // Continue to next event
                        continue;
                    }

                    Some(MediaSseEvent::Message(msg)) => {
                        if let Some(message) = self.handle_sse_message(msg) {
                            return Some(message);
                        }
                        // If no message, continue to next event
                        continue;
                    }

                    Some(MediaSseEvent::Error(e)) => {
                        log::error!("Library media events SSE error: {}", e);
                        if self.handle_connection_error() {
                            // Max retries exceeded, stop subscription
                            return None;
                        }
                        // Otherwise, continue to retry
                        continue;
                    }

                    Some(MediaSseEvent::Closed) | None => {
                        log::warn!("Library media events SSE stream ended");
                        // Clean up task handle
                        if let Some(handle) = self.task_handle.take() {
                            handle.abort();
                        }
                        if self.handle_connection_error() {
                            // Max retries exceeded, stop subscription
                            return None;
                        }
                        // Otherwise, continue to retry
                        continue;
                    }
                }
            } else {
                // Failed to create event source after all retries
                return None;
            }
        }
    }

    async fn create_event_source(&mut self) {
        // Add exponential backoff delay for retries
        if self.retry_count > 0 {
            let delay_secs = std::cmp::min(30, 2u64.pow(self.retry_count - 1));
            log::info!(
                "Retrying media events connection after {} seconds (attempt #{})",
                delay_secs,
                self.retry_count + 1
            );
            tokio::time::sleep(std::time::Duration::from_secs(delay_secs)).await;
        }

        let url = format!("{}{}", self.server_url, v1::events::MEDIA);
        log::info!("Creating media events SSE connection to: {}", url);

        // Create channel for communication
        let (tx, rx) = mpsc::unbounded_channel();
        self.event_receiver = Some(rx);

        let api = Arc::clone(&self.api_service);
        // Spawn task to handle EventSource
        let task_handle = tokio::spawn(async move {
            let client = reqwest::Client::new();
            let mut request = client.get(&url);
            if let Some(token) = api.get_token().await {
                request = request.bearer_auth(token.access_token);
            }

            match reqwest_eventsource::EventSource::new(request) {
                Ok(mut event_source) => {
                    while let Some(event) = event_source.next().await {
                        let sse_event = match event {
                            Ok(reqwest_eventsource::Event::Open) => MediaSseEvent::Open,
                            Ok(reqwest_eventsource::Event::Message(msg)) => {
                                MediaSseEvent::Message(msg)
                            }
                            Err(e) => MediaSseEvent::Error(e.to_string()),
                        };

                        if tx.send(sse_event).is_err() {
                            break;
                        }
                    }

                    let _ = tx.send(MediaSseEvent::Closed);
                }
                Err(err) => {
                    let _ = tx.send(MediaSseEvent::Error(err.to_string()));
                }
            }
        });

        self.task_handle = Some(task_handle);
    }

    fn handle_sse_message(&mut self, msg: eventsource_stream::Event) -> Option<Message> {
        // Skip keepalive messages silently
        if msg.data == "keepalive" || msg.data.is_empty() {
            log::debug!("Received media event keepalive");
            return None;
        }

        if msg.event == "media_event" {
            log::debug!("Received media event: {}", msg.data);

            match serde_json::from_str::<MediaEvent>(&msg.data) {
                Ok(event) => self.convert_media_event(event),
                Err(e) => {
                    log::error!("Failed to parse media event: {} - Data: {}", e, msg.data);
                    // Continue listening for valid messages
                    None
                }
            }
        } else {
            // Unknown event type, log and continue
            log::debug!(
                "Unknown media event type: {} with data: {}",
                msg.event,
                msg.data
            );
            None
        }
    }

    fn handle_connection_error(&mut self) -> bool {
        self.event_receiver = None;
        if let Some(handle) = self.task_handle.take() {
            handle.abort();
        }
        self.retry_count += 1;

        if self.retry_count > self.max_retries {
            log::error!("Max retries exceeded for media events connection");
            // Return true to indicate we should stop
            return true;
        }

        // Return false to indicate we should continue retrying
        false
    }

    fn convert_media_event(&self, event: MediaEvent) -> Option<Message> {
        match event {
            // These events indicate we should refresh our library data
            MediaEvent::MovieAdded { movie } => {
                log::info!("Movie added: {}", movie.title.as_str());
                Some(Message::MediaDiscovered(vec![Media::Movie(movie)]))
            }
            MediaEvent::SeriesAdded { series } => {
                log::info!("Series added: {}", series.title.as_str());
                Some(Message::MediaDiscovered(vec![Media::Series(series)]))
            }
            MediaEvent::SeasonAdded { season } => {
                let mut buf = Uuid::encode_buffer();
                log::info!(
                    "Season added: S{} for series {}",
                    season.season_number.value(),
                    season.series_id.as_str(&mut buf)
                );
                Some(Message::MediaDiscovered(vec![Media::Season(season)]))
            }
            MediaEvent::EpisodeAdded { episode } => {
                log::info!(
                    "Episode added: S{}E{}",
                    episode.season_number.value(),
                    episode.episode_number.value()
                );
                Some(Message::MediaDiscovered(vec![Media::Episode(episode)]))
            }

            // Updates require refreshing existing data
            MediaEvent::MovieUpdated { movie } => {
                log::info!("Movie updated: {}", movie.title.as_str());
                Some(Message::MediaUpdated(Media::Movie(movie)))
            }
            MediaEvent::SeriesUpdated { series } => {
                log::info!("Series updated: {}", series.title.as_str());
                Some(Message::MediaUpdated(Media::Series(series)))
            }
            MediaEvent::SeasonUpdated { season } => {
                log::info!("Season updated: S{}", season.season_number.value());
                Some(Message::MediaUpdated(Media::Season(season)))
            }
            MediaEvent::EpisodeUpdated { episode } => {
                log::info!(
                    "Episode updated: S{}E{}",
                    episode.season_number.value(),
                    episode.episode_number.value()
                );
                Some(Message::MediaUpdated(Media::Episode(episode)))
            }

            // Deletion events
            MediaEvent::MediaDeleted { id } => {
                log::info!("Media deleted: {:?}", id);
                Some(Message::MediaDeleted(id))
            }

            // Scan events are already handled by scan subscription
            MediaEvent::ScanStarted { scan_id, .. } => {
                log::debug!(
                    "Ignoring ScanStarted event {} - handled by scan subscription",
                    scan_id
                );
                None
            }
            MediaEvent::ScanCompleted { scan_id, .. } => {
                log::debug!(
                    "Ignoring ScanCompleted event {} - handled by scan subscription",
                    scan_id
                );
                None
            }
            MediaEvent::ScanProgress { scan_id, .. } => {
                log::debug!(
                    "Ignoring ScanProgress event {} - handled by scan subscription",
                    scan_id
                );
                None
            }
            MediaEvent::ScanFailed { scan_id, error, .. } => {
                log::error!("Scan {} failed: {}", scan_id, error);
                // Could emit a scan failed message if needed
                None
            }
        }
    }
}

impl Drop for MediaEventState {
    fn drop(&mut self) {
        // Clean up the spawned task when the state is dropped
        if let Some(handle) = self.task_handle.take() {
            handle.abort();
        }
    }
}

// Helper extension to convert Media to legacy MediaFile if needed
impl Message {
    /// Create a MediaDiscovered message from media references
    pub fn media_discovered(references: Vec<Media>) -> Self {
        Message::MediaDiscovered(references)
    }

    /// Create a MediaUpdated message from a media reference
    pub fn media_updated(reference: Media) -> Self {
        Message::MediaUpdated(reference)
    }

    /// Create a MediaDeleted message from a media ID
    pub fn media_deleted(id: MediaID) -> Self {
        Message::MediaDeleted(id)
    }
}
