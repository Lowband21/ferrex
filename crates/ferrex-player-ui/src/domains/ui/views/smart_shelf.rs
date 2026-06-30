use ferrex_player_api::api_types::{
    IntelligenceMediaKind, IntelligenceRunStatus, MediaID,
    SmartShelfDraftSource, SmartShelfDraftValidationSeverity,
};
use ferrex_player_intelligence::{
    ProviderReadiness, SmartShelfAlternateState, SmartShelfDraftState,
    SmartShelfItemState, SmartShelfMessage, SmartShelfPhase,
    SmartShelfRunState, SmartShelfSaveStatus, SmartShelfState,
};
use iced::{
    Element, Length,
    widget::{Space, button, column, container, row, text, text_input},
};

use crate::{
    domains::ui::{
        messages::UiMessage,
        shell_ui::Scope,
        smart_shelf::{
            SmartShelfUiMessage, save_conflict_recovery_label,
            save_status_label,
        },
        theme,
    },
    state::State,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmartShelfComposerSummary {
    pub prompt: String,
    pub template_labels: Vec<String>,
    pub selected_template: Option<String>,
    pub media_scope: String,
    pub item_count: u16,
    pub constraints: String,
    pub provider_status: String,
    pub model: String,
    pub can_start: bool,
    pub fallback: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmartShelfProgressSummary {
    pub status: String,
    pub phase: String,
    pub step: String,
    pub provider_model: String,
    pub skeleton_rows: usize,
    pub can_cancel: bool,
    pub can_retry: bool,
    pub can_edit_prompt: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmartShelfDraftReviewSummary {
    pub title: String,
    pub item_count: usize,
    pub validation_issue_count: usize,
    pub locked_count: usize,
    pub replacement_count: usize,
    pub alternate_count: usize,
    pub can_save: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmartShelfSaveReviewSummary {
    pub title: String,
    pub description: String,
    pub scope: String,
    pub visibility: String,
    pub status: String,
    pub conflict_help: String,
    pub error: Option<String>,
}

pub fn smart_shelf_composer_summary(
    state: &State,
) -> SmartShelfComposerSummary {
    let smart = &state.domains.ui.state.smart_shelf.reducer;
    let composer = &smart.composer;
    let provider_status = provider_status_label(&smart.provider);
    let fallback = smart
        .provider
        .fallback_message()
        .map(|(message, _)| message);
    let selected_template =
        composer.selected_template_id.as_ref().and_then(|id| {
            composer
                .templates
                .iter()
                .find(|template| &template.id == id)
                .map(|template| template.label.clone())
        });

    SmartShelfComposerSummary {
        prompt: composer.prompt.clone(),
        template_labels: composer
            .templates
            .iter()
            .map(|template| template.label.clone())
            .collect(),
        selected_template,
        media_scope: composer_media_scope_label(state),
        item_count: composer.item_count,
        constraints: constraints_label(&composer.constraints),
        provider_status,
        model: composer
            .model
            .clone()
            .or_else(|| provider_model(&smart.provider))
            .unwrap_or_else(|| "Default model".to_string()),
        can_start: smart.provider.allows_start()
            && !composer.prompt.trim().is_empty()
            && !matches!(
                smart.phase,
                SmartShelfPhase::Starting
                    | SmartShelfPhase::Running
                    | SmartShelfPhase::Saving
            ),
        fallback,
    }
}

pub fn smart_shelf_progress_summary(
    smart: &SmartShelfState,
) -> SmartShelfProgressSummary {
    let run = smart.run.as_ref();
    let status = run
        .map(|run| run_status_label(run.status).to_string())
        .unwrap_or_else(|| phase_label(smart.phase).to_string());
    let phase = run
        .and_then(|run| run.current_phase.clone())
        .unwrap_or_else(|| phase_label(smart.phase).to_string());
    let step = run
        .map(step_label)
        .unwrap_or_else(|| "Preparing run".to_string());
    let provider_model = run
        .map(provider_model_from_run)
        .or_else(|| provider_readiness_model(&smart.provider))
        .unwrap_or_else(|| "Provider/model pending".to_string());

    SmartShelfProgressSummary {
        status,
        phase,
        step,
        provider_model,
        skeleton_rows: usize::from(smart.composer.item_count.min(8).max(3)),
        can_cancel: run.is_some_and(SmartShelfRunState::can_cancel),
        can_retry: matches!(
            smart.phase,
            SmartShelfPhase::DraftError
                | SmartShelfPhase::Cancelled
                | SmartShelfPhase::ProviderUnavailable
        ),
        can_edit_prompt: matches!(
            smart.phase,
            SmartShelfPhase::DraftError
                | SmartShelfPhase::Cancelled
                | SmartShelfPhase::ProviderUnavailable
        ),
    }
}

pub fn smart_shelf_draft_review_summary(
    smart: &SmartShelfState,
) -> Option<SmartShelfDraftReviewSummary> {
    let draft = smart.draft.as_ref()?;
    Some(SmartShelfDraftReviewSummary {
        title: draft.title.clone(),
        item_count: draft.items.len(),
        validation_issue_count: draft.validation.issues.len(),
        locked_count: draft.locked_count(),
        replacement_count: draft.replacements_count(),
        alternate_count: draft.alternates.len(),
        can_save: draft.can_save()
            && !matches!(smart.save.status, SmartShelfSaveStatus::Saving),
    })
}

pub fn smart_shelf_save_review_summary(
    smart: &SmartShelfState,
) -> Option<SmartShelfSaveReviewSummary> {
    let draft = smart.draft.as_ref()?;
    let title = smart
        .save
        .confirmation
        .as_ref()
        .map(|confirmation| confirmation.title.clone())
        .unwrap_or_else(|| draft.title.clone());
    let error = smart
        .save
        .conflict
        .as_ref()
        .map(|conflict| conflict.failure.message.clone())
        .or_else(|| {
            smart
                .save
                .last_error
                .as_ref()
                .map(|failure| failure.message.clone())
        });

    Some(SmartShelfSaveReviewSummary {
        title,
        description: draft
            .description
            .clone()
            .unwrap_or_else(|| "No description supplied".to_string()),
        scope: save_scope_label(draft),
        visibility: "Private manual collection".to_string(),
        status: save_status_label(smart.save.status).to_string(),
        conflict_help: "Duplicate media, media-scope mismatches, stale draft versions, and API conflicts stay recoverable here before retrying.".to_string(),
        error,
    })
}

pub fn view_smart_shelf_surface(
    state: &State,
) -> Option<Element<'_, UiMessage>> {
    let surface = &state.domains.ui.state.smart_shelf;
    if !surface.open {
        return None;
    }

    let fonts = &state.domains.ui.state.size_provider.font;
    let smart = &surface.reducer;

    let header = row![
        column![
            text("Smart shelf")
                .size(fonts.title)
                .color(theme::MediaServerTheme::TEXT_PRIMARY),
            text("Generate a grounded private collection without changing exact catalog Search.")
                .size(fonts.caption)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
        ]
        .spacing(4)
        .width(Length::Fill),
        button("Close")
            .on_press(SmartShelfUiMessage::CloseRequested.into())
            .style(theme::Button::Secondary.style()),
    ]
    .align_y(iced::Alignment::Center)
    .spacing(12);

    let mut panel = column![header].spacing(16);

    if let Some(notice) = surface.notice.as_ref() {
        panel = panel.push(notice_banner(notice.message.clone()));
    }

    if surface.confirm_discard {
        panel = panel.push(discard_confirmation(fonts));
    } else {
        panel = panel.push(match smart.save.status {
            SmartShelfSaveStatus::Confirming => {
                save_confirmation_panel(smart, fonts)
            }
            _ => match smart.phase {
                SmartShelfPhase::ProviderUnavailable => {
                    provider_fallback_panel(state, fonts)
                }
                SmartShelfPhase::Starting
                | SmartShelfPhase::Running
                | SmartShelfPhase::Cancelling => progress_panel(smart, fonts),
                SmartShelfPhase::DraftReady
                | SmartShelfPhase::DraftInvalid
                | SmartShelfPhase::Saving
                | SmartShelfPhase::Saved
                | SmartShelfPhase::SaveConflict
                | SmartShelfPhase::SaveError => {
                    draft_review_panel(smart, fonts)
                }
                SmartShelfPhase::DraftError | SmartShelfPhase::Cancelled => {
                    column![
                        progress_panel(smart, fonts),
                        composer_panel(state, fonts)
                    ]
                    .spacing(16)
                    .into()
                }
                SmartShelfPhase::Idle => composer_panel(state, fonts),
            },
        });
    }

    let panel = container(panel)
        .padding(24)
        .width(Length::Fixed(680.0))
        .height(Length::Fill)
        .style(theme::Container::Card.style());

    Some(
        container(
            row![Space::new().width(Length::Fill), panel]
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .padding([72, 24])
        .width(Length::Fill)
        .height(Length::Fill)
        .style(theme::Container::HeaderAccent.style())
        .into(),
    )
}

fn composer_panel<'a>(
    state: &'a State,
    fonts: &crate::infra::design_tokens::fonts::FontTokens,
) -> Element<'a, UiMessage> {
    let smart = &state.domains.ui.state.smart_shelf.reducer;
    let composer = &smart.composer;
    let summary = smart_shelf_composer_summary(state);

    let mut template_row = row![].spacing(8).align_y(iced::Alignment::Center);
    for template in &composer.templates {
        let selected = composer.selected_template_id.as_deref()
            == Some(template.id.as_str());
        let style = if selected {
            theme::Button::Primary.style()
        } else {
            theme::Button::Secondary.style()
        };
        template_row = template_row.push(
            button(text(template.label.as_str()).size(fonts.caption))
                .on_press(
                    SmartShelfUiMessage::Reducer(
                        SmartShelfMessage::TemplateSelected(
                            template.id.clone(),
                        ),
                    )
                    .into(),
                )
                .style(style),
        );
    }

    let mut count_row = row![].spacing(8).align_y(iced::Alignment::Center);
    for count in [6_u16, 8, 12] {
        let style = if composer.item_count == count {
            theme::Button::Primary.style()
        } else {
            theme::Button::Secondary.style()
        };
        count_row = count_row.push(
            button(text(count.to_string()).size(fonts.caption))
                .on_press(
                    SmartShelfUiMessage::Reducer(
                        SmartShelfMessage::ItemCountChanged(count),
                    )
                    .into(),
                )
                .style(style),
        );
    }

    let mut scope_row = row![
        badge(summary.media_scope.clone(), fonts.caption),
        button("All libraries")
            .on_press(
                SmartShelfUiMessage::Reducer(
                    SmartShelfMessage::LibrarySelected(None)
                )
                .into(),
            )
            .style(theme::Button::Secondary.style()),
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center);

    if let Scope::Library(library_id) = state.domains.ui.state.scope {
        scope_row = scope_row.push(
            button("Use current library")
                .on_press(
                    SmartShelfUiMessage::Reducer(
                        SmartShelfMessage::LibrarySelected(Some(library_id)),
                    )
                    .into(),
                )
                .style(theme::Button::Secondary.style()),
        );
    }

    let provider_color = if smart.provider.allows_start() {
        theme::MediaServerTheme::SUCCESS
    } else {
        theme::MediaServerTheme::WARNING
    };

    let mut start_button =
        button("Generate draft").style(theme::Button::Primary.style());
    if summary.can_start {
        start_button = start_button.on_press(
            SmartShelfUiMessage::Reducer(SmartShelfMessage::StartRequested)
                .into(),
        );
    }

    let mut fields = column![
        text("Describe the shelf")
            .size(fonts.subtitle)
            .color(theme::MediaServerTheme::TEXT_PRIMARY),
        text_input("e.g. Moody movies for a rainy Friday", &composer.prompt)
            .padding(12)
            .size(fonts.body)
            .style(theme::TextInput::style())
            .on_input(|value| SmartShelfUiMessage::Reducer(
                SmartShelfMessage::PromptChanged(value)
            )
            .into())
            .on_submit(
                SmartShelfUiMessage::Reducer(SmartShelfMessage::StartRequested)
                    .into(),
            ),
        text("Templates")
            .size(fonts.caption)
            .color(theme::MediaServerTheme::TEXT_SECONDARY),
        template_row,
        text("Media scope")
            .size(fonts.caption)
            .color(theme::MediaServerTheme::TEXT_SECONDARY),
        scope_row,
        row![
            column![
                text("Item count")
                    .size(fonts.caption)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY),
                count_row,
            ]
            .spacing(8),
            Space::new().width(Length::Fill),
            column![
                text("Provider/model")
                    .size(fonts.caption)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY),
                text(format!(
                    "{} · {}",
                    summary.provider_status, summary.model
                ))
                .size(fonts.caption)
                .color(provider_color),
            ]
            .spacing(8),
        ]
        .align_y(iced::Alignment::Start),
        text_input(
            "Model override (optional)",
            composer.model.as_deref().unwrap_or("")
        )
        .padding(12)
        .size(fonts.body)
        .style(theme::TextInput::style())
        .on_input(|value| SmartShelfUiMessage::Reducer(
            SmartShelfMessage::ModelChanged(Some(value))
        )
        .into()),
        text(format!("Optional constraints: {}", summary.constraints))
            .size(fonts.caption)
            .color(theme::MediaServerTheme::TEXT_SECONDARY),
        row![
            start_button,
            button("Refresh provider")
                .on_press(
                    SmartShelfUiMessage::Reducer(
                        SmartShelfMessage::ProviderRefreshRequested,
                    )
                    .into(),
                )
                .style(theme::Button::Secondary.style()),
        ]
        .spacing(12)
        .align_y(iced::Alignment::Center),
    ]
    .spacing(12);

    if let Some(error) = composer.validation_error.as_ref() {
        fields = fields.push(
            text(error.message.clone())
                .size(fonts.caption)
                .color(theme::MediaServerTheme::ERROR),
        );
    }

    if let Some(fallback) = summary.fallback {
        fields = fields.push(
            text(fallback)
                .size(fonts.caption)
                .color(theme::MediaServerTheme::WARNING),
        );
    }

    container(fields)
        .padding(18)
        .style(theme::Container::Card.style())
        .into()
}

fn provider_fallback_panel<'a>(
    state: &'a State,
    fonts: &crate::infra::design_tokens::fonts::FontTokens,
) -> Element<'a, UiMessage> {
    let surface = &state.domains.ui.state.smart_shelf;
    let message = surface
        .provider_fallback
        .as_ref()
        .map(|fallback| fallback.message.clone())
        .or_else(|| {
            surface
                .reducer
                .provider
                .fallback_message()
                .map(|(message, _)| message)
        })
        .unwrap_or_else(|| "Provider readiness is unavailable".to_string());
    let retryable = surface
        .provider_fallback
        .as_ref()
        .map(|fallback| fallback.retryable)
        .unwrap_or(true);

    let mut fallback_actions = row![
        button("Edit prompt")
            .on_press(
                SmartShelfUiMessage::Reducer(
                    SmartShelfMessage::EditPromptRequested
                )
                .into(),
            )
            .style(theme::Button::Secondary.style()),
    ]
    .spacing(12);
    if retryable {
        fallback_actions = fallback_actions.push(
            button("Retry provider check")
                .on_press(
                    SmartShelfUiMessage::Reducer(
                        SmartShelfMessage::ProviderRefreshRequested,
                    )
                    .into(),
                )
                .style(theme::Button::Primary.style()),
        );
    }

    column![
        container(
            column![
                text("Provider fallback")
                    .size(fonts.subtitle)
                    .color(theme::MediaServerTheme::TEXT_PRIMARY),
                text(message)
                    .size(fonts.body)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY),
                fallback_actions,
            ]
            .spacing(12),
        )
        .padding(18)
        .style(theme::Container::Card.style()),
        composer_panel(state, fonts),
    ]
    .spacing(16)
    .into()
}

fn progress_panel<'a>(
    smart: &'a SmartShelfState,
    fonts: &crate::infra::design_tokens::fonts::FontTokens,
) -> Element<'a, UiMessage> {
    let summary = smart_shelf_progress_summary(smart);
    let mut skeleton = column![].spacing(8);
    for index in 0..summary.skeleton_rows {
        skeleton = skeleton.push(
            container(
                row![
                    text(format!("#{}", index + 1))
                        .size(fonts.caption)
                        .color(theme::MediaServerTheme::TEXT_SECONDARY),
                    text("Finding grounded media and reasons…")
                        .size(fonts.caption)
                        .color(theme::MediaServerTheme::TEXT_SECONDARY),
                ]
                .spacing(10),
            )
            .padding(10)
            .style(theme::Container::HeaderAccent.style()),
        );
    }

    let mut actions = row![].spacing(12).align_y(iced::Alignment::Center);
    if summary.can_cancel {
        actions = actions.push(
            button("Cancel")
                .on_press(
                    SmartShelfUiMessage::Reducer(
                        SmartShelfMessage::CancelRequested,
                    )
                    .into(),
                )
                .style(theme::Button::Secondary.style()),
        );
    }
    if summary.can_retry {
        actions = actions.push(
            button("Retry")
                .on_press(
                    SmartShelfUiMessage::Reducer(
                        SmartShelfMessage::RetryRequested,
                    )
                    .into(),
                )
                .style(theme::Button::Primary.style()),
        );
    }
    if summary.can_edit_prompt {
        actions = actions.push(
            button("Edit prompt")
                .on_press(
                    SmartShelfUiMessage::Reducer(
                        SmartShelfMessage::EditPromptRequested,
                    )
                    .into(),
                )
                .style(theme::Button::Secondary.style()),
        );
    }

    container(
        column![
            text("Generating draft")
                .size(fonts.subtitle)
                .color(theme::MediaServerTheme::TEXT_PRIMARY),
            row![
                badge(summary.status, fonts.caption),
                badge(summary.phase, fonts.caption),
                badge(summary.step, fonts.caption),
            ]
            .spacing(8),
            text(summary.provider_model)
                .size(fonts.caption)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
            skeleton,
            actions,
        ]
        .spacing(12),
    )
    .padding(18)
    .style(theme::Container::Card.style())
    .into()
}

fn draft_review_panel<'a>(
    smart: &'a SmartShelfState,
    fonts: &crate::infra::design_tokens::fonts::FontTokens,
) -> Element<'a, UiMessage> {
    let Some(draft) = smart.draft.as_ref() else {
        return empty_panel(
            "No draft loaded",
            "Run the composer before reviewing a smart shelf.",
            fonts,
        );
    };
    let summary =
        smart_shelf_draft_review_summary(smart).expect("draft summary");

    let mut items = column![].spacing(12);
    for item in &draft.items {
        items = items.push(draft_item_card(item, draft, fonts));
    }

    let mut validation = column![].spacing(8);
    for issue in &draft.validation.issues {
        let color =
            if issue.severity == SmartShelfDraftValidationSeverity::Error {
                theme::MediaServerTheme::ERROR
            } else {
                theme::MediaServerTheme::WARNING
            };
        validation = validation.push(
            text(format!("{:?}: {}", issue.code, issue.message))
                .size(fonts.caption)
                .color(color),
        );
    }

    let mut save_button =
        button("Save private collection").style(theme::Button::Primary.style());
    if summary.can_save {
        save_button = save_button.on_press(
            SmartShelfUiMessage::Reducer(SmartShelfMessage::SaveRequested)
                .into(),
        );
    }

    let mut actions = row![
        button("Regenerate unlocked")
            .on_press(
                SmartShelfUiMessage::Reducer(
                    SmartShelfMessage::RegenerateUnlockedRequested,
                )
                .into(),
            )
            .style(theme::Button::Secondary.style()),
        button("Discard")
            .on_press(
                SmartShelfUiMessage::Reducer(
                    SmartShelfMessage::DiscardRequested
                )
                .into(),
            )
            .style(theme::Button::Secondary.style()),
        save_button,
    ]
    .spacing(12)
    .align_y(iced::Alignment::Center);

    if matches!(smart.save.status, SmartShelfSaveStatus::Saving) {
        actions = actions.push(badge("Saving…", fonts.caption));
    }

    let mut content = column![
        text(summary.title)
            .size(fonts.subtitle)
            .color(theme::MediaServerTheme::TEXT_PRIMARY),
        text(format!(
            "{} items · {} locked · {} replacement{} · {} alternate{}",
            summary.item_count,
            summary.locked_count,
            summary.replacement_count,
            if summary.replacement_count == 1 {
                ""
            } else {
                "s"
            },
            summary.alternate_count,
            if summary.alternate_count == 1 {
                ""
            } else {
                "s"
            },
        ))
        .size(fonts.caption)
        .color(theme::MediaServerTheme::TEXT_SECONDARY),
    ]
    .spacing(12);

    if summary.validation_issue_count > 0 {
        content = content.push(validation);
    }

    content = content.push(items).push(actions);

    if let Some(save_summary) = smart_shelf_save_review_summary(smart)
        && let Some(error) = save_summary.error
    {
        content = content.push(
            container(
                column![
                    text(save_summary.status)
                        .size(fonts.caption)
                        .color(theme::MediaServerTheme::WARNING),
                    text(error)
                        .size(fonts.caption)
                        .color(theme::MediaServerTheme::ERROR),
                    text(save_summary.conflict_help)
                        .size(fonts.caption)
                        .color(theme::MediaServerTheme::TEXT_SECONDARY),
                    conflict_recovery_actions(smart, fonts),
                ]
                .spacing(8),
            )
            .padding(12)
            .style(theme::Container::HeaderAccent.style()),
        );
    }

    container(content)
        .padding(18)
        .style(theme::Container::Card.style())
        .into()
}

fn draft_item_card<'a>(
    item: &'a SmartShelfItemState,
    draft: &'a SmartShelfDraftState,
    fonts: &crate::infra::design_tokens::fonts::FontTokens,
) -> Element<'a, UiMessage> {
    let mut source_row = row![].spacing(8);
    for source in &item.sources {
        source_row =
            source_row.push(badge(source_label(source), fonts.caption));
    }
    if item.sources.is_empty() {
        source_row = source_row.push(badge("No source chip", fonts.caption));
    }

    let matching_alternates = draft
        .alternates
        .iter()
        .filter(|alternate| {
            alternate.target_ordinal.is_none()
                || alternate.target_ordinal == Some(item.ordinal)
        })
        .collect::<Vec<_>>();
    let mut alternates = column![].spacing(8);
    for alternate in matching_alternates {
        alternates = alternates.push(alternate_row(item, alternate, fonts));
    }

    let lock_label = if item.locked { "Unlock" } else { "Lock" };
    let replacement = item
        .replacement_of
        .map(|media_id| {
            format!("Replacement for {}", media_id_label(&media_id))
        })
        .unwrap_or_else(|| "Original selection".to_string());

    container(
        column![
            row![
                text(format!(
                    "{}. {}",
                    item.ordinal,
                    item.title
                        .clone()
                        .unwrap_or_else(|| media_id_label(&item.media_id))
                ))
                .size(fonts.body)
                .color(theme::MediaServerTheme::TEXT_PRIMARY),
                Space::new().width(Length::Fill),
                button(lock_label)
                    .on_press(
                        SmartShelfUiMessage::Reducer(
                            SmartShelfMessage::ToggleLock(item.media_id,)
                        )
                        .into(),
                    )
                    .style(theme::Button::Secondary.style()),
            ]
            .align_y(iced::Alignment::Center),
            text(
                item.subtitle
                    .clone()
                    .unwrap_or_else(|| media_id_label(&item.media_id))
            )
            .size(fonts.caption)
            .color(theme::MediaServerTheme::TEXT_SECONDARY),
            text(item.reason.clone().unwrap_or_else(|| {
                "No grounded reason supplied".to_string()
            }))
            .size(fonts.caption)
            .color(theme::MediaServerTheme::TEXT_SECONDARY),
            source_row,
            badge(replacement, fonts.caption),
            alternates,
        ]
        .spacing(8),
    )
    .padding(12)
    .style(theme::Container::HeaderAccent.style())
    .into()
}

fn alternate_row<'a>(
    target: &'a SmartShelfItemState,
    alternate: &'a SmartShelfAlternateState,
    fonts: &crate::infra::design_tokens::fonts::FontTokens,
) -> Element<'a, UiMessage> {
    let mut source_row = row![].spacing(6);
    for source in &alternate.sources {
        source_row =
            source_row.push(badge(source_label(source), fonts.caption));
    }
    if alternate.sources.is_empty() {
        source_row = source_row.push(badge("No source chip", fonts.caption));
    }

    row![
        column![
            text(
                alternate
                    .title
                    .clone()
                    .unwrap_or_else(|| media_id_label(&alternate.media_id))
            )
            .size(fonts.caption)
            .color(theme::MediaServerTheme::TEXT_PRIMARY),
            text(
                alternate
                    .reason
                    .clone()
                    .unwrap_or_else(|| "Alternate replacement".to_string())
            )
            .size(fonts.caption)
            .color(theme::MediaServerTheme::TEXT_SECONDARY),
            source_row,
        ]
        .spacing(4)
        .width(Length::Fill),
        button("Replace")
            .on_press(
                SmartShelfUiMessage::Reducer(
                    SmartShelfMessage::ReplaceWithAlternate {
                        target_media_id: target.media_id,
                        alternate_media_id: alternate.media_id,
                    }
                )
                .into(),
            )
            .style(theme::Button::Secondary.style()),
    ]
    .align_y(iced::Alignment::Center)
    .spacing(8)
    .into()
}

fn save_confirmation_panel<'a>(
    smart: &'a SmartShelfState,
    fonts: &crate::infra::design_tokens::fonts::FontTokens,
) -> Element<'a, UiMessage> {
    let Some(summary) = smart_shelf_save_review_summary(smart) else {
        return empty_panel(
            "Nothing to save",
            "Load a valid draft first.",
            fonts,
        );
    };

    container(
        column![
            text("Confirm save")
                .size(fonts.subtitle)
                .color(theme::MediaServerTheme::TEXT_PRIMARY),
            text(format!("Title: {}", summary.title))
                .size(fonts.body)
                .color(theme::MediaServerTheme::TEXT_PRIMARY),
            text(format!("Description: {}", summary.description))
                .size(fonts.caption)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
            text(format!("Scope: {}", summary.scope))
                .size(fonts.caption)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
            text(format!("Visibility: {}", summary.visibility))
                .size(fonts.caption)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
            text(summary.conflict_help)
                .size(fonts.caption)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
            row![
                button("Back to review")
                    .on_press(
                        SmartShelfUiMessage::CancelSaveConfirmation.into()
                    )
                    .style(theme::Button::Secondary.style()),
                button("Save collection")
                    .on_press(
                        SmartShelfUiMessage::Reducer(
                            SmartShelfMessage::SaveConfirmed
                        )
                        .into(),
                    )
                    .style(theme::Button::Primary.style()),
            ]
            .spacing(12)
        ]
        .spacing(12),
    )
    .padding(18)
    .style(theme::Container::Card.style())
    .into()
}

fn conflict_recovery_actions<'a>(
    smart: &'a SmartShelfState,
    fonts: &crate::infra::design_tokens::fonts::FontTokens,
) -> Element<'a, UiMessage> {
    let mut actions = row![].spacing(8);
    if let Some(conflict) = smart.save.conflict.as_ref() {
        for action in &conflict.recovery_actions {
            actions = actions.push(
                button(
                    text(save_conflict_recovery_label(*action))
                        .size(fonts.caption),
                )
                .on_press(
                    SmartShelfUiMessage::Reducer(
                        SmartShelfMessage::RecoverSaveConflict(*action),
                    )
                    .into(),
                )
                .style(theme::Button::Secondary.style()),
            );
        }
    } else {
        actions = actions.push(
            button("Retry")
                .on_press(
                    SmartShelfUiMessage::Reducer(
                        SmartShelfMessage::SaveConfirmed,
                    )
                    .into(),
                )
                .style(theme::Button::Secondary.style()),
        );
    }
    actions.into()
}

fn discard_confirmation(
    fonts: &crate::infra::design_tokens::fonts::FontTokens,
) -> Element<'_, UiMessage> {
    container(
        column![
            text("Discard smart-shelf work?")
                .size(fonts.subtitle)
                .color(theme::MediaServerTheme::TEXT_PRIMARY),
            text("The current prompt, progress, draft selections, locks, replacements, and save confirmation will be cleared.")
                .size(fonts.caption)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
            row![
                button("Keep editing")
                    .on_press(SmartShelfUiMessage::CancelDiscard.into())
                    .style(theme::Button::Secondary.style()),
                button("Discard")
                    .on_press(SmartShelfUiMessage::ConfirmDiscard.into())
                    .style(theme::Button::Primary.style()),
            ]
            .spacing(12),
        ]
        .spacing(12),
    )
    .padding(18)
    .style(theme::Container::Card.style())
    .into()
}

fn empty_panel<'a>(
    title: impl Into<String>,
    body: impl Into<String>,
    fonts: &crate::infra::design_tokens::fonts::FontTokens,
) -> Element<'a, UiMessage> {
    container(
        column![
            text(title.into())
                .size(fonts.subtitle)
                .color(theme::MediaServerTheme::TEXT_PRIMARY),
            text(body.into())
                .size(fonts.caption)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
        ]
        .spacing(8),
    )
    .padding(18)
    .style(theme::Container::Card.style())
    .into()
}

fn notice_banner<'a>(message: String) -> Element<'a, UiMessage> {
    container(
        row![
            text(message).color(theme::MediaServerTheme::TEXT_PRIMARY),
            Space::new().width(Length::Fill),
            button("Dismiss")
                .on_press(SmartShelfUiMessage::DismissNotice.into())
                .style(theme::Button::Text.style()),
        ]
        .align_y(iced::Alignment::Center),
    )
    .padding(12)
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

fn provider_status_label(readiness: &ProviderReadiness) -> String {
    match readiness {
        ProviderReadiness::Unknown => "Provider unknown".to_string(),
        ProviderReadiness::Checking => "Checking provider".to_string(),
        ProviderReadiness::Ready { provider, .. } => {
            format!("{provider} ready")
        }
        ProviderReadiness::Degraded {
            provider, message, ..
        } => message
            .as_ref()
            .map(|message| format!("{provider} degraded: {message}"))
            .unwrap_or_else(|| format!("{provider} degraded")),
        ProviderReadiness::Unavailable { message, .. } => {
            format!("Unavailable: {message}")
        }
    }
}

fn provider_model(readiness: &ProviderReadiness) -> Option<String> {
    match readiness {
        ProviderReadiness::Ready { model, .. }
        | ProviderReadiness::Degraded { model, .. } => model.clone(),
        ProviderReadiness::Unknown
        | ProviderReadiness::Checking
        | ProviderReadiness::Unavailable { .. } => None,
    }
}

fn provider_readiness_model(readiness: &ProviderReadiness) -> Option<String> {
    match readiness {
        ProviderReadiness::Ready { provider, model }
        | ProviderReadiness::Degraded {
            provider, model, ..
        } => Some(
            model
                .as_ref()
                .map(|model| format!("{provider} · {model}"))
                .unwrap_or_else(|| provider.clone()),
        ),
        ProviderReadiness::Unknown
        | ProviderReadiness::Checking
        | ProviderReadiness::Unavailable { .. } => None,
    }
}

fn provider_model_from_run(run: &SmartShelfRunState) -> String {
    match (&run.provider, &run.model) {
        (Some(provider), Some(model)) => format!("{provider} · {model}"),
        (Some(provider), None) => provider.clone(),
        (None, Some(model)) => model.clone(),
        (None, None) => "Provider/model pending".to_string(),
    }
}

fn run_status_label(status: IntelligenceRunStatus) -> &'static str {
    match status {
        IntelligenceRunStatus::Queued => "Queued",
        IntelligenceRunStatus::Running => "Running",
        IntelligenceRunStatus::Succeeded => "Succeeded",
        IntelligenceRunStatus::Failed => "Failed",
        IntelligenceRunStatus::Cancelled => "Cancelled",
    }
}

fn phase_label(phase: SmartShelfPhase) -> &'static str {
    match phase {
        SmartShelfPhase::Idle => "Composer",
        SmartShelfPhase::ProviderUnavailable => "Provider fallback",
        SmartShelfPhase::Starting => "Starting",
        SmartShelfPhase::Running => "Generating",
        SmartShelfPhase::Cancelling => "Cancelling",
        SmartShelfPhase::Cancelled => "Cancelled",
        SmartShelfPhase::DraftReady => "Draft ready",
        SmartShelfPhase::DraftInvalid => "Draft needs review",
        SmartShelfPhase::DraftError => "Draft error",
        SmartShelfPhase::Saving => "Saving",
        SmartShelfPhase::Saved => "Saved",
        SmartShelfPhase::SaveConflict => "Save conflict",
        SmartShelfPhase::SaveError => "Save error",
    }
}

fn step_label(run: &SmartShelfRunState) -> String {
    match (run.current_step, run.max_steps) {
        (Some(current), Some(max)) => format!("Step {current}/{max}"),
        (Some(current), None) => format!("Step {current}"),
        _ => "Preparing grounded draft".to_string(),
    }
}

fn composer_media_scope_label(state: &State) -> String {
    let composer = &state.domains.ui.state.smart_shelf.reducer.composer;
    let library = composer
        .library_id
        .map(|library_id| format!("Library {}", library_id))
        .unwrap_or_else(|| "All libraries".to_string());
    let kinds = composer
        .media_kinds
        .iter()
        .map(|kind| media_kind_label(*kind))
        .collect::<Vec<_>>()
        .join(" + ");
    format!("{library} · {kinds}")
}

fn media_kind_label(kind: IntelligenceMediaKind) -> &'static str {
    match kind {
        IntelligenceMediaKind::Movie => "movies",
        IntelligenceMediaKind::Series => "series",
        IntelligenceMediaKind::Season => "seasons",
        IntelligenceMediaKind::Episode => "episodes",
    }
}

fn constraints_label(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "No extra constraints".to_string(),
        serde_json::Value::Object(object) if object.is_empty() => {
            "No extra constraints".to_string()
        }
        serde_json::Value::Object(object) => object
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(", "),
        _ => "Template constraints active".to_string(),
    }
}

fn save_scope_label(draft: &SmartShelfDraftState) -> String {
    let kinds = draft
        .items
        .iter()
        .map(|item| match item.media_id {
            MediaID::Movie(_) => "movies",
            MediaID::Series(_) => "series",
            MediaID::Season(_) => "seasons",
            MediaID::Episode(_) => "episodes",
        })
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .join(" + ");
    format!(
        "Accepted {} item{}{}",
        draft.items.len(),
        if draft.items.len() == 1 { "" } else { "s" },
        if kinds.is_empty() {
            "".to_string()
        } else {
            format!(" ({kinds})")
        }
    )
}

fn media_id_label(media_id: &MediaID) -> String {
    match media_id {
        MediaID::Movie(id) => format!("movie {}", id),
        MediaID::Series(id) => format!("series {}", id),
        MediaID::Season(id) => format!("season {}", id),
        MediaID::Episode(id) => format!("episode {}", id),
    }
}

fn source_label(source: &SmartShelfDraftSource) -> String {
    source
        .label
        .clone()
        .or_else(|| source.field.clone())
        .or_else(|| {
            source.evidence.as_ref().map(|summary| summary.text.clone())
        })
        .unwrap_or_else(|| "Grounded source".to_string())
}
