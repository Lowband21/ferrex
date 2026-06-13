//! Compatibility shim for the extracted `ferrex-player-media` data domain.

pub mod messages;
pub mod selectors;
pub mod update;

pub use ferrex_player_media::{
    MediaDomain, MediaDomainState, MediaExternalEvent,
};

impl MediaExternalEvent for crate::common::messages::CrossDomainEvent {
    fn clear_current_show_data(&self) -> bool {
        matches!(self, Self::ClearCurrentShowData)
    }
}
