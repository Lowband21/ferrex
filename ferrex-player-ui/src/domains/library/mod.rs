//! Compatibility shim for the extracted `ferrex-player-library` data domain.

pub mod messages;
pub mod update;
pub mod update_handlers;

#[cfg(feature = "demo")]
pub use ferrex_player_library::DemoControlsState;
pub use ferrex_player_library::{
    LibrariesLoadState, LibraryDomain, LibraryDomainState,
    LibraryExternalEvent, media_root_browser, repo_snapshot, repository,
    server, types,
};

impl LibraryExternalEvent for crate::common::messages::CrossDomainEvent {
    fn is_user_authenticated(&self) -> bool {
        matches!(self, Self::UserAuthenticated(_, _))
    }

    fn is_database_cleared(&self) -> bool {
        matches!(self, Self::DatabaseCleared)
    }

    fn is_clear_libraries(&self) -> bool {
        matches!(self, Self::ClearLibraries)
    }
}
