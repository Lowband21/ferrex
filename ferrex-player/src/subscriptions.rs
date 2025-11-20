//! Root-level subscription composition

use crate::common::messages::DomainMessage;
use crate::state_refactored::State;
use iced::Subscription;

/// Composes all domain subscriptions into a single batch
pub fn subscription(state: &State) -> Subscription<DomainMessage> {
    let mut subscriptions = vec![
        // Auth domain subscriptions
        crate::domains::auth::messages::subscriptions::subscription(state),
        // Media/Player domain subscriptions
        crate::domains::media::messages::subscriptions::subscription(state),
        // Library domain subscriptions
        crate::domains::library::messages::subscriptions::subscription(state),
        // Metadata domain subscriptions
        crate::domains::metadata::messages::subscriptions::subscription(state),
        // UI domain subscriptions
        crate::domains::ui::messages::subscriptions::subscription(state),
    ];
    
    // Add MediaStore notifier subscription - check for needed refreshes every 200ms
    // NOTE: We can't capture the notifier in the closure due to Iced limitations
    // Instead, we'll check the notifier state in the update function
    let notifier_subscription = iced::time::every(std::time::Duration::from_millis(200))
        .map(|_| {
            // Always emit a check message, let the update function decide if refresh is needed
            DomainMessage::Ui(crate::domains::ui::messages::Message::CheckMediaStoreRefresh)
        });
    subscriptions.push(notifier_subscription);
    
    Subscription::batch(subscriptions)
}