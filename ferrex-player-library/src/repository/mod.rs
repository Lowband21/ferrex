//! Compatibility re-exports for the extracted player media repository crate.

pub mod accessor {
    pub use ferrex_player_repository::accessor::*;
}

pub mod media_repo {
    pub use ferrex_player_repository::media_repo::*;
}

pub mod movie_batches {
    pub use ferrex_player_repository::movie_batches::*;
}

pub mod series_bundles {
    pub use ferrex_player_repository::series_bundles::*;
}

pub mod yoke_cache {
    pub use ferrex_player_repository::yoke_cache::*;
}

pub use ferrex_player_repository::*;
