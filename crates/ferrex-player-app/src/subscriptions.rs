//! Root-level subscription composition for the Ferrex player app shell.

use iced::Subscription;

use crate::{common::messages::DomainMessage, state::State};

/// Compose all app/domain subscriptions into a single root subscription.
pub fn subscription(state: &State) -> Subscription<DomainMessage> {
    ferrex_player_ui::subscriptions::subscription(state)
}
