use chrono::{DateTime, Utc};
use ferrex_core::{
    api::types::collections::{
        CollectionDetail, CollectionDuplicatePolicy, CollectionKind,
        CollectionLimitPolicy, CollectionLimitWindow,
        CollectionMaterializationState, CollectionMediaKind,
        CollectionMediaScope, CollectionMember,
        CollectionMemberAvailabilityStatus, CollectionMemberKey,
        CollectionPresentationMode, CollectionRuleField,
        CollectionRuleOperator, CollectionRulePredicate, CollectionRuleValue,
        CollectionSortDirection, CollectionSortKey, CollectionSortPolicy,
        CollectionSource, CollectionSummary, CollectionVisibility,
        DynamicCollectionRule, ShelfPlacement,
    },
    player_prelude::{EpisodeID, MovieID, SeriesID},
};
use ferrex_model::MediaID;
use iced::{
    Element, Length,
    widget::{
        Space, button, column, container, pick_list, row, scrollable, text,
        text_input,
    },
};

use crate::{
    domains::ui::{
        collections::{self, CollectionItemMoveDirection, CollectionsMessage},
        messages::UiMessage,
        shell_ui::UiShellMessage,
        tabs::{
            CollectionCreateFormState, CollectionDetailLoadState,
            CollectionEditFormState, CollectionItemActionState,
            CollectionItemMutationKind, CollectionItemsLoadState,
            CollectionItemsState, CollectionMediaPickerState,
            CollectionMediaScopeChoice, CollectionPickerItem,
            CollectionRefreshState, CollectionsLoadState,
        },
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollectionItemAction {
    ViewMovie(MovieID),
    ViewSeries(SeriesID),
    ViewEpisode(EpisodeID),
}

impl CollectionItemAction {
    fn shell_message(self) -> UiShellMessage {
        match self {
            Self::ViewMovie(movie_id) => {
                UiShellMessage::ViewMovieDetails(movie_id)
            }
            Self::ViewSeries(series_id) => {
                UiShellMessage::ViewTvShow(series_id)
            }
            Self::ViewEpisode(episode_id) => {
                UiShellMessage::ViewEpisode(episode_id)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectionItemRow {
    pub item_key: CollectionMemberKey,
    pub position: u32,
    pub title: String,
    pub subtitle: String,
    pub media_kind: String,
    pub availability: String,
    pub action: Option<CollectionItemAction>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectionItemsViewModel {
    pub rows: Vec<CollectionItemRow>,
    pub loaded_count: usize,
    pub hidden_count: usize,
    pub total_count: u64,
    pub can_load_more: bool,
    pub status_summary: String,
    pub hidden_summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectionStatusSummary {
    pub source_summary: String,
    pub provenance_summary: String,
    pub rule_summary: String,
    pub materialization_summary: String,
    pub refresh_available: bool,
    pub refresh_label: Option<String>,
    pub refresh_error: Option<String>,
}

pub fn collection_item_rows(
    items: &[CollectionMember],
) -> Vec<CollectionItemRow> {
    let mut visible: Vec<_> = items
        .iter()
        .filter(|item| is_collection_member_visible(item))
        .collect();
    visible.sort_by(|left, right| {
        left.position
            .cmp(&right.position)
            .then_with(|| left.item_key.cmp(&right.item_key))
    });

    visible
        .into_iter()
        .map(|item| CollectionItemRow {
            item_key: item.item_key.clone(),
            position: item.position,
            title: item.title.clone(),
            subtitle: item.subtitle.clone().unwrap_or_else(|| {
                media_kind_label(item.media_type).to_string()
            }),
            media_kind: media_kind_label(item.media_type).to_string(),
            availability: availability_label(item.availability.status)
                .to_string(),
            action: collection_item_action(item),
        })
        .collect()
}

pub fn collection_items_view_model(
    item_state: Option<&CollectionItemsState>,
    summary_item_count: u32,
) -> CollectionItemsViewModel {
    let items = item_state
        .map(|state| state.items.as_slice())
        .unwrap_or(&[]);
    let rows = collection_item_rows(items);
    let hidden_count = items
        .iter()
        .filter(|item| !is_collection_member_visible(item))
        .count();
    let loaded_count = items.len();
    let total_count = item_state
        .and_then(|state| state.page.as_ref().map(|page| page.total))
        .unwrap_or(summary_item_count as u64);
    let can_load_more = item_state.is_some_and(CollectionItemsState::has_more);
    let status_summary = if loaded_count == 0 {
        format!("{} reported by API", item_count_label_u64(total_count))
    } else {
        format!(
            "Showing {} visible of {} loaded · {} reported by API",
            rows.len(),
            loaded_count,
            item_count_label_u64(total_count)
        )
    };
    let hidden_summary = (hidden_count > 0).then(|| {
        format!(
            "{} unavailable or missing item{} hidden from the normal detail view",
            hidden_count,
            if hidden_count == 1 { "" } else { "s" }
        )
    });

    CollectionItemsViewModel {
        rows,
        loaded_count,
        hidden_count,
        total_count,
        can_load_more,
        status_summary,
        hidden_summary,
    }
}

pub fn collection_status_summary(
    detail: &CollectionDetail,
    item_state: Option<&CollectionItemsState>,
    refresh_state: Option<&CollectionRefreshState>,
) -> CollectionStatusSummary {
    let summary = &detail.summary;
    let refresh_state = refresh_state.cloned().unwrap_or_default();
    let refresh_available = can_refresh_collection(detail);
    let refresh_label = refresh_available.then(|| match refresh_state {
        CollectionRefreshState::Idle => "Refresh materialization".to_string(),
        CollectionRefreshState::Refreshing => "Refreshing…".to_string(),
        CollectionRefreshState::Error(_) => "Retry refresh".to_string(),
    });
    let refresh_error = match refresh_state {
        CollectionRefreshState::Error(message) => Some(message),
        CollectionRefreshState::Idle | CollectionRefreshState::Refreshing => {
            None
        }
    };
    let materialization = item_state
        .and_then(|state| state.materialization.as_ref())
        .unwrap_or(&summary.materialization);

    CollectionStatusSummary {
        source_summary: source_summary(summary),
        provenance_summary: provenance_summary(summary),
        rule_summary: rule_summary(summary, detail.rule.as_ref()),
        materialization_summary: materialization_summary(materialization),
        refresh_available,
        refresh_label,
        refresh_error,
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

    if let Some(create_form) = tab.map(|tab| &tab.create_form)
        && create_form.is_open
    {
        body = body.push(collection_create_form(create_form, fonts));
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
    let item_state = tab.and_then(|tab| tab.item_states.get(&collection_id));
    let refresh_state =
        tab.and_then(|tab| tab.refresh_states.get(&collection_id));
    let summary = tab.and_then(|tab| tab.summary(collection_id));

    match detail_state {
        Some(CollectionDetailLoadState::Loaded(detail)) => {
            collection_detail_content(state, detail, item_state, refresh_state)
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
                    "Fetching metadata, status, and paginated items.",
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
            button("New manual collection")
                .on_press(CollectionsMessage::ToggleCreateForm.into())
                .style(theme::Button::Primary.style()),
            button(if loading { "Refreshing…" } else { "Refresh" })
                .on_press(CollectionsMessage::Refresh.into())
                .style(theme::Button::Secondary.style()),
        ]
        .align_y(iced::Alignment::Center),
    )
    .width(Length::Fill)
    .into()
}

fn collection_create_form<'a>(
    form: &'a CollectionCreateFormState,
    fonts: &crate::infra::design_tokens::fonts::FontTokens,
) -> Element<'a, UiMessage> {
    let mut submit = button(if form.submitting {
        "Creating…"
    } else {
        "Create manual collection"
    })
    .style(theme::Button::Primary.style());
    if !form.submitting {
        submit = submit.on_press(CollectionsMessage::SubmitCreate.into());
    }

    let mut fields = column![
        text("Create manual collection")
            .size(fonts.subtitle)
            .color(theme::MediaServerTheme::TEXT_PRIMARY),
        text("Start with editable metadata and a default media scope. Dynamic rule builders stay out of this flow.")
            .size(fonts.caption)
            .color(theme::MediaServerTheme::TEXT_SECONDARY),
        text_input("Collection title", &form.title)
            .padding(12)
            .size(fonts.body)
            .style(theme::TextInput::style())
            .on_input(|value| CollectionsMessage::CreateTitleChanged(value).into())
            .on_submit(CollectionsMessage::SubmitCreate.into()),
        text_input("Description (optional)", &form.description)
            .padding(12)
            .size(fonts.body)
            .style(theme::TextInput::style())
            .on_input(|value| CollectionsMessage::CreateDescriptionChanged(value).into()),
        row![
            column![
                text("Media scope default")
                    .size(fonts.caption)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY),
                pick_list(
                    CollectionMediaScopeChoice::OPTIONS,
                    Some(form.media_scope),
                    |scope| CollectionsMessage::CreateScopeChanged(scope).into(),
                )
                .placeholder("Media scope")
                .width(Length::Fixed(220.0)),
            ]
            .spacing(6),
            Space::new().width(Length::Fill),
            submit,
            button("Cancel")
                .on_press(CollectionsMessage::ToggleCreateForm.into())
                .style(theme::Button::Secondary.style()),
        ]
        .align_y(iced::Alignment::End)
        .spacing(12),
    ]
    .spacing(12);

    if let Some(error) = form.error.as_deref() {
        fields = fields.push(
            text(error)
                .size(fonts.caption)
                .color(theme::MediaServerTheme::ERROR),
        );
    }

    container(fields)
        .padding(18)
        .style(theme::Container::Card.style())
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
    item_state: Option<&'a CollectionItemsState>,
    refresh_state: Option<&'a CollectionRefreshState>,
) -> Element<'a, UiMessage> {
    let fonts = &state.domains.ui.state.size_provider.font;
    let summary = &detail.summary;
    let row_model = collection_summary_row(summary);
    let status = collection_status_summary(detail, item_state, refresh_state);
    let items_model =
        collection_items_view_model(item_state, summary.item_count);
    let collection_id = summary.identity.id;
    let can_edit = collections::is_manual_collection(summary);
    let tab = collections::collections_tab(state);
    let edit_form = tab.and_then(|tab| tab.edit_forms.get(&collection_id));
    let picker_state =
        tab.and_then(|tab| tab.picker_states.get(&collection_id));
    let item_action_state =
        tab.and_then(|tab| tab.item_action_states.get(&collection_id));

    let mut content = column![
        collection_detail_header_card(row_model.clone(), can_edit, fonts),
        collection_status_cards(
            status,
            detail,
            collection_id,
            refresh_state,
            fonts,
        ),
    ]
    .spacing(18);

    content = if can_edit {
        if let Some(edit_form) = edit_form {
            content.push(collection_manual_editing_section(
                collection_id,
                edit_form,
                picker_state,
                fonts,
            ))
        } else {
            content.push(collection_editor_state_notice(fonts))
        }
    } else {
        content.push(collection_read_only_notice(summary, fonts))
    };

    content = content.push(collection_items_section(
        items_model,
        item_state,
        collection_id,
        can_edit,
        item_action_state,
        fonts,
    ));

    detail_shell(row_model.title.clone(), content.into(), fonts)
}

fn collection_detail_header_card<'a>(
    row_model: CollectionSummaryRow,
    editable: bool,
    fonts: &crate::infra::design_tokens::fonts::FontTokens,
) -> Element<'a, UiMessage> {
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
                    badge(
                        if editable {
                            "Manual editing enabled"
                        } else {
                            "Read-only"
                        },
                        fonts.caption,
                    ),
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
    .style(theme::Container::Card.style())
    .into()
}

fn collection_status_cards<'a>(
    status: CollectionStatusSummary,
    detail: &'a CollectionDetail,
    collection_id: ferrex_core::api::types::collections::CollectionId,
    refresh_state: Option<&CollectionRefreshState>,
    fonts: &crate::infra::design_tokens::fonts::FontTokens,
) -> Element<'a, UiMessage> {
    let refreshing =
        matches!(refresh_state, Some(CollectionRefreshState::Refreshing));
    let refresh_control: Element<'a, UiMessage> =
        if let Some(label) = status.refresh_label.clone() {
            let mut control =
                button(text(label)).style(theme::Button::Secondary.style());
            if !refreshing {
                control = control.on_press(
                    CollectionsMessage::RefreshMaterialization(collection_id)
                        .into(),
                );
            }
            control.into()
        } else {
            text("Refresh unavailable for this source")
                .size(fonts.caption)
                .color(theme::MediaServerTheme::TEXT_SECONDARY)
                .into()
        };

    let source_heading = if collections::is_manual_collection(&detail.summary) {
        "Collection source"
    } else {
        "Read-only source"
    };
    let mut source_card = column![
        text(source_heading)
            .size(fonts.subtitle)
            .color(theme::MediaServerTheme::TEXT_PRIMARY),
        text(status.source_summary.clone())
            .size(fonts.body)
            .color(theme::MediaServerTheme::TEXT_SECONDARY),
        text(status.provenance_summary.clone())
            .size(fonts.caption)
            .color(theme::MediaServerTheme::TEXT_SECONDARY),
        refresh_control,
    ]
    .spacing(8);

    if let Some(message) = status.refresh_error.as_deref() {
        source_card = source_card.push(
            text(format!("Refresh failed: {message}"))
                .size(fonts.caption)
                .color(theme::MediaServerTheme::ERROR),
        );
    }

    let metadata = row(vec![
        metadata_tile("Rule / source", status.rule_summary.clone(), fonts),
        metadata_tile(
            "Materialization",
            status.materialization_summary.clone(),
            fonts,
        ),
        metadata_tile(
            "Shelf placement",
            shelf_placements_summary(&detail.shelf_placements),
            fonts,
        ),
    ])
    .spacing(12);

    column![
        container(source_card)
            .padding(18)
            .style(theme::Container::Card.style()),
        metadata,
    ]
    .spacing(12)
    .into()
}

fn metadata_tile<'a>(
    title: &'a str,
    body: String,
    fonts: &crate::infra::design_tokens::fonts::FontTokens,
) -> Element<'a, UiMessage> {
    container(
        column![
            text(title)
                .size(fonts.caption)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
            text(body)
                .size(fonts.body)
                .color(theme::MediaServerTheme::TEXT_PRIMARY),
        ]
        .spacing(6),
    )
    .padding(14)
    .width(Length::FillPortion(1))
    .style(theme::Container::Card.style())
    .into()
}

fn collection_read_only_notice<'a>(
    summary: &CollectionSummary,
    fonts: &crate::infra::design_tokens::fonts::FontTokens,
) -> Element<'a, UiMessage> {
    container(
        column![
            text("Read-only collection")
                .size(fonts.subtitle)
                .color(theme::MediaServerTheme::TEXT_PRIMARY),
            text(format!(
                "{} collections are intentionally locked in the desktop editor.",
                source_label(summary.source)
            ))
            .size(fonts.body)
            .color(theme::MediaServerTheme::TEXT_SECONDARY),
            text("Manual metadata, membership, and ordering controls are available only for manual collections. Dynamic rule-builder UI is intentionally out of scope here.")
                .size(fonts.caption)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
        ]
        .spacing(8),
    )
    .padding(18)
    .style(theme::Container::Card.style())
    .into()
}

fn collection_editor_state_notice<'a>(
    fonts: &crate::infra::design_tokens::fonts::FontTokens,
) -> Element<'a, UiMessage> {
    container(
        text("Manual editor state is loading. Reopen the collection or refresh if controls do not appear.")
            .size(fonts.caption)
            .color(theme::MediaServerTheme::TEXT_SECONDARY),
    )
    .padding(18)
    .style(theme::Container::Card.style())
    .into()
}

fn collection_manual_editing_section<'a>(
    collection_id: ferrex_core::api::types::collections::CollectionId,
    form: &'a CollectionEditFormState,
    picker: Option<&'a CollectionMediaPickerState>,
    fonts: &crate::infra::design_tokens::fonts::FontTokens,
) -> Element<'a, UiMessage> {
    let mut section = column![
        row![
            column![
                text("Manual collection editor")
                    .size(fonts.subtitle)
                    .color(theme::MediaServerTheme::TEXT_PRIMARY),
                text("Update metadata, archive the collection, and add existing media. Dynamic rule-builder controls are not part of this manual flow.")
                    .size(fonts.caption)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY),
            ]
            .spacing(4),
            Space::new().width(Length::Fill),
            badge("Manual only", fonts.caption),
        ]
        .align_y(iced::Alignment::Center),
        row![
            collection_metadata_editor(collection_id, form, fonts),
            collection_media_picker(collection_id, picker, fonts),
        ]
        .spacing(12),
    ]
    .spacing(12);

    if form.conflict || picker.is_some_and(|picker| picker.conflict) {
        section = section.push(conflict_recovery_banner(collection_id, fonts));
    }

    container(section)
        .padding(18)
        .style(theme::Container::Card.style())
        .into()
}

fn collection_metadata_editor<'a>(
    collection_id: ferrex_core::api::types::collections::CollectionId,
    form: &'a CollectionEditFormState,
    fonts: &crate::infra::design_tokens::fonts::FontTokens,
) -> Element<'a, UiMessage> {
    let busy = form.saving || form.archiving;
    let mut save = button(if form.saving {
        "Saving…"
    } else {
        "Save metadata"
    })
    .style(theme::Button::Primary.style());
    if form.is_dirty && !busy {
        save = save
            .on_press(CollectionsMessage::SaveMetadata(collection_id).into());
    }

    let mut archive = button(if form.archiving {
        "Archiving…"
    } else {
        "Archive collection"
    })
    .style(theme::Button::Destructive.style());
    if !busy {
        archive =
            archive.on_press(CollectionsMessage::Archive(collection_id).into());
    }

    let mut content = column![
        text("Metadata")
            .size(fonts.body)
            .color(theme::MediaServerTheme::TEXT_PRIMARY),
        text_input("Collection title", &form.title)
            .padding(12)
            .size(fonts.body)
            .style(theme::TextInput::style())
            .on_input(move |value| CollectionsMessage::EditTitleChanged(
                collection_id,
                value
            )
            .into())
            .on_submit(CollectionsMessage::SaveMetadata(collection_id).into()),
        text_input("Description", &form.description)
            .padding(12)
            .size(fonts.body)
            .style(theme::TextInput::style())
            .on_input(move |value| CollectionsMessage::EditDescriptionChanged(
                collection_id,
                value
            )
            .into()),
        row![
            column![
                text("Media scope")
                    .size(fonts.caption)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY),
                pick_list(
                    CollectionMediaScopeChoice::OPTIONS,
                    Some(form.media_scope),
                    move |scope| CollectionsMessage::EditScopeChanged(
                        collection_id,
                        scope
                    )
                    .into(),
                )
                .placeholder("Media scope")
                .width(Length::Fixed(220.0)),
            ]
            .spacing(6),
            Space::new().width(Length::Fill),
            save,
            archive,
        ]
        .align_y(iced::Alignment::End)
        .spacing(10),
    ]
    .spacing(10)
    .width(Length::FillPortion(1));

    if let Some(error) = form.error.as_deref() {
        content = content.push(
            text(error)
                .size(fonts.caption)
                .color(theme::MediaServerTheme::ERROR),
        );
    }

    if form.conflict {
        content = content.push(
            text("The server has a newer revision. Reload latest, review the recovered values, then retry your change.")
                .size(fonts.caption)
                .color(theme::MediaServerTheme::WARNING),
        );
    }

    container(content)
        .padding(14)
        .width(Length::FillPortion(1))
        .style(theme::Container::HeaderAccent.style())
        .into()
}

fn collection_media_picker<'a>(
    collection_id: ferrex_core::api::types::collections::CollectionId,
    picker: Option<&'a CollectionMediaPickerState>,
    fonts: &crate::infra::design_tokens::fonts::FontTokens,
) -> Element<'a, UiMessage> {
    let query = picker.map(|picker| picker.query.as_str()).unwrap_or("");
    let searching = picker.is_some_and(|picker| picker.searching);
    let adding = picker.and_then(|picker| picker.adding);
    let busy = searching || adding.is_some();
    let mut search = button(if searching { "Searching…" } else { "Search" })
        .style(theme::Button::Secondary.style());
    if !searching {
        search = search
            .on_press(CollectionsMessage::SearchPicker(collection_id).into());
    }

    let mut content = column![
        text("Add media")
            .size(fonts.body)
            .color(theme::MediaServerTheme::TEXT_PRIMARY),
        row![
            text_input("Search existing media by title", query)
                .padding(12)
                .size(fonts.body)
                .style(theme::TextInput::style())
                .on_input(move |value| CollectionsMessage::PickerQueryChanged(
                    collection_id,
                    value
                )
                .into())
                .on_submit(
                    CollectionsMessage::SearchPicker(collection_id).into()
                ),
            search,
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center),
    ]
    .spacing(10)
    .width(Length::FillPortion(1));

    if let Some(error) = picker.and_then(|picker| picker.error.as_deref()) {
        content = content.push(
            text(error)
                .size(fonts.caption)
                .color(theme::MediaServerTheme::ERROR),
        );
    }

    if picker.is_some_and(|picker| picker.conflict) {
        content = content.push(
            text("Membership changed on the server. Reload latest before retrying this add.")
                .size(fonts.caption)
                .color(theme::MediaServerTheme::WARNING),
        );
    }

    if picker.is_none_or(|picker| picker.results.is_empty())
        && !query.trim().is_empty()
        && !searching
    {
        content = content.push(
            text("No picker results loaded yet. Search to browse matching server media.")
                .size(fonts.caption)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
        );
    } else {
        for item in picker
            .map(|picker| picker.results.as_slice())
            .unwrap_or(&[])
            .iter()
            .take(6)
            .cloned()
        {
            content = content.push(collection_picker_result_card(
                collection_id,
                item,
                busy,
                adding,
                fonts,
            ));
        }
    }

    container(content)
        .padding(14)
        .width(Length::FillPortion(1))
        .style(theme::Container::HeaderAccent.style())
        .into()
}

fn collection_picker_result_card<'a>(
    collection_id: ferrex_core::api::types::collections::CollectionId,
    item: CollectionPickerItem,
    busy: bool,
    adding: Option<MediaID>,
    fonts: &crate::infra::design_tokens::fonts::FontTokens,
) -> Element<'a, UiMessage> {
    let title = item.title.clone();
    let media_kind = item.media_kind;
    let subtitle = item
        .subtitle
        .clone()
        .unwrap_or_else(|| media_kind_label(media_kind).to_string());
    let adding_this = adding == Some(item.media_id);
    let mut add = button(if adding_this { "Adding…" } else { "Add" })
        .style(theme::Button::Primary.style());
    if !busy {
        add = add.on_press(
            CollectionsMessage::AddPickerItem {
                collection_id,
                item,
            }
            .into(),
        );
    }

    container(
        row![
            column![
                text(title)
                    .size(fonts.body)
                    .color(theme::MediaServerTheme::TEXT_PRIMARY),
                text(format!(
                    "{} · {}",
                    media_kind_label(media_kind),
                    subtitle
                ))
                .size(fonts.caption)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
            ]
            .spacing(4)
            .width(Length::Fill),
            add,
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center),
    )
    .padding(10)
    .style(theme::Container::Card.style())
    .into()
}

fn conflict_recovery_banner<'a>(
    collection_id: ferrex_core::api::types::collections::CollectionId,
    fonts: &crate::infra::design_tokens::fonts::FontTokens,
) -> Element<'a, UiMessage> {
    container(
        row![
            text("A stale-version conflict was detected. Reload the latest collection state, then retry the action.")
                .size(fonts.caption)
                .color(theme::MediaServerTheme::WARNING),
            Space::new().width(Length::Fill),
            button("Reload latest")
                .on_press(CollectionsMessage::ReloadAfterConflict(collection_id).into())
                .style(theme::Button::Secondary.style()),
        ]
        .spacing(12)
        .align_y(iced::Alignment::Center),
    )
    .padding(12)
    .style(theme::Container::HeaderAccent.style())
    .into()
}

fn collection_items_section<'a>(
    model: CollectionItemsViewModel,
    item_state: Option<&'a CollectionItemsState>,
    collection_id: ferrex_core::api::types::collections::CollectionId,
    editable: bool,
    action_state: Option<&'a CollectionItemActionState>,
    fonts: &crate::infra::design_tokens::fonts::FontTokens,
) -> Element<'a, UiMessage> {
    let order_help = if editable && model.can_load_more {
        "Load all items before reordering so the saved order is stable"
    } else if editable {
        "Move up/down controls save an explicit stable order"
    } else {
        "Stable collection order"
    };

    let mut section = column![
        row![
            column![
                text("Collection items")
                    .size(fonts.subtitle)
                    .color(theme::MediaServerTheme::TEXT_PRIMARY),
                text(model.status_summary.clone())
                    .size(fonts.caption)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY),
            ]
            .spacing(4),
            Space::new().width(Length::Fill),
            text(order_help)
                .size(fonts.caption)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
        ]
        .align_y(iced::Alignment::Center)
    ]
    .spacing(12);

    if let Some(hidden_summary) = model.hidden_summary.clone() {
        section = section.push(
            text(hidden_summary)
                .size(fonts.caption)
                .color(theme::MediaServerTheme::WARNING),
        );
    }

    if let Some(CollectionItemsLoadState::Error(message)) =
        item_state.map(|state| &state.load_state)
    {
        section = section.push(item_error_banner(collection_id, message));
    }

    if let Some(message) = action_state.and_then(|state| state.error.as_deref())
    {
        section = section.push(item_action_banner(
            collection_id,
            message,
            action_state.is_some_and(|state| state.conflict),
            fonts,
        ));
    } else if action_state.is_some_and(|state| state.conflict) {
        section = section.push(conflict_recovery_banner(collection_id, fonts));
    }

    if model.rows.is_empty() {
        let loading = item_state.is_none_or(|state| {
            matches!(
                state.load_state,
                CollectionItemsLoadState::NotLoaded
                    | CollectionItemsLoadState::Loading
            )
        });
        section = section.push(if loading {
            center_panel(
                "Loading collection items…",
                "Fetching the first page of materialized members.",
                None,
                fonts,
            )
        } else if model.hidden_count > 0 {
            center_panel(
                "No available items to show",
                "Unavailable, missing, or archived members are hidden from the normal detail view.",
                None,
                fonts,
            )
        } else {
            center_panel(
                "No items in this collection",
                if editable {
                    "Search for existing media above to add the first manual item."
                } else {
                    "The API did not return visible materialized members for this collection."
                },
                None,
                fonts,
            )
        });
    } else {
        section = section.push(collection_item_grid(
            model.rows.clone(),
            collection_id,
            editable,
            action_state,
            fonts,
        ));
    }

    if model.can_load_more {
        let label = if matches!(
            item_state.map(|state| &state.load_state),
            Some(CollectionItemsLoadState::LoadingMore)
        ) {
            "Loading more…"
        } else {
            "Load more items"
        };
        let mut load_more =
            button(label).style(theme::Button::Secondary.style());
        if !matches!(
            item_state.map(|state| &state.load_state),
            Some(CollectionItemsLoadState::LoadingMore)
        ) {
            load_more = load_more.on_press(
                CollectionsMessage::LoadMoreItems(collection_id).into(),
            );
        }
        section = section.push(load_more);
    }

    container(section)
        .padding(18)
        .style(theme::Container::Card.style())
        .into()
}

fn item_error_banner<'a>(
    collection_id: ferrex_core::api::types::collections::CollectionId,
    message: &'a str,
) -> Element<'a, UiMessage> {
    container(
        row![
            text(format!("Items failed to load: {message}"))
                .color(theme::MediaServerTheme::ERROR),
            Space::new().width(Length::Fill),
            button("Retry items")
                .on_press(
                    CollectionsMessage::RetryDetailItems(collection_id).into()
                )
                .style(theme::Button::Text.style()),
        ]
        .align_y(iced::Alignment::Center),
    )
    .padding(12)
    .style(theme::Container::HeaderAccent.style())
    .into()
}

fn item_action_banner<'a>(
    collection_id: ferrex_core::api::types::collections::CollectionId,
    message: &'a str,
    conflict: bool,
    fonts: &crate::infra::design_tokens::fonts::FontTokens,
) -> Element<'a, UiMessage> {
    let action = if conflict {
        button("Reload latest")
            .on_press(
                CollectionsMessage::ReloadAfterConflict(collection_id).into(),
            )
            .style(theme::Button::Secondary.style())
    } else {
        button("Retry items")
            .on_press(
                CollectionsMessage::RetryDetailItems(collection_id).into(),
            )
            .style(theme::Button::Text.style())
    };

    container(
        row![
            text(message)
                .size(fonts.caption)
                .color(theme::MediaServerTheme::ERROR),
            Space::new().width(Length::Fill),
            action,
        ]
        .spacing(12)
        .align_y(iced::Alignment::Center),
    )
    .padding(12)
    .style(theme::Container::HeaderAccent.style())
    .into()
}

fn collection_item_grid<'a>(
    rows: Vec<CollectionItemRow>,
    collection_id: ferrex_core::api::types::collections::CollectionId,
    editable: bool,
    action_state: Option<&'a CollectionItemActionState>,
    fonts: &crate::infra::design_tokens::fonts::FontTokens,
) -> Element<'a, UiMessage> {
    let total = rows.len();
    let mut grid = column![].spacing(12);
    for (chunk_index, chunk) in rows.chunks(3).enumerate() {
        let mut cards = Vec::new();
        for (offset, row_model) in chunk.iter().enumerate() {
            cards.push(collection_item_card(
                row_model.clone(),
                collection_id,
                editable,
                chunk_index * 3 + offset,
                total,
                action_state,
                fonts,
            ));
        }
        for _ in chunk.len()..3 {
            cards.push(Space::new().width(Length::FillPortion(1)).into());
        }
        grid = grid.push(row(cards).spacing(12));
    }
    grid.into()
}

fn collection_item_card<'a>(
    row_model: CollectionItemRow,
    collection_id: ferrex_core::api::types::collections::CollectionId,
    editable: bool,
    index: usize,
    total: usize,
    action_state: Option<&'a CollectionItemActionState>,
    fonts: &crate::infra::design_tokens::fonts::FontTokens,
) -> Element<'a, UiMessage> {
    let mut body = column![
        row![
            text(format!("#{:02}", row_model.position))
                .size(fonts.caption)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
            Space::new().width(Length::Fill),
            badge(row_model.media_kind.clone(), fonts.caption),
        ]
        .align_y(iced::Alignment::Center),
        text(row_model.title.clone())
            .size(fonts.body)
            .color(theme::MediaServerTheme::TEXT_PRIMARY),
        text(row_model.subtitle.clone())
            .size(fonts.caption)
            .color(theme::MediaServerTheme::TEXT_SECONDARY),
        text(row_model.availability.clone())
            .size(fonts.caption)
            .color(theme::MediaServerTheme::TEXT_SECONDARY),
    ]
    .spacing(8);

    if editable {
        body = body.push(collection_item_controls(
            collection_id,
            &row_model,
            index,
            total,
            action_state,
        ));
    }

    let content = container(body)
        .padding(14)
        .width(Length::Fill)
        .height(Length::Fixed(if editable { 214.0 } else { 154.0 }))
        .style(theme::Container::HeaderAccent.style());

    if editable {
        content.width(Length::FillPortion(1)).into()
    } else if let Some(action) = row_model.action {
        button(content)
            .on_press(action.shell_message().into())
            .style(theme::Button::MediaCard.style())
            .width(Length::FillPortion(1))
            .into()
    } else {
        content.width(Length::FillPortion(1)).into()
    }
}

fn collection_item_controls<'a>(
    collection_id: ferrex_core::api::types::collections::CollectionId,
    row_model: &CollectionItemRow,
    index: usize,
    total: usize,
    action_state: Option<&'a CollectionItemActionState>,
) -> Element<'a, UiMessage> {
    let in_flight = action_state.and_then(|state| state.in_flight.as_ref());
    let busy = in_flight.is_some();
    let removing_this = matches!(
        in_flight,
        Some(CollectionItemMutationKind::Removing(key)) if key == &row_model.item_key
    );
    let moving_this = matches!(
        in_flight,
        Some(CollectionItemMutationKind::Reordering(key)) if key == &row_model.item_key
    );

    let mut controls = row![].spacing(6).align_y(iced::Alignment::Center);
    if let Some(action) = row_model.action {
        controls = controls.push(
            button("Open")
                .on_press(action.shell_message().into())
                .style(theme::Button::Text.style()),
        );
    }

    let mut move_up = button(if moving_this { "Moving…" } else { "Move up" })
        .style(theme::Button::Secondary.style());
    if !busy && index > 0 {
        move_up = move_up.on_press(
            CollectionsMessage::MoveItem {
                collection_id,
                item_key: row_model.item_key.clone(),
                direction: CollectionItemMoveDirection::Up,
            }
            .into(),
        );
    }

    let mut move_down = button(if moving_this {
        "Moving…"
    } else {
        "Move down"
    })
    .style(theme::Button::Secondary.style());
    if !busy && index + 1 < total {
        move_down = move_down.on_press(
            CollectionsMessage::MoveItem {
                collection_id,
                item_key: row_model.item_key.clone(),
                direction: CollectionItemMoveDirection::Down,
            }
            .into(),
        );
    }

    let mut remove = button(if removing_this {
        "Removing…"
    } else {
        "Remove"
    })
    .style(theme::Button::Destructive.style());
    if !busy {
        remove = remove.on_press(
            CollectionsMessage::RemoveItem {
                collection_id,
                item_key: row_model.item_key.clone(),
            }
            .into(),
        );
    }

    controls.push(move_up).push(move_down).push(remove).into()
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

fn is_collection_member_visible(item: &CollectionMember) -> bool {
    !matches!(
        item.availability.status,
        CollectionMemberAvailabilityStatus::Missing
            | CollectionMemberAvailabilityStatus::Unavailable
            | CollectionMemberAvailabilityStatus::Archived
    )
}

fn collection_item_action(
    item: &CollectionMember,
) -> Option<CollectionItemAction> {
    match item.media_id {
        MediaID::Movie(movie_id) => {
            Some(CollectionItemAction::ViewMovie(movie_id))
        }
        MediaID::Series(series_id) => {
            Some(CollectionItemAction::ViewSeries(series_id))
        }
        MediaID::Episode(episode_id) => {
            Some(CollectionItemAction::ViewEpisode(episode_id))
        }
        MediaID::Season(_) => None,
    }
}

fn availability_label(
    status: CollectionMemberAvailabilityStatus,
) -> &'static str {
    match status {
        CollectionMemberAvailabilityStatus::Available => "Available",
        CollectionMemberAvailabilityStatus::Pending => "Pending availability",
        CollectionMemberAvailabilityStatus::Missing => "Missing",
        CollectionMemberAvailabilityStatus::Unavailable => "Unavailable",
        CollectionMemberAvailabilityStatus::Archived => "Archived",
    }
}

fn source_summary(summary: &CollectionSummary) -> String {
    let source = match summary.source {
        CollectionSource::Manual => "Manual collection",
        CollectionSource::DynamicRule => "Dynamic rule collection",
        CollectionSource::Tmdb => "TMDB-backed collection",
        CollectionSource::System => "System collection",
        CollectionSource::Imported => "Imported collection",
    };
    format!(
        "{source} · {} · {} · {}",
        kind_label(summary.kind),
        visibility_label(summary.visibility),
        presentation_label(summary.presentation)
    )
}

fn provenance_summary(summary: &CollectionSummary) -> String {
    let provenance = &summary.provenance;
    let mut parts =
        vec![format!("Source: {}", source_label(provenance.source))];
    if let Some(imported_from) = provenance.imported_from.as_deref() {
        parts.push(format!("Imported from {imported_from}"));
    }
    if let Some(external_id) = provenance.external_id.as_deref() {
        parts.push(format!("External id {external_id}"));
    }
    if let Some(generated_by) = provenance.generated_by.as_deref() {
        parts.push(format!("Generated by {generated_by}"));
    }
    if let Some(rule_hash) = provenance.rule_hash.as_deref() {
        parts.push(format!("Rule hash {rule_hash}"));
    }
    if let Some(last_refreshed_at) = provenance.last_refreshed_at {
        parts.push(format!(
            "Last refreshed {}",
            format_datetime(last_refreshed_at)
        ));
    }
    parts.join(" · ")
}

fn rule_summary(
    summary: &CollectionSummary,
    rule: Option<&DynamicCollectionRule>,
) -> String {
    rule.map(|rule| {
        format!(
            "Schema v{} · {} · {} · {}",
            rule.schema_version,
            predicate_summary(&rule.predicate),
            sort_policy_summary(&rule.sort),
            limit_policy_summary(&rule.limit)
        )
    })
    .unwrap_or_else(|| {
        if collections::is_manual_collection(summary) {
            "No dynamic rule attached; membership is managed manually"
                .to_string()
        } else {
            "No dynamic rule attached; membership is read-only here".to_string()
        }
    })
}

fn materialization_summary(
    materialization: &ferrex_core::api::types::collections::CollectionMaterializationStatus,
) -> String {
    let mut parts = vec![format!(
        "{} · {}",
        materialization_state_label(materialization.state),
        item_count_label(materialization.item_count)
    )];
    if let Some(generated_at) = materialization.generated_at {
        parts.push(format!("Evaluated at {}", format_datetime(generated_at)));
    }
    if let Some(expires_at) = materialization.expires_at {
        parts.push(format!("Expires {}", format_datetime(expires_at)));
    }
    if let Some(rule_hash) = materialization.rule_hash.as_deref() {
        parts.push(format!("Rule hash {rule_hash}"));
    }
    if let Some(last_error) = materialization.last_error.as_deref() {
        parts.push(format!("Error: {last_error}"));
    }
    if materialization
        .expires_at
        .is_some_and(|expires_at| expires_at < Utc::now())
        && !matches!(
            materialization.state,
            CollectionMaterializationState::Stale
        )
    {
        parts.push("stale".to_string());
    }
    parts.join(" · ")
}

fn materialization_state_label(
    state: CollectionMaterializationState,
) -> &'static str {
    match state {
        CollectionMaterializationState::NotMaterialized => "Not materialized",
        CollectionMaterializationState::Pending => "Pending",
        CollectionMaterializationState::Refreshing => "Refreshing",
        CollectionMaterializationState::Ready => "Ready",
        CollectionMaterializationState::Stale => "Stale",
        CollectionMaterializationState::Failed => "Failed",
    }
}

fn predicate_summary(predicate: &CollectionRulePredicate) -> String {
    match predicate {
        CollectionRulePredicate::All { clauses } if clauses.is_empty() => {
            "all items".to_string()
        }
        CollectionRulePredicate::All { clauses } => {
            format!(
                "all of {} condition{}",
                clauses.len(),
                plural(clauses.len())
            )
        }
        CollectionRulePredicate::Any { clauses } => {
            format!(
                "any of {} condition{}",
                clauses.len(),
                plural(clauses.len())
            )
        }
        CollectionRulePredicate::Not { clause } => {
            format!("not ({})", predicate_summary(clause))
        }
        CollectionRulePredicate::Field {
            field,
            operator,
            value,
        } => format!(
            "{} {} {}",
            rule_field_label(*field),
            rule_operator_label(*operator),
            rule_value_label(value)
        ),
    }
}

fn sort_policy_summary(sort: &CollectionSortPolicy) -> String {
    if sort.keys.is_empty() {
        return "default stable order".to_string();
    }

    sort.keys
        .iter()
        .map(sort_key_summary)
        .collect::<Vec<_>>()
        .join(", ")
}

fn sort_key_summary(key: &CollectionSortKey) -> String {
    let direction = match key.direction {
        CollectionSortDirection::Asc => "ascending",
        CollectionSortDirection::Desc => "descending",
    };
    format!("{:?} {direction}", key.field)
}

fn limit_policy_summary(limit: &CollectionLimitPolicy) -> String {
    let mut parts = Vec::new();
    if let Some(max_items) = limit.max_items {
        parts.push(format!("max {max_items}"));
    }
    if let Some(per_media_type) = limit.per_media_type {
        parts.push(format!("{per_media_type} per media type"));
    }
    parts.push(match limit.window {
        CollectionLimitWindow::All => "all window".to_string(),
        CollectionLimitWindow::Newest => "newest window".to_string(),
        CollectionLimitWindow::Oldest => "oldest window".to_string(),
        CollectionLimitWindow::RecentlyAdded => {
            "recently added window".to_string()
        }
        CollectionLimitWindow::RecentlyUpdated => {
            "recently updated window".to_string()
        }
    });
    parts.join(" · ")
}

fn rule_field_label(field: CollectionRuleField) -> &'static str {
    match field {
        CollectionRuleField::MediaType => "media type",
        CollectionRuleField::LibraryId => "library",
        CollectionRuleField::Title => "title",
        CollectionRuleField::SortTitle => "sort title",
        CollectionRuleField::Genre => "genre",
        CollectionRuleField::ReleaseYear => "release year",
        CollectionRuleField::AddedAt => "added date",
        CollectionRuleField::UpdatedAt => "updated date",
        CollectionRuleField::RuntimeMinutes => "runtime",
        CollectionRuleField::AudienceRating => "audience rating",
        CollectionRuleField::CriticRating => "critic rating",
        CollectionRuleField::WatchStatus => "watch status",
        CollectionRuleField::Availability => "availability",
        CollectionRuleField::TmdbId => "TMDB id",
        CollectionRuleField::ActorName => "actor",
        CollectionRuleField::DirectorName => "director",
    }
}

fn rule_operator_label(operator: CollectionRuleOperator) -> &'static str {
    match operator {
        CollectionRuleOperator::Equals => "equals",
        CollectionRuleOperator::NotEquals => "does not equal",
        CollectionRuleOperator::Contains => "contains",
        CollectionRuleOperator::StartsWith => "starts with",
        CollectionRuleOperator::In => "is in",
        CollectionRuleOperator::GreaterThan => "is greater than",
        CollectionRuleOperator::GreaterThanOrEqual => "is at least",
        CollectionRuleOperator::LessThan => "is less than",
        CollectionRuleOperator::LessThanOrEqual => "is at most",
        CollectionRuleOperator::Between => "is between",
        CollectionRuleOperator::Exists => "exists",
    }
}

fn rule_value_label(value: &CollectionRuleValue) -> String {
    match value {
        CollectionRuleValue::String(value) => value.clone(),
        CollectionRuleValue::Strings(values) => values.join(", "),
        CollectionRuleValue::Integer(value) => value.to_string(),
        CollectionRuleValue::Integers(values) => values
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", "),
        CollectionRuleValue::Decimal(value) => value.clone(),
        CollectionRuleValue::Boolean(value) => value.to_string(),
        CollectionRuleValue::Date(value) => value.clone(),
        CollectionRuleValue::Uuid(value) => value.to_string(),
        CollectionRuleValue::MediaType(kind) => {
            media_kind_label(*kind).to_string()
        }
        CollectionRuleValue::Availability(status) => {
            availability_label(*status).to_string()
        }
    }
}

fn can_refresh_collection(detail: &CollectionDetail) -> bool {
    detail.rule.is_some()
        && (matches!(
            detail.summary.kind,
            CollectionKind::DynamicRule | CollectionKind::System
        ) || matches!(
            detail.summary.source,
            CollectionSource::DynamicRule | CollectionSource::System
        ))
}

fn shelf_placements_summary(placements: &[ShelfPlacement]) -> String {
    if placements.is_empty() {
        return "No shelf placements".to_string();
    }

    placements
        .iter()
        .map(|placement| {
            format!(
                "{:?}/{} at #{}{}",
                placement.surface,
                placement.shelf_key,
                placement.position,
                if placement.pinned { " pinned" } else { "" }
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_datetime(value: DateTime<Utc>) -> String {
    value.format("%b %d, %Y %H:%M UTC").to_string()
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

fn item_count_label(count: u32) -> String {
    item_count_label_u64(count as u64)
}

fn item_count_label_u64(count: u64) -> String {
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
