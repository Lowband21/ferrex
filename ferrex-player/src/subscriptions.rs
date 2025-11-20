//! Root-level subscription composition

use crate::common::messages::DomainMessage;
use crate::state_refactored::State;
use iced::Subscription;

/// Composes all domain subscriptions into a single batch
pub fn subscription(state: &State) -> Subscription<DomainMessage> {
    // Profile the entire subscription evaluation
    #[cfg(any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ))]
    profiling::scope!("Application::Subscription::Total");
    // Profile subscription composition
    #[cfg(any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ))]
    profiling::scope!("Subscription::Composition");

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

    Subscription::batch(subscriptions)
}
