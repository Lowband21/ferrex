use crate::{
    common::ui_utils::icon_text_with_size,
    domains::ui::{messages::UiMessage, theme, widgets::image_for::image_for},
    infra::design_tokens::SizeProvider,
};

use super::{
    DetailAction, DetailActionRole, DetailArtLayout, DetailArtwork,
    DetailBackdrop, DetailBackdropControl, DetailBackdropScrim,
    DetailCastMember, DetailCastSection, DetailComposition, DetailEmptyState,
    DetailFact, DetailFactPanel, DetailLayoutPlan, DetailMetadataPill,
    DetailNotice, DetailOverviewSection, DetailPageModel, DetailRailItem,
    DetailRelationshipRail, DetailSection, DetailTechnicalItem,
    DetailTechnicalSection, DetailTone,
};
use ferrex_core::player_prelude::Priority;
use ferrex_model::ImageSize;
use iced::{
    Alignment, Background, Border, Color, Element, Length, Shadow, Theme,
    Vector,
    widget::{
        Column, Row, Space, Stack, button, column, container, row, scrollable,
        text,
    },
};
use lucide_icons::Icon;

/// Render a repository-free detail page model with a precomputed layout plan.
pub fn view_detail_page<'a>(
    model: &'a DetailPageModel,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Element<'a, UiMessage> {
    let mut body = Column::new()
        .spacing(plan.section_grid.gap)
        .padding([plan.page_padding_y, plan.page_padding_x])
        .width(Length::Fill)
        .max_width(plan.content_width);

    if let Some(empty) = model.empty_state.as_ref().filter(|_| model.is_empty())
    {
        body = body.push(view_empty_state(empty, sizes));
    } else {
        body = body.push(view_detail_hero(model, plan, sizes));
        body = body.push(view_sections(&model.sections, plan, sizes));
    }

    if !model.backdrop_controls.is_empty() {
        body = body.push(view_backdrop_controls(
            &model.backdrop_controls,
            plan,
            sizes,
        ));
    }

    let content =
        container(scrollable(body).width(Length::Fill).height(Length::Fill))
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(iced::alignment::Horizontal::Center);

    if let Some(backdrop) = &model.backdrop {
        Stack::new()
            .push(view_backdrop(backdrop, plan, sizes))
            .push(content)
            .into()
    } else {
        content.into()
    }
}

/// Render the hero block shared by desktop, compact, cinematic, and ten-foot
/// detail pages.
pub fn view_detail_hero<'a>(
    model: &'a DetailPageModel,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Element<'a, UiMessage> {
    let art = view_hero_art(&model.hero_art, plan, sizes);
    let summary = view_summary(model, plan, sizes);

    let hero: Element<'a, UiMessage> = match plan.composition {
        DetailComposition::CompactPortrait => column![art, summary]
            .spacing(plan.hero_gap)
            .align_x(Alignment::Center)
            .width(Length::Fill)
            .into(),
        _ => row![art, summary]
            .spacing(plan.hero_gap)
            .align_y(Alignment::Center)
            .width(Length::Fill)
            .into(),
    };

    container(hero)
        .padding(sizes.spacing.lg)
        .width(Length::Fill)
        .style(detail_panel_style(DetailTone::Neutral))
        .into()
}

pub fn view_hero_art<'a>(
    artwork: &'a DetailArtwork,
    plan: &DetailLayoutPlan,
    _sizes: &SizeProvider,
) -> Element<'a, UiMessage> {
    view_artwork(
        artwork,
        plan.hero_art,
        Priority::Visible,
        Length::Fixed(plan.hero_art.width),
        Length::Fixed(plan.hero_art.height),
    )
}

pub fn view_metadata_pills<'a>(
    metadata: &'a [DetailMetadataPill],
    sizes: &SizeProvider,
) -> Element<'a, UiMessage> {
    let mut pills = Row::new().spacing(sizes.spacing.xs);
    for pill in metadata {
        pills = pills.push(
            container(
                text(pill.label.clone())
                    .size(sizes.font.small)
                    .color(tone_text_color(pill.tone)),
            )
            .padding([sizes.spacing.xs, sizes.spacing.sm])
            .style(pill_style(pill.tone)),
        );
    }
    pills.into()
}

pub fn view_action_cluster<'a>(
    actions: &'a [DetailAction],
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Element<'a, UiMessage> {
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

pub fn view_sections<'a>(
    sections: &'a [DetailSection],
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Element<'a, UiMessage> {
    if sections.is_empty() {
        return Space::new().into();
    }

    let columns = plan.section_grid.columns.max(1);
    let mut outer = Column::new().spacing(plan.section_grid.gap);
    let mut current = Row::new()
        .spacing(plan.section_grid.gap)
        .align_y(Alignment::Start);
    let mut count = 0usize;

    for section in sections {
        current = current.push(view_section(section, plan, sizes));
        count += 1;
        if count == columns {
            let completed = std::mem::replace(
                &mut current,
                Row::new()
                    .spacing(plan.section_grid.gap)
                    .align_y(Alignment::Start),
            );
            outer = outer.push(completed);
            count = 0;
        }
    }

    if count > 0 {
        outer = outer.push(current);
    }

    outer.into()
}

pub fn view_section<'a>(
    section: &'a DetailSection,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Element<'a, UiMessage> {
    match section {
        DetailSection::Overview(section) => {
            view_overview_section(section, plan, sizes)
        }
        DetailSection::Facts(section) => view_fact_panel(section, plan, sizes),
        DetailSection::Cast(section) => view_cast_section(section, plan, sizes),
        DetailSection::Technical(section) => {
            view_technical_section(section, plan, sizes)
        }
        DetailSection::RelationshipRail(section) => {
            view_relationship_rail(section, plan, sizes)
        }
        DetailSection::Empty(section) => view_empty_state(section, sizes),
        DetailSection::Notice(section) => view_notice(section, sizes),
    }
}

pub fn view_overview_section<'a>(
    section: &'a DetailOverviewSection,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Element<'a, UiMessage> {
    view_panel(
        &section.title,
        text(section.body.clone())
            .size(sizes.font.body)
            .color(theme::MediaServerTheme::TEXT_PRIMARY)
            .width(Length::Fill)
            .into(),
        plan,
        sizes,
    )
}

pub fn view_fact_panel<'a>(
    section: &'a DetailFactPanel,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Element<'a, UiMessage> {
    let mut facts = Column::new().spacing(sizes.spacing.sm);
    for fact in &section.facts {
        facts = facts.push(view_fact(fact, sizes));
    }

    view_panel(&section.title, facts.into(), plan, sizes)
}

pub fn view_cast_section<'a>(
    section: &'a DetailCastSection,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Element<'a, UiMessage> {
    if section.members.is_empty() {
        return view_panel(
            &section.title,
            text(section.empty_message.clone().unwrap_or_else(|| {
                "No cast information is available.".to_string()
            }))
            .size(sizes.font.caption)
            .color(theme::MediaServerTheme::TEXT_SECONDARY)
            .into(),
            plan,
            sizes,
        );
    }

    let mut row = Row::new().spacing(plan.rail.gap);
    for member in &section.members {
        row = row.push(view_cast_member(member, plan, sizes));
    }

    view_panel(
        &section.title,
        horizontal_scroller(row, plan.rail.card_height + sizes.spacing.xl),
        plan,
        sizes,
    )
}

pub fn view_technical_section<'a>(
    section: &'a DetailTechnicalSection,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Element<'a, UiMessage> {
    if section.items.is_empty() {
        return view_panel(
            &section.title,
            text(section.empty_message.clone().unwrap_or_else(|| {
                "No technical metadata is available.".to_string()
            }))
            .size(sizes.font.caption)
            .color(theme::MediaServerTheme::TEXT_SECONDARY)
            .into(),
            plan,
            sizes,
        );
    }

    let mut row = Row::new().spacing(sizes.spacing.sm);
    for item in &section.items {
        row = row.push(view_technical_item(item, sizes));
    }

    view_panel(&section.title, row.into(), plan, sizes)
}

pub fn view_relationship_rail<'a>(
    section: &'a DetailRelationshipRail,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Element<'a, UiMessage> {
    if section.items.is_empty() {
        return view_panel(
            &section.title,
            text(section.empty_message.clone().unwrap_or_else(|| {
                "No related titles are available.".to_string()
            }))
            .size(sizes.font.caption)
            .color(theme::MediaServerTheme::TEXT_SECONDARY)
            .into(),
            plan,
            sizes,
        );
    }

    let mut row = Row::new().spacing(plan.rail.gap);
    for item in &section.items {
        row = row.push(view_rail_item(item, plan, sizes));
    }

    view_panel(
        &section.title,
        horizontal_scroller(row, plan.rail.card_height + sizes.spacing.xl),
        plan,
        sizes,
    )
}

pub fn view_empty_state<'a>(
    empty: &'a DetailEmptyState,
    sizes: &SizeProvider,
) -> Element<'a, UiMessage> {
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

pub fn view_backdrop_controls<'a>(
    controls: &'a [DetailBackdropControl],
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Element<'a, UiMessage> {
    let mut row = Row::new()
        .spacing(sizes.spacing.xs)
        .align_y(Alignment::Center)
        .width(Length::Fill);

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

fn view_summary<'a>(
    model: &'a DetailPageModel,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Element<'a, UiMessage> {
    let mut summary =
        Column::new().spacing(sizes.spacing.sm).width(Length::Fill);

    if let Some(eyebrow) = &model.eyebrow {
        summary = summary.push(
            text(eyebrow.clone())
                .size(sizes.font.caption)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
        );
    }

    summary = summary.push(
        text(model.title.clone())
            .size(match plan.composition {
                DetailComposition::TenFoot => sizes.font.display * 1.45,
                DetailComposition::CinematicWide => sizes.font.display * 1.25,
                DetailComposition::CompactPortrait => sizes.font.title,
                _ => sizes.font.display,
            })
            .color(theme::MediaServerTheme::TEXT_PRIMARY),
    );

    if let Some(subtitle) = &model.subtitle {
        summary = summary.push(
            text(subtitle.clone())
                .size(sizes.font.subtitle)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
        );
    }

    if !model.metadata.is_empty() {
        summary = summary.push(view_metadata_pills(&model.metadata, sizes));
    }

    if !model.actions.is_empty() {
        summary =
            summary.push(view_action_cluster(&model.actions, plan, sizes));
    }

    summary.into()
}

fn view_action_button<'a>(
    action: &'a DetailAction,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Element<'a, UiMessage> {
    let disabled = action.on_press.is_none();
    let mut label_row = Row::new()
        .spacing(sizes.spacing.xs)
        .align_y(Alignment::Center);
    if let Some(icon) = action.icon {
        label_row = label_row.push(icon_text_with_size(icon, sizes.icon.sm));
    }
    label_row =
        label_row.push(text(action.label.clone()).size(sizes.font.body));

    let mut content = Column::new()
        .spacing(2.0)
        .align_x(Alignment::Center)
        .push(label_row);
    if let Some(subtitle) = &action.subtitle {
        content = content.push(
            text(subtitle.clone())
                .size(sizes.font.small)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
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

fn view_fact<'a>(
    fact: &'a DetailFact,
    sizes: &SizeProvider,
) -> Element<'a, UiMessage> {
    row![
        text(fact.label.clone())
            .size(sizes.font.small)
            .color(theme::MediaServerTheme::TEXT_SECONDARY)
            .width(Length::FillPortion(1)),
        text(fact.value.clone())
            .size(sizes.font.caption)
            .color(tone_text_color(fact.tone))
            .width(Length::FillPortion(2)),
    ]
    .spacing(sizes.spacing.sm)
    .align_y(Alignment::Center)
    .into()
}

fn view_cast_member<'a>(
    member: &'a DetailCastMember,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Element<'a, UiMessage> {
    let image_width = (plan.rail.card_width * 0.68).max(72.0);
    let image_height = image_width * 1.5;
    let image = view_artwork(
        &member.artwork,
        DetailArtLayout {
            width: image_width,
            height: image_height,
            corner_radius: sizes.scale(10.0),
            aspect: super::DetailArtAspect::Poster,
        },
        Priority::Preload,
        Length::Fixed(image_width),
        Length::Fixed(image_height),
    );

    let mut content = Column::new()
        .spacing(sizes.spacing.xs)
        .align_x(Alignment::Center)
        .width(Length::Fixed(plan.rail.card_width))
        .push(image)
        .push(
            text(member.name.clone())
                .size(sizes.font.small)
                .color(theme::MediaServerTheme::TEXT_PRIMARY)
                .width(Length::Fill)
                .center(),
        );

    if let Some(role) = &member.role {
        content = content.push(
            text(role.clone())
                .size(sizes.font.micro)
                .color(theme::MediaServerTheme::TEXT_SECONDARY)
                .width(Length::Fill)
                .center(),
        );
    }

    content.into()
}

fn view_technical_item<'a>(
    item: &'a DetailTechnicalItem,
    sizes: &SizeProvider,
) -> Element<'a, UiMessage> {
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

fn view_rail_item<'a>(
    item: &'a DetailRailItem,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Element<'a, UiMessage> {
    let image_height = plan.rail.card_width * 9.0 / 16.0;
    let image = view_artwork(
        &item.artwork,
        DetailArtLayout {
            width: plan.rail.card_width,
            height: image_height,
            corner_radius: sizes.scale(10.0),
            aspect: super::DetailArtAspect::Still,
        },
        Priority::Preload,
        Length::Fixed(plan.rail.card_width),
        Length::Fixed(image_height),
    );

    let content = Column::new()
        .spacing(sizes.spacing.xs)
        .width(Length::Fixed(plan.rail.card_width))
        .push(image)
        .push(
            text(item.title.clone())
                .size(sizes.font.caption)
                .color(theme::MediaServerTheme::TEXT_PRIMARY)
                .width(Length::Fill),
        )
        .push(
            text(item.subtitle.clone().unwrap_or_default())
                .size(sizes.font.small)
                .color(theme::MediaServerTheme::TEXT_SECONDARY)
                .width(Length::Fill),
        );

    if let Some(message) = &item.on_press {
        button(content)
            .padding(0)
            .style(detail_action_button_style(
                DetailActionRole::Secondary,
                false,
            ))
            .on_press(message.clone())
            .into()
    } else {
        container(content).into()
    }
}

fn view_notice<'a>(
    notice: &'a DetailNotice,
    sizes: &SizeProvider,
) -> Element<'a, UiMessage> {
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

    container(content)
        .width(Length::Fill)
        .padding(sizes.spacing.md)
        .style(detail_panel_style(notice.tone))
        .into()
}

fn view_panel<'a>(
    title: &str,
    body: Element<'a, UiMessage>,
    _plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Element<'a, UiMessage> {
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
        .height(Length::Shrink)
        .padding(sizes.spacing.md)
        .style(detail_panel_style(DetailTone::Neutral))
        .into()
}

fn view_backdrop<'a>(
    backdrop: &'a DetailBackdrop,
    plan: &DetailLayoutPlan,
    _sizes: &SizeProvider,
) -> Element<'a, UiMessage> {
    let image = view_artwork(
        &backdrop.artwork,
        DetailArtLayout {
            width: plan.viewport_width,
            height: plan.backdrop.height,
            corner_radius: 0.0,
            aspect: super::DetailArtAspect::Still,
        },
        Priority::Visible,
        Length::Fixed(plan.viewport_width),
        Length::Fixed(plan.backdrop.height),
    );

    let scrim_opacity = match backdrop.scrim {
        DetailBackdropScrim::None => 0.0,
        DetailBackdropScrim::Light => plan.backdrop.scrim_opacity * 0.65,
        DetailBackdropScrim::Heavy => plan.backdrop.scrim_opacity,
    };

    Stack::new()
        .push(image)
        .push(
            container(Space::new())
                .width(Length::Fill)
                .height(Length::Fixed(plan.backdrop.height))
                .style(backdrop_scrim_style(scrim_opacity)),
        )
        .into()
}

fn view_artwork<'a>(
    artwork: &'a DetailArtwork,
    layout: DetailArtLayout,
    priority: Priority,
    width: Length,
    height: Length,
) -> Element<'a, UiMessage> {
    match artwork {
        DetailArtwork::Poster {
            media_uuid,
            image_id,
            placeholder,
            ..
        } => image_for(*media_uuid)
            .iid(*image_id)
            .skip_request(image_id.is_none())
            .request_size(ImageSize::poster_large())
            .display_size(layout.width, layout.height)
            .radius(layout.corner_radius)
            .priority(priority)
            .placeholder(*placeholder)
            .tight_bounds()
            .no_animation()
            .into(),
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
        DetailArtwork::Backdrop {
            media_uuid,
            image_id,
            ..
        } => image_for(*media_uuid)
            .iid(*image_id)
            .skip_request(image_id.is_none())
            .request_size(ImageSize::backdrop())
            .display_size(layout.width, layout.height)
            .radius(layout.corner_radius)
            .priority(priority)
            .placeholder(Icon::Image)
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

fn horizontal_scroller<'a>(
    row: Row<'a, UiMessage>,
    height: f32,
) -> Element<'a, UiMessage> {
    scrollable(row)
        .direction(scrollable::Direction::Horizontal(
            scrollable::Scrollbar::default().scroller_width(4).margin(2),
        ))
        .height(Length::Fixed(height))
        .into()
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
                Color::from_rgba(1.0, 1.0, 1.0, 0.12)
            }
        };
        container::Style {
            text_color: Some(theme::MediaServerTheme::TEXT_PRIMARY),
            background: Some(Background::Color(Color::from_rgba(
                0.06, 0.055, 0.075, 0.86,
            ))),
            border: Border {
                color: border_color,
                width: 1.0,
                radius: 18.0.into(),
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
            1.0, 1.0, 1.0, 0.09,
        ))),
        border: Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.14),
            width: 1.0,
            radius: 999.0.into(),
        },
        shadow: Shadow::default(),
        snap: false,
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
                radius: 14.0.into(),
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

fn backdrop_scrim_style(
    opacity: f32,
) -> impl Fn(&Theme) -> container::Style + Clone {
    move |_| container::Style {
        text_color: None,
        background: Some(Background::Color(Color::from_rgba(
            0.0,
            0.0,
            0.0,
            opacity.clamp(0.0, 1.0),
        ))),
        border: Border::default(),
        shadow: Shadow::default(),
        snap: false,
    }
}
