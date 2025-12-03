//! Display section update handlers

use ferrex_model::PosterSize;

use super::messages::DisplayMessage;
use super::state::{GridSize, ThemePreference};
use crate::common::messages::DomainUpdateResult;
use crate::state::State;

/// Main message handler for display section
pub fn handle_message(
    state: &mut State,
    message: DisplayMessage,
) -> DomainUpdateResult {
    match message {
        // Theme
        DisplayMessage::SetTheme(theme) => set_theme(state, theme),

        // Grid Layout
        DisplayMessage::SetGridSize(size) => set_grid_size(state, size),
        DisplayMessage::SetPosterTitlesOnHover(enabled) => {
            set_poster_titles_on_hover(state, enabled)
        }
        DisplayMessage::SetShowRecentlyWatched(enabled) => {
            set_show_recently_watched(state, enabled)
        }
        DisplayMessage::SetShowContinueWatching(enabled) => {
            set_show_continue_watching(state, enabled)
        }
        DisplayMessage::SetSidebarCollapsed(collapsed) => {
            set_sidebar_collapsed(state, collapsed)
        }

        // Poster
        DisplayMessage::SetPosterBaseWidth(width) => {
            set_poster_base_width(state, width)
        }
        DisplayMessage::SetPosterBaseHeight(height) => {
            set_poster_base_height(state, height)
        }
        DisplayMessage::SetPosterCornerRadius(radius) => {
            set_poster_corner_radius(state, radius)
        }
        DisplayMessage::SetPosterTextAreaHeight(height) => {
            set_poster_text_area_height(state, height)
        }

        // Spacing
        DisplayMessage::SetGridEffectiveSpacing(spacing) => {
            set_grid_effective_spacing(state, spacing)
        }
        DisplayMessage::SetGridRowSpacing(spacing) => {
            set_grid_row_spacing(state, spacing)
        }
        DisplayMessage::SetGridViewportPadding(padding) => {
            set_grid_viewport_padding(state, padding)
        }
        DisplayMessage::SetGridTopPadding(padding) => {
            set_grid_top_padding(state, padding)
        }
        DisplayMessage::SetGridBottomPadding(padding) => {
            set_grid_bottom_padding(state, padding)
        }

        // Animation
        DisplayMessage::SetAnimationHoverScale(scale) => {
            set_animation_hover_scale(state, scale)
        }
        DisplayMessage::SetAnimationDefaultDuration(ms) => {
            set_animation_default_duration(state, ms)
        }
        DisplayMessage::SetAnimationTextureFadeInitial(ms) => {
            set_animation_texture_fade_initial(state, ms)
        }
        DisplayMessage::SetAnimationTextureFade(ms) => {
            set_animation_texture_fade(state, ms)
        }

        // Poster Quality
        DisplayMessage::SetLibraryPosterQuality(quality) => {
            set_library_poster_quality(state, quality)
        }
        DisplayMessage::SetDetailPosterQuality(quality) => {
            set_detail_poster_quality(state, quality)
        }
    }
}

// Theme handlers
fn set_theme(state: &mut State, theme: ThemePreference) -> DomainUpdateResult {
    // TODO: Update theme in state and apply to UI
    let _ = (state, theme);
    DomainUpdateResult::none()
}

// Grid Layout handlers
fn set_grid_size(state: &mut State, size: GridSize) -> DomainUpdateResult {
    let _ = (state, size);
    DomainUpdateResult::none()
}

fn set_poster_titles_on_hover(
    state: &mut State,
    enabled: bool,
) -> DomainUpdateResult {
    let _ = (state, enabled);
    DomainUpdateResult::none()
}

fn set_show_recently_watched(
    state: &mut State,
    enabled: bool,
) -> DomainUpdateResult {
    let _ = (state, enabled);
    DomainUpdateResult::none()
}

fn set_show_continue_watching(
    state: &mut State,
    enabled: bool,
) -> DomainUpdateResult {
    let _ = (state, enabled);
    DomainUpdateResult::none()
}

fn set_sidebar_collapsed(
    state: &mut State,
    collapsed: bool,
) -> DomainUpdateResult {
    let _ = (state, collapsed);
    DomainUpdateResult::none()
}

// Poster handlers - accept String for UI-visible fields, parse and validate
fn set_poster_base_width(
    state: &mut State,
    value: String,
) -> DomainUpdateResult {
    if let Ok(width) = value.parse::<f32>() {
        if width >= 100.0 && width <= 500.0 {
            state.domains.settings.display.poster_base_width = width;
        }
    }
    DomainUpdateResult::none()
}

fn set_poster_base_height(
    state: &mut State,
    value: String,
) -> DomainUpdateResult {
    if let Ok(height) = value.parse::<f32>() {
        if height >= 150.0 && height <= 750.0 {
            state.domains.settings.display.poster_base_height = height;
        }
    }
    DomainUpdateResult::none()
}

fn set_poster_corner_radius(
    state: &mut State,
    value: String,
) -> DomainUpdateResult {
    if let Ok(radius) = value.parse::<f32>() {
        if radius >= 0.0 && radius <= 50.0 {
            state.domains.settings.display.poster_corner_radius = radius;
        }
    }
    DomainUpdateResult::none()
}

fn set_poster_text_area_height(
    state: &mut State,
    height: f32,
) -> DomainUpdateResult {
    let _ = (state, height);
    DomainUpdateResult::none()
}

// Spacing handlers - accept String for UI-visible fields
fn set_grid_effective_spacing(
    state: &mut State,
    value: String,
) -> DomainUpdateResult {
    if let Ok(spacing) = value.parse::<f32>() {
        if spacing >= 0.0 && spacing <= 100.0 {
            state.domains.settings.display.grid_effective_spacing = spacing;
        }
    }
    DomainUpdateResult::none()
}

fn set_grid_row_spacing(
    state: &mut State,
    value: String,
) -> DomainUpdateResult {
    if let Ok(spacing) = value.parse::<f32>() {
        if spacing >= 0.0 && spacing <= 200.0 {
            state.domains.settings.display.grid_row_spacing = spacing;
        }
    }
    DomainUpdateResult::none()
}

fn set_grid_viewport_padding(
    state: &mut State,
    padding: f32,
) -> DomainUpdateResult {
    let _ = (state, padding);
    DomainUpdateResult::none()
}

fn set_grid_top_padding(state: &mut State, padding: f32) -> DomainUpdateResult {
    let _ = (state, padding);
    DomainUpdateResult::none()
}

fn set_grid_bottom_padding(
    state: &mut State,
    padding: f32,
) -> DomainUpdateResult {
    let _ = (state, padding);
    DomainUpdateResult::none()
}

// Animation handlers - accept String for UI-visible fields
fn set_animation_hover_scale(
    state: &mut State,
    value: String,
) -> DomainUpdateResult {
    if let Ok(scale) = value.parse::<f32>() {
        if scale >= 1.0 && scale <= 1.5 {
            state.domains.settings.display.animation_hover_scale = scale;
        }
    }
    DomainUpdateResult::none()
}

fn set_animation_default_duration(
    state: &mut State,
    value: String,
) -> DomainUpdateResult {
    if let Ok(ms) = value.parse::<u64>() {
        if ms >= 100 && ms <= 2000 {
            state.domains.settings.display.animation_default_duration_ms = ms;
        }
    }
    DomainUpdateResult::none()
}

fn set_animation_texture_fade_initial(
    state: &mut State,
    ms: u64,
) -> DomainUpdateResult {
    let _ = (state, ms);
    DomainUpdateResult::none()
}

fn set_animation_texture_fade(
    state: &mut State,
    ms: u64,
) -> DomainUpdateResult {
    let _ = (state, ms);
    DomainUpdateResult::none()
}

// Poster Quality handlers
fn set_library_poster_quality(
    state: &mut State,
    quality: PosterSize,
) -> DomainUpdateResult {
    state.domains.settings.display.library_poster_quality = quality;
    DomainUpdateResult::none()
}

fn set_detail_poster_quality(
    state: &mut State,
    quality: PosterSize,
) -> DomainUpdateResult {
    state.domains.settings.display.detail_poster_quality = quality;
    DomainUpdateResult::none()
}
