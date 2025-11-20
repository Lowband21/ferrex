use crate::messages::DomainMessage;
use crate::state::State;
use iced::Subscription;

/// Composes all domain subscriptions into a single batch
pub fn subscription(state: &State) -> Subscription<DomainMessage> {
    Subscription::batch(vec![
        // Auth domain subscriptions
        crate::messages::auth::subscriptions::subscription(state),
        // Media/Player domain subscriptions
        crate::messages::media::subscriptions::subscription(state),
        // Library domain subscriptions
        crate::messages::library::subscriptions::subscription(state),
        // Metadata domain subscriptions
        crate::messages::metadata::subscriptions::subscription(state),
        // UI domain subscriptions
        crate::messages::ui::subscriptions::subscription(state),
    ])
}
