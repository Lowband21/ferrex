//! Trait surfaces that describe interactions with Ferrex data models.

pub mod details_like;
pub mod id;
pub mod media_like;
pub mod media_ops;
pub mod sub_like;

/// Frequently used trait combinators for UI and orchestration crates.
pub mod prelude {
    pub use super::details_like::{
        MediaDetails, SeasonDetailsLike, SeriesDetailsLike,
    };
    pub use super::id::MediaIDLike;
    pub use super::media_like::MediaLike;
    pub use super::media_ops::{Browsable, Details, MediaOps, Playable};
    pub use super::sub_like::{EpisodeLike, MovieLike, SeasonLike, SeriesLike};
    pub use ferrex_model::details::MediaDetailsOptionLike;
}
