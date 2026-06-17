use crate::{
    infra::{
        constants::layout::{calculations::ScaledLayout, grid},
        design_tokens::SizeProvider,
    },
    state::InterfaceMode,
};

const COMPACT_PORTRAIT_MAX_WIDTH: f32 = 720.0;
const TALL_PORTRAIT_MAX_WIDTH: f32 = 960.0;
const TALL_PORTRAIT_MIN_ASPECT: f32 = 1.25;
const COMPACT_LANDSCAPE_MAX_WIDTH: f32 = 960.0;
const COMPACT_LANDSCAPE_MAX_HEIGHT: f32 = 560.0;
const CINEMATIC_MIN_WIDTH: f32 = 1_440.0;
const CINEMATIC_MIN_ASPECT: f32 = 1.70;
const POSTER_ASPECT: f32 = 2.0 / 3.0;
const STILL_ASPECT: f32 = 16.0 / 9.0;

/// Interface family used by the pure detail layout solver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DetailInterfaceMode {
    Desktop,
    TenFoot,
}

impl From<InterfaceMode> for DetailInterfaceMode {
    fn from(mode: InterfaceMode) -> Self {
        if mode.is_tenfoot() {
            Self::TenFoot
        } else {
            Self::Desktop
        }
    }
}

/// Value object consumed by [`solve_detail_layout`].
///
/// Route code should create this with [`DetailLayoutInput::from_runtime`] so the
/// solver receives dimensions copied from `SizeProvider` and `ScaledLayout`
/// instead of hard-coded route-local constants.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DetailLayoutInput {
    pub viewport_width: f32,
    pub viewport_height: f32,
    pub scale: f32,
    pub header_height: f32,
    pub interface_mode: DetailInterfaceMode,
    pub scaled_poster_width: f32,
    pub scaled_poster_height: f32,
    pub scaled_poster_gap: f32,
    pub hero_art_aspect: DetailArtAspect,
}

impl DetailLayoutInput {
    pub fn from_runtime(
        viewport_width: f32,
        viewport_height: f32,
        header_height: f32,
        interface_mode: impl Into<DetailInterfaceMode>,
        size_provider: &SizeProvider,
        scaled_layout: &ScaledLayout,
    ) -> Self {
        Self {
            viewport_width,
            viewport_height,
            scale: size_provider.scale,
            header_height,
            interface_mode: interface_mode.into(),
            scaled_poster_width: scaled_layout.poster_width,
            scaled_poster_height: scaled_layout.poster_height,
            scaled_poster_gap: scaled_layout.poster_gap(),
            hero_art_aspect: DetailArtAspect::Poster,
        }
    }

    pub fn with_hero_art_aspect(mut self, aspect: DetailArtAspect) -> Self {
        self.hero_art_aspect = aspect;
        self
    }
}

/// High-level composition selected for the current viewport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DetailComposition {
    /// One-column phone/narrow layout: art, title, actions, and panels stack
    /// vertically with rail overflow handled by horizontal scrollables.
    CompactPortrait,
    /// Short or narrow landscape layout: art docks beside summary text while
    /// secondary panels remain single-column below the hero.
    CompactLandscape,
    /// Default desktop layout: poster and summary share a balanced hero with a
    /// two-column section grid beneath it.
    BalancedDesktop,
    /// Wide desktop/ultrawide layout: backdrop gets more vertical presence and
    /// sections expand to a three-column grid with wider relationship rails.
    CinematicWide,
    /// 10-foot layout: focus targets are larger, spacing is wider, and hero
    /// actions remain horizontal for remote-control traversal.
    TenFoot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DetailAxis {
    Horizontal,
    Vertical,
}

/// Fully-resolved dimensions for rendering a detail page.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DetailLayoutPlan {
    pub composition: DetailComposition,
    pub viewport_width: f32,
    pub viewport_height: f32,
    pub available_height: f32,
    pub scale: f32,
    pub content_width: f32,
    pub content_max_width: f32,
    pub page_padding_x: f32,
    pub page_padding_y: f32,
    pub hero_gap: f32,
    pub hero_art: DetailArtLayout,
    pub backdrop: DetailBackdropLayout,
    pub action_cluster: DetailActionClusterLayout,
    pub section_grid: DetailSectionGridLayout,
    pub rail: DetailRailLayout,
}

impl DetailLayoutPlan {
    /// Resolve foreground-stage geometry for the detail hero at the current
    /// vertical scroll offset.
    pub fn foreground_stage(
        &self,
        scroll_offset_y: f32,
    ) -> DetailForegroundLayout {
        foreground_stage_layout(self, scroll_offset_y)
    }

    /// Resolve shader-facing Theater Plate readability geometry for the detail
    /// hero at the current vertical scroll offset.
    pub fn theater_plate_layout(
        &self,
        scroll_offset_y: f32,
    ) -> DetailTheaterPlateLayout {
        theater_plate_layout(self, scroll_offset_y)
    }
}

/// Normalized viewport rectangle used by Theater Plate shader placement.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DetailTheaterPlateRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl DetailTheaterPlateRect {
    pub fn center(self) -> [f32; 2] {
        [self.x + self.width * 0.5, self.y + self.height * 0.5]
    }

    pub fn half_size(self) -> [f32; 2] {
        [self.width * 0.5, self.height * 0.5]
    }

    pub fn right(self) -> f32 {
        self.x + self.width
    }

    pub fn bottom(self) -> f32 {
        self.y + self.height
    }
}

/// Pixel-space rectangle for foreground-stage primitives.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DetailForegroundRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl DetailForegroundRect {
    pub fn center(self) -> [f32; 2] {
        [self.x + self.width * 0.5, self.y + self.height * 0.5]
    }

    pub fn half_size(self) -> [f32; 2] {
        [self.width * 0.5, self.height * 0.5]
    }

    pub fn right(self) -> f32 {
        self.x + self.width
    }

    pub fn bottom(self) -> f32 {
        self.y + self.height
    }

    pub fn normalized(
        self,
        viewport_width: f32,
        viewport_height: f32,
    ) -> DetailTheaterPlateRect {
        normalize_rect(self.into(), viewport_width, viewport_height)
    }
}

/// Foreground safe-area gutters around the stage in viewport pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DetailSafeGutters {
    pub left: f32,
    pub right: f32,
    pub top: f32,
    pub bottom: f32,
}

/// Bounded foreground stage for the hero, sections, and rail deck.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DetailForegroundStage {
    pub rect: DetailForegroundRect,
    pub stage_width: f32,
}

/// Readability lobe for copy and the larger Theater Plate focus mask.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DetailReadableCopyLobe {
    pub text_rect: DetailForegroundRect,
    pub plate_rect: DetailForegroundRect,
    pub max_width: f32,
}

/// Hero-art anchor resolved independently from the route's generic gaps.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DetailHeroArtAnchor {
    pub rect: DetailForegroundRect,
    pub anchor: [f32; 2],
}

/// Control shelf geometry for primary/secondary action rows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DetailControlShelf {
    pub rect: DetailForegroundRect,
    pub axis: DetailAxis,
    pub button_width: f32,
    pub button_height: f32,
    pub gap: f32,
}

/// Section-band geometry for the panels below the hero stage.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DetailSectionBandLayout {
    pub rect: DetailForegroundRect,
    pub columns: usize,
    pub column_width: f32,
    pub gap: f32,
    pub panel_min_height: f32,
}

/// Rail-deck spans for relationship rows inside the foreground stage.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DetailRailDeckLayout {
    pub rect: DetailForegroundRect,
    pub card_width: f32,
    pub card_height: f32,
    pub gap: f32,
    pub visible_rows: usize,
    pub visible_span: f32,
}

/// Surface intensity tokens consumed by foreground renderers and tests.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DetailSurfaceIntensityTokens {
    pub stage: f32,
    pub readable_copy_lobe: f32,
    pub hero_art: f32,
    pub control_shelf: f32,
    pub section_band: f32,
    pub rail_deck: f32,
}

/// Explicit Theater Plate foreground-stage primitives in viewport pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DetailForegroundLayout {
    pub viewport_width: f32,
    pub viewport_height: f32,
    pub stage: DetailForegroundStage,
    pub safe_gutters: DetailSafeGutters,
    pub readable_copy_lobe: DetailReadableCopyLobe,
    pub hero_art_anchor: DetailHeroArtAnchor,
    pub control_shelf: DetailControlShelf,
    pub section_bands: DetailSectionBandLayout,
    pub rail_deck: DetailRailDeckLayout,
    pub surface_intensity: DetailSurfaceIntensityTokens,
}

/// Theater Plate geometry and composition-strength controls derived from a
/// [`DetailLayoutPlan`]. Rectangles are normalized to the full viewport.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DetailTheaterPlateLayout {
    pub content_rect: DetailTheaterPlateRect,
    pub plate_rect: DetailTheaterPlateRect,
    pub scrim_rect: DetailTheaterPlateRect,
    pub hero_art_rect: DetailTheaterPlateRect,
    pub plate_opacity: f32,
    pub plate_radius_px: f32,
    pub plate_feather_px: f32,
    pub scrim_opacity: f32,
    pub top_feather_uv: f32,
    pub bottom_feather_uv: f32,
    pub side_falloff: f32,
    pub ambient_opacity_scale: f32,
    pub vignette_opacity: f32,
    pub grain_opacity_scale: f32,
    pub backdrop_opacity: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DetailArtLayout {
    pub width: f32,
    pub height: f32,
    pub corner_radius: f32,
    pub aspect: DetailArtAspect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DetailArtAspect {
    Poster,
    Still,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DetailBackdropLayout {
    pub height: f32,
    pub control_height: f32,
    pub scrim_opacity: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DetailActionClusterLayout {
    pub axis: DetailAxis,
    pub button_width: f32,
    pub button_height: f32,
    pub gap: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DetailSectionGridLayout {
    pub columns: usize,
    pub gap: f32,
    pub panel_min_height: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DetailRailLayout {
    pub card_width: f32,
    pub card_height: f32,
    pub gap: f32,
    pub visible_rows: usize,
}

/// Resolve a detail layout without reading application state or repositories.
///
/// The only inputs are viewport width/height, UI scale, header height,
/// interface mode, and pre-scaled design-token dimensions copied into
/// [`DetailLayoutInput`].
pub fn solve_detail_layout(input: DetailLayoutInput) -> DetailLayoutPlan {
    let viewport_width = input.viewport_width.max(1.0);
    let viewport_height = input.viewport_height.max(1.0);
    let scale = input.scale.clamp(0.5, 3.0);
    let header_height = input.header_height.max(0.0).min(viewport_height);
    let available_height = (viewport_height - header_height).max(1.0);
    let aspect = viewport_width / viewport_height.max(1.0);
    let is_portrait = viewport_height >= viewport_width;
    let is_tall_portrait =
        viewport_height / viewport_width.max(1.0) >= TALL_PORTRAIT_MIN_ASPECT;

    let composition = if input.interface_mode == DetailInterfaceMode::TenFoot {
        DetailComposition::TenFoot
    } else if (viewport_width <= COMPACT_PORTRAIT_MAX_WIDTH && is_portrait)
        || (viewport_width <= TALL_PORTRAIT_MAX_WIDTH && is_tall_portrait)
    {
        DetailComposition::CompactPortrait
    } else if viewport_width <= COMPACT_LANDSCAPE_MAX_WIDTH
        || available_height <= COMPACT_LANDSCAPE_MAX_HEIGHT
    {
        DetailComposition::CompactLandscape
    } else if viewport_width >= CINEMATIC_MIN_WIDTH
        && aspect >= CINEMATIC_MIN_ASPECT
    {
        DetailComposition::CinematicWide
    } else {
        DetailComposition::BalancedDesktop
    };

    let page_padding_x = page_padding_x(composition, scale, viewport_width);
    let page_padding_y = page_padding_y(composition, scale, available_height);
    let content_max_width = content_max_width(composition, scale);
    let content_width = (viewport_width - page_padding_x * 2.0)
        .max(1.0)
        .min(content_max_width);
    let hero_gap = clamp_scaled_value(
        input.scaled_poster_gap * 2.0,
        12.0,
        match composition {
            DetailComposition::TenFoot => 72.0,
            DetailComposition::CinematicWide => 56.0,
            _ => 44.0,
        },
        scale,
    );

    let hero_art = hero_art_layout(
        composition,
        content_width,
        available_height,
        scale,
        input.scaled_poster_width,
        input.scaled_poster_height,
        input.hero_art_aspect,
    );
    let backdrop = backdrop_layout(composition, available_height, scale);
    let action_cluster =
        action_cluster_layout(composition, content_width, scale);
    let section_grid = section_grid_layout(composition, content_width, scale);
    let rail = rail_layout(
        composition,
        content_width,
        available_height,
        scale,
        input.scaled_poster_gap,
    );

    DetailLayoutPlan {
        composition,
        viewport_width,
        viewport_height,
        available_height,
        scale,
        content_width,
        content_max_width,
        page_padding_x,
        page_padding_y,
        hero_gap,
        hero_art,
        backdrop,
        action_cluster,
        section_grid,
        rail,
    }
}

/// Convenience runtime entry point for views that already have the central
/// scaling objects in hand.
pub fn solve_detail_layout_from_runtime(
    viewport_width: f32,
    viewport_height: f32,
    header_height: f32,
    interface_mode: impl Into<DetailInterfaceMode>,
    size_provider: &SizeProvider,
    scaled_layout: &ScaledLayout,
) -> DetailLayoutPlan {
    solve_detail_layout(DetailLayoutInput::from_runtime(
        viewport_width,
        viewport_height,
        header_height,
        interface_mode,
        size_provider,
        scaled_layout,
    ))
}

fn page_padding_x(
    composition: DetailComposition,
    scale: f32,
    viewport_width: f32,
) -> f32 {
    let desired = match composition {
        DetailComposition::CompactPortrait => 16.0,
        DetailComposition::CompactLandscape => 24.0,
        DetailComposition::BalancedDesktop => 40.0,
        DetailComposition::CinematicWide => 56.0,
        DetailComposition::TenFoot => 72.0,
    };
    let max_for_viewport = (viewport_width * 0.10).max(12.0);
    clamp_scaled(desired, 12.0, max_for_viewport / scale.max(0.1), scale)
}

fn page_padding_y(
    composition: DetailComposition,
    scale: f32,
    available_height: f32,
) -> f32 {
    let desired = match composition {
        DetailComposition::CompactPortrait => 16.0,
        DetailComposition::CompactLandscape => 20.0,
        DetailComposition::BalancedDesktop => 32.0,
        DetailComposition::CinematicWide => 42.0,
        DetailComposition::TenFoot => 54.0,
    };
    let max_for_viewport = (available_height * 0.12).max(10.0);
    clamp_scaled(desired, 10.0, max_for_viewport / scale.max(0.1), scale)
}

fn content_max_width(composition: DetailComposition, scale: f32) -> f32 {
    match composition {
        DetailComposition::CompactPortrait
        | DetailComposition::CompactLandscape => f32::INFINITY,
        DetailComposition::BalancedDesktop => 1_220.0 * scale,
        // Fill common 16:9 screens instead of centering a web-app-width card,
        // while still bounding ultrawide stages to a readable theater shelf.
        DetailComposition::CinematicWide => 1_920.0 * scale,
        DetailComposition::TenFoot => 2_220.0 * scale,
    }
}

fn hero_art_layout(
    composition: DetailComposition,
    content_width: f32,
    available_height: f32,
    scale: f32,
    scaled_poster_width: f32,
    scaled_poster_height: f32,
    art_aspect: DetailArtAspect,
) -> DetailArtLayout {
    if art_aspect == DetailArtAspect::Still {
        return still_hero_art_layout(
            composition,
            content_width,
            available_height,
            scale,
        );
    }

    let poster_width = scaled_poster_width.max(1.0);
    let poster_height = scaled_poster_height.max(1.0);
    let (desired_height, min_height, max_height, cap_height, aspect) =
        match composition {
            DetailComposition::CompactPortrait => {
                let width = clamp_to_available(
                    poster_width * 0.95,
                    132.0 * scale,
                    240.0 * scale,
                    content_width * 0.72,
                );
                return DetailArtLayout {
                    width,
                    height: width / POSTER_ASPECT,
                    corner_radius: clamp_scaled(3.0, 0.0, 6.0, scale),
                    aspect: DetailArtAspect::Poster,
                };
            }
            DetailComposition::CompactLandscape => (
                poster_height * 0.82,
                180.0 * scale,
                310.0 * scale,
                available_height * 0.68,
                DetailArtAspect::Poster,
            ),
            DetailComposition::BalancedDesktop => (
                poster_height * 1.20,
                320.0 * scale,
                520.0 * scale,
                available_height * 0.68,
                DetailArtAspect::Poster,
            ),
            DetailComposition::CinematicWide => (
                poster_height * 1.45,
                420.0 * scale,
                620.0 * scale,
                available_height * 0.76,
                DetailArtAspect::Poster,
            ),
            DetailComposition::TenFoot => (
                poster_height * 1.45,
                400.0 * scale,
                640.0 * scale,
                available_height * 0.72,
                DetailArtAspect::Poster,
            ),
        };

    let height =
        clamp_to_available(desired_height, min_height, max_height, cap_height);
    DetailArtLayout {
        width: height * POSTER_ASPECT,
        height,
        corner_radius: clamp_scaled(3.0, 0.0, 6.0, scale),
        aspect,
    }
}

fn still_hero_art_layout(
    composition: DetailComposition,
    content_width: f32,
    available_height: f32,
    scale: f32,
) -> DetailArtLayout {
    let desired_width = match composition {
        DetailComposition::CompactPortrait => content_width * 0.92,
        DetailComposition::CompactLandscape => content_width * 0.44,
        DetailComposition::BalancedDesktop => content_width * 0.46,
        DetailComposition::CinematicWide => content_width * 0.52,
        DetailComposition::TenFoot => content_width * 0.48,
    };
    let max_width = match composition {
        DetailComposition::CompactPortrait => content_width * 0.96,
        DetailComposition::CompactLandscape => 560.0 * scale,
        DetailComposition::BalancedDesktop => 660.0 * scale,
        DetailComposition::CinematicWide => 840.0 * scale,
        DetailComposition::TenFoot => 940.0 * scale,
    };
    let cap_height = match composition {
        DetailComposition::CompactPortrait => available_height * 0.40,
        DetailComposition::CompactLandscape => available_height * 0.62,
        DetailComposition::BalancedDesktop => available_height * 0.58,
        DetailComposition::CinematicWide => available_height * 0.66,
        DetailComposition::TenFoot => available_height * 0.62,
    };
    let width = clamp_to_available(
        desired_width,
        220.0 * scale,
        max_width,
        cap_height * STILL_ASPECT,
    )
    .min(content_width);

    DetailArtLayout {
        width,
        height: width / STILL_ASPECT,
        corner_radius: clamp_scaled(3.0, 0.0, 6.0, scale),
        aspect: DetailArtAspect::Still,
    }
}

fn backdrop_layout(
    composition: DetailComposition,
    available_height: f32,
    scale: f32,
) -> DetailBackdropLayout {
    let desired = match composition {
        DetailComposition::CompactPortrait => 150.0,
        DetailComposition::CompactLandscape => 120.0,
        DetailComposition::BalancedDesktop => 260.0,
        DetailComposition::CinematicWide => 390.0,
        DetailComposition::TenFoot => 470.0,
    } * scale;
    let max_height = match composition {
        DetailComposition::CompactPortrait => available_height * 0.28,
        DetailComposition::CompactLandscape => available_height * 0.24,
        DetailComposition::BalancedDesktop => available_height * 0.36,
        DetailComposition::CinematicWide => available_height * 0.48,
        DetailComposition::TenFoot => available_height * 0.52,
    };
    let height =
        clamp_to_available(desired, 96.0 * scale, 520.0 * scale, max_height);
    let scrim_opacity = match composition {
        DetailComposition::TenFoot => 0.70,
        DetailComposition::CinematicWide => 0.58,
        DetailComposition::BalancedDesktop => 0.48,
        DetailComposition::CompactLandscape
        | DetailComposition::CompactPortrait => 0.62,
    };

    DetailBackdropLayout {
        height,
        control_height: clamp_scaled(32.0, 28.0, 56.0, scale),
        scrim_opacity,
    }
}

fn action_cluster_layout(
    composition: DetailComposition,
    content_width: f32,
    scale: f32,
) -> DetailActionClusterLayout {
    let axis = match composition {
        DetailComposition::CompactPortrait => DetailAxis::Vertical,
        _ => DetailAxis::Horizontal,
    };
    let desired_width = match composition {
        DetailComposition::TenFoot => 250.0,
        DetailComposition::CinematicWide => 220.0,
        _ => 188.0,
    } * scale;
    let max_width = match axis {
        DetailAxis::Vertical => content_width,
        DetailAxis::Horizontal => (content_width * 0.34).max(1.0),
    };

    DetailActionClusterLayout {
        axis,
        button_width: clamp_to_available(
            desired_width,
            132.0 * scale,
            280.0 * scale,
            max_width,
        ),
        button_height: clamp_scaled(
            match composition {
                DetailComposition::TenFoot => 66.0,
                _ => 54.0,
            },
            44.0,
            76.0,
            scale,
        ),
        gap: clamp_scaled(10.0, 8.0, 20.0, scale),
    }
}

fn section_grid_layout(
    composition: DetailComposition,
    content_width: f32,
    scale: f32,
) -> DetailSectionGridLayout {
    let columns = match composition {
        DetailComposition::CompactPortrait
        | DetailComposition::CompactLandscape => 1,
        DetailComposition::BalancedDesktop => 2,
        DetailComposition::CinematicWide | DetailComposition::TenFoot => 3,
    };
    let min_panel_width = match composition {
        DetailComposition::TenFoot => 360.0,
        _ => 280.0,
    } * scale;
    let gap = clamp_scaled(18.0, 12.0, 36.0, scale);
    let columns =
        columns.min(columns_that_fit(content_width, min_panel_width, gap));

    DetailSectionGridLayout {
        columns,
        gap,
        panel_min_height: clamp_scaled(150.0, 120.0, 260.0, scale),
    }
}

fn rail_layout(
    composition: DetailComposition,
    content_width: f32,
    available_height: f32,
    scale: f32,
    scaled_poster_gap: f32,
) -> DetailRailLayout {
    let desired_width = match composition {
        DetailComposition::CompactPortrait => 148.0,
        DetailComposition::CompactLandscape => 168.0,
        DetailComposition::BalancedDesktop => 190.0,
        DetailComposition::CinematicWide => 218.0,
        DetailComposition::TenFoot => 312.0,
    } * scale;
    let max_width = match composition {
        DetailComposition::CompactPortrait => content_width * 0.72,
        DetailComposition::TenFoot => content_width * 0.28,
        _ => content_width * 0.24,
    };
    let card_width = clamp_to_available(
        desired_width,
        118.0 * scale,
        330.0 * scale,
        max_width,
    );
    let card_height = match composition {
        DetailComposition::TenFoot => card_width * 0.50,
        _ => card_width / STILL_ASPECT + 64.0 * scale,
    };
    let gap = scaled_poster_gap.max(grid::EFFECTIVE_SPACING * scale * 0.75);
    let visible_rows = match composition {
        DetailComposition::TenFoot if available_height < 700.0 * scale => 1,
        DetailComposition::TenFoot => 2,
        _ => 1,
    };

    DetailRailLayout {
        card_width,
        card_height,
        gap,
        visible_rows,
    }
}

fn columns_that_fit(
    content_width: f32,
    min_panel_width: f32,
    gap: f32,
) -> usize {
    if content_width <= min_panel_width {
        return 1;
    }
    ((content_width + gap) / (min_panel_width + gap))
        .floor()
        .max(1.0) as usize
}

fn clamp_scaled(value: f32, min: f32, max: f32, scale: f32) -> f32 {
    clamp_scaled_value(value * scale, min, max, scale)
}

fn clamp_scaled_value(value: f32, min: f32, max: f32, scale: f32) -> f32 {
    let min = min * scale;
    let max = max * scale;
    value.clamp(min, max)
}

fn clamp_to_available(value: f32, min: f32, max: f32, available: f32) -> f32 {
    let upper = max.min(available.max(1.0));
    if upper < min {
        upper.max(1.0)
    } else {
        value.clamp(min, upper)
    }
}

fn theater_plate_layout(
    plan: &DetailLayoutPlan,
    scroll_offset_y: f32,
) -> DetailTheaterPlateLayout {
    let foreground = foreground_stage_layout(plan, scroll_offset_y);
    let viewport_width = foreground.viewport_width;
    let viewport_height = foreground.viewport_height;
    let header_height = (plan.viewport_height - plan.available_height)
        .clamp(0.0, plan.viewport_height);
    let scrim = scrim_rect_px(
        plan,
        header_height,
        foreground.stage.rect.into(),
        viewport_height,
    );
    let controls = theater_plate_controls(plan.composition);

    DetailTheaterPlateLayout {
        content_rect: foreground
            .stage
            .rect
            .normalized(viewport_width, viewport_height),
        plate_rect: foreground
            .readable_copy_lobe
            .plate_rect
            .normalized(viewport_width, viewport_height),
        scrim_rect: normalize_rect(scrim, viewport_width, viewport_height),
        hero_art_rect: foreground
            .hero_art_anchor
            .rect
            .normalized(viewport_width, viewport_height),
        plate_opacity: controls.plate_opacity,
        plate_radius_px: controls.plate_radius_px,
        plate_feather_px: controls.plate_feather_px,
        scrim_opacity: controls.scrim_opacity,
        top_feather_uv: controls.top_feather_uv,
        bottom_feather_uv: controls.bottom_feather_uv,
        side_falloff: controls.side_falloff,
        ambient_opacity_scale: controls.ambient_opacity_scale,
        vignette_opacity: controls.vignette_opacity,
        grain_opacity_scale: controls.grain_opacity_scale,
        backdrop_opacity: controls.backdrop_opacity,
    }
}

#[derive(Debug, Clone, Copy)]
struct RectPx {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

impl RectPx {
    fn right(self) -> f32 {
        self.x + self.width
    }

    fn bottom(self) -> f32 {
        self.y + self.height
    }
}

impl From<RectPx> for DetailForegroundRect {
    fn from(rect: RectPx) -> Self {
        Self {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: rect.height,
        }
    }
}

impl From<DetailForegroundRect> for RectPx {
    fn from(rect: DetailForegroundRect) -> Self {
        Self {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: rect.height,
        }
    }
}

fn foreground_stage_layout(
    plan: &DetailLayoutPlan,
    scroll_offset_y: f32,
) -> DetailForegroundLayout {
    let viewport_width = plan.viewport_width.max(1.0);
    let viewport_height = plan.viewport_height.max(1.0);
    let header_height = (plan.viewport_height - plan.available_height)
        .clamp(0.0, plan.viewport_height);
    let scroll_offset_y = scroll_offset_y.max(0.0);
    let hero_top = header_height + plan.page_padding_y - scroll_offset_y;
    let (stage_x, stage_width) = foreground_stage_horizontal_bounds(plan);
    let stage_rect = content_rect_px(plan, stage_x, hero_top, stage_width);
    let summary_rect = summary_rect_px(plan, stage_x, hero_top, stage_width);
    let copy_text_rect = readable_copy_text_rect_px(
        plan,
        summary_rect,
        stage_rect,
        readable_copy_width_cap(plan, stage_width),
    );
    let plate_rect = expanded_rect_px(
        copy_text_rect,
        plate_expansion_x(plan),
        plate_expansion_y(plan),
        viewport_width,
        viewport_height,
    );
    let hero_art_rect = hero_art_rect_px(plan, stage_x, hero_top, stage_width);
    let hero_art_anchor_rect: DetailForegroundRect = hero_art_rect.into();
    let control_shelf = control_shelf_layout(plan, copy_text_rect);
    let section_bands = section_band_layout(plan, stage_rect);
    let rail_deck = rail_deck_layout(plan, stage_rect, section_bands.rect);

    DetailForegroundLayout {
        viewport_width,
        viewport_height,
        stage: DetailForegroundStage {
            rect: stage_rect.into(),
            stage_width: stage_rect.width,
        },
        safe_gutters: safe_gutters_for_stage(
            plan,
            stage_rect,
            header_height,
            viewport_width,
        ),
        readable_copy_lobe: DetailReadableCopyLobe {
            text_rect: copy_text_rect.into(),
            plate_rect: plate_rect.into(),
            max_width: readable_copy_width_cap(plan, stage_width),
        },
        hero_art_anchor: DetailHeroArtAnchor {
            rect: hero_art_anchor_rect,
            anchor: hero_art_anchor_rect.center(),
        },
        control_shelf,
        section_bands,
        rail_deck,
        surface_intensity: surface_intensity_tokens(plan.composition),
    }
}

fn foreground_stage_horizontal_bounds(plan: &DetailLayoutPlan) -> (f32, f32) {
    let viewport_width = plan.viewport_width.max(1.0);
    match plan.composition {
        DetailComposition::TenFoot => {
            let stage_width = plan
                .content_width
                .min((viewport_width - plan.page_padding_x * 2.0).max(1.0))
                .max(1.0);
            (tenfoot_stage_left(plan), stage_width)
        }
        _ => {
            let body_width = plan.content_width.min(viewport_width).max(1.0);
            let body_left = ((viewport_width - body_width) * 0.5).max(0.0);
            let stage_x = body_left + plan.page_padding_x;
            let stage_width = (body_width - plan.page_padding_x * 2.0)
                .max(1.0)
                .min(viewport_width);
            (stage_x, stage_width)
        }
    }
}

fn safe_gutters_for_stage(
    plan: &DetailLayoutPlan,
    stage_rect: RectPx,
    header_height: f32,
    viewport_width: f32,
) -> DetailSafeGutters {
    DetailSafeGutters {
        left: stage_rect.x.max(0.0),
        right: (viewport_width - stage_rect.right()).max(0.0),
        top: (header_height + plan.page_padding_y)
            .clamp(0.0, plan.viewport_height.max(1.0)),
        bottom: plan.page_padding_y.max(0.0),
    }
}

fn readable_copy_width_cap(plan: &DetailLayoutPlan, stage_width: f32) -> f32 {
    let cap = match plan.composition {
        DetailComposition::CompactPortrait => stage_width,
        DetailComposition::CompactLandscape => {
            (stage_width * 0.72).min(640.0 * plan.scale)
        }
        DetailComposition::BalancedDesktop => {
            (stage_width * 0.58).min(760.0 * plan.scale)
        }
        DetailComposition::CinematicWide => {
            (stage_width * 0.48).min(840.0 * plan.scale)
        }
        DetailComposition::TenFoot => {
            (stage_width * 0.52).min(1_080.0 * plan.scale)
        }
    };

    cap.clamp(1.0, stage_width.max(1.0))
}

fn readable_copy_text_rect_px(
    plan: &DetailLayoutPlan,
    rect: RectPx,
    stage: RectPx,
    max_width: f32,
) -> RectPx {
    let stage_right = stage.right();
    let unclamped_x = match plan.composition {
        DetailComposition::CompactPortrait
            if rect.width.min(max_width) < stage.width =>
        {
            stage.x + ((stage.width - rect.width.min(max_width)) * 0.5).max(0.0)
        }
        _ => rect.x,
    };
    let x = unclamped_x
        .max(stage.x)
        .min((stage_right - 1.0).max(stage.x));
    let max_inside_stage = (stage_right - x).max(1.0);
    let width = rect.width.min(max_width).min(max_inside_stage).max(1.0);

    RectPx { width, x, ..rect }
}

fn control_shelf_layout(
    plan: &DetailLayoutPlan,
    copy_rect: RectPx,
) -> DetailControlShelf {
    let height = plan
        .action_cluster
        .button_height
        .min(copy_rect.height)
        .max(1.0);
    let y = (copy_rect.bottom() - height).max(copy_rect.y);
    let rect = RectPx {
        x: copy_rect.x,
        y,
        width: copy_rect.width,
        height,
    };

    DetailControlShelf {
        rect: rect.into(),
        axis: plan.action_cluster.axis,
        button_width: plan.action_cluster.button_width.min(copy_rect.width),
        button_height: height,
        gap: plan.action_cluster.gap,
    }
}

fn section_band_layout(
    plan: &DetailLayoutPlan,
    stage: RectPx,
) -> DetailSectionBandLayout {
    let columns = plan.section_grid.columns.max(1);
    let gap = plan.section_grid.gap.max(0.0);
    let total_gap = gap * columns.saturating_sub(1) as f32;
    let column_width = ((stage.width - total_gap) / columns as f32).max(1.0);
    let rect = RectPx {
        x: stage.x,
        y: stage.bottom() + gap,
        width: stage.width,
        height: plan.section_grid.panel_min_height.max(1.0),
    };

    DetailSectionBandLayout {
        rect: rect.into(),
        columns,
        column_width,
        gap,
        panel_min_height: plan.section_grid.panel_min_height,
    }
}

fn rail_deck_layout(
    plan: &DetailLayoutPlan,
    stage: RectPx,
    section_band: DetailForegroundRect,
) -> DetailRailDeckLayout {
    let gap = plan.rail.gap.max(0.0);
    let card_width = plan.rail.card_width.min(stage.width).max(1.0);
    let card_height = plan.rail.card_height.max(1.0);
    let cards_that_fit =
        ((stage.width + gap) / (card_width + gap)).floor().max(1.0);
    let visible_span = (cards_that_fit * card_width
        + (cards_that_fit - 1.0).max(0.0) * gap)
        .min(stage.width)
        .max(card_width.min(stage.width));
    let visible_rows = plan.rail.visible_rows.max(1);
    let height = visible_rows as f32 * card_height
        + visible_rows.saturating_sub(1) as f32 * gap;
    let rect = RectPx {
        x: stage.x,
        y: section_band.bottom() + plan.section_grid.gap.max(0.0),
        width: visible_span,
        height,
    };

    DetailRailDeckLayout {
        rect: rect.into(),
        card_width,
        card_height,
        gap,
        visible_rows,
        visible_span,
    }
}

fn surface_intensity_tokens(
    composition: DetailComposition,
) -> DetailSurfaceIntensityTokens {
    match composition {
        DetailComposition::CompactPortrait => DetailSurfaceIntensityTokens {
            stage: 0.72,
            readable_copy_lobe: 0.82,
            hero_art: 0.70,
            control_shelf: 0.78,
            section_band: 0.74,
            rail_deck: 0.72,
        },
        DetailComposition::CompactLandscape => DetailSurfaceIntensityTokens {
            stage: 0.66,
            readable_copy_lobe: 0.76,
            hero_art: 0.64,
            control_shelf: 0.72,
            section_band: 0.68,
            rail_deck: 0.66,
        },
        DetailComposition::BalancedDesktop => DetailSurfaceIntensityTokens {
            stage: 0.54,
            readable_copy_lobe: 0.62,
            hero_art: 0.52,
            control_shelf: 0.58,
            section_band: 0.56,
            rail_deck: 0.54,
        },
        DetailComposition::CinematicWide => DetailSurfaceIntensityTokens {
            stage: 0.44,
            readable_copy_lobe: 0.52,
            hero_art: 0.42,
            control_shelf: 0.48,
            section_band: 0.46,
            rail_deck: 0.44,
        },
        DetailComposition::TenFoot => DetailSurfaceIntensityTokens {
            stage: 0.78,
            readable_copy_lobe: 0.86,
            hero_art: 0.74,
            control_shelf: 0.82,
            section_band: 0.80,
            rail_deck: 0.78,
        },
    }
}

#[derive(Debug, Clone, Copy)]
struct TheaterPlateControls {
    plate_opacity: f32,
    plate_radius_px: f32,
    plate_feather_px: f32,
    scrim_opacity: f32,
    top_feather_uv: f32,
    bottom_feather_uv: f32,
    side_falloff: f32,
    ambient_opacity_scale: f32,
    vignette_opacity: f32,
    grain_opacity_scale: f32,
    backdrop_opacity: f32,
}

fn summary_rect_px(
    plan: &DetailLayoutPlan,
    content_x: f32,
    hero_top: f32,
    content_width: f32,
) -> RectPx {
    let (hero_width, hero_height) = match plan.composition {
        DetailComposition::TenFoot => tenfoot_hero_art_size(plan),
        _ => (plan.hero_art.width, plan.hero_art.height),
    };
    let padding = match plan.composition {
        DetailComposition::TenFoot => tenfoot_detail_hero_padding(plan),
        _ => shared_detail_hero_padding(plan),
    };
    let remaining_width =
        (content_width - padding * 2.0 - hero_width - plan.hero_gap)
            .max(content_width * 0.42)
            .max(1.0);
    match plan.composition {
        DetailComposition::CompactPortrait => {
            let top = hero_top + padding + hero_height + plan.hero_gap;
            let max_height = (plan.available_height * 0.58).max(1.0);
            RectPx {
                x: content_x + padding,
                y: top,
                width: (content_width - padding * 2.0).max(1.0),
                height: (hero_height * 0.76)
                    .clamp(220.0_f32.min(max_height), max_height),
            }
        }
        DetailComposition::CompactLandscape => RectPx {
            x: content_x + padding + hero_width + plan.hero_gap * 0.78,
            y: hero_top + padding + hero_height * 0.06,
            width: remaining_width,
            height: hero_height * 0.92,
        },
        DetailComposition::BalancedDesktop => RectPx {
            x: content_x + padding + hero_width + plan.hero_gap * 0.78,
            y: hero_top + padding + hero_height * 0.10,
            width: remaining_width,
            height: hero_height * 0.84,
        },
        DetailComposition::CinematicWide => RectPx {
            x: content_x + padding + hero_width + plan.hero_gap * 0.72,
            y: hero_top + padding + hero_height * 0.16,
            width: remaining_width,
            height: hero_height * 0.72,
        },
        DetailComposition::TenFoot => RectPx {
            x: content_x + padding + hero_width + plan.hero_gap * 0.70,
            y: hero_top + padding + hero_height * 0.08,
            width: remaining_width,
            height: hero_height * 0.88,
        },
    }
}

fn content_rect_px(
    plan: &DetailLayoutPlan,
    content_x: f32,
    hero_top: f32,
    content_width: f32,
) -> RectPx {
    let height = match plan.composition {
        DetailComposition::CompactPortrait => {
            let padding = shared_detail_hero_padding(plan);
            padding
                + plan.hero_art.height
                + plan.hero_gap
                + plan.hero_art.height * 0.76
        }
        DetailComposition::TenFoot => {
            let padding = tenfoot_detail_hero_padding(plan);
            plan.backdrop
                .height
                .max(plan.hero_art.height + padding * 2.0)
        }
        _ => plan.hero_art.height + shared_detail_hero_padding(plan) * 2.0,
    };

    RectPx {
        x: content_x,
        y: hero_top,
        width: content_width,
        height: height.max(1.0),
    }
}

fn hero_art_rect_px(
    plan: &DetailLayoutPlan,
    content_x: f32,
    hero_top: f32,
    content_width: f32,
) -> RectPx {
    let (x, y, width, height) = match plan.composition {
        DetailComposition::TenFoot => {
            let stage_left = tenfoot_stage_left(plan);
            let padding = tenfoot_detail_hero_padding(plan);
            let (width, height) = tenfoot_hero_art_size(plan);
            (stage_left + padding, hero_top + padding, width, height)
        }
        DetailComposition::CompactPortrait => {
            let padding = shared_detail_hero_padding(plan);
            let inner_width = (content_width - padding * 2.0).max(1.0);
            let centered_offset =
                ((inner_width - plan.hero_art.width) * 0.5).max(0.0);
            (
                content_x + padding + centered_offset,
                hero_top + padding,
                plan.hero_art.width,
                plan.hero_art.height,
            )
        }
        _ => {
            let padding = shared_detail_hero_padding(plan);
            (
                content_x + padding,
                hero_top + padding,
                plan.hero_art.width,
                plan.hero_art.height,
            )
        }
    };

    RectPx {
        x,
        y,
        width: width.max(1.0),
        height: height.max(1.0),
    }
}

fn tenfoot_hero_art_size(plan: &DetailLayoutPlan) -> (f32, f32) {
    if plan.hero_art.aspect != DetailArtAspect::Still {
        return (plan.hero_art.width, plan.hero_art.height);
    }

    let max_width = (plan.content_width * 0.42)
        .max(plan.hero_art.width)
        .max(1.0);
    let desired_height = (plan.hero_art.height * 0.70).max(1.0);
    let desired_width = desired_height * STILL_ASPECT;
    if desired_width > max_width {
        (max_width, max_width / STILL_ASPECT)
    } else {
        (desired_width, desired_height)
    }
}

fn shared_detail_hero_padding(plan: &DetailLayoutPlan) -> f32 {
    24.0 * plan.scale
}

fn tenfoot_detail_hero_padding(plan: &DetailLayoutPlan) -> f32 {
    (plan.page_padding_y * 0.55)
        .min(plan.available_height * 0.028)
        .clamp(16.0, 30.0)
}

fn tenfoot_stage_left(plan: &DetailLayoutPlan) -> f32 {
    let inner_width =
        (plan.viewport_width - plan.page_padding_x * 2.0).max(1.0);
    plan.page_padding_x + ((inner_width - plan.content_width) * 0.5).max(0.0)
}

fn scrim_rect_px(
    plan: &DetailLayoutPlan,
    header_height: f32,
    content: RectPx,
    viewport_height: f32,
) -> RectPx {
    let desired_bottom = match plan.composition {
        DetailComposition::CompactPortrait => {
            content.bottom() + plan.page_padding_y * 1.35
        }
        DetailComposition::CompactLandscape => {
            content.bottom() + plan.page_padding_y * 1.15
        }
        DetailComposition::BalancedDesktop => {
            (header_height + plan.backdrop.height + plan.page_padding_y)
                .max(content.bottom() + plan.page_padding_y * 0.50)
        }
        DetailComposition::CinematicWide => {
            (header_height + plan.backdrop.height + plan.page_padding_y * 0.50)
                .max(content.bottom() + plan.page_padding_y * 0.28)
        }
        DetailComposition::TenFoot => {
            (header_height + plan.backdrop.height + plan.page_padding_y)
                .max(content.bottom() + plan.page_padding_y * 0.70)
        }
    };

    let top = header_height.max(0.0);
    let bottom = desired_bottom.clamp(top + 1.0, viewport_height);
    RectPx {
        x: 0.0,
        y: top,
        width: plan.viewport_width.max(1.0),
        height: bottom - top,
    }
}

fn expanded_rect_px(
    rect: RectPx,
    expand_x: f32,
    expand_y: f32,
    viewport_width: f32,
    viewport_height: f32,
) -> RectPx {
    let viewport_width = viewport_width.max(1.0);
    let viewport_height = viewport_height.max(1.0);
    let x = (rect.x - expand_x).clamp(0.0, (viewport_width - 1.0).max(0.0));
    let y = (rect.y - expand_y).clamp(0.0, (viewport_height - 1.0).max(0.0));
    let right = (rect.right() + expand_x).clamp(x + 1.0, viewport_width);
    let bottom = (rect.bottom() + expand_y).clamp(y + 1.0, viewport_height);
    RectPx {
        x,
        y,
        width: right - x,
        height: bottom - y,
    }
}

fn normalize_rect(
    rect: RectPx,
    viewport_width: f32,
    viewport_height: f32,
) -> DetailTheaterPlateRect {
    let viewport_width = viewport_width.max(1.0);
    let viewport_height = viewport_height.max(1.0);
    DetailTheaterPlateRect {
        x: (rect.x / viewport_width).clamp(0.0, 1.0),
        y: (rect.y / viewport_height).clamp(0.0, 1.0),
        width: (rect.width / viewport_width).clamp(0.0, 1.0),
        height: (rect.height / viewport_height).clamp(0.0, 1.0),
    }
}

fn plate_expansion_x(plan: &DetailLayoutPlan) -> f32 {
    match plan.composition {
        DetailComposition::CompactPortrait => plan.page_padding_x * 1.35,
        DetailComposition::CompactLandscape => plan.page_padding_x * 1.25,
        DetailComposition::BalancedDesktop => plan.page_padding_x * 1.10,
        DetailComposition::CinematicWide => plan.page_padding_x * 0.88,
        DetailComposition::TenFoot => plan.page_padding_x * 1.55,
    }
    .max(plan.hero_gap * 0.32)
}

fn plate_expansion_y(plan: &DetailLayoutPlan) -> f32 {
    match plan.composition {
        DetailComposition::CompactPortrait => plan.page_padding_y * 2.15,
        DetailComposition::CompactLandscape => plan.page_padding_y * 1.80,
        DetailComposition::BalancedDesktop => plan.page_padding_y * 1.45,
        DetailComposition::CinematicWide => plan.page_padding_y * 1.10,
        DetailComposition::TenFoot => plan.page_padding_y * 2.10,
    }
    .max(plan.action_cluster.button_height * 0.38)
}

fn theater_plate_controls(
    composition: DetailComposition,
) -> TheaterPlateControls {
    match composition {
        DetailComposition::CompactPortrait => TheaterPlateControls {
            plate_opacity: 0.66,
            plate_radius_px: 56.0,
            plate_feather_px: 150.0,
            scrim_opacity: 0.76,
            top_feather_uv: 0.18,
            bottom_feather_uv: 0.48,
            side_falloff: 0.58,
            ambient_opacity_scale: 0.72,
            vignette_opacity: 0.66,
            grain_opacity_scale: 1.15,
            backdrop_opacity: 0.58,
        },
        DetailComposition::CompactLandscape => TheaterPlateControls {
            plate_opacity: 0.61,
            plate_radius_px: 52.0,
            plate_feather_px: 136.0,
            scrim_opacity: 0.70,
            top_feather_uv: 0.16,
            bottom_feather_uv: 0.42,
            side_falloff: 0.52,
            ambient_opacity_scale: 0.76,
            vignette_opacity: 0.60,
            grain_opacity_scale: 1.10,
            backdrop_opacity: 0.62,
        },
        DetailComposition::BalancedDesktop => TheaterPlateControls {
            plate_opacity: 0.48,
            plate_radius_px: 48.0,
            plate_feather_px: 116.0,
            scrim_opacity: 0.54,
            top_feather_uv: 0.12,
            bottom_feather_uv: 0.34,
            side_falloff: 0.38,
            ambient_opacity_scale: 0.90,
            vignette_opacity: 0.48,
            grain_opacity_scale: 1.0,
            backdrop_opacity: 0.80,
        },
        DetailComposition::CinematicWide => TheaterPlateControls {
            plate_opacity: 0.36,
            plate_radius_px: 44.0,
            plate_feather_px: 104.0,
            scrim_opacity: 0.44,
            top_feather_uv: 0.10,
            bottom_feather_uv: 0.28,
            side_falloff: 0.30,
            ambient_opacity_scale: 1.0,
            vignette_opacity: 0.42,
            grain_opacity_scale: 0.92,
            backdrop_opacity: 0.92,
        },
        DetailComposition::TenFoot => TheaterPlateControls {
            plate_opacity: 0.68,
            plate_radius_px: 88.0,
            plate_feather_px: 220.0,
            scrim_opacity: 0.78,
            top_feather_uv: 0.16,
            bottom_feather_uv: 0.48,
            side_falloff: 0.62,
            ambient_opacity_scale: 0.72,
            vignette_opacity: 0.70,
            grain_opacity_scale: 0.0,
            backdrop_opacity: 0.78,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::{
        constants::layout::{calculations::ScaledLayout, grid},
        design_tokens::{ScalingContext, SizeProvider},
    };

    fn input(
        width: f32,
        height: f32,
        scale: f32,
        header: f32,
        mode: DetailInterfaceMode,
    ) -> DetailLayoutInput {
        let sizes =
            SizeProvider::new(ScalingContext::new().with_user_scale(scale));
        let layout = ScaledLayout::new(sizes.scale, grid::EFFECTIVE_SPACING);
        DetailLayoutInput::from_runtime(
            width, height, header, mode, &sizes, &layout,
        )
    }

    #[test]
    fn detail_layout_selects_required_viewport_matrix() {
        let cases = [
            (
                input(390.0, 844.0, 1.0, 50.0, DetailInterfaceMode::Desktop),
                DetailComposition::CompactPortrait,
            ),
            (
                input(812.0, 375.0, 1.0, 50.0, DetailInterfaceMode::Desktop),
                DetailComposition::CompactLandscape,
            ),
            (
                input(1_280.0, 800.0, 1.0, 50.0, DetailInterfaceMode::Desktop),
                DetailComposition::BalancedDesktop,
            ),
            (
                input(
                    1_920.0,
                    1_080.0,
                    1.0,
                    50.0,
                    DetailInterfaceMode::Desktop,
                ),
                DetailComposition::CinematicWide,
            ),
            (
                input(1_280.0, 720.0, 1.25, 0.0, DetailInterfaceMode::TenFoot),
                DetailComposition::TenFoot,
            ),
        ];

        for (input, expected) in cases {
            let plan = solve_detail_layout(input);
            assert_eq!(plan.composition, expected);
            assert!(plan.content_width <= input.viewport_width);
            assert!(plan.hero_art.width <= plan.content_width);
            assert!(plan.backdrop.height <= plan.available_height);
        }
    }

    #[test]
    fn detail_layout_clamps_compact_portrait_art_to_content_width() {
        let plan = solve_detail_layout(input(
            320.0,
            568.0,
            2.0,
            50.0,
            DetailInterfaceMode::Desktop,
        ));

        assert_eq!(plan.composition, DetailComposition::CompactPortrait);
        assert!(plan.hero_art.width <= plan.content_width * 0.72 + 0.01);
        assert!(plan.hero_art.height <= plan.available_height);
        assert_eq!(plan.action_cluster.axis, DetailAxis::Vertical);
        assert!(plan.action_cluster.button_width <= plan.content_width);
    }

    #[test]
    fn detail_layout_clamps_cinematic_width_and_uses_three_columns() {
        let plan = solve_detail_layout(input(
            3_840.0,
            1_600.0,
            1.0,
            50.0,
            DetailInterfaceMode::Desktop,
        ));

        assert_eq!(plan.composition, DetailComposition::CinematicWide);
        assert!((plan.content_width - 1_920.0).abs() < 0.01);
        assert_eq!(plan.section_grid.columns, 3);
        assert!(plan.rail.card_width <= 330.0);
    }

    #[test]
    fn detail_layout_tenfoot_respects_header_height_and_focus_rows() {
        let short_scaled = solve_detail_layout(input(
            1_280.0,
            720.0,
            1.5,
            96.0,
            DetailInterfaceMode::TenFoot,
        ));
        let full_hd = solve_detail_layout(input(
            1_920.0,
            1_080.0,
            1.0,
            0.0,
            DetailInterfaceMode::TenFoot,
        ));

        assert_eq!(short_scaled.composition, DetailComposition::TenFoot);
        assert!((short_scaled.available_height - 624.0).abs() < 0.01);
        assert_eq!(short_scaled.rail.visible_rows, 1);
        assert_eq!(short_scaled.action_cluster.axis, DetailAxis::Horizontal);
        assert!(
            short_scaled.backdrop.height
                <= short_scaled.available_height * 0.52 + 0.01
        );
        assert_eq!(full_hd.rail.visible_rows, 2);
    }

    #[test]
    fn detail_layout_uses_responsive_still_hero_aspect() {
        let plan = solve_detail_layout(
            input(1_280.0, 720.0, 1.0, 50.0, DetailInterfaceMode::Desktop)
                .with_hero_art_aspect(DetailArtAspect::Still),
        );

        assert_eq!(plan.hero_art.aspect, DetailArtAspect::Still);
        assert!(
            (plan.hero_art.width / plan.hero_art.height - STILL_ASPECT).abs()
                < 0.01
        );
        assert!(plan.hero_art.width <= plan.content_width);
        assert!(plan.hero_art.height <= plan.available_height * 0.58 + 0.01);
    }

    #[test]
    fn detail_layout_viewport_matrix_keeps_art_actions_and_rails_bounded() {
        let matrix = [
            (480.0, 900.0, DetailComposition::CompactPortrait),
            (640.0, 480.0, DetailComposition::CompactLandscape),
            (800.0, 600.0, DetailComposition::CompactLandscape),
            (1_024.0, 768.0, DetailComposition::BalancedDesktop),
            (1_280.0, 720.0, DetailComposition::BalancedDesktop),
            (1_366.0, 768.0, DetailComposition::BalancedDesktop),
            (1_920.0, 1_080.0, DetailComposition::CinematicWide),
            (2_560.0, 1_440.0, DetailComposition::CinematicWide),
            (3_440.0, 1_440.0, DetailComposition::CinematicWide),
            (900.0, 1_600.0, DetailComposition::CompactPortrait),
        ];

        for (width, height, expected) in matrix {
            for aspect in [DetailArtAspect::Poster, DetailArtAspect::Still] {
                let plan = solve_detail_layout(
                    input(
                        width,
                        height,
                        1.0,
                        50.0,
                        DetailInterfaceMode::Desktop,
                    )
                    .with_hero_art_aspect(aspect),
                );

                assert_eq!(
                    plan.composition, expected,
                    "{width}x{height} should choose {expected:?}"
                );
                assert!(plan.content_width <= plan.content_max_width);
                assert!(plan.content_width <= plan.viewport_width);
                assert!(plan.page_padding_x * 2.0 < plan.viewport_width);
                assert!(plan.page_padding_y * 2.0 < plan.available_height);
                assert!(plan.hero_art.width <= plan.content_width + 0.01);
                assert!(plan.hero_art.height <= plan.available_height + 0.01);
                assert!(
                    plan.action_cluster.button_width <= plan.content_width,
                    "action button should stay inside content at {width}x{height}"
                );
                assert!(plan.action_cluster.button_height > 0.0);
                assert!(plan.section_grid.columns >= 1);
                assert!(plan.section_grid.columns <= 3);
                assert!(plan.rail.card_width <= plan.content_width + 0.01);
                assert!(plan.rail.card_height > 0.0);
                assert!(plan.backdrop.height <= plan.available_height + 0.01);
            }
        }
    }

    #[test]
    fn foreground_stage_primitives_cover_required_viewport_matrix() {
        let matrix = [
            (
                "compact",
                390.0,
                844.0,
                1.0,
                50.0,
                DetailInterfaceMode::Desktop,
                DetailArtAspect::Poster,
                DetailComposition::CompactPortrait,
            ),
            (
                "desktop",
                1_280.0,
                720.0,
                1.0,
                50.0,
                DetailInterfaceMode::Desktop,
                DetailArtAspect::Poster,
                DetailComposition::BalancedDesktop,
            ),
            (
                "ultrawide",
                3_440.0,
                1_440.0,
                1.0,
                50.0,
                DetailInterfaceMode::Desktop,
                DetailArtAspect::Still,
                DetailComposition::CinematicWide,
            ),
            (
                "tenfoot",
                1_920.0,
                1_080.0,
                1.0,
                0.0,
                DetailInterfaceMode::TenFoot,
                DetailArtAspect::Still,
                DetailComposition::TenFoot,
            ),
        ];

        for (name, width, height, scale, header, mode, aspect, expected) in
            matrix
        {
            let plan = solve_detail_layout(
                input(width, height, scale, header, mode)
                    .with_hero_art_aspect(aspect),
            );
            let foreground = plan.foreground_stage(0.0);
            let theater = plan.theater_plate_layout(0.0);

            assert_eq!(plan.composition, expected, "{name} composition");
            assert!(foreground.stage.stage_width > 0.0, "{name} stage");
            assert!(
                (foreground.stage.stage_width - foreground.stage.rect.width)
                    .abs()
                    < 0.01,
                "{name} stage width should be explicit"
            );
            assert!(
                foreground.safe_gutters.left
                    + foreground.stage.stage_width
                    + foreground.safe_gutters.right
                    <= width + 0.01,
                "{name} gutters should bound stage"
            );

            for rect in [
                foreground.stage.rect,
                foreground.readable_copy_lobe.text_rect,
                foreground.readable_copy_lobe.plate_rect,
                foreground.hero_art_anchor.rect,
                foreground.control_shelf.rect,
                foreground.section_bands.rect,
                foreground.rail_deck.rect,
            ] {
                assert!(rect.width > 0.0, "{name} rect {rect:?}");
                assert!(rect.height > 0.0, "{name} rect {rect:?}");
                assert!(rect.x >= -0.01, "{name} rect {rect:?}");
                assert!(rect.right() <= width + 0.01, "{name} rect {rect:?}");
            }

            assert!(
                foreground.readable_copy_lobe.text_rect.width
                    <= foreground.readable_copy_lobe.max_width + 0.01,
                "{name} copy width should honor cap"
            );
            assert!(
                foreground.readable_copy_lobe.text_rect.width
                    <= foreground.stage.stage_width + 0.01,
                "{name} copy lobe should stay inside stage"
            );
            assert!(
                foreground.readable_copy_lobe.plate_rect.x
                    <= foreground.readable_copy_lobe.text_rect.x + 0.01,
                "{name} plate should cover copy"
            );
            assert!(
                foreground.readable_copy_lobe.plate_rect.right() + 0.01
                    >= foreground.readable_copy_lobe.text_rect.right(),
                "{name} plate should cover copy"
            );
            assert!(
                foreground.rail_deck.visible_span
                    <= foreground.stage.stage_width + 0.01,
                "{name} rail span should fit the stage"
            );
            assert!(
                (foreground.rail_deck.rect.width
                    - foreground.rail_deck.visible_span)
                    .abs()
                    < 0.01,
                "{name} rail deck width should expose visible span"
            );

            let expected_copy_cap = match expected {
                DetailComposition::CompactPortrait => {
                    foreground.stage.stage_width
                }
                DetailComposition::CompactLandscape => 640.0 * plan.scale,
                DetailComposition::BalancedDesktop => 760.0 * plan.scale,
                DetailComposition::CinematicWide => 840.0 * plan.scale,
                DetailComposition::TenFoot => 1_080.0 * plan.scale,
            };
            assert!(
                foreground.readable_copy_lobe.max_width
                    <= expected_copy_cap + 0.01,
                "{name} copy cap"
            );

            for value in [
                foreground.surface_intensity.stage,
                foreground.surface_intensity.readable_copy_lobe,
                foreground.surface_intensity.hero_art,
                foreground.surface_intensity.control_shelf,
                foreground.surface_intensity.section_band,
                foreground.surface_intensity.rail_deck,
            ] {
                assert!((0.0..=1.0).contains(&value), "{name} intensity");
            }

            assert_eq!(
                theater.hero_art_rect,
                foreground.hero_art_anchor.rect.normalized(width, height),
                "{name} hero art should map to Theater Plate geometry"
            );
            assert_eq!(
                theater.plate_rect,
                foreground
                    .readable_copy_lobe
                    .plate_rect
                    .normalized(width, height),
                "{name} readable copy lobe should map to Theater Plate geometry"
            );
        }
    }

    #[test]
    fn foreground_stage_primitives_follow_scroll_offset_for_viewport_matrix() {
        let matrix = [
            (390.0, 844.0, 50.0, DetailInterfaceMode::Desktop),
            (1_280.0, 720.0, 50.0, DetailInterfaceMode::Desktop),
            (3_440.0, 1_440.0, 50.0, DetailInterfaceMode::Desktop),
            (1_920.0, 1_080.0, 0.0, DetailInterfaceMode::TenFoot),
        ];

        for (width, height, header, mode) in matrix {
            let plan =
                solve_detail_layout(input(width, height, 1.0, header, mode));
            let base = plan.foreground_stage(0.0);
            let scrolled = plan.foreground_stage(96.0);

            assert!(scrolled.stage.rect.y < base.stage.rect.y);
            assert!(
                (base.stage.rect.y - scrolled.stage.rect.y - 96.0).abs() < 0.01
            );
            assert!(
                scrolled.readable_copy_lobe.text_rect.y
                    < base.readable_copy_lobe.text_rect.y
            );
            assert!(
                scrolled.hero_art_anchor.rect.y < base.hero_art_anchor.rect.y
            );
            assert_eq!(base.safe_gutters.top, scrolled.safe_gutters.top);

            let theater_base = plan.theater_plate_layout(0.0);
            let theater_scrolled = plan.theater_plate_layout(96.0);
            assert!(
                theater_scrolled.content_rect.y <= theater_base.content_rect.y
            );
            assert!(
                theater_scrolled.hero_art_rect.y
                    <= theater_base.hero_art_rect.y
            );
        }
    }

    #[test]
    fn theater_plate_layout_derives_readability_rects_for_viewport_matrix() {
        let matrix = [
            (480.0, 900.0, DetailArtAspect::Poster),
            (640.0, 480.0, DetailArtAspect::Poster),
            (1_280.0, 720.0, DetailArtAspect::Poster),
            (1_920.0, 1_080.0, DetailArtAspect::Poster),
            (2_560.0, 1_440.0, DetailArtAspect::Still),
            (900.0, 1_600.0, DetailArtAspect::Still),
        ];

        for (width, height, aspect) in matrix {
            let plan = solve_detail_layout(
                input(width, height, 1.0, 50.0, DetailInterfaceMode::Desktop)
                    .with_hero_art_aspect(aspect),
            );
            let theater = plan.theater_plate_layout(0.0);

            for rect in [
                theater.content_rect,
                theater.plate_rect,
                theater.scrim_rect,
                theater.hero_art_rect,
            ] {
                assert!(rect.x >= 0.0 && rect.x <= 1.0, "rect {rect:?}");
                assert!(rect.y >= 0.0 && rect.y <= 1.0, "rect {rect:?}");
                assert!(rect.width > 0.0, "rect {rect:?}");
                assert!(rect.height > 0.0, "rect {rect:?}");
                assert!(rect.right() <= 1.0 + 0.001, "rect {rect:?}");
                assert!(rect.bottom() <= 1.0 + 0.001, "rect {rect:?}");
            }

            assert!(
                theater.plate_rect.right() >= theater.content_rect.x,
                "plate should overlap hero content at {width}x{height}"
            );
            assert!(
                theater.plate_rect.bottom() >= theater.content_rect.y,
                "plate should overlap hero content at {width}x{height}"
            );
            assert!(
                theater.scrim_rect.bottom() >= theater.plate_rect.y,
                "scrim should reach the readability lobe at {width}x{height}"
            );
        }
    }

    #[test]
    fn theater_plate_layout_maps_adaptive_hero_art_rects() {
        let compact_plan = solve_detail_layout(input(
            480.0,
            900.0,
            1.0,
            50.0,
            DetailInterfaceMode::Desktop,
        ));
        let compact = compact_plan.theater_plate_layout(0.0);
        let wide_plan = solve_detail_layout(input(
            1_920.0,
            1_080.0,
            1.0,
            50.0,
            DetailInterfaceMode::Desktop,
        ));
        let wide = wide_plan.theater_plate_layout(0.0);
        let tenfoot_plan = solve_detail_layout(input(
            1_920.0,
            1_080.0,
            1.0,
            0.0,
            DetailInterfaceMode::TenFoot,
        ));
        let tenfoot = tenfoot_plan.theater_plate_layout(0.0);
        let tenfoot_still_plan = solve_detail_layout(
            input(1_920.0, 1_080.0, 1.0, 0.0, DetailInterfaceMode::TenFoot)
                .with_hero_art_aspect(DetailArtAspect::Still),
        );
        let tenfoot_still = tenfoot_still_plan.theater_plate_layout(0.0);
        let still_plan = solve_detail_layout(
            input(1_280.0, 720.0, 1.0, 50.0, DetailInterfaceMode::Desktop)
                .with_hero_art_aspect(DetailArtAspect::Still),
        );
        let still = still_plan.theater_plate_layout(0.0);

        assert!(compact.hero_art_rect.x > 0.0);
        assert!(compact.hero_art_rect.width > wide.hero_art_rect.width);
        assert!(
            (compact.hero_art_rect.width * compact_plan.viewport_width
                - compact_plan.hero_art.width)
                .abs()
                < 0.01
        );
        assert!(
            (wide.hero_art_rect.width * wide_plan.viewport_width
                - wide_plan.hero_art.width)
                .abs()
                < 0.01
        );

        let expected_tenfoot_x = tenfoot_stage_left(&tenfoot_plan)
            + tenfoot_detail_hero_padding(&tenfoot_plan);
        assert!(
            (tenfoot.hero_art_rect.x * tenfoot_plan.viewport_width
                - expected_tenfoot_x)
                .abs()
                < 0.01
        );
        assert!(
            (tenfoot.hero_art_rect.width * tenfoot_plan.viewport_width
                - tenfoot_plan.hero_art.width)
                .abs()
                < 0.01
        );
        assert!(
            (tenfoot.hero_art_rect.height * tenfoot_plan.viewport_height
                - tenfoot_plan.hero_art.height)
                .abs()
                < 0.01
        );

        let (tenfoot_still_width, tenfoot_still_height) =
            tenfoot_hero_art_size(&tenfoot_still_plan);
        assert!(
            (tenfoot_still.hero_art_rect.width
                * tenfoot_still_plan.viewport_width
                - tenfoot_still_width)
                .abs()
                < 0.01
        );
        assert!(
            (tenfoot_still.hero_art_rect.height
                * tenfoot_still_plan.viewport_height
                - tenfoot_still_height)
                .abs()
                < 0.01
        );

        let still_aspect = (still.hero_art_rect.width
            * still_plan.viewport_width)
            / (still.hero_art_rect.height * still_plan.viewport_height);
        assert!((still_aspect - STILL_ASPECT).abs() < 0.01);

        let scrolled = still_plan.theater_plate_layout(120.0);
        assert!(scrolled.hero_art_rect.y < still.hero_art_rect.y);
    }

    #[test]
    fn theater_plate_layout_uses_stronger_compact_abstraction_than_wide() {
        let compact_portrait = solve_detail_layout(input(
            390.0,
            844.0,
            1.0,
            50.0,
            DetailInterfaceMode::Desktop,
        ))
        .theater_plate_layout(0.0);
        let compact_landscape = solve_detail_layout(input(
            812.0,
            375.0,
            1.0,
            50.0,
            DetailInterfaceMode::Desktop,
        ))
        .theater_plate_layout(0.0);
        let wide = solve_detail_layout(input(
            1_920.0,
            1_080.0,
            1.0,
            50.0,
            DetailInterfaceMode::Desktop,
        ))
        .theater_plate_layout(0.0);

        for compact in [compact_portrait, compact_landscape] {
            assert!(compact.plate_opacity > wide.plate_opacity);
            assert!(compact.scrim_opacity > wide.scrim_opacity);
            assert!(compact.side_falloff > wide.side_falloff);
            assert!(compact.backdrop_opacity < wide.backdrop_opacity);
            assert!(compact.ambient_opacity_scale < wide.ambient_opacity_scale);
        }
    }

    #[test]
    fn theater_plate_layout_tenfoot_uses_couch_distance_masks_and_static_grain()
    {
        let theater = solve_detail_layout(input(
            1_920.0,
            1_080.0,
            1.0,
            0.0,
            DetailInterfaceMode::TenFoot,
        ))
        .theater_plate_layout(0.0);

        assert!(theater.plate_opacity >= 0.68);
        assert!(theater.plate_radius_px >= 88.0);
        assert!(theater.plate_feather_px >= 220.0);
        assert!(theater.scrim_opacity >= 0.78);
        assert!(theater.side_falloff >= 0.62);
        assert_eq!(theater.grain_opacity_scale, 0.0);
        assert!(theater.backdrop_opacity <= 0.80);
    }

    #[test]
    fn theater_plate_layout_tenfoot_matrix_keeps_readability_regions_bounded() {
        let matrix = [
            (1_280.0, 720.0),
            (1_280.0, 800.0),
            (1_366.0, 768.0),
            (1_920.0, 1_080.0),
            (2_560.0, 1_440.0),
        ];

        for (width, height) in matrix {
            let plan = solve_detail_layout(input(
                width,
                height,
                1.0,
                0.0,
                DetailInterfaceMode::TenFoot,
            ));
            let theater = plan.theater_plate_layout(0.0);

            assert_eq!(plan.composition, DetailComposition::TenFoot);
            assert!(plan.content_width <= width);
            assert!(plan.hero_art.width + plan.hero_gap < plan.content_width);
            assert_eq!(theater.scrim_rect.y, 0.0);
            assert!(theater.content_rect.width > 0.70);
            assert!(
                theater.plate_rect.width > theater.content_rect.width * 0.35
            );
            assert!(theater.plate_rect.right() <= 1.0 + 0.001);
            assert!(theater.plate_rect.bottom() <= 1.0 + 0.001);
            assert!(theater.scrim_rect.bottom() <= 1.0 + 0.001);
        }
    }

    #[test]
    fn theater_plate_layout_scrolls_plate_with_content() {
        let plan = solve_detail_layout(input(
            1_280.0,
            720.0,
            1.0,
            50.0,
            DetailInterfaceMode::Desktop,
        ));
        let base = plan.theater_plate_layout(0.0);
        let scrolled = plan.theater_plate_layout(160.0);

        assert!(scrolled.content_rect.y < base.content_rect.y);
        assert!(scrolled.plate_rect.y < base.plate_rect.y);
        assert_eq!(base.scrim_rect.x, scrolled.scrim_rect.x);
        assert_eq!(base.scrim_rect.width, scrolled.scrim_rect.width);
    }
}
