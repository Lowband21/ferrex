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
    infra::{design_tokens::SizeProvider, service_registry},
};

use super::{
    DetailAction, DetailActionRole, DetailArtLayout, DetailArtwork,
    DetailBackdropControl, DetailCastMember, DetailCastSection,
    DetailComposition, DetailEmptyState, DetailFact, DetailFactPanel,
    DetailLayoutPlan, DetailMetadataPill, DetailNotice, DetailOverviewSection,
    DetailPageModel, DetailRailItem, DetailRelationshipRail, DetailSection,
    DetailTechnicalItem, DetailTechnicalSection, DetailTone,
};
use ferrex_core::player_prelude::{ImageRequest, Priority};
use ferrex_model::ImageSize;
use iced::{
    Alignment, Background, Border, Color, ContentFit, Element, Length, Shadow,
    Theme, Vector,
    widget::{
        Column, Row, Space, button, column, container, image as iced_image,
        mouse_area, row, scrollable, text,
    },
};
use iced_aw::menu::{Item, Menu, MenuBar};
use lucide_icons::Icon;

/// Render the hero block shared by desktop, compact, cinematic, and ten-foot
/// detail pages.
pub fn view_detail_hero(
    model: &DetailPageModel,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Element<'static, UiMessage> {
    let art = view_hero_art(&model.hero_art, plan, sizes);
    let summary = view_summary(model, plan, sizes);

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

    horizontal_scroller(pills, sizes.font.small + sizes.spacing.lg)
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
    let mut current = Row::new()
        .spacing(plan.section_grid.gap)
        .align_y(Alignment::Start);
    let mut count = 0usize;

    for section in sections
        .iter()
        .filter(|section| !matches!(section, DetailSection::Overview(_)))
    {
        if matches!(section, DetailSection::Cast(_)) {
            if count > 0 {
                let completed = std::mem::replace(
                    &mut current,
                    Row::new()
                        .spacing(plan.section_grid.gap)
                        .align_y(Alignment::Start),
                );
                outer = outer.push(completed);
                count = 0;
            }
            outer = outer.push(view_section(section, plan, sizes));
            continue;
        }

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

pub fn view_section(
    section: &DetailSection,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Element<'static, UiMessage> {
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

pub fn view_overview_section(
    section: &DetailOverviewSection,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Element<'static, UiMessage> {
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

pub fn view_fact_panel(
    section: &DetailFactPanel,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Element<'static, UiMessage> {
    let mut facts = Column::new().spacing(sizes.spacing.sm);
    for fact in &section.facts {
        facts = facts.push(view_fact(fact, sizes));
    }

    view_panel(&section.title, facts.into(), plan, sizes)
}

pub fn view_cast_section(
    section: &DetailCastSection,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Element<'static, UiMessage> {
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

    let mut row = Row::new().spacing(plan.rail.gap).align_y(Alignment::Start);
    for member in &section.members {
        row = row.push(view_cast_member(member, plan, sizes));
    }

    let image_width = (cast_card_width(plan) * 0.72).clamp(72.0, 180.0);
    let cast_card_height = image_width * 1.5
        + sizes.font.small
        + sizes.font.micro
        + sizes.spacing.xl;

    view_panel(
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

    view_panel(
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

    view_panel(
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

fn view_summary(
    model: &DetailPageModel,
    plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Element<'static, UiMessage> {
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

    if let Some(overview) = hero_overview(model) {
        summary = summary.push(
            text(overview.to_string())
                .size(match plan.composition {
                    DetailComposition::TenFoot => sizes.font.body * 1.12,
                    DetailComposition::CinematicWide => sizes.font.body * 1.05,
                    _ => sizes.font.body,
                })
                .color(theme::MediaServerTheme::TEXT_PRIMARY)
                .width(Length::Fill),
        );
    }

    if !model.actions.is_empty() {
        summary =
            summary.push(view_action_cluster(&model.actions, plan, sizes));
    }

    summary.into()
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
    if !action.menu_items.is_empty() {
        return view_action_menu(action, plan, sizes);
    }

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
    label_row =
        label_row.push(text(action.label.clone()).size(sizes.font.body));

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

fn view_fact(
    fact: &DetailFact,
    sizes: &SizeProvider,
) -> Element<'static, UiMessage> {
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
    if let DetailArtwork::Profile { image_id, alt, .. } = artwork
        && let Some(iid) = image_id
    {
        let request = ImageRequest::new(*iid, ImageSize::profile())
            .with_priority(Priority::Preload);
        if let Some(service) = service_registry::get_image_service() {
            if let Some(handle) = service.get(&request) {
                return iced_image(handle)
                    .width(Length::Fixed(width))
                    .height(Length::Fixed(height))
                    .content_fit(ContentFit::Cover)
                    .border_radius(sizes.scale(3.0))
                    .into();
            }
            service.request_image(request);
        }

        return cast_profile_placeholder(alt, width, height, sizes);
    }

    cast_profile_placeholder(artwork.label(), width, height, sizes)
}

fn cast_profile_placeholder(
    label: &str,
    width: f32,
    height: f32,
    sizes: &SizeProvider,
) -> Element<'static, UiMessage> {
    container(
        column![
            icon_text_with_size(Icon::User, sizes.icon.lg)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
            text(label.to_string())
                .size(sizes.font.micro)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
        ]
        .spacing(sizes.spacing.xs)
        .align_x(Alignment::Center),
    )
    .width(Length::Fixed(width))
    .height(Length::Fixed(height))
    .align_x(iced::alignment::Horizontal::Center)
    .align_y(iced::alignment::Vertical::Center)
    .clip(true)
    .style(detail_panel_style(DetailTone::Muted))
    .into()
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

    max_image_height + sizes.font.caption + sizes.font.small + sizes.spacing.xl
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
) -> Element<'static, UiMessage> {
    let image_layout = rail_art_layout(&item.artwork, plan, sizes);
    let image = view_artwork(
        &item.artwork,
        image_layout,
        Priority::Preload,
        Length::Fixed(image_layout.width),
        Length::Fixed(image_layout.height),
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

fn view_notice(
    notice: &DetailNotice,
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

    container(content)
        .width(Length::Fill)
        .padding(sizes.spacing.md)
        .style(detail_panel_style(notice.tone))
        .into()
}

fn view_panel(
    title: &str,
    body: Element<'static, UiMessage>,
    _plan: &DetailLayoutPlan,
    sizes: &SizeProvider,
) -> Element<'static, UiMessage> {
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
                container(view_rail_item(&section.items[idx], plan, sizes))
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
