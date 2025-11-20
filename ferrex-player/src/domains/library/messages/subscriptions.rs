use crate::common::messages::DomainMessage;
use crate::state_refactored::State;
use iced::Subscription;
use std::collections::HashSet;
use std::sync::Arc;

/// Creates all library-related subscriptions
pub fn subscription(state: &State) -> Subscription<DomainMessage> {
    let mut subscriptions = vec![];

    // Subscribe to scan progress for each active scan ID we know about
    if !state.server_url.is_empty() {
        let api = Arc::clone(&state.api_service);
        let mut scan_ids: HashSet<_> = state
            .domains
            .library
            .state
            .active_scans
            .keys()
            .copied()
            .collect();
        scan_ids.extend(
            state.domains.library.state.latest_progress.keys().copied(),
        );

        for scan_id in scan_ids {
            subscriptions.push(
                super::scan_subscription::scan_progress(
                    state.server_url.clone(),
                    Arc::clone(&api),
                    scan_id,
                )
                .map(DomainMessage::Library),
            );
        }
    }

    // Subscribe to media events SSE stream
    if !state.server_url.is_empty() {
        subscriptions.push(
            super::media_events_subscription::media_events(
                state.server_url.clone(),
                Arc::clone(&state.api_service),
            )
            .map(DomainMessage::Library),
        );
    }

    Subscription::batch(subscriptions)
}
