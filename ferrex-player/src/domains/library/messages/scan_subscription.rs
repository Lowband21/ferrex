use super::Message;
use ferrex_core::ScanProgress;
use iced::Subscription;
use tokio::sync::mpsc;

/// Creates a subscription to monitor library scan progress via Server-Sent Events (SSE)
pub fn scan_progress(server_url: String, scan_id: String) -> Subscription<Message> {
    #[derive(Debug, Clone, Hash)]
    struct ScanProgressId(String, String);

    Subscription::run_with(
        ScanProgressId(server_url.clone(), scan_id.clone()),
        |ScanProgressId(server_url, scan_id)| {
            futures::stream::unfold(
                ScanState::new(server_url.to_string(), scan_id.to_string()),
                |mut state| async move {
                    match state.next_event().await {
                        Some(message) => Some((message, state)),
                        None => {
                            // Stream has ended, no more events
                            None
                        }
                    }
                },
            )
        },
    )
}

/// Internal event type for channel communication
#[derive(Debug)]
enum ScanEvent {
    Open,
    Message(eventsource_stream::Event),
    Error(String),
    Closed,
}

/// State machine for SSE scan progress subscription
struct ScanState {
    server_url: String,
    scan_id: String,
    event_receiver: Option<mpsc::UnboundedReceiver<ScanEvent>>,
    task_handle: Option<tokio::task::JoinHandle<()>>,
    retry_count: u32,
    max_retries: u32,
}

impl ScanState {
    fn new(server_url: String, scan_id: String) -> Self {
        Self {
            server_url,
            scan_id,
            event_receiver: None,
            task_handle: None,
            retry_count: 0,
            max_retries: 3,
        }
    }

    async fn next_event(&mut self) -> Option<Message> {
        loop {
            // Create event source if needed
            if self.event_receiver.is_none() {
                self.create_event_source();
            }

            // Try to get next event from channel
            if let Some(receiver) = &mut self.event_receiver {
                match receiver.recv().await {
                    Some(ScanEvent::Open) => {
                        log::info!("SSE connection opened for scan {}", self.scan_id);
                        self.retry_count = 0; // Reset retry count on successful connection

                        // Fetch initial progress
                        if let Some(message) = self.fetch_initial_progress().await {
                            return Some(message);
                        }
                        // Continue to next event if no initial progress
                        continue;
                    }

                    Some(ScanEvent::Message(msg)) => {
                        if let Some(message) = self.handle_sse_message(msg) {
                            return Some(message);
                        }
                        // Continue to next event if no message
                        continue;
                    }

                    Some(ScanEvent::Error(e)) => {
                        log::error!("SSE error for scan {}: {}", self.scan_id, e);
                        if let Some(message) = self.handle_connection_error().await {
                            return Some(message);
                        }
                        // Continue to retry
                        continue;
                    }

                    Some(ScanEvent::Closed) | None => {
                        log::warn!("SSE stream ended for scan {}", self.scan_id);
                        // Clean up task handle
                        if let Some(handle) = self.task_handle.take() {
                            handle.abort();
                        }
                        // Try HTTP fallback before giving up
                        return self.fetch_progress_http().await;
                    }
                }
            } else {
                // Failed to create event source
                return Some(Message::ScanCompleted(Err(
                    "Failed to establish connection".to_string(),
                )));
            }
        }
    }

    fn create_event_source(&mut self) {
        let url = format!("{}/scan/progress/{}/sse", self.server_url, self.scan_id);
        log::info!("Creating SSE connection to: {}", url);

        // Create channel for communication
        let (tx, rx) = mpsc::unbounded_channel();
        self.event_receiver = Some(rx);

        // Spawn task to handle EventSource
        let task_handle = tokio::spawn(async move {
            use futures::StreamExt;

            let mut event_source = reqwest_eventsource::EventSource::get(&url);

            while let Some(event) = event_source.next().await {
                let scan_event = match event {
                    Ok(reqwest_eventsource::Event::Open) => ScanEvent::Open,
                    Ok(reqwest_eventsource::Event::Message(msg)) => ScanEvent::Message(msg),
                    Err(e) => ScanEvent::Error(e.to_string()),
                };

                if tx.send(scan_event).is_err() {
                    // Receiver dropped, exit task
                    break;
                }
            }

            // Send closed event
            let _ = tx.send(ScanEvent::Closed);
        });

        self.task_handle = Some(task_handle);
    }

    fn handle_sse_message(&mut self, msg: eventsource_stream::Event) -> Option<Message> {
        // Skip keepalive messages silently
        if msg.data == "keepalive" || msg.data.is_empty() {
            log::debug!("Received SSE keepalive");
            // Return None to continue to next event
            return None;
        }

        if msg.event == "progress" {
            log::debug!("Received scan progress SSE event");

            match serde_json::from_str::<ScanProgress>(&msg.data) {
                Ok(progress) => {
                    log::info!(
                        "Scan progress: {}/{} files, status: {:?}",
                        progress.scanned_files,
                        progress.total_files,
                        progress.status
                    );

                    // Check if scan is complete
                    if matches!(progress.status, ferrex_core::ScanStatus::Completed) {
                        Some(Message::ScanCompleted(Ok(self.scan_id.clone())))
                    } else if matches!(progress.status, ferrex_core::ScanStatus::Failed) {
                        Some(Message::ScanCompleted(Err(progress
                            .errors
                            .first()
                            .cloned()
                            .unwrap_or_else(|| "Scan failed".to_string()))))
                    } else {
                        Some(Message::ScanProgressUpdate(progress))
                    }
                }
                Err(e) => {
                    log::error!("Failed to parse scan progress: {} - Data: {}", e, msg.data);
                    // Continue listening for valid messages
                    None
                }
            }
        } else if msg.event == "complete" {
            log::info!("Received scan complete event");
            Some(Message::ScanCompleted(Ok(self.scan_id.clone())))
        } else if msg.event == "error" {
            log::error!("Received scan error event: {}", msg.data);
            Some(Message::ScanCompleted(Err(msg.data)))
        } else {
            // Unknown event type, continue listening
            log::debug!("Unknown SSE event type: {}", msg.event);
            None
        }
    }

    async fn handle_connection_error(&mut self) -> Option<Message> {
        self.retry_count += 1;

        if self.retry_count > self.max_retries {
            log::error!("Max retries exceeded for scan {}", self.scan_id);
            return Some(Message::ScanCompleted(Err(
                "Connection lost after multiple retries".to_string(),
            )));
        }

        // Clean up current connection
        self.event_receiver = None;
        if let Some(handle) = self.task_handle.take() {
            handle.abort();
        }

        // Wait before retry
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        // Continue in the loop to retry
        None
    }

    async fn fetch_initial_progress(&mut self) -> Option<Message> {
        match self.fetch_scan_progress().await {
            Ok(progress) => {
                log::info!(
                    "Initial scan status: {:?}, files: {}/{}",
                    progress.status,
                    progress.scanned_files,
                    progress.total_files
                );
                Some(Message::ScanProgressUpdate(progress))
            }
            Err(e) => {
                log::warn!("Failed to fetch initial progress: {}", e);
                // Don't fail the subscription, just continue listening for SSE events
                None
            }
        }
    }

    async fn fetch_progress_http(&mut self) -> Option<Message> {
        match self.fetch_scan_progress().await {
            Ok(progress) => {
                // Check if scan is complete
                if matches!(progress.status, ferrex_core::ScanStatus::Completed) {
                    Some(Message::ScanCompleted(Ok(self.scan_id.clone())))
                } else if matches!(progress.status, ferrex_core::ScanStatus::Failed) {
                    Some(Message::ScanCompleted(Err(progress
                        .errors
                        .first()
                        .cloned()
                        .unwrap_or_else(|| "Scan failed".to_string()))))
                } else {
                    // Scan is still ongoing but SSE connection failed
                    // Return a message indicating the scan is still running
                    Some(Message::ScanProgressUpdate(progress))
                }
            }
            Err(e) => {
                // Scan might be complete or connection lost
                log::error!("HTTP fallback failed: {}", e);
                Some(Message::ScanCompleted(Err(format!(
                    "Connection lost: {}",
                    e
                ))))
            }
        }
    }

    async fn fetch_scan_progress(&self) -> Result<ScanProgress, String> {
        let response = reqwest::get(format!(
            "{}/scan/progress/{}",
            self.server_url, self.scan_id
        ))
        .await
        .map_err(|e| format!("Network error: {}", e))?;

        let json = response
            .json::<serde_json::Value>()
            .await
            .map_err(|e| format!("Parse error: {}", e))?;

        // Extract progress from response
        let progress_data = json
            .get("progress")
            .ok_or_else(|| "No progress field in response".to_string())?;

        serde_json::from_value::<ScanProgress>(progress_data.clone())
            .map_err(|e| format!("Failed to parse progress: {}", e))
    }
}

impl Drop for ScanState {
    fn drop(&mut self) {
        // Clean up the spawned task when the state is dropped
        if let Some(handle) = self.task_handle.take() {
            handle.abort();
        }
    }
}
