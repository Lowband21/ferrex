//! Shared types for the virtual carousel module

use crate::domains::ui::views::grid::types::CardSize;
use uuid::Uuid;

/// Unique key for identifying carousels throughout the app.
/// Using a strongly-typed key avoids brittle string matching and enables
/// scoped state per carousel instance.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CarouselKey {
    AllMovies,
    AllSeries,
    ShowSeasons(Uuid),    // series_id
    SeasonEpisodes(Uuid), // season_id
    LibraryMovies(Uuid),  // library_id
    LibrarySeries(Uuid),  // library_id
    AuthUsers,
    Custom(&'static str),
}

/// Carousel paging and boundary behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WrapMode {
    /// Finite (clamped) carousel.
    Finite,
    /// Infinite wrap-around carousel. Indexing wraps with modulo arithmetic.
    Infinite,
}

/// Static configuration for a carousel instance. These can be derived from
/// presets (poster, episode, profile) or provided ad-hoc by callsites.
#[derive(Debug, Clone, Copy)]
pub struct CarouselConfig {
    /// Base item width when not deriving from a card size.
    /// If `card_size` is provided, this is ignored and the width is derived from it.
    pub item_width: f32,
    pub item_spacing: f32,
    pub overscan_items_before: usize,
    pub overscan_items_after: usize,
    pub wrap_mode: WrapMode,
    /// Optional card size to derive item width from. When set, the carousel
    /// will compute the effective item width based on this card size and the
    /// `include_animation_padding` flag.
    pub card_size: Option<CardSize>,
    /// Whether to include horizontal animation padding (e.g., for flip animations)
    /// when deriving width from `card_size`.
    pub include_animation_padding: bool,
}

impl CarouselConfig {
    /// Basic sane defaults suitable for standard poster cards.
    pub const fn poster_defaults() -> Self {
        Self {
            // Derive width from a standard poster card (Medium = 200x300)
            item_width: 0.0,
            item_spacing: 15.0,
            overscan_items_before: 2,
            overscan_items_after: 2,
            wrap_mode: WrapMode::Finite,
            card_size: Some(CardSize::Medium),
            include_animation_padding: true,
        }
    }

    /// Defaults for wide episode still cards.
    pub const fn episode_defaults() -> Self {
        Self {
            // Wide cards typically 400x225
            item_width: 0.0,
            item_spacing: 15.0,
            overscan_items_before: 2,
            overscan_items_after: 2,
            wrap_mode: WrapMode::Finite,
            card_size: Some(CardSize::Wide),
            include_animation_padding: true,
        }
    }

    /// Defaults for profile/avatar style cards (e.g., cast/users).
    pub const fn profile_defaults() -> Self {
        Self {
            // Small avatar-style cards
            item_width: 0.0,
            item_spacing: 20.0,
            overscan_items_before: 2,
            overscan_items_after: 2,
            wrap_mode: WrapMode::Finite,
            card_size: Some(CardSize::Small),
            include_animation_padding: true,
        }
    }
}
