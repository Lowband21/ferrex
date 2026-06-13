//! Root-level update composition for the Ferrex player app shell.

use iced::Task;

use crate::{common::messages::DomainMessage, state::State};

/// Route a root app message through the assembled player domain update graph.
pub fn update(
    state: &mut State,
    message: DomainMessage,
) -> Task<DomainMessage> {
    ferrex_player_ui::update::update(state, message)
}
