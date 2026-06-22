use chrono::Utc;
use ferrex_core::api::types::collections::{
    CollectionDetail, CollectionDuplicatePolicy, CollectionKind,
    CollectionMaterializationState, CollectionMediaKind, CollectionMediaScope,
    CollectionPresentationMode, CollectionSource, CollectionSummary,
    CollectionVisibility,
};
use iced::{
    Element, Length,
    widget::{Space, button, column, container, row, scrollable, text},
};

use crate::{
    domains::ui::{
        collections::{self, CollectionsMessage},
        messages::UiMessage,
        shell_ui::UiShellMessage,
        tabs::{CollectionDetailLoadState, CollectionsLoadState},
        theme,
    },
    state::State,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectionSummaryRow {
    pub title: String,
    pub description: String,
    pub media_scope: String,
    pub kind: String,
    pub source: String,
    pub visibility: String,
    pub presentation: String,
    pub status: String,
    pub artwork: String,
    pub theme: String,
    pub duplicate_policy: String,
    pub item_count: String,
    pub materialization: String,
    pub is_stale: bool,
}

pub fn collection_summary_row(
    summary: &CollectionSummary,
) -> CollectionSummaryRow {
    let materialization = materialization_label(summary);
    CollectionSummaryRow {
        title: summary.title.clone(),
        description: summary
            .description
            .clone()
            .unwrap_or_else(|| "No description provided".to_string()),
        media_scope: media_scope_label(&summary.media_scope),
        kind: kind_label(summary.kind).to_string(),
        source: source_label(summary.source).to_string(),
        visibility: visibility_label(summary.visibility).to_string(),
        presentation: presentation_label(summary.presentation).to_string(),
        status: status_label(summary),
        artwork: artwork_label(summary),
        theme: theme_label(summary),
        duplicate_policy: duplicate_policy_label(summary.duplicate_policy)
            .to_string(),
        item_count: item_count_label(summary.item_count),
        materialization,
        is_stale: is_materialization_stale(summary),
    }
}

pub fn view_collections(state: &State) -> Element<'_, UiMessage> {
    let fonts = &state.domains.ui.state.size_provider.font;
    let tab = collections::collections_tab(state);
    let load_state = tab
        .map(|tab| tab.load_state.clone())
        .unwrap_or(CollectionsLoadState::NotLoaded);
    let summaries = tab.map(|tab| tab.summaries.as_slice()).unwrap_or(&[]);
    let scrollable_id = tab
        .map(|tab| tab.scrollable_id.clone())
        .unwrap_or_else(|| iced::widget::Id::from("collections-tab"));

    let mut body = column![collections_header(state)].spacing(18).padding(24);

    if let CollectionsLoadState::Error(message) = &load_state {
        body = body.push(error_banner(message.clone()));
    }

    body = match &load_state {
        CollectionsLoadState::NotLoaded | CollectionsLoadState::Loading
            if summaries.is_empty() =>
        {
            body.push(center_panel(
                "Loading collections…",
                "Fetching collection summaries from the server.",
                None,
                fonts,
            ))
        }
        CollectionsLoadState::Empty => body.push(center_panel(
            "No collections yet",
            "Collections will appear here after they are created or imported.",
            Some("Refresh"),
            fonts,
        )),
        CollectionsLoadState::Error(_) if summaries.is_empty() => {
            body.push(center_panel(
                "Collections could not load",
                "Check the server connection and try again.",
                Some("Retry"),
                fonts,
            ))
        }
        _ => {
            let mut list = column![].spacing(14);
            for summary in summaries {
                list = list.push(collection_card(summary, state));
            }
            body.push(list)
        }
    };

    scrollable(body)
        .id(scrollable_id)
        .on_scroll(|viewport| {
            crate::domains::ui::interaction_ui::InteractionMessage::TabGridScrolled(
                viewport,
            )
            .into()
        })
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

pub fn view_collection_detail(
    state: &State,
    collection_id: ferrex_core::api::types::collections::CollectionId,
) -> Element<'_, UiMessage> {
    let fonts = &state.domains.ui.state.size_provider.font;
    let tab = collections::collections_tab(state);
    let detail_state =
        tab.and_then(|tab| tab.detail_states.get(&collection_id));
    let summary = tab.and_then(|tab| tab.summary(collection_id));

    match detail_state {
        Some(CollectionDetailLoadState::Loaded(detail)) => {
            collection_detail_content(state, detail)
        }
        Some(CollectionDetailLoadState::Error(message)) => {
            let title = summary
                .map(|summary| summary.title.clone())
                .unwrap_or_else(|| "Collection detail".to_string());
            detail_shell(
                title,
                column![
                    error_banner(message.clone()),
                    button("Retry detail load")
                        .on_press(
                            UiShellMessage::ViewCollection(collection_id)
                                .into()
                        )
                        .style(theme::Button::Secondary.style())
                ]
                .spacing(16)
                .into(),
                fonts,
            )
        }
        Some(CollectionDetailLoadState::Loading)
        | Some(CollectionDetailLoadState::NotLoaded)
        | None => {
            let title = summary
                .map(|summary| summary.title.clone())
                .unwrap_or_else(|| "Collection detail".to_string());
            detail_shell(
                title,
                center_panel(
                    "Loading collection detail…",
                    "Fetching rules, shelf placements, and item previews.",
                    None,
                    fonts,
                ),
                fonts,
            )
        }
    }
}

fn collections_header(state: &State) -> Element<'_, UiMessage> {
    let fonts = &state.domains.ui.state.size_provider.font;
    let total = collections::collections_tab(state)
        .map(|tab| tab.summaries.len())
        .unwrap_or(0);
    let loading = collections::collections_tab(state)
        .map(|tab| tab.load_state.is_loading())
        .unwrap_or(false);
    let subtitle = if total == 0 {
        "Browse curated, manual, imported, and dynamic server collections"
            .to_string()
    } else {
        format!(
            "{total} collection{} available",
            if total == 1 { "" } else { "s" }
        )
    };

    container(
        row![
            column![
                text("Collections")
                    .size(fonts.title)
                    .color(theme::MediaServerTheme::TEXT_PRIMARY),
                text(subtitle)
                    .size(fonts.caption)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY),
            ]
            .spacing(6),
            Space::new().width(Length::Fill),
            button(if loading { "Refreshing…" } else { "Refresh" })
                .on_press(CollectionsMessage::Refresh.into())
                .style(theme::Button::Secondary.style()),
        ]
        .align_y(iced::Alignment::Center),
    )
    .width(Length::Fill)
    .into()
}

fn collection_card<'a>(
    summary: &'a CollectionSummary,
    state: &'a State,
) -> Element<'a, UiMessage> {
    let fonts = &state.domains.ui.state.size_provider.font;
    let row_model = collection_summary_row(summary);
    let badges = row(vec![
        badge(row_model.kind.clone(), fonts.caption),
        badge(row_model.source.clone(), fonts.caption),
        badge(row_model.visibility.clone(), fonts.caption),
        badge(row_model.status.clone(), fonts.caption),
    ])
    .spacing(8)
    .align_y(iced::Alignment::Center);

    let materialization_color = if row_model.is_stale {
        theme::MediaServerTheme::WARNING
    } else {
        theme::MediaServerTheme::TEXT_SECONDARY
    };

    let content = row![
        collection_art_block(
            row_model.artwork.clone(),
            row_model.theme.clone(),
            fonts.caption,
        ),
        column![
            row![
                text(row_model.title.clone())
                    .size(fonts.subtitle)
                    .color(theme::MediaServerTheme::TEXT_PRIMARY),
                Space::new().width(Length::Fill),
                text(row_model.item_count.clone())
                    .size(fonts.body)
                    .color(theme::MediaServerTheme::TEXT_PRIMARY),
            ]
            .align_y(iced::Alignment::Center),
            text(row_model.description.clone())
                .size(fonts.body)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
            badges,
            row![
                text(row_model.media_scope.clone())
                    .size(fonts.caption)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY),
                text("•")
                    .size(fonts.caption)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY),
                text(row_model.presentation.clone())
                    .size(fonts.caption)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY),
                text("•")
                    .size(fonts.caption)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY),
                text(row_model.duplicate_policy.clone())
                    .size(fonts.caption)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
            text(row_model.materialization.clone())
                .size(fonts.caption)
                .color(materialization_color),
        ]
        .spacing(8)
        .width(Length::Fill),
    ]
    .spacing(16)
    .align_y(iced::Alignment::Center);

    button(
        container(content)
            .padding(18)
            .width(Length::Fill)
            .style(theme::Container::Card.style()),
    )
    .on_press(UiShellMessage::ViewCollection(summary.identity.id).into())
    .style(theme::Button::MediaCard.style())
    .width(Length::Fill)
    .into()
}

fn collection_art_block<'a>(
    artwork: String,
    theme_name: String,
    font_size: f32,
) -> Element<'a, UiMessage> {
    container(
        column![
            text(artwork)
                .size(font_size)
                .color(theme::MediaServerTheme::TEXT_PRIMARY),
            text(theme_name)
                .size(font_size)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
        ]
        .spacing(6)
        .align_x(iced::Alignment::Center),
    )
    .width(Length::Fixed(168.0))
    .height(Length::Fixed(104.0))
    .padding(12)
    .align_x(iced::alignment::Horizontal::Center)
    .align_y(iced::alignment::Vertical::Center)
    .style(theme::Container::HeaderAccent.style())
    .into()
}

fn badge<'a>(
    label: impl Into<String>,
    font_size: f32,
) -> Element<'a, UiMessage> {
    container(
        text(label.into())
            .size(font_size)
            .color(theme::MediaServerTheme::TEXT_PRIMARY),
    )
    .padding([4, 8])
    .style(theme::Container::HeaderAccent.style())
    .into()
}

fn error_banner<'a>(message: impl Into<String>) -> Element<'a, UiMessage> {
    container(
        row![
            text(message.into()).color(theme::MediaServerTheme::ERROR),
            Space::new().width(Length::Fill),
            button("Retry")
                .on_press(CollectionsMessage::Retry.into())
                .style(theme::Button::Text.style()),
        ]
        .align_y(iced::Alignment::Center),
    )
    .padding(12)
    .style(theme::Container::Card.style())
    .into()
}

fn center_panel<'a>(
    title: &'a str,
    body: &'a str,
    action: Option<&'a str>,
    fonts: &crate::infra::design_tokens::fonts::FontTokens,
) -> Element<'a, UiMessage> {
    let mut content = column![
        text(title)
            .size(fonts.subtitle)
            .color(theme::MediaServerTheme::TEXT_PRIMARY),
        text(body)
            .size(fonts.body)
            .color(theme::MediaServerTheme::TEXT_SECONDARY),
    ]
    .spacing(12)
    .align_x(iced::Alignment::Center);

    if let Some(label) = action {
        content = content.push(
            button(label)
                .on_press(CollectionsMessage::Retry.into())
                .style(theme::Button::Secondary.style()),
        );
    }

    container(content)
        .width(Length::Fill)
        .height(Length::Fixed(260.0))
        .align_x(iced::alignment::Horizontal::Center)
        .align_y(iced::alignment::Vertical::Center)
        .style(theme::Container::Default.style())
        .into()
}

fn collection_detail_content<'a>(
    state: &'a State,
    detail: &'a CollectionDetail,
) -> Element<'a, UiMessage> {
    let fonts = &state.domains.ui.state.size_provider.font;
    let summary = &detail.summary;
    let row_model = collection_summary_row(summary);

    let mut preview = column![
        text("Item preview")
            .size(fonts.subtitle)
            .color(theme::MediaServerTheme::TEXT_PRIMARY)
    ]
    .spacing(10);

    if detail.items_preview.is_empty() {
        preview = preview.push(
            text("No preview items are currently materialized.")
                .size(fonts.body)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
        );
    } else {
        for item in &detail.items_preview {
            preview = preview.push(
                container(
                    row![
                        text(format!("#{:02}", item.position))
                            .size(fonts.caption)
                            .color(theme::MediaServerTheme::TEXT_SECONDARY),
                        column![
                            text(item.title.as_str())
                                .size(fonts.body)
                                .color(theme::MediaServerTheme::TEXT_PRIMARY),
                            text(
                                item.subtitle.as_deref().unwrap_or(
                                    media_kind_label(item.media_type)
                                )
                            )
                            .size(fonts.caption)
                            .color(theme::MediaServerTheme::TEXT_SECONDARY),
                        ]
                        .spacing(4),
                        Space::new().width(Length::Fill),
                        text(format!("{:?}", item.availability.status))
                            .size(fonts.caption)
                            .color(theme::MediaServerTheme::TEXT_SECONDARY),
                    ]
                    .spacing(12)
                    .align_y(iced::Alignment::Center),
                )
                .padding(12)
                .style(theme::Container::Card.style()),
            );
        }
    }

    let rule_text = detail
        .rule
        .as_ref()
        .map(|rule| format!("Dynamic rule schema v{}", rule.schema_version))
        .unwrap_or_else(|| "No dynamic rule attached".to_string());
    let shelf_text = if detail.shelf_placements.is_empty() {
        "No shelf placements".to_string()
    } else {
        format!(
            "{} shelf placement{}",
            detail.shelf_placements.len(),
            if detail.shelf_placements.len() == 1 {
                ""
            } else {
                "s"
            }
        )
    };

    detail_shell(
        row_model.title.clone(),
        column![
            container(
                row![
                    collection_art_block(
                        row_model.artwork.clone(),
                        row_model.theme.clone(),
                        fonts.caption,
                    ),
                    column![
                        text(row_model.description.clone())
                            .size(fonts.body)
                            .color(theme::MediaServerTheme::TEXT_SECONDARY),
                        row(vec![
                            badge(row_model.kind.clone(), fonts.caption),
                            badge(row_model.source.clone(), fonts.caption),
                            badge(row_model.visibility.clone(), fonts.caption),
                            badge(row_model.status.clone(), fonts.caption),
                        ])
                        .spacing(8),
                        text(row_model.media_scope.clone())
                            .size(fonts.caption)
                            .color(theme::MediaServerTheme::TEXT_SECONDARY),
                        text(row_model.materialization.clone())
                            .size(fonts.caption)
                            .color(if row_model.is_stale {
                                theme::MediaServerTheme::WARNING
                            } else {
                                theme::MediaServerTheme::TEXT_SECONDARY
                            }),
                    ]
                    .spacing(8)
                    .width(Length::Fill),
                ]
                .spacing(16)
                .align_y(iced::Alignment::Center),
            )
            .padding(18)
            .style(theme::Container::Card.style()),
            container(
                column![
                    text("Collection metadata")
                        .size(fonts.subtitle)
                        .color(theme::MediaServerTheme::TEXT_PRIMARY),
                    text(rule_text)
                        .size(fonts.body)
                        .color(theme::MediaServerTheme::TEXT_SECONDARY),
                    text(shelf_text)
                        .size(fonts.body)
                        .color(theme::MediaServerTheme::TEXT_SECONDARY),
                    text(format!("Revision {}", summary.version.revision))
                        .size(fonts.caption)
                        .color(theme::MediaServerTheme::TEXT_SECONDARY),
                ]
                .spacing(8),
            )
            .padding(18)
            .style(theme::Container::Card.style()),
            preview,
        ]
        .spacing(18)
        .into(),
        fonts,
    )
}

fn detail_shell<'a>(
    title: String,
    content: Element<'a, UiMessage>,
    fonts: &crate::infra::design_tokens::fonts::FontTokens,
) -> Element<'a, UiMessage> {
    scrollable(
        column![
            text(title)
                .size(fonts.title)
                .color(theme::MediaServerTheme::TEXT_PRIMARY),
            content,
        ]
        .spacing(18)
        .padding(32),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn item_count_label(count: u32) -> String {
    format!("{count} item{}", if count == 1 { "" } else { "s" })
}

fn status_label(summary: &CollectionSummary) -> String {
    if summary.timestamps.archived_at.is_some() {
        "Archived".to_string()
    } else if matches!(
        summary.materialization.state,
        CollectionMaterializationState::Failed
    ) {
        "Needs attention".to_string()
    } else {
        "Active".to_string()
    }
}

fn materialization_label(summary: &CollectionSummary) -> String {
    let state = match summary.materialization.state {
        CollectionMaterializationState::NotMaterialized => "Not materialized",
        CollectionMaterializationState::Pending => "Materialization pending",
        CollectionMaterializationState::Refreshing => {
            "Refreshing materialization"
        }
        CollectionMaterializationState::Ready => "Materialized",
        CollectionMaterializationState::Stale => "Materialization stale",
        CollectionMaterializationState::Failed => "Materialization failed",
    };
    let mut label = format!(
        "{state} · {}",
        item_count_label(summary.materialization.item_count)
    );
    if let Some(error) = &summary.materialization.last_error {
        label.push_str(" · ");
        label.push_str(error);
    }
    if is_materialization_stale(summary)
        && !matches!(
            summary.materialization.state,
            CollectionMaterializationState::Stale
        )
    {
        label.push_str(" · stale");
    }
    label
}

fn is_materialization_stale(summary: &CollectionSummary) -> bool {
    matches!(
        summary.materialization.state,
        CollectionMaterializationState::Stale
            | CollectionMaterializationState::Failed
    ) || summary
        .materialization
        .expires_at
        .is_some_and(|expires_at| expires_at < Utc::now())
}

fn artwork_label(summary: &CollectionSummary) -> String {
    if summary.artwork.poster_iid.is_some() {
        "Poster artwork".to_string()
    } else if summary.artwork.backdrop_iid.is_some() {
        "Backdrop artwork".to_string()
    } else if summary.artwork.thumbnail_iid.is_some() {
        "Thumbnail artwork".to_string()
    } else if summary.artwork.provider_image_path.is_some() {
        "Provider artwork".to_string()
    } else {
        match summary.artwork.source {
            ferrex_core::api::types::collections::CollectionArtworkSource::None => {
                "No artwork".to_string()
            }
            source => format!("{:?} artwork", source),
        }
    }
}

fn theme_label(summary: &CollectionSummary) -> String {
    summary
        .theme
        .primary_color_hex
        .as_deref()
        .or(summary.theme.secondary_color_hex.as_deref())
        .or(summary.artwork.accent_color_hex.as_deref())
        .map(|color| format!("Theme {color}"))
        .or_else(|| {
            summary
                .theme
                .icon
                .as_ref()
                .map(|icon| format!("Icon {icon}"))
        })
        .unwrap_or_else(|| "Default theme".to_string())
}

fn media_scope_label(scope: &CollectionMediaScope) -> String {
    match scope {
        CollectionMediaScope::All => "All media".to_string(),
        CollectionMediaScope::Types { media_types } => {
            if media_types.is_empty() {
                "All media types".to_string()
            } else {
                format!(
                    "{} only",
                    media_types
                        .iter()
                        .map(|kind| media_kind_label(*kind))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        }
        CollectionMediaScope::Library {
            library_id,
            media_types,
        } => {
            let types = if media_types.is_empty() {
                "all media".to_string()
            } else {
                media_types
                    .iter()
                    .map(|kind| media_kind_label(*kind))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            format!("Library {library_id} · {types}")
        }
        CollectionMediaScope::ExplicitItems { item_keys } => {
            format!(
                "{} explicit item{}",
                item_keys.len(),
                if item_keys.len() == 1 { "" } else { "s" }
            )
        }
    }
}

fn media_kind_label(kind: CollectionMediaKind) -> &'static str {
    match kind {
        CollectionMediaKind::Movie => "Movies",
        CollectionMediaKind::Series => "Series",
        CollectionMediaKind::Season => "Seasons",
        CollectionMediaKind::Episode => "Episodes",
    }
}

fn kind_label(kind: CollectionKind) -> &'static str {
    match kind {
        CollectionKind::Manual => "Manual",
        CollectionKind::DynamicRule => "Dynamic",
        CollectionKind::TmdbList => "TMDB list",
        CollectionKind::TmdbCollection => "TMDB collection",
        CollectionKind::System => "System",
    }
}

fn source_label(source: CollectionSource) -> &'static str {
    match source {
        CollectionSource::Manual => "Manual source",
        CollectionSource::DynamicRule => "Rule source",
        CollectionSource::Tmdb => "TMDB source",
        CollectionSource::System => "System source",
        CollectionSource::Imported => "Imported source",
    }
}

fn visibility_label(visibility: CollectionVisibility) -> &'static str {
    match visibility {
        CollectionVisibility::Private => "Private",
        CollectionVisibility::Shared => "Shared",
        CollectionVisibility::Public => "Public",
        CollectionVisibility::System => "System visible",
    }
}

fn presentation_label(
    presentation: CollectionPresentationMode,
) -> &'static str {
    match presentation {
        CollectionPresentationMode::Shelf => "Shelf presentation",
        CollectionPresentationMode::Grid => "Grid presentation",
        CollectionPresentationMode::List => "List presentation",
        CollectionPresentationMode::Playlist => "Playlist presentation",
        CollectionPresentationMode::Hero => "Hero presentation",
        CollectionPresentationMode::Hidden => "Hidden presentation",
    }
}

fn duplicate_policy_label(policy: CollectionDuplicatePolicy) -> &'static str {
    match policy {
        CollectionDuplicatePolicy::KeepAll => "Keeps duplicates",
        CollectionDuplicatePolicy::DeduplicateMedia => "Dedupes media",
        CollectionDuplicatePolicy::DeduplicateLogical => {
            "Dedupes logical items"
        }
        CollectionDuplicatePolicy::RejectDuplicates => "Rejects duplicates",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};
    use ferrex_core::api::types::collections::{
        CollectionArtwork, CollectionIdentity, CollectionMaterializationStatus,
        CollectionOwner, CollectionProvenance, CollectionScope,
        CollectionTimestamps, CollectionVersion,
    };
    use uuid::Uuid;

    fn summary() -> CollectionSummary {
        let id = ferrex_core::api::types::collections::CollectionId(
            Uuid::from_u128(0x64700000000000000000000000000001),
        );
        CollectionSummary {
            identity: CollectionIdentity::for_id(id),
            title: "Weekend Queue".to_string(),
            description: Some("Movies for Friday night".to_string()),
            kind: CollectionKind::DynamicRule,
            source: CollectionSource::Tmdb,
            owner: CollectionOwner::default(),
            scope: CollectionScope::User,
            visibility: CollectionVisibility::Shared,
            presentation: CollectionPresentationMode::Grid,
            media_scope: CollectionMediaScope::Types {
                media_types: vec![CollectionMediaKind::Movie],
            },
            duplicate_policy: CollectionDuplicatePolicy::DeduplicateLogical,
            artwork: CollectionArtwork {
                accent_color_hex: Some("#335577".to_string()),
                ..CollectionArtwork::default()
            },
            theme: ferrex_core::api::types::collections::CollectionTheme {
                primary_color_hex: Some("#112233".to_string()),
                ..Default::default()
            },
            provenance: CollectionProvenance::default(),
            version: CollectionVersion::default(),
            timestamps: CollectionTimestamps {
                created_at: Utc::now() - Duration::days(2),
                updated_at: Utc::now(),
                archived_at: None,
            },
            item_count: 7,
            materialization: CollectionMaterializationStatus {
                state: CollectionMaterializationState::Stale,
                item_count: 7,
                ..CollectionMaterializationStatus::default()
            },
        }
    }

    #[test]
    fn collection_summary_row_surfaces_listing_badges_and_status() {
        let row = collection_summary_row(&summary());

        assert_eq!(row.title, "Weekend Queue");
        assert_eq!(row.description, "Movies for Friday night");
        assert_eq!(row.kind, "Dynamic");
        assert_eq!(row.source, "TMDB source");
        assert_eq!(row.visibility, "Shared");
        assert_eq!(row.media_scope, "Movies only");
        assert_eq!(row.item_count, "7 items");
        assert_eq!(row.theme, "Theme #112233");
        assert!(row.materialization.contains("Materialization stale"));
        assert!(row.is_stale);
    }
}
