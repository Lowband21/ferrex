//! Shared types for the virtual carousel module

use crate::domains::ui::views::grid::types::CardSize;
use uuid::Uuid;

/// Media owner namespace for detail rail carousel identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DetailCarouselOwnerKind {
    Movie,
    Series,
    Season,
    Episode,
}

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
    DetailCast {
        owner_kind: DetailCarouselOwnerKind,
        owner_id: Uuid,
    },
    DetailSeriesEpisodes(Uuid),  // series_id
    DetailEpisodeSiblings(Uuid), // season_id
    DetailRelated {
        owner_kind: DetailCarouselOwnerKind,
        owner_id: Uuid,
    },
    Custom(&'static str),
}

impl CarouselKey {
    pub fn is_detail_rail(&self) -> bool {
        matches!(
            self,
            Self::ShowSeasons(_)
                | Self::SeasonEpisodes(_)
                | Self::DetailCast { .. }
                | Self::DetailSeriesEpisodes(_)
                | Self::DetailEpisodeSiblings(_)
                | Self::DetailRelated { .. }
        )
    }
}

/// Carousel paging and boundary behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WrapMode {
    /// Finite (clamped) carousel.
    Finite,
    /// Infinite wrap-around carousel. Indexing wraps with modulo arithmetic.
    Infinite,
}

/// Overscan behavior for a carousel instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverscanPolicy {
    /// Keep the configured before/after item counts fixed.
    Fixed,
    /// Derive detail-rail overscan from the current viewport page size.
    DetailRailAdaptive,
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
    /// Policy used when recomputing overscan after viewport or metric changes.
    pub overscan_policy: OverscanPolicy,
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
            overscan_policy: OverscanPolicy::Fixed,
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
            overscan_policy: OverscanPolicy::Fixed,
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
            overscan_policy: OverscanPolicy::Fixed,
        }
    }

    /// Detail rail configuration using caller-supplied, layout-solved metrics.
    ///
    /// Unlike card-size presets, these dimensions are already resolved by the
    /// detail layout solver for the current scale and viewport.
    pub const fn detail_rail(item_width: f32, item_spacing: f32) -> Self {
        Self {
            item_width,
            item_spacing,
            overscan_items_before: 1,
            overscan_items_after: 2,
            wrap_mode: WrapMode::Finite,
            card_size: None,
            include_animation_padding: false,
            overscan_policy: OverscanPolicy::DetailRailAdaptive,
        }
    }

    pub fn effective_item_width(self, scale: f32) -> f32 {
        if let Some(card_size) = self.card_size {
            let (w, _h) = card_size.scaled_dimensions(scale);
            if self.include_animation_padding {
                let pad = crate::infra::constants::animation::calculate_horizontal_padding(w);
                w + 2.0 * pad
            } else {
                w
            }
        } else {
            self.item_width
        }
    }
}
