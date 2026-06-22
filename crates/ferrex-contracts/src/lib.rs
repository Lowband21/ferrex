//! Trait surfaces that describe interactions with Ferrex data models.
//!
//! `ferrex-contracts` layers object-safe and archive-friendly traits over the
//! concrete structs in `ferrex-model`. Player and server code use these traits to
//! write generic UI, repository, and orchestration logic that can work with owned
//! model values and rkyv archived values without duplicating media-type pattern
//! matching.

/// Metadata detail traits shared by owned and archived detail records.
pub mod details_like;
/// Media identifier traits for UUID-backed movie, series, season, and episode ids.
pub mod id;
/// Traits that normalize media enum access across owned and archived references.
pub mod media_like;
/// Common media operation traits for playable and browsable references.
pub mod media_ops;
/// Type-specific movie, series, season, and episode traits.
pub mod sub_like;

/// Frequently used trait combinators for UI and orchestration crates.
pub mod prelude {
    pub use super::details_like::{SeasonDetailsLike, SeriesDetailsLike};
    pub use super::id::MediaIDLike;
    pub use super::media_like::MediaLike;
    pub use super::media_ops::{Browsable, Details, MediaOps, Playable};
    pub use super::sub_like::{EpisodeLike, MovieLike, SeasonLike, SeriesLike};
}
