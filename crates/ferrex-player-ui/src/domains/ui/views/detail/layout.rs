use crate::{
    infra::{
        constants::layout::{calculations::ScaledLayout, grid},
        design_tokens::SizeProvider,
    },
    state::InterfaceMode,
};

const COMPACT_PORTRAIT_MAX_WIDTH: f32 = 720.0;
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
        }
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

    let composition = if input.interface_mode == DetailInterfaceMode::TenFoot {
        DetailComposition::TenFoot
    } else if viewport_width <= COMPACT_PORTRAIT_MAX_WIDTH
        && viewport_height >= viewport_width
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
        DetailComposition::BalancedDesktop => 1_180.0 * scale,
        DetailComposition::CinematicWide => 1_560.0 * scale,
        DetailComposition::TenFoot => 1_760.0 * scale,
    }
}

fn hero_art_layout(
    composition: DetailComposition,
    content_width: f32,
    available_height: f32,
    scale: f32,
    scaled_poster_width: f32,
    scaled_poster_height: f32,
) -> DetailArtLayout {
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
                    corner_radius: clamp_scaled(12.0, 8.0, 24.0, scale),
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
                poster_height * 1.08,
                300.0 * scale,
                500.0 * scale,
                available_height * 0.66,
                DetailArtAspect::Poster,
            ),
            DetailComposition::CinematicWide => (
                poster_height * 1.18,
                360.0 * scale,
                580.0 * scale,
                available_height * 0.74,
                DetailArtAspect::Poster,
            ),
            DetailComposition::TenFoot => (
                poster_height * 1.28,
                360.0 * scale,
                620.0 * scale,
                available_height * 0.70,
                DetailArtAspect::Poster,
            ),
        };

    let height =
        clamp_to_available(desired_height, min_height, max_height, cap_height);
    DetailArtLayout {
        width: height * POSTER_ASPECT,
        height,
        corner_radius: clamp_scaled(14.0, 8.0, 28.0, scale),
        aspect,
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
                _ => 44.0,
            },
            38.0,
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
        assert!((plan.content_width - 1_560.0).abs() < 0.01);
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
}
