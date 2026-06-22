//! Image sizing, ownership, query, and request DTOs.
//!
//! This module groups the server/client contract for poster, backdrop, episode,
//! and profile images together with typed sizes and fetch priorities.

/// Pixel dimensions returned for resolved image assets.
pub mod dimensions;
/// Provider fetch request/response helper DTOs.
pub mod fetch;
/// Stored image metadata and ownership records.
pub mod metadata;
/// Image query options used by API handlers.
pub mod query;
/// Client-facing image request DTOs.
pub mod request;
/// Typed poster/backdrop/profile/episode size enums.
pub mod sizes;

pub use dimensions::*;
pub use fetch::*;
pub use metadata::*;
pub use query::*;
pub use request::*;
pub use sizes::*;
