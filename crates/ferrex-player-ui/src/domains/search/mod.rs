//! Compatibility shim for the extracted `ferrex-player-search` data domain.

pub mod messages;
pub mod update;

pub use ferrex_player_search::{
    SearchDomain, SearchDomainEvent, SearchEvent, SearchExternalEvent,
    SearchMessage, SearchMode, SearchPresentation, SearchResponse,
    SearchService, SearchState, SearchStrategy, TenFootKeyboardAction,
    TenFootKeyboardDirection, TenFootKeyboardKey, TenFootKeyboardState,
    calibrator, error, keyboard, metrics, service, types,
};

impl SearchExternalEvent for crate::common::messages::CrossDomainEvent {
    fn selected_library_changed(&self) -> bool {
        matches!(self, Self::LibrarySelected(_) | Self::LibraryChanged(_))
    }

    fn selected_home(&self) -> bool {
        matches!(self, Self::LibrarySelectHome)
    }
}
