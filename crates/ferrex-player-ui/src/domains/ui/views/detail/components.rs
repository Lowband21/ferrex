use crate::{
    common::ui_utils::icon_text_with_size,
    domains::ui::{
        messages::UiMessage,
        theme,
        views::virtual_carousel::{
            CarouselKey, VirtualCarouselMessage, VirtualCarouselState,
        },
        widgets::image_for::image_for,
    },
    infra::design_tokens::SizeProvider,
};

use super::{
    DetailAction, DetailActionRole, DetailArtAspect, DetailArtLayout,
    DetailArtwork, DetailBackdropControl, DetailCastMember, DetailCastSection,
    DetailColorIntent, DetailComposition, DetailEmptyState, DetailFact,
    DetailFactLayoutMode, DetailFactPanel, DetailLayoutPlan,
    DetailMetadataPill, DetailNotice, DetailOverviewSection, DetailPageModel,
    DetailRailItem, DetailRelationshipRail, DetailSection,
    DetailSurfaceIntensityTokens, DetailTechnicalItem, DetailTechnicalSection,
    DetailTextAlignment, DetailTextOverflow, DetailTextRole, DetailTextStyle,
    DetailTone,
};
use ferrex_core::player_prelude::Priority;
use ferrex_model::ImageSize;
use iced::{
    Alignment, Background, Border, Color, Element, Length, Shadow, Theme,
    Vector, alignment,
    widget::{
        Column, Row, Space, button, column, container, mouse_area, row,
        scrollable, text, text::Wrapping,
    },
};
use iced_aw::menu::{Item, Menu, MenuBar};
use lucide_icons::Icon;

/// Semantic foreground surface families for Theater Plate detail stages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DetailForegroundSurface {
    StageField,
    ProjectionShelf,
    ControlShelf,
    RailBand,
    CastBand,
    FactRibbon,
    MetadataRibbon,
    TechnicalRibbon,
    NoticeSlab,
    EmptyState,
}

/// Resolved style tokens for a semantic foreground surface.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DetailForegroundSurfaceTokens {
    pub surface: DetailForegroundSurface,
    pub tone: DetailTone,
    pub intensity: f32,
    pub background: Color,
    pub edge: Color,
    pub text: Color,
    pub border_width: f32,
    pub radius: f32,
    pub shadow_blur: f32,
    pub padding_scale: f32,
}

/// Adapter for detail rails that should use an already-registered TV carousel.
#[derive(Debug, Clone, Copy)]
pub struct DetailRegisteredRailAdapter<'a> {
    pub key: &'a CarouselKey,
    pub carousel_state: &'a VirtualCarouselState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DetailActionSurfaceMode {
    Pressable,
    Disabled,
    Menu,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DetailStageSectionRenderState {
    pub surface: DetailForegroundSurface,
    pub empty: bool,
    pub full_width: bool,
}

pub fn detail_action_surface_mode(
    action: &DetailAction,
) -> DetailActionSurfaceMode {
    if !action.menu_items.is_empty() {
        DetailActionSurfaceMode::Menu
    } else if action.on_press.is_none() {
        DetailActionSurfaceMode::Disabled
    } else {
        DetailActionSurfaceMode::Pressable
    }
}

pub fn detail_stage_section_render_state(
    section: &DetailSection,
) -> DetailStageSectionRenderState {
    let empty = match section {
        DetailSection::Overview(section) => section.body.trim().is_empty(),
        DetailSection::Facts(section) => section.facts.is_empty(),
        DetailSection::Cast(section) => section.members.is_empty(),
        DetailSection::Technical(section) => section.items.is_empty(),
        DetailSection::RelationshipRail(section) => section.items.is_empty(),
        DetailSection::Empty(_) => true,
        DetailSection::Notice(_) => false,
    };
    let surface = if empty {
        DetailForegroundSurface::EmptyState
    } else {
        match section {
            DetailSection::Overview(_) => {
                DetailForegroundSurface::ProjectionShelf
            }
            DetailSection::Facts(_) => DetailForegroundSurface::FactRibbon,
            DetailSection::Cast(_) => DetailForegroundSurface::CastBand,
            DetailSection::Technical(_) => {
                DetailForegroundSurface::TechnicalRibbon
            }
            DetailSection::RelationshipRail(_) => {
                DetailForegroundSurface::RailBand
            }
            DetailSection::Empty(_) => DetailForegroundSurface::EmptyState,
            DetailSection::Notice(_) => DetailForegroundSurface::NoticeSlab,
        }
    };
    let full_width = matches!(
        section,
        DetailSection::Cast(_)
            | DetailSection::RelationshipRail(_)
            | DetailSection::Empty(_)
            | DetailSection::Notice(_)
    );

    DetailStageSectionRenderState {
        surface,
        empty,
        full_width,
    }
}

pub fn detail_foreground_surface_tokens(
    plan: &DetailLayoutPlan,
    surface: DetailForegroundSurface,
    tone: DetailTone,
) -> DetailForegroundSurfaceTokens {
    let foreground = plan.foreground_stage(0.0);
    let intensity = surface_intensity(foreground.surface_intensity, surface)
        .clamp(0.0, 1.0);
    let accent = tone_accent_color(tone);
    let text = match tone {
        DetailTone::Muted => theme::MediaServerTheme::TEXT_SECONDARY,
        _ => theme::MediaServerTheme::TEXT_PRIMARY,
    };

    let (background, edge, border_width, radius, shadow_blur, padding_scale) =
        match surface {
            DetailForegroundSurface::StageField => (
                Color::from_rgba(
                    0.010,
                    0.010,
                    0.018,
                    0.035 + intensity * 0.065,
                ),
                Color::TRANSPARENT,
                0.0,
                0.0,
                0.0,
                1.00,
            ),
            DetailForegroundSurface::ProjectionShelf => (
                Color::from_rgba(0.018, 0.016, 0.030, 0.26 + intensity * 0.24),
                Color::from_rgba(accent.r, accent.g, accent.b, 0.08),
                0.0,
                1.0,
                16.0 + intensity * 16.0,
                1.05,
            ),
            DetailForegroundSurface::ControlShelf => (
                Color::from_rgba(0.034, 0.026, 0.046, 0.38 + intensity * 0.24),
                Color::from_rgba(accent.r, accent.g, accent.b, 0.18),
                0.0,
                0.0,
                12.0 + intensity * 12.0,
                0.82,
            ),
            DetailForegroundSurface::RailBand => (
                Color::from_rgba(0.014, 0.014, 0.022, 0.22 + intensity * 0.24),
                Color::from_rgba(1.0, 1.0, 1.0, 0.06),
                0.0,
                0.0,
                10.0 + intensity * 14.0,
                0.96,
            ),
            DetailForegroundSurface::CastBand => (
                Color::from_rgba(0.020, 0.024, 0.030, 0.28 + intensity * 0.24),
                Color::from_rgba(0.70, 0.82, 1.0, 0.08),
                0.0,
                0.0,
                8.0 + intensity * 12.0,
                0.92,
            ),
            DetailForegroundSurface::FactRibbon => (
                Color::from_rgba(0.050, 0.038, 0.064, 0.34 + intensity * 0.22),
                Color::from_rgba(accent.r, accent.g, accent.b, 0.14),
                0.0,
                2.0,
                7.0 + intensity * 9.0,
                0.78,
            ),
            DetailForegroundSurface::MetadataRibbon => (
                Color::from_rgba(1.0, 1.0, 1.0, 0.07 + intensity * 0.07),
                Color::TRANSPARENT,
                0.0,
                2.0,
                0.0,
                0.58,
            ),
            DetailForegroundSurface::TechnicalRibbon => (
                Color::from_rgba(0.030, 0.050, 0.070, 0.34 + intensity * 0.22),
                Color::from_rgba(0.36, 0.70, 1.0, 0.14),
                0.0,
                2.0,
                7.0 + intensity * 9.0,
                0.74,
            ),
            DetailForegroundSurface::NoticeSlab => (
                notice_background_color(tone, intensity),
                Color::from_rgba(accent.r, accent.g, accent.b, 0.34),
                1.0,
                1.0,
                10.0 + intensity * 12.0,
                0.90,
            ),
            DetailForegroundSurface::EmptyState => (
                Color::from_rgba(0.026, 0.026, 0.034, 0.28 + intensity * 0.18),
                Color::from_rgba(1.0, 1.0, 1.0, 0.06),
                0.0,
                0.0,
                5.0 + intensity * 7.0,
                1.00,
            ),
        };

    DetailForegroundSurfaceTokens {
        surface,
        tone,
        intensity,
        background,
        edge,
        text,
        border_width,
        radius,
        shadow_blur,
        padding_scale,
    }
}

pub fn view_detail_stage(
    model: &DetailPageModel,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Element<'static, UiMessage> {
    view_detail_stage_with_registered_rails(model, plan, sizes, &[])
}

pub fn view_detail_stage_with_registered_rails(
    model: &DetailPageModel,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
    registered_rails: &[DetailRegisteredRailAdapter<'_>],
) -> Element<'static, UiMessage> {
    let mut body = Column::new()
        .spacing(plan.section_grid.gap)
        .width(Length::Fill)
        .max_width(plan.content_width);

    if let Some(empty) = model.empty_state.as_ref().filter(|_| model.is_empty())
    {
        body = body.push(view_empty_stage(empty, plan, sizes));
    } else {
        body = body.push(view_stage_hero(model, plan, sizes));
        body = body.push(view_stage_sections(
            &model.sections,
            plan,
            sizes,
            registered_rails,
        ));
    }

    if !model.backdrop_controls.is_empty() {
        body = body.push(view_backdrop_controls(
            &model.backdrop_controls,
            plan,
            sizes,
        ));
    }

    container(body)
        .padding([plan.page_padding_y, plan.page_padding_x])
        .width(Length::Fill)
        .align_x(iced::alignment::Horizontal::Center)
        .style(foreground_surface_style(detail_foreground_surface_tokens(
            plan,
            DetailForegroundSurface::StageField,
            DetailTone::Neutral,
        )))
        .into()
}

pub fn view_stage_hero(
    model: &DetailPageModel,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Element<'static, UiMessage> {
    let art = view_hero_art(&model.hero_art, plan, sizes);
    let summary = view_stage_summary(model, plan, sizes);

    let hero: Element<'static, UiMessage> = match plan.composition {
        DetailComposition::CompactPortrait => column![art, summary]
            .spacing(plan.hero_gap)
            .align_x(Alignment::Center)
            .width(Length::Fill)
            .into(),
        _ => row![art, summary]
            .spacing(plan.hero_gap)
            .align_y(Alignment::End)
            .width(Length::Fill)
            .into(),
    };

    container(hero)
        .padding(stage_surface_padding(
            sizes.spacing.lg,
            detail_foreground_surface_tokens(
                plan,
                DetailForegroundSurface::ProjectionShelf,
                DetailTone::Neutral,
            ),
        ))
        .width(Length::Fill)
        .style(foreground_surface_style(detail_foreground_surface_tokens(
            plan,
            DetailForegroundSurface::ProjectionShelf,
            DetailTone::Neutral,
        )))
        .into()
}

pub fn view_metadata_ribbons(
    metadata: &[DetailMetadataPill],
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Element<'static, UiMessage> {
    if metadata.is_empty() {
        return Space::new().into();
    }

    let style = plan.typography.role(DetailTextRole::Metadata);
    let mut ribbons =
        Row::new().spacing(plan.typography.metrics.metadata_pill_gap);
    for pill in metadata {
        let tokens = detail_foreground_surface_tokens(
            plan,
            DetailForegroundSurface::MetadataRibbon,
            pill.tone,
        );
        ribbons = ribbons.push(
            container(styled_text(
                pill.label.clone(),
                style,
                tone_text_color(pill.tone),
                Length::Shrink,
                false,
            ))
            .padding([
                sizes.spacing.xs * tokens.padding_scale,
                sizes.spacing.sm * tokens.padding_scale,
            ])
            .style(foreground_surface_style(tokens)),
        );
    }

    horizontal_scroller(
        ribbons,
        style.line_height_px() + plan.typography.metrics.metadata_spacing,
    )
}

pub fn view_control_shelf(
    actions: &[DetailAction],
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Element<'static, UiMessage> {
    if actions.is_empty() {
        return Space::new().into();
    }

    let content: Element<'static, UiMessage> = match plan.action_cluster.axis {
        super::DetailAxis::Vertical => {
            let mut column = Column::new().spacing(plan.action_cluster.gap);
            for action in actions {
                column = column.push(view_action_button_on_surface(
                    action,
                    plan,
                    sizes,
                    DetailForegroundSurface::ControlShelf,
                ));
            }
            column.into()
        }
        super::DetailAxis::Horizontal => {
            let mut row = Row::new()
                .spacing(plan.action_cluster.gap)
                .align_y(Alignment::Center);
            for action in actions {
                row = row.push(view_action_button_on_surface(
                    action,
                    plan,
                    sizes,
                    DetailForegroundSurface::ControlShelf,
                ));
            }
            row.into()
        }
    };

    let tokens = detail_foreground_surface_tokens(
        plan,
        DetailForegroundSurface::ControlShelf,
        DetailTone::Neutral,
    );
    container(content)
        .padding(stage_surface_padding(sizes.spacing.sm, tokens))
        .width(Length::Shrink)
        .style(foreground_surface_style(tokens))
        .into()
}

pub fn view_stage_sections(
    sections: &[DetailSection],
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
    registered_rails: &[DetailRegisteredRailAdapter<'_>],
) -> Element<'static, UiMessage> {
    if sections.is_empty() {
        return Space::new().into();
    }

    let columns = plan.section_grid.columns.max(1);
    let mut outer = Column::new().spacing(plan.section_grid.gap);
    let mut current: Vec<&DetailSection> = Vec::with_capacity(columns);

    for section in sections.iter() {
        let state = detail_stage_section_render_state(section);
        if state.full_width {
            if !current.is_empty() {
                outer = outer.push(view_stage_section_row(
                    &current,
                    plan,
                    sizes,
                    registered_rails,
                ));
                current.clear();
            }
            outer = outer.push(view_stage_section(
                section,
                plan,
                sizes,
                registered_rails,
            ));
            continue;
        }

        current.push(section);
        if current.len() == columns {
            outer = outer.push(view_stage_section_row(
                &current,
                plan,
                sizes,
                registered_rails,
            ));
            current.clear();
        }
    }

    if !current.is_empty() {
        outer = outer.push(view_stage_section_row(
            &current,
            plan,
            sizes,
            registered_rails,
        ));
    }

    outer.into()
}

fn view_stage_section_row(
    sections: &[&DetailSection],
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
    registered_rails: &[DetailRegisteredRailAdapter<'_>],
) -> Element<'static, UiMessage> {
    let matched_panel_height =
        matched_stage_overview_fact_panel_height(sections, plan, sizes);
    let mut row = Row::new()
        .spacing(plan.section_grid.gap)
        .align_y(Alignment::Start)
        .width(Length::Fill);

    for section in sections {
        row = row.push(view_stage_section_with_height(
            section,
            plan,
            sizes,
            registered_rails,
            matched_panel_height,
        ));
    }

    row.into()
}

fn matched_stage_overview_fact_panel_height(
    sections: &[&DetailSection],
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Option<f32> {
    if sections.len() < 2 {
        return None;
    }

    let row_columns = sections.len().max(1);
    let overview_height =
        sections.iter().find_map(|section| match section {
            DetailSection::Overview(section) => Some(
                stage_overview_panel_height(section, plan, sizes, row_columns),
            ),
            _ => None,
        });
    let fact_height = sections.iter().find_map(|section| match section {
        DetailSection::Facts(section) => {
            Some(stage_fact_panel_height(section, plan, sizes))
        }
        _ => None,
    });

    overview_height.zip(fact_height).map(|(overview, facts)| {
        overview.max(facts).max(plan.section_grid.panel_min_height)
    })
}

fn stage_overview_panel_height(
    section: &DetailOverviewSection,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
    row_columns: usize,
) -> f32 {
    let body_style = plan.typography.overview_body;
    stage_surface_chrome_height(
        plan,
        sizes,
        DetailForegroundSurface::ProjectionShelf,
        DetailTone::Neutral,
    ) + estimated_text_height(
        &section.body,
        body_style,
        section_panel_body_width(plan, sizes, row_columns),
    )
}

fn stage_fact_panel_height(
    section: &DetailFactPanel,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> f32 {
    let tokens = detail_foreground_surface_tokens(
        plan,
        DetailForegroundSurface::FactRibbon,
        DetailTone::Neutral,
    );
    let item_padding = stage_surface_padding(sizes.spacing.xs, tokens);
    let row_count = section.facts.len();
    let row_spacing = sizes.spacing.xs * row_count.saturating_sub(1) as f32;

    stage_surface_chrome_height(
        plan,
        sizes,
        DetailForegroundSurface::FactRibbon,
        DetailTone::Neutral,
    ) + row_count as f32 * (fact_row_height(plan, sizes) + item_padding * 2.0)
        + row_spacing
}

fn stage_surface_chrome_height(
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
    surface: DetailForegroundSurface,
    tone: DetailTone,
) -> f32 {
    let tokens = detail_foreground_surface_tokens(plan, surface, tone);
    stage_surface_padding(sizes.spacing.md, tokens) * 2.0
        + text_budget_height(plan.typography.section_title)
        + sizes.spacing.sm
}

pub fn view_stage_section(
    section: &DetailSection,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
    registered_rails: &[DetailRegisteredRailAdapter<'_>],
) -> Element<'static, UiMessage> {
    view_stage_section_with_height(section, plan, sizes, registered_rails, None)
}

fn view_stage_section_with_height(
    section: &DetailSection,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
    registered_rails: &[DetailRegisteredRailAdapter<'_>],
    matched_panel_height: Option<f32>,
) -> Element<'static, UiMessage> {
    match section {
        DetailSection::Overview(section) => {
            let body_style = plan.typography.overview_body;
            view_projection_shelf_with_height(
                Some(&section.title),
                styled_text(
                    section.body.clone(),
                    body_style,
                    detail_text_color(body_style.color_intent),
                    Length::Fill,
                    false,
                ),
                plan,
                sizes,
                matched_panel_height,
            )
        }
        DetailSection::Facts(section) => view_fact_ribbon_with_height(
            section,
            plan,
            sizes,
            matched_panel_height,
        ),
        DetailSection::Cast(section) => view_cast_band(section, plan, sizes),
        DetailSection::Technical(section) => {
            view_technical_ribbon(section, plan, sizes)
        }
        DetailSection::RelationshipRail(section) => {
            if let Some(key) = section.carousel_key.as_ref()
                && let Some(adapter) =
                    registered_rails.iter().find(|adapter| adapter.key == key)
            {
                return view_registered_relationship_rail_deck(
                    section,
                    key.clone(),
                    adapter.carousel_state,
                    plan,
                    sizes,
                );
            }

            view_relationship_rail_deck(section, plan, sizes)
        }
        DetailSection::Empty(section) => view_empty_stage(section, plan, sizes),
        DetailSection::Notice(section) => {
            view_notice_slab(section, plan, sizes)
        }
    }
}

pub fn view_projection_shelf(
    title: Option<&str>,
    body: Element<'static, UiMessage>,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Element<'static, UiMessage> {
    view_projection_shelf_with_height(title, body, plan, sizes, None)
}

fn view_projection_shelf_with_height(
    title: Option<&str>,
    body: Element<'static, UiMessage>,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
    matched_panel_height: Option<f32>,
) -> Element<'static, UiMessage> {
    let tokens = detail_foreground_surface_tokens(
        plan,
        DetailForegroundSurface::ProjectionShelf,
        DetailTone::Neutral,
    );
    let mut content = Column::new().spacing(sizes.spacing.sm);
    if let Some(title) = title {
        content = content.push(
            text(title.to_string())
                .size(sizes.font.subtitle)
                .color(theme::MediaServerTheme::TEXT_PRIMARY),
        );
    }
    content = content.push(body);

    container(content)
        .width(Length::Fill)
        .height(
            matched_panel_height
                .map(Length::Fixed)
                .unwrap_or(Length::Shrink),
        )
        .padding(stage_surface_padding(sizes.spacing.md, tokens))
        .clip(true)
        .style(foreground_surface_style(tokens))
        .into()
}

pub fn view_fact_ribbon(
    section: &DetailFactPanel,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Element<'static, UiMessage> {
    view_fact_ribbon_with_height(section, plan, sizes, None)
}

fn view_fact_ribbon_with_height(
    section: &DetailFactPanel,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
    matched_panel_height: Option<f32>,
) -> Element<'static, UiMessage> {
    if section.facts.is_empty() {
        return view_empty_stage_message(
            &section.title,
            "No facts are available for this title.",
            None,
            plan,
            sizes,
        );
    }

    let mut facts = Column::new().spacing(sizes.spacing.xs);
    for fact in &section.facts {
        let tokens = detail_foreground_surface_tokens(
            plan,
            DetailForegroundSurface::FactRibbon,
            fact.tone,
        );
        facts = facts.push(
            container(view_fact(fact, plan, sizes))
                .padding(stage_surface_padding(sizes.spacing.xs, tokens))
                .style(foreground_surface_style(tokens)),
        );
    }

    view_stage_surface_shell_with_height(
        &section.title,
        facts.into(),
        DetailForegroundSurface::FactRibbon,
        DetailTone::Neutral,
        plan,
        sizes,
        matched_panel_height,
    )
}

pub fn view_cast_band(
    section: &DetailCastSection,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Element<'static, UiMessage> {
    if section.members.is_empty() {
        return view_empty_stage_message(
            &section.title,
            section
                .empty_message
                .as_deref()
                .unwrap_or("No cast information is available."),
            Some(Icon::Users),
            plan,
            sizes,
        );
    }

    let mut row = Row::new().spacing(plan.rail.gap).align_y(Alignment::Start);
    for member in &section.members {
        row = row.push(view_cast_member(member, plan, sizes));
    }

    let image_width = (cast_card_width(plan) * 0.72).clamp(72.0, 180.0);
    let cast_card_height = image_width * 1.5
        + sizes.font.small
        + sizes.font.micro
        + sizes.spacing.xl;
    view_stage_surface_shell(
        &section.title,
        horizontal_scroller(row, cast_card_height),
        DetailForegroundSurface::CastBand,
        DetailTone::Neutral,
        plan,
        sizes,
    )
}

pub fn view_technical_ribbon(
    section: &DetailTechnicalSection,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Element<'static, UiMessage> {
    if section.items.is_empty() {
        return view_empty_stage_message(
            &section.title,
            section
                .empty_message
                .as_deref()
                .unwrap_or("No technical metadata is available."),
            Some(Icon::Info),
            plan,
            sizes,
        );
    }

    let mut row = Row::new().spacing(sizes.spacing.sm);
    for item in &section.items {
        row = row.push(view_technical_item(item, sizes));
    }

    view_stage_surface_shell(
        &section.title,
        horizontal_scroller(
            row,
            plan.action_cluster.button_height + sizes.spacing.md,
        ),
        DetailForegroundSurface::TechnicalRibbon,
        DetailTone::Neutral,
        plan,
        sizes,
    )
}

pub fn view_relationship_rail_deck(
    section: &DetailRelationshipRail,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Element<'static, UiMessage> {
    if section.items.is_empty() {
        return view_empty_stage_message(
            &section.title,
            section
                .empty_message
                .as_deref()
                .unwrap_or("No related titles are available."),
            Some(Icon::Layers),
            plan,
            sizes,
        );
    }

    let mut row = Row::new().spacing(plan.rail.gap);
    for item in &section.items {
        row = row.push(view_rail_item(item, plan, sizes, Priority::Preload));
    }

    view_stage_surface_shell(
        &section.title,
        horizontal_scroller(row, rail_scroll_height(section, plan, sizes)),
        DetailForegroundSurface::RailBand,
        DetailTone::Neutral,
        plan,
        sizes,
    )
}

pub fn view_registered_relationship_rail_deck(
    section: &DetailRelationshipRail,
    key: CarouselKey,
    carousel_state: &VirtualCarouselState,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Element<'static, UiMessage> {
    if section.items.is_empty() {
        return view_relationship_rail_deck(section, plan, sizes);
    }

    let row =
        registered_relationship_rail_row(section, carousel_state, plan, sizes);
    view_stage_surface_shell(
        &section.title,
        registered_horizontal_scroller(
            row,
            rail_scroll_height(section, plan, sizes),
            key,
            carousel_state,
        ),
        DetailForegroundSurface::RailBand,
        DetailTone::Neutral,
        plan,
        sizes,
    )
}

pub fn view_notice_slab(
    notice: &DetailNotice,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Element<'static, UiMessage> {
    let content = Column::new()
        .spacing(sizes.spacing.xs)
        .push(
            text(notice.title.clone())
                .size(sizes.font.subtitle)
                .color(tone_text_color(notice.tone)),
        )
        .push(
            text(notice.message.clone())
                .size(sizes.font.caption)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
        );

    let tokens = detail_foreground_surface_tokens(
        plan,
        DetailForegroundSurface::NoticeSlab,
        notice.tone,
    );
    container(content)
        .width(Length::Fill)
        .padding(stage_surface_padding(sizes.spacing.md, tokens))
        .style(foreground_surface_style(tokens))
        .into()
}

pub fn view_empty_stage(
    empty: &DetailEmptyState,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Element<'static, UiMessage> {
    view_empty_stage_message(
        &empty.title,
        &empty.message,
        empty.icon,
        plan,
        sizes,
    )
}

fn view_stage_summary(
    model: &DetailPageModel,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Element<'static, UiMessage> {
    let summary_width = stage_summary_width(plan, sizes);
    let mut summary =
        Column::new().spacing(0).width(Length::Fixed(summary_width));

    if let Some(eyebrow) = &model.eyebrow {
        let style = role_style_for_measure(
            plan,
            DetailTextRole::HeroEyebrow,
            summary_width,
        );
        summary = push_title_block_role(
            summary,
            styled_text(
                eyebrow.clone(),
                style,
                detail_text_color(style.color_intent),
                Length::Fixed(style.measure),
                true,
            ),
            style.spacing_after,
        );
    }

    let title_style =
        role_style_for_measure(plan, DetailTextRole::HeroTitle, summary_width);
    summary = push_title_block_role(
        summary,
        styled_text(
            model.title.clone(),
            title_style,
            detail_text_color(title_style.color_intent),
            Length::Fixed(title_style.measure),
            true,
        ),
        title_style.spacing_after,
    );

    if let Some(subtitle) = &model.subtitle {
        let style = role_style_for_measure(
            plan,
            DetailTextRole::HeroSubtitle,
            summary_width,
        );
        summary = push_title_block_role(
            summary,
            styled_text(
                subtitle.clone(),
                style,
                detail_text_color(style.color_intent),
                Length::Fixed(style.measure),
                true,
            ),
            style.spacing_after,
        );
    }

    if !model.metadata.is_empty() {
        let style = role_style_for_measure(
            plan,
            DetailTextRole::Metadata,
            summary_width,
        );
        summary = push_title_block_role(
            summary,
            view_metadata_ribbons(&model.metadata, plan, sizes),
            style.spacing_after,
        );
    }

    if !model.actions.is_empty() {
        summary = summary.push(view_control_shelf(&model.actions, plan, sizes));
    }

    container(summary)
        .width(Length::Fixed(summary_width))
        .align_x(horizontal_alignment(plan.typography.metrics.hero_alignment))
        .into()
}

fn stage_summary_width(plan: &DetailLayoutPlan, sizes: &SizeProvider) -> f32 {
    let shelf_padding = stage_surface_padding(
        sizes.spacing.lg,
        detail_foreground_surface_tokens(
            plan,
            DetailForegroundSurface::ProjectionShelf,
            DetailTone::Neutral,
        ),
    );
    let inner_width = (plan.content_width - shelf_padding * 2.0).max(1.0);
    let available = match plan.composition {
        DetailComposition::CompactPortrait => inner_width,
        _ => (inner_width - plan.hero_art.width - plan.hero_gap).max(1.0),
    };

    plan.typography
        .metrics
        .hero_copy_width
        .min(available)
        .max(1.0)
}

fn role_style_for_measure(
    plan: &DetailLayoutPlan,
    role: DetailTextRole,
    measure: f32,
) -> DetailTextStyle {
    let mut style = plan.typography.role(role);
    style.measure = style.measure.min(measure.max(1.0)).max(1.0);
    style
}

fn view_stage_surface_shell(
    title: &str,
    body: Element<'static, UiMessage>,
    surface: DetailForegroundSurface,
    tone: DetailTone,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Element<'static, UiMessage> {
    view_stage_surface_shell_with_height(
        title, body, surface, tone, plan, sizes, None,
    )
}

fn view_stage_surface_shell_with_height(
    title: &str,
    body: Element<'static, UiMessage>,
    surface: DetailForegroundSurface,
    tone: DetailTone,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
    matched_panel_height: Option<f32>,
) -> Element<'static, UiMessage> {
    let tokens = detail_foreground_surface_tokens(plan, surface, tone);
    let content = Column::new()
        .spacing(sizes.spacing.sm)
        .push(
            text(title.to_string())
                .size(sizes.font.subtitle)
                .color(theme::MediaServerTheme::TEXT_PRIMARY),
        )
        .push(body);

    container(content)
        .width(Length::Fill)
        .height(
            matched_panel_height
                .map(Length::Fixed)
                .unwrap_or(Length::Shrink),
        )
        .padding(stage_surface_padding(sizes.spacing.md, tokens))
        .clip(true)
        .style(foreground_surface_style(tokens))
        .into()
}

fn view_empty_stage_message(
    title: &str,
    message: &str,
    icon: Option<Icon>,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Element<'static, UiMessage> {
    let tokens = detail_foreground_surface_tokens(
        plan,
        DetailForegroundSurface::EmptyState,
        DetailTone::Muted,
    );
    let mut content = Column::new()
        .spacing(sizes.spacing.sm)
        .align_x(Alignment::Center);

    if let Some(icon) = icon {
        content = content.push(
            icon_text_with_size(icon, sizes.icon.xl)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
        );
    }

    content = content
        .push(
            text(title.to_string())
                .size(sizes.font.title)
                .color(theme::MediaServerTheme::TEXT_PRIMARY),
        )
        .push(
            text(message.to_string())
                .size(sizes.font.body)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
        );

    container(content)
        .width(Length::Fill)
        .padding(stage_surface_padding(sizes.spacing.xl, tokens))
        .align_x(iced::alignment::Horizontal::Center)
        .style(foreground_surface_style(tokens))
        .into()
}

fn stage_surface_padding(
    base: f32,
    tokens: DetailForegroundSurfaceTokens,
) -> f32 {
    (base * tokens.padding_scale).max(0.0)
}

fn surface_intensity(
    tokens: DetailSurfaceIntensityTokens,
    surface: DetailForegroundSurface,
) -> f32 {
    match surface {
        DetailForegroundSurface::StageField => tokens.stage,
        DetailForegroundSurface::ProjectionShelf => tokens.readable_copy_lobe,
        DetailForegroundSurface::ControlShelf => tokens.control_shelf,
        DetailForegroundSurface::RailBand
        | DetailForegroundSurface::CastBand => tokens.rail_deck,
        DetailForegroundSurface::FactRibbon
        | DetailForegroundSurface::TechnicalRibbon
        | DetailForegroundSurface::NoticeSlab
        | DetailForegroundSurface::EmptyState => tokens.section_band,
        DetailForegroundSurface::MetadataRibbon => tokens.readable_copy_lobe,
    }
}

fn action_role_tone(role: DetailActionRole) -> DetailTone {
    match role {
        DetailActionRole::Primary | DetailActionRole::Toggle => {
            DetailTone::Accent
        }
        DetailActionRole::Destructive => DetailTone::Danger,
        DetailActionRole::Back | DetailActionRole::Secondary => {
            DetailTone::Neutral
        }
    }
}

fn tone_accent_color(tone: DetailTone) -> Color {
    match tone {
        DetailTone::Neutral | DetailTone::Muted => {
            theme::MediaServerTheme::BORDER_COLOR
        }
        DetailTone::Accent => theme::MediaServerTheme::ACCENT,
        DetailTone::Success => theme::MediaServerTheme::SUCCESS,
        DetailTone::Warning => theme::MediaServerTheme::WARNING,
        DetailTone::Danger => theme::MediaServerTheme::ERROR,
    }
}

fn notice_background_color(tone: DetailTone, intensity: f32) -> Color {
    let accent = tone_accent_color(tone);
    let lift = 0.030 + intensity * 0.050;
    Color::from_rgba(
        (accent.r * 0.18 + lift).min(1.0),
        (accent.g * 0.18 + lift).min(1.0),
        (accent.b * 0.18 + lift).min(1.0),
        0.50 + intensity * 0.24,
    )
}

fn foreground_surface_style(
    tokens: DetailForegroundSurfaceTokens,
) -> impl Fn(&Theme) -> container::Style + Clone {
    move |_| container::Style {
        text_color: Some(tokens.text),
        background: Some(Background::Color(tokens.background)),
        border: Border {
            color: tokens.edge,
            width: tokens.border_width,
            radius: tokens.radius.into(),
        },
        shadow: if tokens.shadow_blur > 0.0 {
            Shadow {
                color: Color::from_rgba(0.0, 0.0, 0.0, 0.24),
                offset: Vector::new(0.0, tokens.shadow_blur * 0.10),
                blur_radius: tokens.shadow_blur,
            }
        } else {
            Shadow::default()
        },
        snap: false,
    }
}

/// Render the hero block shared by desktop, compact, cinematic, and ten-foot
/// detail pages.
pub fn view_detail_hero(
    model: &DetailPageModel,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Element<'static, UiMessage> {
    let art = view_hero_art(&model.hero_art, plan, sizes);
    let summary = view_title_block(model, plan, sizes);

    let hero: Element<'static, UiMessage> = match plan.composition {
        DetailComposition::CompactPortrait => column![art, summary]
            .spacing(plan.hero_gap)
            .align_x(Alignment::Center)
            .width(Length::Fill)
            .into(),
        _ => row![art, summary]
            .spacing(plan.hero_gap)
            .align_y(Alignment::End)
            .width(Length::Fill)
            .into(),
    };

    container(hero)
        .padding(sizes.spacing.lg)
        .width(Length::Fill)
        .into()
}

pub fn view_hero_art(
    artwork: &DetailArtwork,
    plan: &DetailLayoutPlan,
    _sizes: &SizeProvider,
) -> Element<'static, UiMessage> {
    view_artwork(
        artwork,
        plan.hero_art,
        Priority::Visible,
        Length::Fixed(plan.hero_art.width),
        Length::Fixed(plan.hero_art.height),
    )
}

pub fn view_metadata_pills(
    metadata: &[DetailMetadataPill],
    sizes: &SizeProvider,
) -> Element<'static, UiMessage> {
    let mut row = Row::new()
        .spacing(sizes.spacing.sm)
        .align_y(Alignment::Center);
    let groups = metadata_render_groups(metadata);

    if !groups.inline_labels.is_empty() {
        row = row.push(
            text(groups.inline_labels.join(" • "))
                .size(sizes.font.small)
                .line_height(1.18)
                .wrapping(Wrapping::None)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
        );
    }

    for chip in groups.chips {
        row = row.push(
            container(
                text(chip.label.clone())
                    .size(sizes.font.small)
                    .line_height(1.18)
                    .wrapping(Wrapping::None)
                    .color(tone_text_color(chip.tone)),
            )
            .padding([sizes.spacing.xs, sizes.spacing.sm])
            .style(pill_style(chip.tone)),
        );
    }

    horizontal_scroller(row, sizes.font.small * 1.18 + sizes.spacing.lg)
}

fn view_metadata_group_for_plan(
    metadata: &[DetailMetadataPill],
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Element<'static, UiMessage> {
    let style = plan.typography.metadata;
    let mut row = Row::new()
        .spacing(plan.typography.metrics.metadata_pill_gap)
        .align_y(Alignment::Center);
    let groups = metadata_render_groups(metadata);

    if !groups.inline_labels.is_empty() {
        row = row.push(
            text(groups.inline_labels.join(" • "))
                .size(style.size)
                .line_height(style.line_height)
                .wrapping(Wrapping::None)
                .align_x(text_alignment(style.alignment))
                .color(detail_text_color(style.color_intent)),
        );
    }

    for chip in groups.chips {
        row = row.push(
            container(
                text(chip.label.clone())
                    .size(style.size)
                    .line_height(style.line_height)
                    .wrapping(Wrapping::None)
                    .color(tone_text_color(chip.tone)),
            )
            .padding([
                (plan.typography.metrics.metadata_spacing * 0.5)
                    .max(sizes.spacing.xs),
                sizes.spacing.sm,
            ])
            .style(pill_style(chip.tone)),
        );
    }

    horizontal_scroller(
        row,
        style.line_height_px() + plan.typography.metrics.metadata_spacing,
    )
}

pub fn view_action_cluster(
    actions: &[DetailAction],
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Element<'static, UiMessage> {
    match plan.action_cluster.axis {
        super::DetailAxis::Vertical => {
            let mut column = Column::new().spacing(plan.action_cluster.gap);
            for action in actions {
                column = column.push(view_action_button(action, plan, sizes));
            }
            column.into()
        }
        super::DetailAxis::Horizontal => {
            let mut row = Row::new()
                .spacing(plan.action_cluster.gap)
                .align_y(Alignment::Center);
            for action in actions {
                row = row.push(view_action_button(action, plan, sizes));
            }
            row.into()
        }
    }
}

pub fn view_sections(
    sections: &[DetailSection],
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Element<'static, UiMessage> {
    if sections.is_empty() {
        return Space::new().into();
    }

    let columns = plan.section_grid.columns.max(1);
    let mut outer = Column::new().spacing(plan.section_grid.gap);
    let mut current: Vec<&DetailSection> = Vec::with_capacity(columns);

    for section in sections.iter() {
        if matches!(section, DetailSection::Cast(_)) {
            if !current.is_empty() {
                outer = outer.push(view_section_row(&current, plan, sizes));
                current.clear();
            }
            outer = outer.push(view_section(section, plan, sizes));
            continue;
        }

        current.push(section);
        if current.len() == columns {
            outer = outer.push(view_section_row(&current, plan, sizes));
            current.clear();
        }
    }

    if !current.is_empty() {
        outer = outer.push(view_section_row(&current, plan, sizes));
    }

    outer.into()
}

fn view_section_row(
    sections: &[&DetailSection],
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Element<'static, UiMessage> {
    let matched_panel_height =
        matched_overview_fact_panel_height(sections, plan, sizes);
    let mut row = Row::new()
        .spacing(plan.section_grid.gap)
        .align_y(Alignment::Start)
        .width(Length::Fill);

    for section in sections {
        row = row.push(view_section_with_panel_height(
            section,
            plan,
            sizes,
            matched_panel_height,
        ));
    }

    row.into()
}

fn matched_overview_fact_panel_height(
    sections: &[&DetailSection],
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Option<f32> {
    if sections.len() < 2 {
        return None;
    }

    let row_columns = sections.len().max(1);
    let overview_height = sections.iter().find_map(|section| match section {
        DetailSection::Overview(section) => {
            Some(overview_panel_height(section, plan, sizes, row_columns))
        }
        _ => None,
    });
    let fact_height = sections.iter().find_map(|section| match section {
        DetailSection::Facts(section) => {
            Some(fact_panel_height(section, plan, sizes))
        }
        _ => None,
    });

    overview_height.zip(fact_height).map(|(overview, facts)| {
        overview.max(facts).max(plan.section_grid.panel_min_height)
    })
}

fn overview_panel_height(
    section: &DetailOverviewSection,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
    row_columns: usize,
) -> f32 {
    let body_style = plan.typography.overview_body;
    panel_chrome_height(plan, sizes)
        + estimated_text_height(
            &section.body,
            body_style,
            section_panel_body_width(plan, sizes, row_columns),
        )
}

fn fact_panel_height(
    section: &DetailFactPanel,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> f32 {
    let row_count = match plan.typography.metrics.fact_layout_mode {
        DetailFactLayoutMode::TwoColumn if section.facts.len() > 1 => {
            section.facts.len().div_ceil(2)
        }
        _ => section.facts.len(),
    };
    let row_spacing = sizes.spacing.sm * row_count.saturating_sub(1) as f32;

    panel_chrome_height(plan, sizes)
        + row_count as f32 * fact_row_height(plan, sizes)
        + row_spacing
}

fn fact_row_height(plan: &DetailLayoutPlan, sizes: &SizeProvider) -> f32 {
    let label_height = text_budget_height(plan.typography.fact_label);
    let value_height = text_budget_height(plan.typography.fact_value);

    match plan.typography.metrics.fact_layout_mode {
        DetailFactLayoutMode::Stacked => {
            label_height + sizes.spacing.xs + value_height
        }
        DetailFactLayoutMode::Inline | DetailFactLayoutMode::TwoColumn => {
            label_height.max(value_height)
        }
    }
}

fn panel_chrome_height(plan: &DetailLayoutPlan, sizes: &SizeProvider) -> f32 {
    sizes.spacing.md * 2.0
        + text_budget_height(plan.typography.section_title)
        + sizes.spacing.sm
}

fn section_panel_body_width(
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
    row_columns: usize,
) -> f32 {
    let columns = row_columns.max(1);
    let total_gap = plan.section_grid.gap * columns.saturating_sub(1) as f32;
    let panel_width =
        ((plan.content_width - total_gap) / columns as f32).max(1.0);

    (panel_width - sizes.spacing.md * 2.0).max(1.0)
}

fn estimated_text_height(
    content: &str,
    style: DetailTextStyle,
    width: f32,
) -> f32 {
    let budgeted_lines = style.max_lines().unwrap_or(1) as usize;
    let estimated_lines = estimate_wrapped_lines(content, style, width);

    style.line_height_px() * budgeted_lines.max(estimated_lines) as f32
}

fn estimate_wrapped_lines(
    content: &str,
    style: DetailTextStyle,
    width: f32,
) -> usize {
    let content = content.trim();
    if content.is_empty() {
        return 1;
    }

    if matches!(
        style.overflow,
        DetailTextOverflow::SingleLineEllipsis
            | DetailTextOverflow::HorizontalScroll
    ) {
        return 1;
    }

    let average_glyph_width = (style.size * 0.62).max(1.0);
    let chars_per_line =
        (width / average_glyph_width).floor().max(1.0) as usize;

    content
        .lines()
        .map(|line| estimate_paragraph_lines(line, chars_per_line))
        .sum::<usize>()
        .max(1)
}

fn estimate_paragraph_lines(line: &str, chars_per_line: usize) -> usize {
    let mut lines = 1usize;
    let mut current_len = 0usize;

    for word in line.split_whitespace() {
        let word_len = word.chars().count();
        let separator = usize::from(current_len > 0);

        if current_len > 0
            && current_len + separator + word_len > chars_per_line
        {
            lines += 1;
            current_len = word_len;
        } else {
            current_len += separator + word_len;
        }

        while current_len > chars_per_line {
            lines += 1;
            current_len -= chars_per_line;
        }
    }

    lines
}

pub fn view_section(
    section: &DetailSection,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Element<'static, UiMessage> {
    view_section_with_panel_height(section, plan, sizes, None)
}

fn view_section_with_panel_height(
    section: &DetailSection,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
    matched_panel_height: Option<f32>,
) -> Element<'static, UiMessage> {
    match section {
        DetailSection::Overview(section) => view_overview_section_with_height(
            section,
            plan,
            sizes,
            matched_panel_height,
        ),
        DetailSection::Facts(section) => view_fact_panel_with_height(
            section,
            plan,
            sizes,
            matched_panel_height,
        ),
        DetailSection::Cast(section) => view_cast_section(section, plan, sizes),
        DetailSection::Technical(section) => {
            view_technical_section(section, plan, sizes)
        }
        DetailSection::RelationshipRail(section) => {
            view_relationship_rail(section, plan, sizes)
        }
        DetailSection::Empty(section) => view_empty_state(section, sizes),
        DetailSection::Notice(section) => view_notice(section, plan, sizes),
    }
}

pub fn view_overview_section(
    section: &DetailOverviewSection,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Element<'static, UiMessage> {
    view_overview_section_with_height(section, plan, sizes, None)
}

fn view_overview_section_with_height(
    section: &DetailOverviewSection,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
    matched_panel_height: Option<f32>,
) -> Element<'static, UiMessage> {
    let body_style = plan.typography.overview_body;
    view_panel_compat_with_height(
        &section.title,
        styled_text(
            section.body.clone(),
            body_style,
            detail_text_color(body_style.color_intent),
            Length::Fill,
            false,
        ),
        plan,
        sizes,
        matched_panel_height,
    )
}

pub fn view_fact_panel(
    section: &DetailFactPanel,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Element<'static, UiMessage> {
    view_fact_panel_with_height(section, plan, sizes, None)
}

fn view_fact_panel_with_height(
    section: &DetailFactPanel,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
    matched_panel_height: Option<f32>,
) -> Element<'static, UiMessage> {
    let facts: Element<'static, UiMessage> =
        match plan.typography.metrics.fact_layout_mode {
            DetailFactLayoutMode::TwoColumn if section.facts.len() > 1 => {
                let mut rows = Column::new().spacing(sizes.spacing.sm);
                for pair in section.facts.chunks(2) {
                    let mut row = Row::new()
                        .spacing(plan.section_grid.gap.min(sizes.spacing.lg))
                        .align_y(Alignment::Start);
                    for fact in pair {
                        row = row.push(
                            container(view_fact(fact, plan, sizes))
                                .width(Length::FillPortion(1)),
                        );
                    }
                    if pair.len() == 1 {
                        row = row
                            .push(Space::new().width(Length::FillPortion(1)));
                    }
                    rows = rows.push(row);
                }
                rows.into()
            }
            _ => {
                let mut column = Column::new().spacing(sizes.spacing.sm);
                for fact in &section.facts {
                    column = column.push(view_fact(fact, plan, sizes));
                }
                column.into()
            }
        };

    view_panel_compat_with_height(
        &section.title,
        facts,
        plan,
        sizes,
        matched_panel_height,
    )
}

pub fn view_cast_section(
    section: &DetailCastSection,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Element<'static, UiMessage> {
    if section.members.is_empty() {
        return view_panel_compat(
            &section.title,
            {
                let caption_style = plan.typography.caption;
                text(section.empty_message.clone().unwrap_or_else(|| {
                    "No cast information is available.".to_string()
                }))
                .size(caption_style.size)
                .line_height(caption_style.line_height)
                .color(detail_text_color(caption_style.color_intent))
                .into()
            },
            plan,
            sizes,
        );
    }

    let mut row = Row::new().spacing(plan.rail.gap).align_y(Alignment::Start);
    for member in &section.members {
        row = row.push(view_cast_member(member, plan, sizes));
    }

    let image_width = (cast_card_width(plan) * 0.72).clamp(72.0, 180.0);
    let cast_card_height = image_width * 1.5
        + text_budget_height(plan.typography.cast_name)
        + text_budget_height(plan.typography.cast_role)
        + sizes.spacing.xl;

    view_panel_compat(
        &section.title,
        horizontal_scroller(row, cast_card_height),
        plan,
        sizes,
    )
}

pub fn view_technical_section(
    section: &DetailTechnicalSection,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Element<'static, UiMessage> {
    if section.items.is_empty() {
        return view_panel_compat(
            &section.title,
            {
                let caption_style = plan.typography.caption;
                text(section.empty_message.clone().unwrap_or_else(|| {
                    "No technical metadata is available.".to_string()
                }))
                .size(caption_style.size)
                .line_height(caption_style.line_height)
                .color(detail_text_color(caption_style.color_intent))
                .into()
            },
            plan,
            sizes,
        );
    }

    let mut row = Row::new().spacing(sizes.spacing.sm);
    for item in &section.items {
        row = row.push(view_technical_item(item, sizes));
    }

    view_panel_compat(
        &section.title,
        horizontal_scroller(
            row,
            plan.action_cluster.button_height + sizes.spacing.md,
        ),
        plan,
        sizes,
    )
}

pub fn view_relationship_rail(
    section: &DetailRelationshipRail,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Element<'static, UiMessage> {
    if section.items.is_empty() {
        return view_panel_compat(
            &section.title,
            {
                let caption_style = plan.typography.caption;
                text(section.empty_message.clone().unwrap_or_else(|| {
                    "No related titles are available.".to_string()
                }))
                .size(caption_style.size)
                .line_height(caption_style.line_height)
                .color(detail_text_color(caption_style.color_intent))
                .into()
            },
            plan,
            sizes,
        );
    }

    let mut row = Row::new().spacing(plan.rail.gap);
    for item in &section.items {
        row = row.push(view_rail_item(item, plan, sizes, Priority::Preload));
    }

    view_panel_compat(
        &section.title,
        horizontal_scroller(row, rail_scroll_height(section, plan, sizes)),
        plan,
        sizes,
    )
}

pub fn view_registered_relationship_rail(
    section: &DetailRelationshipRail,
    key: CarouselKey,
    carousel_state: &VirtualCarouselState,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Element<'static, UiMessage> {
    if section.items.is_empty() {
        return view_relationship_rail(section, plan, sizes);
    }

    let row =
        registered_relationship_rail_row(section, carousel_state, plan, sizes);

    view_panel_compat(
        &section.title,
        registered_horizontal_scroller(
            row,
            rail_scroll_height(section, plan, sizes),
            key,
            carousel_state,
        ),
        plan,
        sizes,
    )
}

pub fn view_empty_state(
    empty: &DetailEmptyState,
    sizes: &SizeProvider,
) -> Element<'static, UiMessage> {
    let mut content = Column::new()
        .spacing(sizes.spacing.sm)
        .align_x(Alignment::Center);

    if let Some(icon) = empty.icon {
        content = content.push(
            icon_text_with_size(icon, sizes.icon.xl)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
        );
    }

    content = content
        .push(
            text(empty.title.clone())
                .size(sizes.font.title)
                .color(theme::MediaServerTheme::TEXT_PRIMARY),
        )
        .push(
            text(empty.message.clone())
                .size(sizes.font.body)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
        );

    container(content)
        .width(Length::Fill)
        .padding(sizes.spacing.xl)
        .align_x(iced::alignment::Horizontal::Center)
        .style(detail_panel_style(DetailTone::Muted))
        .into()
}

pub fn view_backdrop_controls(
    controls: &[DetailBackdropControl],
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Element<'static, UiMessage> {
    let mut row = Row::new()
        .spacing(sizes.spacing.xs)
        .align_y(Alignment::Center);

    for control in controls {
        row = row.push(
            button(text(control.label.clone()).size(sizes.font.small))
                .on_press(control.on_press.clone())
                .padding([sizes.spacing.xs, sizes.spacing.sm])
                .height(Length::Fixed(plan.backdrop.control_height))
                .style(theme::Button::BackdropControl.style()),
        );
    }

    container(row)
        .width(Length::Fill)
        .align_x(iced::alignment::Horizontal::Right)
        .into()
}

fn view_title_block(
    model: &DetailPageModel,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Element<'static, UiMessage> {
    let mut block = Column::new()
        .spacing(0)
        .width(Length::Fixed(plan.typography.metrics.hero_copy_width));

    if let Some(eyebrow) = &model.eyebrow {
        let style = plan.typography.hero_eyebrow;
        block = push_title_block_role(
            block,
            styled_text(
                eyebrow.clone(),
                style,
                detail_text_color(style.color_intent),
                Length::Fill,
                true,
            ),
            style.spacing_after,
        );
    }

    let title_style = plan.typography.hero_title;
    block = push_title_block_role(
        block,
        styled_text(
            model.title.clone(),
            title_style,
            detail_text_color(title_style.color_intent),
            Length::Fill,
            true,
        ),
        title_style.spacing_after,
    );

    if let Some(subtitle) = &model.subtitle {
        let style = plan.typography.hero_subtitle;
        block = push_title_block_role(
            block,
            styled_text(
                subtitle.clone(),
                style,
                detail_text_color(style.color_intent),
                Length::Fill,
                true,
            ),
            style.spacing_after,
        );
    }

    if !model.metadata.is_empty() {
        let style = plan.typography.metadata;
        block = push_title_block_role(
            block,
            view_metadata_group_for_plan(&model.metadata, plan, sizes),
            style.spacing_after,
        );
    }

    if let Some(overview) = hero_overview(model) {
        let style = plan.typography.hero_overview;
        block = push_title_block_role(
            block,
            styled_text(
                overview.to_string(),
                style,
                detail_text_color(style.color_intent),
                Length::Fill,
                true,
            ),
            style.spacing_after,
        );
    }

    if !model.actions.is_empty() {
        block = block.push(view_action_cluster(&model.actions, plan, sizes));
    }

    container(block)
        .width(Length::Fill)
        .align_x(horizontal_alignment(plan.typography.metrics.hero_alignment))
        .into()
}

fn push_title_block_role(
    block: Column<'static, UiMessage>,
    role: Element<'static, UiMessage>,
    spacing_after: f32,
) -> Column<'static, UiMessage> {
    block
        .push(role)
        .push(Space::new().height(Length::Fixed(spacing_after)))
}

fn hero_overview(model: &DetailPageModel) -> Option<&str> {
    model.sections.iter().find_map(|section| match section {
        DetailSection::Overview(overview) => Some(overview.body.as_str()),
        _ => None,
    })
}

fn view_action_button(
    action: &DetailAction,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Element<'static, UiMessage> {
    let surface_mode = detail_action_surface_mode(action);
    if matches!(surface_mode, DetailActionSurfaceMode::Menu) {
        return view_action_menu(action, plan, sizes);
    }

    let disabled = matches!(surface_mode, DetailActionSurfaceMode::Disabled);
    let mut label_row = Row::new()
        .spacing(sizes.spacing.xs)
        .align_y(Alignment::Center);
    if let Some(icon) = action.icon {
        label_row = label_row.push(icon_text_with_size(icon, sizes.icon.sm));
    }
    let label_style = plan.typography.action_label;
    label_row = label_row.push(
        text(action.label.clone())
            .size(label_style.size)
            .line_height(label_style.line_height)
            .color(detail_text_color(label_style.color_intent)),
    );

    let mut content = Column::new()
        .spacing(2.0)
        .align_x(Alignment::Center)
        .push(label_row);
    if let Some(subtitle) = &action.subtitle {
        let subtitle_style = plan.typography.action_subtitle;
        content = content.push(
            text(subtitle.clone())
                .size(subtitle_style.size)
                .line_height(subtitle_style.line_height)
                .color(detail_text_color(subtitle_style.color_intent)),
        );
    }

    let mut button = button(content)
        .padding([sizes.spacing.xs, sizes.spacing.md])
        .width(Length::Fixed(plan.action_cluster.button_width))
        .height(Length::Fixed(plan.action_cluster.button_height))
        .style(detail_action_button_style(action.role, disabled));

    if let Some(message) = &action.on_press {
        button = button.on_press(message.clone());
    }

    button.into()
}

fn view_action_menu(
    action: &DetailAction,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Element<'static, UiMessage> {
    let mut label_row = Row::new()
        .spacing(sizes.spacing.xs)
        .align_y(Alignment::Center);
    if let Some(icon) = action.icon {
        label_row = label_row.push(icon_text_with_size(icon, sizes.icon.sm));
    }
    let label_style = plan.typography.action_label;
    label_row = label_row.push(
        text(action.label.clone())
            .size(label_style.size)
            .line_height(label_style.line_height)
            .color(detail_text_color(label_style.color_intent)),
    );

    let trigger = button(label_row)
        .padding([sizes.spacing.xs, sizes.spacing.md])
        .width(Length::Fixed(plan.action_cluster.button_width))
        .height(Length::Fixed(plan.action_cluster.button_height))
        .style(detail_action_button_style(action.role, false));

    let mut items: Vec<Item<'static, UiMessage, Theme, iced::Renderer>> =
        Vec::new();
    for item in &action.menu_items {
        let item_button =
            button(text(item.label.clone()).size(sizes.font.small))
                .on_press(item.on_press.clone())
                .style(theme::Button::HeaderMenuSecondary.style());
        items.push(Item::new(item_button));
    }

    let menu = Menu::new(items)
        .max_width(plan.action_cluster.button_width.max(220.0))
        .spacing(0.0)
        .offset(0.0);

    MenuBar::new(vec![Item::with_menu(trigger, menu)])
        .spacing(0.0)
        .height(Length::Shrink)
        .close_on_item_click(true)
        .into()
}

fn view_action_button_on_surface(
    action: &DetailAction,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
    surface: DetailForegroundSurface,
) -> Element<'static, UiMessage> {
    let surface_mode = detail_action_surface_mode(action);
    if matches!(surface_mode, DetailActionSurfaceMode::Menu) {
        return view_action_menu_on_surface(action, plan, sizes, surface);
    }

    let disabled = matches!(surface_mode, DetailActionSurfaceMode::Disabled);
    let label_style = role_style_for_measure(
        plan,
        DetailTextRole::ActionLabel,
        action_label_text_width(action, plan, sizes),
    );
    let mut label_row = Row::new()
        .spacing(sizes.spacing.xs)
        .align_y(Alignment::Center);
    if let Some(icon) = action.icon {
        label_row = label_row.push(icon_text_with_size(icon, sizes.icon.sm));
    }
    label_row = label_row.push(styled_text(
        action.label.clone(),
        label_style,
        detail_text_color(label_style.color_intent),
        Length::Fixed(label_style.measure),
        true,
    ));

    let mut content = Column::new()
        .spacing(action_text_spacing(plan, sizes))
        .align_x(Alignment::Center)
        .push(label_row);
    if let Some(subtitle) = &action.subtitle {
        let subtitle_style = role_style_for_measure(
            plan,
            DetailTextRole::ActionSubtitle,
            action_button_inner_width(plan, sizes),
        );
        content = content.push(styled_text(
            subtitle.clone(),
            subtitle_style,
            detail_text_color(subtitle_style.color_intent),
            Length::Fixed(subtitle_style.measure),
            true,
        ));
    }

    let mut button = button(content)
        .padding([sizes.spacing.xs, sizes.spacing.md])
        .width(Length::Fixed(plan.action_cluster.button_width))
        .height(Length::Fixed(plan.action_cluster.button_height))
        .style(detail_action_button_style_on_surface(
            action.role,
            disabled,
            detail_foreground_surface_tokens(
                plan,
                surface,
                action_role_tone(action.role),
            ),
        ));

    if let Some(message) = &action.on_press {
        button = button.on_press(message.clone());
    }

    button.into()
}

fn view_action_menu_on_surface(
    action: &DetailAction,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
    surface: DetailForegroundSurface,
) -> Element<'static, UiMessage> {
    let label_style = role_style_for_measure(
        plan,
        DetailTextRole::ActionLabel,
        action_label_text_width(action, plan, sizes),
    );
    let mut label_row = Row::new()
        .spacing(sizes.spacing.xs)
        .align_y(Alignment::Center);
    if let Some(icon) = action.icon {
        label_row = label_row.push(icon_text_with_size(icon, sizes.icon.sm));
    }
    label_row = label_row.push(styled_text(
        action.label.clone(),
        label_style,
        detail_text_color(label_style.color_intent),
        Length::Fixed(label_style.measure),
        true,
    ));

    let trigger = button(label_row)
        .padding([sizes.spacing.xs, sizes.spacing.md])
        .width(Length::Fixed(plan.action_cluster.button_width))
        .height(Length::Fixed(plan.action_cluster.button_height))
        .style(detail_action_button_style_on_surface(
            action.role,
            false,
            detail_foreground_surface_tokens(
                plan,
                surface,
                action_role_tone(action.role),
            ),
        ));

    let menu_item_style = role_style_for_measure(
        plan,
        DetailTextRole::ActionSubtitle,
        plan.action_cluster.button_width,
    );
    let mut items: Vec<Item<'static, UiMessage, Theme, iced::Renderer>> =
        Vec::new();
    for item in &action.menu_items {
        let item_button = button(styled_text(
            item.label.clone(),
            menu_item_style,
            detail_text_color(menu_item_style.color_intent),
            Length::Shrink,
            false,
        ))
        .on_press(item.on_press.clone())
        .style(theme::Button::HeaderMenuSecondary.style());
        items.push(Item::new(item_button));
    }

    let menu = Menu::new(items)
        .max_width(plan.action_cluster.button_width.max(220.0))
        .spacing(0.0)
        .offset(0.0);

    MenuBar::new(vec![Item::with_menu(trigger, menu)])
        .spacing(0.0)
        .height(Length::Shrink)
        .close_on_item_click(true)
        .into()
}

fn action_button_inner_width(
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> f32 {
    (plan.action_cluster.button_width - sizes.spacing.md * 2.0).max(1.0)
}

fn action_label_text_width(
    action: &DetailAction,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> f32 {
    let icon_width = action
        .icon
        .map(|_| sizes.icon.sm + sizes.spacing.xs)
        .unwrap_or(0.0);

    (action_button_inner_width(plan, sizes) - icon_width).max(1.0)
}

fn action_text_spacing(plan: &DetailLayoutPlan, sizes: &SizeProvider) -> f32 {
    plan.typography
        .action_subtitle
        .spacing_after
        .min(plan.typography.action_label.spacing_after)
        .clamp(2.0, sizes.spacing.sm.max(2.0))
}

fn view_fact(
    fact: &DetailFact,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Element<'static, UiMessage> {
    let label_style = plan.typography.fact_label;
    let value_style = plan.typography.fact_value;
    let label = styled_text(
        fact.label.clone(),
        label_style,
        detail_text_color(label_style.color_intent),
        Length::Fixed(plan.typography.metrics.fact_label_width),
        true,
    );
    let value = styled_text(
        fact.value.clone(),
        value_style,
        tone_text_color(fact.tone),
        Length::Fill,
        true,
    );

    match plan.typography.metrics.fact_layout_mode {
        DetailFactLayoutMode::Stacked => column![
            styled_text(
                fact.label.clone(),
                label_style,
                detail_text_color(label_style.color_intent),
                Length::Fill,
                true,
            ),
            value,
        ]
        .spacing(sizes.spacing.xs)
        .into(),
        DetailFactLayoutMode::Inline | DetailFactLayoutMode::TwoColumn => {
            row![label, value]
                .spacing(sizes.spacing.sm)
                .align_y(Alignment::Start)
                .into()
        }
    }
}

fn cast_card_width(plan: &DetailLayoutPlan) -> f32 {
    match plan.composition {
        DetailComposition::CompactPortrait => (plan.content_width * 0.46)
            .min(plan.rail.card_width)
            .max(112.0),
        DetailComposition::CompactLandscape => plan.rail.card_width.max(140.0),
        DetailComposition::BalancedDesktop => plan.rail.card_width.max(170.0),
        DetailComposition::CinematicWide => plan.rail.card_width.max(200.0),
        DetailComposition::TenFoot => plan.rail.card_width.max(220.0),
    }
}

fn view_cast_profile_image(
    artwork: &DetailArtwork,
    width: f32,
    height: f32,
    sizes: &SizeProvider,
) -> Element<'static, UiMessage> {
    view_artwork(
        artwork,
        DetailArtLayout {
            width,
            height,
            corner_radius: sizes.scale(3.0),
            aspect: DetailArtAspect::Poster,
        },
        Priority::Preload,
        Length::Fixed(width),
        Length::Fixed(height),
    )
}

fn view_cast_member(
    member: &DetailCastMember,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Element<'static, UiMessage> {
    let card_width = cast_card_width(plan);
    let image_width = (card_width * 0.72).clamp(72.0, 180.0);
    let image_height = image_width * 1.5;
    let image = view_cast_profile_image(
        &member.artwork,
        image_width,
        image_height,
        sizes,
    );

    let mut content = Column::new()
        .spacing(sizes.spacing.xs)
        .align_x(Alignment::Center)
        .width(Length::Fixed(card_width))
        .push(image)
        .push({
            let style = plan.typography.cast_name;
            styled_text(
                member.name.clone(),
                style,
                detail_text_color(style.color_intent),
                Length::Fill,
                true,
            )
        });

    if let Some(role) = &member.role {
        let style = plan.typography.cast_role;
        content = content.push(styled_text(
            role.clone(),
            style,
            detail_text_color(style.color_intent),
            Length::Fill,
            true,
        ));
    }

    content.into()
}

fn view_technical_item(
    item: &DetailTechnicalItem,
    sizes: &SizeProvider,
) -> Element<'static, UiMessage> {
    let mut row = Row::new()
        .spacing(sizes.spacing.xs)
        .align_y(Alignment::Center);
    if let Some(icon) = item.icon {
        row = row.push(icon_text_with_size(icon, sizes.icon.sm));
    }
    row = row
        .push(
            text(item.label.clone())
                .size(sizes.font.small)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
        )
        .push(
            text(item.value.clone())
                .size(sizes.font.small)
                .color(tone_text_color(item.tone)),
        );

    container(row)
        .padding([sizes.spacing.xs, sizes.spacing.sm])
        .style(pill_style(item.tone))
        .into()
}

fn rail_scroll_height(
    section: &DetailRelationshipRail,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> f32 {
    let max_image_height = section
        .items
        .iter()
        .map(|item| rail_art_layout(&item.artwork, plan, sizes).height)
        .fold(plan.rail.card_height, f32::max);

    max_image_height
        + text_budget_height(plan.typography.rail_title)
        + text_budget_height(plan.typography.rail_subtitle)
        + sizes.spacing.xl
}

fn rail_art_layout(
    artwork: &DetailArtwork,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> DetailArtLayout {
    let (height, aspect) = match artwork {
        DetailArtwork::Poster { .. } | DetailArtwork::Profile { .. } => (
            plan.rail.card_width / (2.0 / 3.0),
            super::DetailArtAspect::Poster,
        ),
        DetailArtwork::Still { .. } | DetailArtwork::None { .. } => (
            plan.rail.card_width * 9.0 / 16.0,
            super::DetailArtAspect::Still,
        ),
    };

    DetailArtLayout {
        width: plan.rail.card_width,
        height,
        corner_radius: sizes.scale(10.0),
        aspect,
    }
}

fn view_rail_item(
    item: &DetailRailItem,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
    priority: Priority,
) -> Element<'static, UiMessage> {
    let image_layout = rail_art_layout(&item.artwork, plan, sizes);
    let image = view_artwork(
        &item.artwork,
        image_layout,
        priority,
        Length::Fixed(image_layout.width),
        Length::Fixed(image_layout.height),
    );

    let title_style = plan.typography.rail_title;
    let subtitle_style = plan.typography.rail_subtitle;
    let content = Column::new()
        .spacing(sizes.spacing.xs)
        .width(Length::Fixed(plan.rail.card_width))
        .push(image)
        .push(styled_text(
            item.title.clone(),
            title_style,
            detail_text_color(title_style.color_intent),
            Length::Fill,
            true,
        ))
        .push(styled_text(
            item.subtitle.clone().unwrap_or_default(),
            subtitle_style,
            detail_text_color(subtitle_style.color_intent),
            Length::Fill,
            true,
        ));

    if let Some(message) = &item.on_press {
        button(content)
            .padding(0)
            .style(detail_rail_card_button_style())
            .on_press(message.clone())
            .into()
    } else {
        container(content).into()
    }
}

fn view_notice(
    notice: &DetailNotice,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Element<'static, UiMessage> {
    let title_style = plan.typography.notice_title;
    let body_style = plan.typography.notice_body;
    let content = Column::new()
        .spacing(sizes.spacing.xs)
        .push(styled_text(
            notice.title.clone(),
            title_style,
            tone_text_color(notice.tone),
            Length::Fill,
            true,
        ))
        .push(styled_text(
            notice.message.clone(),
            body_style,
            detail_text_color(body_style.color_intent),
            Length::Fill,
            true,
        ));

    container(content)
        .width(Length::Fill)
        .padding(sizes.spacing.md)
        .style(detail_panel_style(notice.tone))
        .into()
}

/// Compatibility glue for legacy detail callers that have not migrated to the
/// Theater Plate foreground stage. New detail stage renderers should use the
/// semantic shelf/band/ribbon functions above instead of this generic panel.
fn view_panel_compat(
    title: &str,
    body: Element<'static, UiMessage>,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Element<'static, UiMessage> {
    view_panel_compat_with_height(title, body, plan, sizes, None)
}

fn view_panel_compat_with_height(
    title: &str,
    body: Element<'static, UiMessage>,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
    matched_panel_height: Option<f32>,
) -> Element<'static, UiMessage> {
    let title_style = plan.typography.section_title;
    let content = Column::new()
        .spacing(sizes.spacing.sm)
        .push(styled_text(
            title.to_string(),
            title_style,
            detail_text_color(title_style.color_intent),
            Length::Fill,
            true,
        ))
        .push(body);

    container(content)
        .width(Length::Fill)
        .height(
            matched_panel_height
                .map(Length::Fixed)
                .unwrap_or(Length::Shrink),
        )
        .padding(sizes.spacing.md)
        .clip(true)
        .style(detail_panel_style(DetailTone::Neutral))
        .into()
}

fn view_artwork(
    artwork: &DetailArtwork,
    layout: DetailArtLayout,
    priority: Priority,
    width: Length,
    height: Length,
) -> Element<'static, UiMessage> {
    match artwork {
        DetailArtwork::Poster {
            media_uuid,
            image_id,
            placeholder,
            request_size,
            theme_color,
            animation,
            face,
            rotation_y,
            ..
        } => {
            let mut image = image_for(*media_uuid)
                .iid(*image_id)
                .skip_request(image_id.is_none())
                .request_size(*request_size)
                .display_size(layout.width, layout.height)
                .radius(layout.corner_radius)
                .priority(priority)
                .placeholder(*placeholder)
                .tight_bounds();

            if let Some(color) = theme_color {
                image = image.theme_color(*color);
            }
            if let Some(animation) = animation {
                image = image.animation_behavior(*animation);
            } else {
                image = image.no_animation();
            }
            if let Some(face) = face {
                image = image.face(*face);
            }
            if let Some(rotation_y) = rotation_y {
                image = image.rotation_y(*rotation_y);
            }

            image.into()
        }
        DetailArtwork::Still {
            media_uuid,
            image_id,
            ..
        } => image_for(*media_uuid)
            .iid(*image_id)
            .skip_request(image_id.is_none())
            .request_size(ImageSize::thumbnail())
            .display_size(layout.width, layout.height)
            .radius(layout.corner_radius)
            .priority(priority)
            .placeholder(Icon::Clapperboard)
            .tight_bounds()
            .no_animation()
            .into(),
        DetailArtwork::Profile {
            media_uuid,
            image_id,
            ..
        } => image_for(*media_uuid)
            .iid(*image_id)
            .skip_request(image_id.is_none())
            .request_size(ImageSize::profile())
            .display_size(layout.width, layout.height)
            .radius(layout.corner_radius)
            .priority(priority)
            .placeholder(Icon::User)
            .tight_bounds()
            .no_animation()
            .into(),
        DetailArtwork::None { label } => container(
            text(label.clone())
                .size(14)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
        )
        .width(width)
        .height(height)
        .align_x(iced::alignment::Horizontal::Center)
        .align_y(iced::alignment::Vertical::Center)
        .style(detail_panel_style(DetailTone::Muted))
        .into(),
    }
}

fn registered_relationship_rail_row(
    section: &DetailRelationshipRail,
    carousel_state: &VirtualCarouselState,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Row<'static, UiMessage> {
    let item_width = carousel_state.item_width.max(plan.rail.card_width);
    let stride = (item_width + carousel_state.item_spacing).max(1.0);
    let fallback_end = section.items.len().min(
        carousel_state
            .items_per_page
            .saturating_add(carousel_state.overscan_after)
            .max(1),
    );
    let visible_range = if carousel_state.visible_range.is_empty()
        && !section.items.is_empty()
    {
        0..fallback_end
    } else {
        carousel_state.visible_range.clone()
    };

    let mut item_row = Row::new().spacing(0);

    if visible_range.start > 0 {
        item_row = item_row.push(
            Space::new()
                .width(Length::Fixed(visible_range.start as f32 * stride)),
        );
    }

    let mut first_item = true;
    for idx in visible_range.clone() {
        if idx < section.items.len() {
            if !first_item {
                item_row = item_row.push(
                    Space::new()
                        .width(Length::Fixed(carousel_state.item_spacing)),
                );
            }
            item_row = item_row.push(
                container(view_rail_item(
                    &section.items[idx],
                    plan,
                    sizes,
                    Priority::Visible,
                ))
                .width(Length::Fixed(item_width))
                .align_x(iced::alignment::Horizontal::Center),
            );
            first_item = false;
        }
    }

    if visible_range.end < section.items.len() {
        let remaining = section.items.len() - visible_range.end;
        item_row = item_row
            .push(Space::new().width(Length::Fixed(remaining as f32 * stride)));
    }

    item_row
}

fn registered_horizontal_scroller(
    row: Row<'static, UiMessage>,
    height: f32,
    key: CarouselKey,
    carousel_state: &VirtualCarouselState,
) -> Element<'static, UiMessage> {
    let key_for_scroll = key.clone();
    let key_for_enter = key.clone();
    let key_for_exit = key;

    let scroll = scrollable(row)
        .id(carousel_state.scrollable_id.clone())
        .direction(scrollable::Direction::Horizontal(
            scrollable::Scrollbar::default().scroller_width(4).margin(2),
        ))
        .on_scroll(move |viewport| {
            UiMessage::VirtualCarousel(VirtualCarouselMessage::ViewportChanged(
                key_for_scroll.clone(),
                viewport,
            ))
        })
        .width(Length::Fill)
        .height(Length::Fixed(height));
    let scroll = container(scroll)
        .width(Length::Fill)
        .height(Length::Fixed(height))
        .clip(true);

    mouse_area(scroll)
        .on_enter(UiMessage::VirtualCarousel(
            VirtualCarouselMessage::FocusKey(key_for_enter),
        ))
        .on_exit(UiMessage::VirtualCarousel(VirtualCarouselMessage::BlurKey(
            key_for_exit,
        )))
        .into()
}

fn horizontal_scroller(
    row: Row<'static, UiMessage>,
    height: f32,
) -> Element<'static, UiMessage> {
    let scroll = scrollable(row)
        .direction(scrollable::Direction::Horizontal(
            scrollable::Scrollbar::default().scroller_width(4).margin(2),
        ))
        .width(Length::Fill)
        .height(Length::Fixed(height));

    container(scroll)
        .width(Length::Fill)
        .height(Length::Fixed(height))
        .clip(true)
        .into()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MetadataRenderGroups {
    inline_labels: Vec<String>,
    chips: Vec<DetailMetadataPill>,
}

fn metadata_render_groups(
    metadata: &[DetailMetadataPill],
) -> MetadataRenderGroups {
    let mut inline_labels = Vec::new();
    let mut chips = Vec::new();

    for item in metadata {
        let label = item.label.trim();
        if label.is_empty() {
            continue;
        }

        if item.renders_as_chip() {
            chips.push(item.clone());
        } else {
            inline_labels.push(label.to_string());
        }
    }

    MetadataRenderGroups {
        inline_labels,
        chips,
    }
}

fn styled_text(
    content: String,
    style: DetailTextStyle,
    color: Color,
    width: Length,
    bounded: bool,
) -> Element<'static, UiMessage> {
    let max_lines = bounded.then(|| style.max_lines()).flatten();
    let wrapping = text_wrapping(style.overflow, max_lines.is_some());
    let mut label = text(content)
        .size(style.size)
        .line_height(style.line_height)
        .width(width)
        .wrapping(wrapping)
        .align_x(text_alignment(style.alignment))
        .color(color);

    if let Some(lines) = max_lines {
        let height = style.line_height_px() * f32::from(lines);
        label = label.height(Length::Fixed(height));
        container(label)
            .width(width)
            .height(Length::Fixed(height))
            .clip(true)
            .into()
    } else {
        label.into()
    }
}

fn text_budget_height(style: DetailTextStyle) -> f32 {
    style.line_height_px() * f32::from(style.max_lines().unwrap_or(1))
}

fn text_wrapping(overflow: DetailTextOverflow, bounded: bool) -> Wrapping {
    match overflow {
        DetailTextOverflow::SingleLineEllipsis
        | DetailTextOverflow::HorizontalScroll => Wrapping::None,
        DetailTextOverflow::Wrap => Wrapping::WordOrGlyph,
        DetailTextOverflow::MultiLine { .. } if bounded => {
            Wrapping::WordOrGlyph
        }
        DetailTextOverflow::MultiLine { .. } => Wrapping::Word,
    }
}

fn text_alignment(
    alignment: DetailTextAlignment,
) -> iced::widget::text::Alignment {
    match alignment {
        DetailTextAlignment::Start => iced::widget::text::Alignment::Left,
        DetailTextAlignment::Center => iced::widget::text::Alignment::Center,
        DetailTextAlignment::End => iced::widget::text::Alignment::Right,
    }
}

fn horizontal_alignment(
    alignment: DetailTextAlignment,
) -> alignment::Horizontal {
    match alignment {
        DetailTextAlignment::Start => alignment::Horizontal::Left,
        DetailTextAlignment::Center => alignment::Horizontal::Center,
        DetailTextAlignment::End => alignment::Horizontal::Right,
    }
}

fn detail_text_color(intent: DetailColorIntent) -> Color {
    match intent {
        DetailColorIntent::Primary => theme::MediaServerTheme::TEXT_PRIMARY,
        DetailColorIntent::Secondary => theme::MediaServerTheme::TEXT_SECONDARY,
        DetailColorIntent::Subdued => theme::MediaServerTheme::TEXT_SUBDUED,
        DetailColorIntent::Dimmed => theme::MediaServerTheme::TEXT_DIMMED,
        DetailColorIntent::Accent => theme::MediaServerTheme::ACCENT,
        DetailColorIntent::Success => theme::MediaServerTheme::SUCCESS,
        DetailColorIntent::Warning => theme::MediaServerTheme::WARNING,
        DetailColorIntent::Error => theme::MediaServerTheme::ERROR,
    }
}

fn tone_text_color(tone: DetailTone) -> Color {
    match tone {
        DetailTone::Neutral => theme::MediaServerTheme::TEXT_PRIMARY,
        DetailTone::Accent => theme::MediaServerTheme::ACCENT,
        DetailTone::Success => theme::MediaServerTheme::SUCCESS,
        DetailTone::Warning => theme::MediaServerTheme::WARNING,
        DetailTone::Danger => theme::MediaServerTheme::ERROR,
        DetailTone::Muted => theme::MediaServerTheme::TEXT_SECONDARY,
    }
}

fn detail_panel_style(
    tone: DetailTone,
) -> impl Fn(&Theme) -> container::Style + Clone {
    move |_| {
        let border_color = match tone {
            DetailTone::Accent => theme::MediaServerTheme::ACCENT,
            DetailTone::Success => theme::MediaServerTheme::SUCCESS,
            DetailTone::Warning => theme::MediaServerTheme::WARNING,
            DetailTone::Danger => theme::MediaServerTheme::ERROR,
            DetailTone::Muted | DetailTone::Neutral => {
                Color::from_rgba(1.0, 1.0, 1.0, 0.14)
            }
        };
        container::Style {
            text_color: Some(theme::MediaServerTheme::TEXT_PRIMARY),
            background: Some(Background::Color(Color::from_rgba(
                0.015, 0.014, 0.02, 0.58,
            ))),
            border: Border {
                color: border_color,
                width: 1.0,
                radius: 0.0.into(),
            },
            shadow: Shadow::default(),
            snap: false,
        }
    }
}

fn detail_rail_card_button_style()
-> impl Fn(&Theme, button::Status) -> button::Style + Clone {
    move |_, status| {
        let hovered =
            matches!(status, button::Status::Hovered | button::Status::Pressed);

        button::Style {
            text_color: theme::MediaServerTheme::TEXT_PRIMARY,
            background: hovered.then_some(Background::Color(Color::from_rgba(
                1.0, 1.0, 1.0, 0.06,
            ))),
            border: Border {
                color: if hovered {
                    Color::from_rgba(1.0, 1.0, 1.0, 0.18)
                } else {
                    Color::TRANSPARENT
                },
                width: if hovered { 1.0 } else { 0.0 },
                radius: 8.0.into(),
            },
            shadow: Shadow::default(),
            snap: false,
        }
    }
}

fn pill_style(tone: DetailTone) -> impl Fn(&Theme) -> container::Style + Clone {
    move |_| container::Style {
        text_color: Some(tone_text_color(tone)),
        background: Some(Background::Color(Color::from_rgba(
            1.0, 1.0, 1.0, 0.11,
        ))),
        border: Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.18),
            width: 1.0,
            radius: 3.0.into(),
        },
        shadow: Shadow::default(),
        snap: false,
    }
}

fn detail_action_button_style_on_surface(
    role: DetailActionRole,
    disabled: bool,
    tokens: DetailForegroundSurfaceTokens,
) -> impl Fn(&Theme, button::Status) -> button::Style + Clone + 'static {
    move |_, status| {
        let hovered =
            matches!(status, button::Status::Hovered | button::Status::Pressed);
        let accent = match role {
            DetailActionRole::Primary | DetailActionRole::Toggle => {
                theme::MediaServerTheme::ACCENT
            }
            DetailActionRole::Destructive => theme::MediaServerTheme::ERROR,
            DetailActionRole::Back | DetailActionRole::Secondary => tokens.edge,
        };
        let background = if disabled {
            Color::from_rgba(0.08, 0.08, 0.09, 0.48)
        } else if role == DetailActionRole::Primary {
            if hovered {
                theme::MediaServerTheme::ACCENT_HOVER
            } else {
                Color::from_rgba(
                    theme::MediaServerTheme::ACCENT.r,
                    theme::MediaServerTheme::ACCENT.g,
                    theme::MediaServerTheme::ACCENT.b,
                    0.82,
                )
            }
        } else if hovered {
            Color::from_rgba(1.0, 1.0, 1.0, 0.18)
        } else {
            Color::from_rgba(1.0, 1.0, 1.0, 0.07 + tokens.intensity * 0.05)
        };

        button::Style {
            text_color: if disabled {
                theme::MediaServerTheme::TEXT_DIMMED
            } else {
                theme::MediaServerTheme::TEXT_PRIMARY
            },
            background: Some(Background::Color(background)),
            border: Border {
                color: if disabled {
                    Color::from_rgba(1.0, 1.0, 1.0, 0.06)
                } else {
                    accent
                },
                width: if disabled { 0.0 } else { 1.0 },
                radius: tokens.radius.max(2.0).into(),
            },
            shadow: if role == DetailActionRole::Primary && !disabled {
                Shadow {
                    color: theme::MediaServerTheme::ACCENT_GLOW,
                    offset: Vector::new(0.0, 0.0),
                    blur_radius: if hovered { 22.0 } else { 14.0 },
                }
            } else {
                Shadow::default()
            },
            snap: false,
        }
    }
}

fn detail_action_button_style(
    role: DetailActionRole,
    disabled: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style + Clone {
    move |_, status| {
        let hovered =
            matches!(status, button::Status::Hovered | button::Status::Pressed);
        let accent = match role {
            DetailActionRole::Primary | DetailActionRole::Toggle => {
                theme::MediaServerTheme::ACCENT
            }
            DetailActionRole::Destructive => theme::MediaServerTheme::ERROR,
            DetailActionRole::Back | DetailActionRole::Secondary => {
                theme::MediaServerTheme::BORDER_COLOR
            }
        };
        let background = if disabled {
            Color::from_rgba(0.10, 0.10, 0.10, 0.55)
        } else if role == DetailActionRole::Primary {
            if hovered {
                theme::MediaServerTheme::ACCENT_HOVER
            } else {
                accent
            }
        } else if hovered {
            Color::from_rgba(1.0, 1.0, 1.0, 0.16)
        } else {
            Color::from_rgba(1.0, 1.0, 1.0, 0.08)
        };

        button::Style {
            text_color: if disabled {
                theme::MediaServerTheme::TEXT_DIMMED
            } else {
                theme::MediaServerTheme::TEXT_PRIMARY
            },
            background: Some(Background::Color(background)),
            border: Border {
                color: if disabled {
                    Color::from_rgba(1.0, 1.0, 1.0, 0.08)
                } else {
                    accent
                },
                width: 1.0,
                radius: 4.0.into(),
            },
            shadow: if role == DetailActionRole::Primary && !disabled {
                Shadow {
                    color: theme::MediaServerTheme::ACCENT_GLOW,
                    offset: Vector::new(0.0, 0.0),
                    blur_radius: if hovered { 18.0 } else { 10.0 },
                }
            } else {
                Shadow::default()
            },
            snap: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domains::ui::views::detail::{
            DetailActionMenuItem, DetailInterfaceMode, DetailLayoutInput,
            solve_detail_layout,
        },
        infra::{
            constants::layout::{calculations::ScaledLayout, grid},
            design_tokens::{ScalingContext, SizeProvider},
        },
    };

    fn test_sizes() -> SizeProvider {
        SizeProvider::new(ScalingContext::default())
    }

    fn layout_plan() -> DetailLayoutPlan {
        let sizes = test_sizes();
        let layout = ScaledLayout::new(sizes.scale, grid::EFFECTIVE_SPACING);
        solve_detail_layout(DetailLayoutInput::from_runtime(
            1_920.0,
            1_080.0,
            50.0,
            DetailInterfaceMode::Desktop,
            &sizes,
            &layout,
        ))
    }

    fn rail_section(count: usize) -> DetailSection {
        DetailSection::RelationshipRail(DetailRelationshipRail {
            id: "rail".to_string(),
            carousel_key: None,
            title: "Related".to_string(),
            empty_message: Some("No related titles".to_string()),
            items: (0..count)
                .map(|index| DetailRailItem {
                    id: format!("item-{index}"),
                    title: format!("Title {index}"),
                    subtitle: None,
                    artwork: DetailArtwork::None {
                        label: "No artwork".to_string(),
                    },
                    on_press: None,
                })
                .collect(),
        })
    }

    #[test]
    fn overview_and_facts_share_side_by_side_panel_height() {
        let plan = layout_plan();
        let sizes = test_sizes();
        let overview = DetailSection::Overview(DetailOverviewSection {
            title: "Synopsis".to_string(),
            body: "A long synopsis establishes the review pressure for the side-by-side detail grid so the details panel does not collapse shorter than the copy block beside it.".to_string(),
        });
        let facts = DetailSection::Facts(DetailFactPanel {
            title: "Details".to_string(),
            facts: vec![DetailFact::neutral("Runtime", "42 min")],
        });

        let matched_height = matched_overview_fact_panel_height(
            &[&overview, &facts],
            &plan,
            &sizes,
        )
        .expect("overview and facts should height-match");
        let expected_height = overview_panel_height(
            match &overview {
                DetailSection::Overview(section) => section,
                _ => unreachable!(),
            },
            &plan,
            &sizes,
            2,
        )
        .max(fact_panel_height(
            match &facts {
                DetailSection::Facts(section) => section,
                _ => unreachable!(),
            },
            &plan,
            &sizes,
        ))
        .max(plan.section_grid.panel_min_height);

        assert!((matched_height - expected_height).abs() < f32::EPSILON);

        let matched_stage_height = matched_stage_overview_fact_panel_height(
            &[&overview, &facts],
            &plan,
            &sizes,
        )
        .expect("stage overview and facts should height-match");
        let expected_stage_height = stage_overview_panel_height(
            match &overview {
                DetailSection::Overview(section) => section,
                _ => unreachable!(),
            },
            &plan,
            &sizes,
            2,
        )
        .max(stage_fact_panel_height(
            match &facts {
                DetailSection::Facts(section) => section,
                _ => unreachable!(),
            },
            &plan,
            &sizes,
        ))
        .max(plan.section_grid.panel_min_height);

        assert!(
            (matched_stage_height - expected_stage_height).abs() < f32::EPSILON
        );
        assert!(
            matched_overview_fact_panel_height(&[&overview], &plan, &sizes)
                .is_none()
        );
        assert!(
            matched_stage_overview_fact_panel_height(
                &[&overview],
                &plan,
                &sizes,
            )
            .is_none()
        );
    }

    #[test]
    fn foreground_surface_tokens_are_semantic_not_generic_panels() {
        let plan = layout_plan();
        let projection = detail_foreground_surface_tokens(
            &plan,
            DetailForegroundSurface::ProjectionShelf,
            DetailTone::Neutral,
        );
        let rail = detail_foreground_surface_tokens(
            &plan,
            DetailForegroundSurface::RailBand,
            DetailTone::Neutral,
        );
        let fact = detail_foreground_surface_tokens(
            &plan,
            DetailForegroundSurface::FactRibbon,
            DetailTone::Accent,
        );
        let metadata = detail_foreground_surface_tokens(
            &plan,
            DetailForegroundSurface::MetadataRibbon,
            DetailTone::Neutral,
        );
        let notice = detail_foreground_surface_tokens(
            &plan,
            DetailForegroundSurface::NoticeSlab,
            DetailTone::Warning,
        );

        assert_eq!(projection.border_width, 0.0);
        assert_eq!(rail.border_width, 0.0);
        assert_eq!(metadata.border_width, 0.0);
        assert!(projection.background.a > metadata.background.a);
        assert_ne!(projection.background, rail.background);
        assert_ne!(fact.background, rail.background);
        assert!(notice.border_width > projection.border_width);
        assert!(notice.background.a > 0.0);
    }

    #[test]
    fn stage_section_render_state_selects_non_empty_and_empty_surfaces() {
        let facts = DetailSection::Facts(DetailFactPanel {
            title: "Facts".to_string(),
            facts: vec![DetailFact::neutral("Runtime", "42 min")],
        });
        let empty_facts = DetailSection::Facts(DetailFactPanel {
            title: "Facts".to_string(),
            facts: Vec::new(),
        });
        let cast = DetailSection::Cast(DetailCastSection {
            title: "Cast".to_string(),
            members: vec![DetailCastMember {
                name: "Performer".to_string(),
                role: Some("Lead".to_string()),
                artwork: DetailArtwork::None {
                    label: "No profile".to_string(),
                },
            }],
            empty_message: None,
        });
        let technical = DetailSection::Technical(DetailTechnicalSection {
            title: "Technical".to_string(),
            items: vec![DetailTechnicalItem {
                label: "Codec".to_string(),
                value: "AV1".to_string(),
                icon: None,
                tone: DetailTone::Neutral,
            }],
            empty_message: None,
        });
        let notice = DetailSection::Notice(DetailNotice {
            title: "Playback unavailable".to_string(),
            message: "Try again after refresh.".to_string(),
            tone: DetailTone::Warning,
        });
        let explicit_empty = DetailSection::Empty(DetailEmptyState {
            title: "No rows".to_string(),
            message: "Refresh the library.".to_string(),
            icon: None,
        });

        assert_eq!(
            detail_stage_section_render_state(&facts).surface,
            DetailForegroundSurface::FactRibbon
        );
        assert_eq!(
            detail_stage_section_render_state(&empty_facts).surface,
            DetailForegroundSurface::EmptyState
        );
        assert_eq!(
            detail_stage_section_render_state(&cast).surface,
            DetailForegroundSurface::CastBand
        );
        assert_eq!(
            detail_stage_section_render_state(&technical).surface,
            DetailForegroundSurface::TechnicalRibbon
        );
        assert_eq!(
            detail_stage_section_render_state(&rail_section(2)).surface,
            DetailForegroundSurface::RailBand
        );
        assert_eq!(
            detail_stage_section_render_state(&rail_section(0)).surface,
            DetailForegroundSurface::EmptyState
        );
        assert_eq!(
            detail_stage_section_render_state(&notice).surface,
            DetailForegroundSurface::NoticeSlab
        );
        assert_eq!(
            detail_stage_section_render_state(&explicit_empty).surface,
            DetailForegroundSurface::EmptyState
        );
        assert!(detail_stage_section_render_state(&rail_section(1)).full_width);
    }

    #[test]
    fn control_shelf_action_modes_preserve_disabled_and_menu_behavior() {
        let pressable = DetailAction::primary("play", "Play", UiMessage::NoOp);
        let disabled = DetailAction::disabled("play", "Play");
        let menu = DetailAction::menu(
            "more",
            "More",
            vec![DetailActionMenuItem::new("Open", UiMessage::NoOp)],
        );

        assert_eq!(
            detail_action_surface_mode(&pressable),
            DetailActionSurfaceMode::Pressable
        );
        assert_eq!(
            detail_action_surface_mode(&disabled),
            DetailActionSurfaceMode::Disabled
        );
        assert_eq!(
            detail_action_surface_mode(&menu),
            DetailActionSurfaceMode::Menu
        );
    }

    #[test]
    fn metadata_render_groups_keep_neutral_values_inline() {
        let groups = metadata_render_groups(&[
            DetailMetadataPill::neutral("2024"),
            DetailMetadataPill::neutral("1h 42m"),
            DetailMetadataPill::playback_state(
                "33% watched",
                DetailTone::Accent,
            ),
            DetailMetadataPill::rating("★ 8.1"),
        ]);

        assert_eq!(groups.inline_labels, vec!["2024", "1h 42m"]);
        assert_eq!(groups.chips.len(), 2);
        assert_eq!(
            groups.chips[0].kind,
            super::super::DetailMetadataKind::PlaybackState
        );
        assert_eq!(
            groups.chips[1].kind,
            super::super::DetailMetadataKind::AudienceRating
        );
    }

    #[test]
    fn text_wrapping_honors_semantic_overflow_budgets() {
        assert_eq!(
            text_wrapping(DetailTextOverflow::SingleLineEllipsis, true),
            Wrapping::None
        );
        assert_eq!(
            text_wrapping(DetailTextOverflow::MultiLine { max_lines: 2 }, true),
            Wrapping::WordOrGlyph
        );
        assert_eq!(
            text_wrapping(DetailTextOverflow::HorizontalScroll, false),
            Wrapping::None
        );
    }
}
