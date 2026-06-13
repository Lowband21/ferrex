//! Root-level subscription composition

use crate::common::messages::DomainMessage;
use crate::state::State;
use iced::Subscription;

/// Composes all domain subscriptions into a single batch
pub fn subscription(state: &State) -> Subscription<DomainMessage> {
    #[cfg(any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ))]
    profiling::scope!("Application::Subscription::Total");

    let subscriptions = vec![
        // Auth domain subscriptions
        //crate::domains::auth::messages::subscriptions::subscription(state),
        // Player domain subscriptions
        crate::domains::player::messages::subscriptions::subscription(state),
        // Library domain subscriptions
        crate::domains::library::messages::subscriptions::subscription(state),
        // Metadata domain subscriptions
        crate::domains::metadata::messages::subscriptions::subscription(state),
        // Search domain subscriptions
        crate::domains::search::messages::subscriptions::subscription(state),
        // UI domain subscriptions
        crate::domains::ui::messages::subscriptions::subscription(state),
        // Global focus traversal
        crate::common::focus::subscription().map(DomainMessage::Focus),
    ];

    Subscription::batch(subscriptions)
}
