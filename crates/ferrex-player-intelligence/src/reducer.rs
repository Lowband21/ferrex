//! Smart-shelf reducer implementation.

use ferrex_player_api::api_types::{
    IntelligenceRunCancelRequest, IntelligenceRunStatus,
};
use uuid::Uuid;

use crate::{
    ProviderReadiness, SmartShelfCommand, SmartShelfDraftState,
    SmartShelfFailure, SmartShelfFailureCode, SmartShelfIntent,
    SmartShelfMessage, SmartShelfNotice, SmartShelfPhase, SmartShelfRunState,
    SmartShelfSaveConflictRecovery, SmartShelfSaveStatus, SmartShelfState,
    state::{
        already_saved_failure, failure_from_run_error, is_terminal_status,
    },
};

/// Commands and UI/application intents produced by a reducer step.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SmartShelfTransition {
    /// Side-effect commands for the app shell to execute.
    pub commands: Vec<SmartShelfCommand>,
    /// UI/application intents for the app shell to render/route.
    pub intents: Vec<SmartShelfIntent>,
}

impl SmartShelfTransition {
    /// Create an empty transition.
    pub fn none() -> Self {
        Self::default()
    }

    /// Append a side-effect command.
    pub fn command(&mut self, command: SmartShelfCommand) {
        self.commands.push(command);
    }

    /// Append a UI/application intent.
    pub fn intent(&mut self, intent: SmartShelfIntent) {
        self.intents.push(intent);
    }

    /// Whether no command or intent was produced.
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty() && self.intents.is_empty()
    }
}

/// Apply one smart-shelf message to state and return commands/intents for the shell.
pub fn reduce(
    state: &mut SmartShelfState,
    message: SmartShelfMessage,
) -> SmartShelfTransition {
    let mut transition = SmartShelfTransition::none();

    match message {
        SmartShelfMessage::ProviderRefreshRequested => {
            state.provider = ProviderReadiness::Checking;
            transition.command(SmartShelfCommand::FetchProviderStatus);
        }
        SmartShelfMessage::ProviderStatusLoaded(status) => {
            let readiness = ProviderReadiness::from_status(&status);
            state.provider = readiness.clone();
            match readiness {
                ProviderReadiness::Ready { .. } => {
                    if state.phase == SmartShelfPhase::ProviderUnavailable {
                        state.phase = SmartShelfPhase::Idle;
                    }
                    state.last_error = None;
                }
                ProviderReadiness::Degraded { message, .. } => {
                    if state.phase == SmartShelfPhase::ProviderUnavailable {
                        state.phase = SmartShelfPhase::Idle;
                    }
                    if let Some(message) = message {
                        transition.intent(SmartShelfIntent::ShowNotice(
                            SmartShelfNotice::warning(message),
                        ));
                    }
                }
                ProviderReadiness::Unavailable { message, retryable } => {
                    state.phase = SmartShelfPhase::ProviderUnavailable;
                    state.last_error =
                        Some(SmartShelfFailure::provider_unavailable(
                            message.clone(),
                            retryable,
                        ));
                    transition.intent(SmartShelfIntent::ShowProviderFallback {
                        message,
                        retryable,
                    });
                }
                ProviderReadiness::Unknown | ProviderReadiness::Checking => {}
            }
        }
        SmartShelfMessage::ProviderStatusFailed(failure) => {
            state.provider = ProviderReadiness::Unavailable {
                message: failure.message.clone(),
                retryable: failure.retryable,
            };
            state.phase = SmartShelfPhase::ProviderUnavailable;
            state.last_error = Some(failure.clone());
            transition.intent(SmartShelfIntent::ShowProviderFallback {
                message: failure.message,
                retryable: failure.retryable,
            });
        }
        SmartShelfMessage::PromptChanged(prompt) => {
            state.composer.prompt = prompt;
            state.composer.selected_template_id = None;
            state.composer.validation_error = None;
            if matches!(
                state.phase,
                SmartShelfPhase::DraftError
                    | SmartShelfPhase::DraftInvalid
                    | SmartShelfPhase::SaveError
            ) {
                state.phase = SmartShelfPhase::Idle;
            }
        }
        SmartShelfMessage::TemplateSelected(template_id) => {
            if !state.composer.select_template(&template_id) {
                let failure = SmartShelfFailure::validation(format!(
                    "Smart-shelf template '{template_id}' is not available"
                ));
                state.composer.validation_error = Some(failure.clone());
                state.last_error = Some(failure.clone());
                transition.intent(SmartShelfIntent::ShowNotice(
                    SmartShelfNotice::error(failure.message),
                ));
            }
        }
        SmartShelfMessage::TemplateCleared => {
            state.composer.clear_template();
        }
        SmartShelfMessage::LibrarySelected(library_id) => {
            state.composer.library_id = library_id;
        }
        SmartShelfMessage::ItemCountChanged(item_count) => {
            state.composer.item_count =
                ferrex_player_api::api_types::clamp_smart_shelf_item_count(
                    item_count,
                );
        }
        SmartShelfMessage::ModelChanged(model) => {
            state.composer.model = model.and_then(|value| {
                let trimmed = value.trim().to_string();
                (!trimmed.is_empty()).then_some(trimmed)
            });
        }
        SmartShelfMessage::StartRequested => {
            start_from_composer(state, &mut transition);
        }
        SmartShelfMessage::StartAccepted(response) => {
            let run = SmartShelfRunState::from_start(response);
            let run_id = run.run_id;
            let status = run.status;
            state.run = Some(run);
            match status {
                IntelligenceRunStatus::Queued
                | IntelligenceRunStatus::Running => {
                    state.phase = SmartShelfPhase::Running;
                    transition.command(SmartShelfCommand::PollRun { run_id });
                }
                IntelligenceRunStatus::Succeeded => {
                    state.phase = SmartShelfPhase::Running;
                    transition.command(SmartShelfCommand::PollRun { run_id });
                }
                IntelligenceRunStatus::Failed => {
                    let failure = SmartShelfFailure::unknown(
                        "Smart-shelf run failed before progress details were available",
                        true,
                    );
                    state.phase = SmartShelfPhase::DraftError;
                    state.last_error = Some(failure.clone());
                    transition
                        .intent(SmartShelfIntent::ShowDraftError(failure));
                }
                IntelligenceRunStatus::Cancelled => {
                    state.phase = SmartShelfPhase::Cancelled;
                    transition.intent(SmartShelfIntent::ShowNotice(
                        SmartShelfNotice::info("Smart-shelf run was cancelled"),
                    ));
                }
            }
        }
        SmartShelfMessage::StartFailed(failure) => {
            state.phase = if failure.is_provider_unavailable() {
                SmartShelfPhase::ProviderUnavailable
            } else {
                SmartShelfPhase::DraftError
            };
            state.last_error = Some(failure.clone());
            if failure.is_provider_unavailable() {
                transition.intent(SmartShelfIntent::ShowProviderFallback {
                    message: failure.message,
                    retryable: failure.retryable,
                });
            } else {
                transition.intent(SmartShelfIntent::ShowDraftError(failure));
            }
        }
        SmartShelfMessage::RunProgressLoaded(response) => {
            let run_id = response.run_id;
            let status = response.status;
            let terminal = response.terminal || is_terminal_status(status);
            state
                .run
                .get_or_insert_with(|| {
                    SmartShelfRunState::from_status(&response)
                })
                .apply_status(&response);

            match status {
                IntelligenceRunStatus::Queued
                | IntelligenceRunStatus::Running => {
                    state.phase = SmartShelfPhase::Running;
                    if !terminal {
                        transition
                            .command(SmartShelfCommand::PollRun { run_id });
                    }
                }
                IntelligenceRunStatus::Succeeded => {
                    if let Some(artifact_id) =
                        response.draft_artifact_ids.first().copied()
                    {
                        state.phase = SmartShelfPhase::Running;
                        state.last_draft_artifact_id = Some(artifact_id);
                        transition.command(SmartShelfCommand::FetchDraft {
                            artifact_id,
                        });
                    } else {
                        let failure = SmartShelfFailure::unknown(
                            "Smart-shelf run finished without a draft artifact",
                            true,
                        );
                        state.phase = SmartShelfPhase::DraftError;
                        state.last_error = Some(failure.clone());
                        transition
                            .intent(SmartShelfIntent::ShowDraftError(failure));
                    }
                }
                IntelligenceRunStatus::Failed => {
                    let failure = failure_from_run_error(
                        response.error,
                        "Smart-shelf run failed before producing a draft",
                    );
                    state.phase = SmartShelfPhase::DraftError;
                    state.last_error = Some(failure.clone());
                    transition
                        .intent(SmartShelfIntent::ShowDraftError(failure));
                }
                IntelligenceRunStatus::Cancelled => {
                    state.phase = SmartShelfPhase::Cancelled;
                    transition.intent(SmartShelfIntent::ShowNotice(
                        SmartShelfNotice::info("Smart-shelf run was cancelled"),
                    ));
                }
            }
        }
        SmartShelfMessage::RunProgressFailed(failure) => {
            state.last_error = Some(failure.clone());
            transition.intent(SmartShelfIntent::ShowNotice(SmartShelfNotice {
                level: if failure.retryable {
                    crate::SmartShelfNoticeLevel::Warning
                } else {
                    crate::SmartShelfNoticeLevel::Error
                },
                message: failure.message,
            }));
        }
        SmartShelfMessage::CancelRequested => {
            if let Some(run_id) = state.cancellable_run_id() {
                state.phase = SmartShelfPhase::Cancelling;
                transition.command(SmartShelfCommand::CancelRun {
                    run_id,
                    request: IntelligenceRunCancelRequest {
                        reason: Some(
                            "User cancelled smart-shelf generation".to_string(),
                        ),
                    },
                });
            } else {
                transition.intent(SmartShelfIntent::ShowNotice(
                    SmartShelfNotice::info(
                        "There is no active smart-shelf run to cancel",
                    ),
                ));
            }
        }
        SmartShelfMessage::CancelFinished(response) => {
            if let Some(run) = state.run.as_mut() {
                if run.run_id == response.run_id {
                    run.status = response.status;
                    run.terminal = true;
                    run.error = response.error.map(SmartShelfFailure::from);
                }
            }
            state.phase = SmartShelfPhase::Cancelled;
            transition.intent(SmartShelfIntent::ShowNotice(
                SmartShelfNotice::info(response.message.unwrap_or_else(|| {
                    "Smart-shelf run was cancelled".to_string()
                })),
            ));
        }
        SmartShelfMessage::CancelFailed(failure) => {
            state.last_error = Some(failure.clone());
            transition.intent(SmartShelfIntent::ShowNotice(
                SmartShelfNotice::error(failure.message),
            ));
        }
        SmartShelfMessage::DraftLoaded(response) => {
            let draft = SmartShelfDraftState::from_response(response);
            state.last_draft_artifact_id = Some(draft.artifact_id);
            let saved_collection_id = draft.saved_collection_id;
            let valid = draft.can_save();
            let validation = draft.validation.clone();
            state.draft = Some(draft);
            state.save.reset();

            if let Some(collection_id) = saved_collection_id {
                state.phase = SmartShelfPhase::Saved;
                state.last_error = Some(already_saved_failure(collection_id));
                transition.intent(SmartShelfIntent::OpenSavedCollection(
                    collection_id,
                ));
            } else if valid {
                state.phase = SmartShelfPhase::DraftReady;
                state.last_error = None;
            } else {
                state.phase = SmartShelfPhase::DraftInvalid;
                let issues = validation.issues;
                if issues.is_empty() {
                    let failure = SmartShelfFailure::validation(
                        "Smart-shelf draft did not contain any saveable items",
                    );
                    state.last_error = Some(failure.clone());
                    transition
                        .intent(SmartShelfIntent::ShowDraftError(failure));
                } else {
                    state.last_error = Some(SmartShelfFailure::validation(
                        "Smart-shelf draft needs review before it can be saved",
                    ));
                    transition
                        .intent(SmartShelfIntent::ShowDraftValidation(issues));
                }
            }
        }
        SmartShelfMessage::DraftLoadFailed(failure) => {
            state.phase = SmartShelfPhase::DraftError;
            state.last_error = Some(failure.clone());
            transition.intent(SmartShelfIntent::ShowDraftError(failure));
        }
        SmartShelfMessage::ToggleLock(media_id) => match state.draft.as_mut() {
            Some(draft) => match draft.toggle_lock(media_id) {
                Ok(locked) => {
                    state.save.reset();
                    if matches!(
                        state.phase,
                        SmartShelfPhase::SaveConflict
                            | SmartShelfPhase::SaveError
                    ) {
                        state.phase = SmartShelfPhase::DraftReady;
                    }
                    let message = if locked {
                        "Smart-shelf item locked"
                    } else {
                        "Smart-shelf item unlocked"
                    };
                    transition.intent(SmartShelfIntent::ShowNotice(
                        SmartShelfNotice::info(message),
                    ));
                }
                Err(failure) => {
                    state.last_error = Some(failure.clone());
                    transition.intent(SmartShelfIntent::ShowNotice(
                        SmartShelfNotice::error(failure.message),
                    ));
                }
            },
            None => missing_draft(state, &mut transition),
        },
        SmartShelfMessage::ReplaceWithAlternate {
            target_media_id,
            alternate_media_id,
        } => match state.draft.as_mut() {
            Some(draft) => match draft
                .replace_with_alternate(target_media_id, alternate_media_id)
            {
                Ok(()) => {
                    state.save.reset();
                    state.phase = SmartShelfPhase::DraftReady;
                    transition.intent(SmartShelfIntent::ShowNotice(
                        SmartShelfNotice::info("Smart-shelf item replaced"),
                    ));
                }
                Err(failure) => {
                    state.last_error = Some(failure.clone());
                    transition.intent(SmartShelfIntent::ShowNotice(
                        SmartShelfNotice::error(failure.message),
                    ));
                }
            },
            None => missing_draft(state, &mut transition),
        },
        SmartShelfMessage::RegenerateUnlockedRequested => {
            regenerate_unlocked(state, &mut transition);
        }
        SmartShelfMessage::RetryRequested => {
            retry_last_operation(state, &mut transition);
        }
        SmartShelfMessage::EditPromptRequested => {
            state.run = None;
            state.draft = None;
            state.save.reset();
            state.phase = SmartShelfPhase::Idle;
            state.last_error = None;
            transition.intent(SmartShelfIntent::FocusPrompt);
        }
        SmartShelfMessage::DiscardRequested => {
            if state.has_recoverable_work() {
                transition.intent(SmartShelfIntent::ConfirmDiscard);
            } else {
                state.reset_work();
                transition.intent(SmartShelfIntent::CloseSmartShelf);
            }
        }
        SmartShelfMessage::DiscardConfirmed => {
            let cancel_run_id = state.cancellable_run_id();
            state.reset_work();
            if let Some(run_id) = cancel_run_id {
                transition.command(SmartShelfCommand::CancelRun {
                    run_id,
                    request: IntelligenceRunCancelRequest {
                        reason: Some(
                            "User discarded smart-shelf generation".to_string(),
                        ),
                    },
                });
            }
            transition.intent(SmartShelfIntent::CloseSmartShelf);
        }
        SmartShelfMessage::SaveRequested => {
            request_save_confirmation(state, &mut transition);
        }
        SmartShelfMessage::SaveConfirmed => {
            confirm_save(state, &mut transition);
        }
        SmartShelfMessage::SaveSucceeded(response) => {
            let collection_id = response.collection_id;
            state.phase = SmartShelfPhase::Saved;
            state.save.succeeded(response);
            if let Some(draft) = state.draft.as_mut() {
                draft.saved_collection_id = Some(collection_id);
                draft.dirty = false;
            }
            state.last_error = None;
            transition
                .intent(SmartShelfIntent::OpenSavedCollection(collection_id));
        }
        SmartShelfMessage::SaveFailed(failure) => {
            let artifact_id = state
                .draft
                .as_ref()
                .map(|draft| draft.artifact_id)
                .or(state.last_draft_artifact_id)
                .unwrap_or_else(Uuid::nil);
            if let Some(conflict) =
                state.save.failed(artifact_id, failure.clone())
            {
                state.phase = SmartShelfPhase::SaveConflict;
                state.last_error = Some(failure);
                transition.intent(SmartShelfIntent::ShowSaveConflict(conflict));
            } else {
                state.phase = SmartShelfPhase::SaveError;
                state.last_error = Some(failure.clone());
                transition.intent(SmartShelfIntent::ShowSaveError(failure));
            }
        }
        SmartShelfMessage::RecoverSaveConflict(action) => {
            recover_save_conflict(state, action, &mut transition);
        }
    }

    transition
}

fn start_from_composer(
    state: &mut SmartShelfState,
    transition: &mut SmartShelfTransition,
) {
    if !state.provider.allows_start() {
        match state.provider.fallback_message() {
            Some((message, retryable)) => {
                state.phase = SmartShelfPhase::ProviderUnavailable;
                transition.intent(SmartShelfIntent::ShowProviderFallback {
                    message,
                    retryable,
                });
                if matches!(
                    state.provider,
                    ProviderReadiness::Unknown | ProviderReadiness::Checking
                ) {
                    state.provider = ProviderReadiness::Checking;
                    transition.command(SmartShelfCommand::FetchProviderStatus);
                }
            }
            None => {}
        }
        return;
    }

    match state.composer.start_request() {
        Ok(request) => {
            state.phase = SmartShelfPhase::Starting;
            state.run = None;
            state.draft = None;
            state.save.reset();
            state.last_error = None;
            state.last_start_request = Some(request.clone());
            transition.command(SmartShelfCommand::StartSmartShelf(request));
        }
        Err(failure) => {
            state.phase = SmartShelfPhase::Idle;
            state.last_error = Some(failure.clone());
            transition.intent(SmartShelfIntent::FocusPrompt);
            transition.intent(SmartShelfIntent::ShowNotice(
                SmartShelfNotice::error(failure.message),
            ));
        }
    }
}

fn regenerate_unlocked(
    state: &mut SmartShelfState,
    transition: &mut SmartShelfTransition,
) {
    if !state.provider.allows_start() {
        if let Some((message, retryable)) = state.provider.fallback_message() {
            state.phase = SmartShelfPhase::ProviderUnavailable;
            transition.intent(SmartShelfIntent::ShowProviderFallback {
                message,
                retryable,
            });
        }
        return;
    }

    let Some(draft) = state.draft.as_ref().cloned() else {
        missing_draft(state, transition);
        return;
    };

    let locked_media_ids = draft.locked_media_ids();
    match state
        .composer
        .start_request_with_regenerate_metadata(&draft)
    {
        Ok(request) => {
            state.phase = SmartShelfPhase::Starting;
            state.run = None;
            state.save.reset();
            state.last_error = None;
            state.last_start_request = Some(request.clone());
            transition.intent(SmartShelfIntent::RegenerateUnlocked {
                locked_media_ids,
            });
            transition.command(SmartShelfCommand::StartSmartShelf(request));
        }
        Err(failure) => {
            state.last_error = Some(failure.clone());
            transition.intent(SmartShelfIntent::FocusPrompt);
            transition.intent(SmartShelfIntent::ShowNotice(
                SmartShelfNotice::error(failure.message),
            ));
        }
    }
}

fn retry_last_operation(
    state: &mut SmartShelfState,
    transition: &mut SmartShelfTransition,
) {
    if matches!(
        state.save.status,
        SmartShelfSaveStatus::Error | SmartShelfSaveStatus::Conflict
    ) {
        if let (Some(draft), Some(request)) =
            (state.draft.as_ref(), state.save.last_request.clone())
        {
            state.phase = SmartShelfPhase::Saving;
            state.save.saving(request.clone());
            transition.command(SmartShelfCommand::SaveSmartShelf {
                artifact_id: draft.artifact_id,
                request,
            });
            return;
        }
    }

    if let Some(artifact_id) = state.last_draft_artifact_id {
        if state.phase == SmartShelfPhase::DraftError {
            transition.command(SmartShelfCommand::FetchDraft { artifact_id });
            return;
        }
    }

    if let Some(run) = state.run.as_ref().filter(|run| !run.terminal) {
        state.phase = SmartShelfPhase::Running;
        transition.command(SmartShelfCommand::PollRun { run_id: run.run_id });
        return;
    }

    if let Some(request) = state.last_start_request.clone() {
        if state.provider.allows_start() {
            state.phase = SmartShelfPhase::Starting;
            transition.command(SmartShelfCommand::StartSmartShelf(request));
            return;
        }
    }

    if matches!(
        state.provider,
        ProviderReadiness::Unknown
            | ProviderReadiness::Checking
            | ProviderReadiness::Unavailable { .. }
    ) {
        state.provider = ProviderReadiness::Checking;
        transition.command(SmartShelfCommand::FetchProviderStatus);
        return;
    }

    transition.intent(SmartShelfIntent::FocusPrompt);
}

fn request_save_confirmation(
    state: &mut SmartShelfState,
    transition: &mut SmartShelfTransition,
) {
    let Some(draft) = state.draft.as_ref() else {
        missing_draft(state, transition);
        return;
    };

    if let Some(collection_id) = draft.saved_collection_id {
        let failure = already_saved_failure(collection_id);
        state.phase = SmartShelfPhase::Saved;
        state.last_error = Some(failure.clone());
        transition.intent(SmartShelfIntent::ShowSaveError(failure));
        transition.intent(SmartShelfIntent::OpenSavedCollection(collection_id));
        return;
    }

    if !draft.can_save() {
        state.phase = SmartShelfPhase::DraftInvalid;
        if draft.validation.issues.is_empty() {
            transition.intent(SmartShelfIntent::ShowNotice(
                SmartShelfNotice::error(
                    "Smart-shelf draft is not ready to save",
                ),
            ));
        } else {
            transition.intent(SmartShelfIntent::ShowDraftValidation(
                draft.validation.issues.clone(),
            ));
        }
        return;
    }

    let confirmation = draft.save_confirmation();
    state.save.confirm(confirmation.clone());
    transition.intent(SmartShelfIntent::ShowSaveConfirmation(confirmation));
}

fn confirm_save(
    state: &mut SmartShelfState,
    transition: &mut SmartShelfTransition,
) {
    let Some(draft) = state.draft.as_ref() else {
        missing_draft(state, transition);
        return;
    };

    if !draft.can_save() {
        state.phase = SmartShelfPhase::DraftInvalid;
        transition.intent(SmartShelfIntent::ShowDraftValidation(
            draft.validation.issues.clone(),
        ));
        return;
    }

    let request = draft.save_request();
    let artifact_id = draft.artifact_id;
    state.phase = SmartShelfPhase::Saving;
    state.save.saving(request.clone());
    transition.command(SmartShelfCommand::SaveSmartShelf {
        artifact_id,
        request,
    });
}

fn recover_save_conflict(
    state: &mut SmartShelfState,
    action: SmartShelfSaveConflictRecovery,
    transition: &mut SmartShelfTransition,
) {
    let Some(conflict) = state.save.conflict.clone() else {
        transition.intent(SmartShelfIntent::ShowNotice(
            SmartShelfNotice::info(
                "There is no active smart-shelf save conflict to recover",
            ),
        ));
        return;
    };

    match action {
        SmartShelfSaveConflictRecovery::ReloadDraft => {
            state.phase = SmartShelfPhase::Running;
            state.save.reset();
            state.last_draft_artifact_id = Some(conflict.artifact_id);
            transition.command(SmartShelfCommand::FetchDraft {
                artifact_id: conflict.artifact_id,
            });
        }
        SmartShelfSaveConflictRecovery::EditSelection => {
            state.phase = SmartShelfPhase::DraftReady;
            state.save.reset();
            transition.intent(SmartShelfIntent::ShowNotice(
                SmartShelfNotice::info(
                    "Review the shelf selection before saving again",
                ),
            ));
        }
        SmartShelfSaveConflictRecovery::RetrySave => {
            if let Some(request) = state.save.last_request.clone() {
                state.phase = SmartShelfPhase::Saving;
                state.save.saving(request.clone());
                transition.command(SmartShelfCommand::SaveSmartShelf {
                    artifact_id: conflict.artifact_id,
                    request,
                });
            } else {
                transition.intent(SmartShelfIntent::ShowNotice(
                    SmartShelfNotice::warning(
                        "The previous smart-shelf save request is no longer available",
                    ),
                ));
            }
        }
        SmartShelfSaveConflictRecovery::Discard => {
            state.reset_work();
            transition.intent(SmartShelfIntent::CloseSmartShelf);
        }
    }
}

fn missing_draft(
    state: &mut SmartShelfState,
    transition: &mut SmartShelfTransition,
) {
    let failure = SmartShelfFailure::new(
        SmartShelfFailureCode::MissingDraft,
        "Load a smart-shelf draft before editing or saving it",
        false,
    );
    state.last_error = Some(failure.clone());
    transition.intent(SmartShelfIntent::ShowNotice(SmartShelfNotice::error(
        failure.message,
    )));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SmartShelfComposer;
    use chrono::Utc;
    use ferrex_player_api::api_types::{
        CollectionArtwork, CollectionDuplicatePolicy, CollectionId,
        CollectionIdentity, CollectionKind, CollectionMediaScope,
        CollectionOwner, CollectionPresentationMode, CollectionProvenance,
        CollectionScope, CollectionSource, CollectionSummary, CollectionTheme,
        CollectionTimestamps, CollectionVersion, CollectionVisibility,
        IntelligenceCaps, IntelligenceError, IntelligenceErrorCode,
        IntelligenceProviderState, IntelligenceProviderStatus,
        IntelligenceRunPurpose, IntelligenceRunStatusResponse,
        IntelligenceSummary, MediaID, MovieID, SmartShelfDraftAlternate,
        SmartShelfDraftContent, SmartShelfDraftItem, SmartShelfDraftResponse,
        SmartShelfDraftSource, SmartShelfDraftValidation,
        SmartShelfDraftValidationIssue, SmartShelfDraftValidationIssueCode,
        SmartShelfError, SmartShelfErrorCode, SmartShelfSaveResponse,
        SmartShelfStartResponse,
    };
    use serde_json::{Value, json};

    fn movie(n: u128) -> MediaID {
        MediaID::Movie(MovieID(Uuid::from_u128(n)))
    }

    fn provider(
        state: IntelligenceProviderState,
    ) -> IntelligenceProviderStatus {
        IntelligenceProviderStatus {
            enabled: matches!(
                state,
                IntelligenceProviderState::Ready
                    | IntelligenceProviderState::Degraded
                    | IntelligenceProviderState::Unavailable
            ),
            provider_name: "test-provider".to_string(),
            base_url: "http://provider.invalid".to_string(),
            api_key_configured: matches!(
                state,
                IntelligenceProviderState::Ready
                    | IntelligenceProviderState::Degraded
                    | IntelligenceProviderState::Unavailable
            ),
            default_model: Some("test-model".to_string()),
            state,
            models: Vec::new(),
            checked_at_epoch_seconds: Some(1),
            error: (state == IntelligenceProviderState::Unavailable).then(
                || IntelligenceError {
                    code: IntelligenceErrorCode::ProviderUnavailable,
                    message: "provider is down".to_string(),
                    retryable: true,
                    details: Value::Null,
                },
            ),
        }
    }

    fn source(media_id: MediaID) -> SmartShelfDraftSource {
        SmartShelfDraftSource {
            label: Some("Library".to_string()),
            media_id: Some(media_id),
            artifact_id: None,
            field: None,
            evidence: Some(IntelligenceSummary::new("Grounded")),
        }
    }

    fn draft_response() -> SmartShelfDraftResponse {
        let first = movie(1);
        let second = movie(2);
        let alternate = movie(3);
        SmartShelfDraftResponse {
            artifact_id: Uuid::from_u128(100),
            run_id: Some(Uuid::from_u128(10)),
            owner_user_id: Some(Uuid::from_u128(20)),
            title: "Rain shelf".to_string(),
            summary: Some(IntelligenceSummary::new("A useful shelf")),
            draft: Some(SmartShelfDraftContent {
                schema_version: 1,
                title: "Rain shelf".to_string(),
                description: Some("Atmospheric picks".to_string()),
                interpreted_intent: Some("rainy night".to_string()),
                requested_constraints: json!({"mood": "rain"}),
                items: vec![
                    SmartShelfDraftItem {
                        ordinal: 1,
                        media_id: first,
                        title: Some("First".to_string()),
                        subtitle: None,
                        year: Some(2020),
                        reason: Some("Grounded reason".to_string()),
                        sources: vec![source(first)],
                        locked: false,
                        replacement_of: None,
                    },
                    SmartShelfDraftItem {
                        ordinal: 2,
                        media_id: second,
                        title: Some("Second".to_string()),
                        subtitle: None,
                        year: Some(2021),
                        reason: Some("Another grounded reason".to_string()),
                        sources: vec![source(second)],
                        locked: false,
                        replacement_of: None,
                    },
                ],
                alternates: vec![SmartShelfDraftAlternate {
                    target_ordinal: Some(1),
                    media_id: alternate,
                    title: Some("Alternate".to_string()),
                    subtitle: None,
                    year: Some(2022),
                    reason: Some("Alternate reason".to_string()),
                    sources: vec![source(alternate)],
                }],
            }),
            validation: SmartShelfDraftValidation {
                valid: true,
                issues: Vec::new(),
            },
            saved_collection_id: None,
        }
    }

    fn invalid_draft_response() -> SmartShelfDraftResponse {
        let mut response = draft_response();
        response.validation = SmartShelfDraftValidation {
            valid: false,
            issues: vec![SmartShelfDraftValidationIssue::for_item(
                SmartShelfDraftValidationIssueCode::MissingReason,
                1,
                movie(1),
                "missing reason",
            )],
        };
        response
    }

    fn progress(
        status: IntelligenceRunStatus,
        artifact_ids: Vec<Uuid>,
    ) -> IntelligenceRunStatusResponse {
        IntelligenceRunStatusResponse {
            run_id: Uuid::from_u128(10),
            purpose: IntelligenceRunPurpose::Recommendation,
            status,
            terminal: is_terminal_status(status),
            current_phase: Some("phase".to_string()),
            provider: Some("test-provider".to_string()),
            model: Some("test-model".to_string()),
            queued_at_epoch_seconds: Some(1),
            started_at_epoch_seconds: Some(2),
            completed_at_epoch_seconds: None,
            current_step: Some(1),
            max_steps: Some(3),
            draft_artifact_ids: artifact_ids,
            output_summary: None,
            error: None,
        }
    }

    fn collection_summary(collection_id: CollectionId) -> CollectionSummary {
        let now = Utc::now();
        CollectionSummary {
            identity: CollectionIdentity::for_id(collection_id),
            title: "Rain shelf".to_string(),
            description: Some("Atmospheric picks".to_string()),
            kind: CollectionKind::Manual,
            source: CollectionSource::Manual,
            owner: CollectionOwner::default(),
            scope: CollectionScope::User,
            visibility: CollectionVisibility::Private,
            presentation: CollectionPresentationMode::Shelf,
            media_scope: CollectionMediaScope::All,
            duplicate_policy: CollectionDuplicatePolicy::DeduplicateMedia,
            artwork: CollectionArtwork::default(),
            theme: CollectionTheme::default(),
            provenance: CollectionProvenance::default(),
            version: CollectionVersion::default(),
            timestamps: CollectionTimestamps {
                created_at: now,
                updated_at: now,
                archived_at: None,
            },
            item_count: 2,
            materialization: Default::default(),
        }
    }

    fn save_response() -> SmartShelfSaveResponse {
        let collection_id = CollectionId(Uuid::from_u128(200));
        SmartShelfSaveResponse {
            draft_artifact_id: Uuid::from_u128(100),
            collection_id,
            collection: collection_summary(collection_id),
            item_count: 2,
            saved_at_epoch_seconds: Some(3),
        }
    }

    fn ready_state() -> SmartShelfState {
        let mut state = SmartShelfState::default();
        reduce(
            &mut state,
            SmartShelfMessage::ProviderStatusLoaded(provider(
                IntelligenceProviderState::Ready,
            )),
        );
        state
    }

    #[test]
    fn normal_flow_starts_polls_fetches_and_loads_valid_draft() {
        let mut state = ready_state();
        reduce(
            &mut state,
            SmartShelfMessage::PromptChanged("rain shelf".to_string()),
        );

        let start = reduce(&mut state, SmartShelfMessage::StartRequested);
        assert_eq!(state.phase, SmartShelfPhase::Starting);
        assert!(matches!(
            start.commands.as_slice(),
            [SmartShelfCommand::StartSmartShelf(request)] if request.prompt == "rain shelf"
        ));

        let poll = reduce(
            &mut state,
            SmartShelfMessage::StartAccepted(SmartShelfStartResponse {
                run_id: Uuid::from_u128(10),
                status: IntelligenceRunStatus::Queued,
                provider: Some("test-provider".to_string()),
                model: Some("test-model".to_string()),
                queued_at_epoch_seconds: Some(1),
                draft_schema_version: 1,
            }),
        );
        assert_eq!(state.phase, SmartShelfPhase::Running);
        assert_eq!(
            poll.commands,
            vec![SmartShelfCommand::PollRun {
                run_id: Uuid::from_u128(10),
            }]
        );

        let fetch = reduce(
            &mut state,
            SmartShelfMessage::RunProgressLoaded(progress(
                IntelligenceRunStatus::Succeeded,
                vec![Uuid::from_u128(100)],
            )),
        );
        assert_eq!(
            fetch.commands,
            vec![SmartShelfCommand::FetchDraft {
                artifact_id: Uuid::from_u128(100),
            }]
        );

        let loaded = reduce(
            &mut state,
            SmartShelfMessage::DraftLoaded(draft_response()),
        );
        assert!(loaded.is_empty());
        assert_eq!(state.phase, SmartShelfPhase::DraftReady);
        assert_eq!(state.draft.as_ref().expect("draft").items.len(), 2);
    }

    #[test]
    fn provider_unavailable_uses_fallback_instead_of_starting() {
        let mut state = SmartShelfState::default();
        let transition = reduce(
            &mut state,
            SmartShelfMessage::ProviderStatusLoaded(provider(
                IntelligenceProviderState::Unavailable,
            )),
        );
        assert_eq!(state.phase, SmartShelfPhase::ProviderUnavailable);
        assert!(transition.commands.is_empty());
        assert!(matches!(
            transition.intents.as_slice(),
            [SmartShelfIntent::ShowProviderFallback {
                retryable: true,
                ..
            }]
        ));

        reduce(
            &mut state,
            SmartShelfMessage::PromptChanged("rain shelf".to_string()),
        );
        let start = reduce(&mut state, SmartShelfMessage::StartRequested);
        assert!(start.commands.is_empty());
        assert!(matches!(
            start.intents.as_slice(),
            [SmartShelfIntent::ShowProviderFallback { .. }]
        ));
    }

    #[test]
    fn validation_errors_focus_prompt_and_show_invalid_draft_issues() {
        let mut state = ready_state();
        let empty_start = reduce(&mut state, SmartShelfMessage::StartRequested);
        assert_eq!(state.phase, SmartShelfPhase::Idle);
        assert!(empty_start.commands.is_empty());
        assert!(empty_start.intents.contains(&SmartShelfIntent::FocusPrompt));

        let invalid = reduce(
            &mut state,
            SmartShelfMessage::DraftLoaded(invalid_draft_response()),
        );
        assert_eq!(state.phase, SmartShelfPhase::DraftInvalid);
        assert!(matches!(
            invalid.intents.as_slice(),
            [SmartShelfIntent::ShowDraftValidation(issues)] if issues.len() == 1
        ));
    }

    #[test]
    fn cancel_emits_cancel_command_and_marks_cancelled() {
        let mut state = ready_state();
        state.run =
            Some(SmartShelfRunState::from_start(SmartShelfStartResponse {
                run_id: Uuid::from_u128(10),
                status: IntelligenceRunStatus::Running,
                provider: None,
                model: None,
                queued_at_epoch_seconds: None,
                draft_schema_version: 1,
            }));
        state.phase = SmartShelfPhase::Running;

        let cancel = reduce(&mut state, SmartShelfMessage::CancelRequested);
        assert_eq!(state.phase, SmartShelfPhase::Cancelling);
        assert!(matches!(
            cancel.commands.as_slice(),
            [SmartShelfCommand::CancelRun { run_id, .. }] if *run_id == Uuid::from_u128(10)
        ));

        let done = reduce(
            &mut state,
            SmartShelfMessage::CancelFinished(
                ferrex_player_api::api_types::IntelligenceRunCancelResponse {
                    run_id: Uuid::from_u128(10),
                    status: IntelligenceRunStatus::Cancelled,
                    cancellation_requested: true,
                    cancelled_at_epoch_seconds: Some(4),
                    message: Some("cancelled".to_string()),
                    error: None,
                },
            ),
        );
        assert_eq!(state.phase, SmartShelfPhase::Cancelled);
        assert!(matches!(
            done.intents.as_slice(),
            [SmartShelfIntent::ShowNotice(_)]
        ));
    }

    #[test]
    fn retry_can_restart_and_edit_prompt_returns_to_composer() {
        let mut state = ready_state();
        reduce(
            &mut state,
            SmartShelfMessage::PromptChanged("rain shelf".to_string()),
        );
        let start = reduce(&mut state, SmartShelfMessage::StartRequested);
        let request = match start.commands.first().expect("start command") {
            SmartShelfCommand::StartSmartShelf(request) => request.clone(),
            other => panic!("unexpected command: {other:?}"),
        };
        reduce(
            &mut state,
            SmartShelfMessage::StartFailed(SmartShelfFailure::unknown(
                "timeout", true,
            )),
        );

        let retry = reduce(&mut state, SmartShelfMessage::RetryRequested);
        assert_eq!(state.phase, SmartShelfPhase::Starting);
        assert_eq!(
            retry.commands,
            vec![SmartShelfCommand::StartSmartShelf(request)]
        );

        let edit = reduce(&mut state, SmartShelfMessage::EditPromptRequested);
        assert_eq!(state.phase, SmartShelfPhase::Idle);
        assert!(state.run.is_none());
        assert!(state.draft.is_none());
        assert_eq!(edit.intents, vec![SmartShelfIntent::FocusPrompt]);
    }

    #[test]
    fn replacements_update_save_items_and_keep_locked_items_protected() {
        let mut state = ready_state();
        reduce(&mut state, SmartShelfMessage::DraftLoaded(draft_response()));

        let lock = reduce(&mut state, SmartShelfMessage::ToggleLock(movie(1)));
        assert!(matches!(
            lock.intents.as_slice(),
            [SmartShelfIntent::ShowNotice(_)]
        ));
        assert!(state.draft.as_ref().unwrap().items[0].locked);

        let blocked = reduce(
            &mut state,
            SmartShelfMessage::ReplaceWithAlternate {
                target_media_id: movie(1),
                alternate_media_id: movie(3),
            },
        );
        assert!(matches!(
            blocked.intents.as_slice(),
            [SmartShelfIntent::ShowNotice(_)]
        ));
        assert_eq!(state.draft.as_ref().unwrap().items[0].media_id, movie(1));

        reduce(&mut state, SmartShelfMessage::ToggleLock(movie(1)));
        reduce(
            &mut state,
            SmartShelfMessage::ReplaceWithAlternate {
                target_media_id: movie(1),
                alternate_media_id: movie(3),
            },
        );
        let draft = state.draft.as_ref().unwrap();
        assert_eq!(draft.items[0].media_id, movie(3));
        assert_eq!(draft.items[0].replacement_of, Some(movie(1)));
        assert!(draft.dirty);

        let save = draft.save_request();
        assert_eq!(save.items[0].media_id, movie(3));
        assert_eq!(save.items[0].replacement_of, Some(movie(1)));
    }

    #[test]
    fn regenerate_unlocked_preserves_locked_media_ids_in_start_request() {
        let mut state = ready_state();
        state.composer.prompt = "rain shelf".to_string();
        reduce(&mut state, SmartShelfMessage::DraftLoaded(draft_response()));
        reduce(&mut state, SmartShelfMessage::ToggleLock(movie(2)));

        let transition =
            reduce(&mut state, SmartShelfMessage::RegenerateUnlockedRequested);
        assert_eq!(state.phase, SmartShelfPhase::Starting);
        assert!(matches!(
            transition.intents.as_slice(),
            [SmartShelfIntent::RegenerateUnlocked { locked_media_ids }] if locked_media_ids == &vec![movie(2)]
        ));
        assert!(matches!(
            transition.commands.as_slice(),
            [SmartShelfCommand::StartSmartShelf(request)]
                if request.locked_media_ids == vec![movie(2)]
                    && request.metadata["regenerate_unlocked"] == json!(true)
                    && request.metadata["previous_artifact_id"] == json!(Uuid::from_u128(100))
        ));
    }

    #[test]
    fn save_confirmation_and_success_open_saved_collection() {
        let mut state = ready_state();
        reduce(&mut state, SmartShelfMessage::DraftLoaded(draft_response()));

        let confirmation = reduce(&mut state, SmartShelfMessage::SaveRequested);
        assert_eq!(state.save.status, SmartShelfSaveStatus::Confirming);
        assert!(matches!(
            confirmation.intents.as_slice(),
            [SmartShelfIntent::ShowSaveConfirmation(summary)] if summary.item_count == 2
        ));

        let command = reduce(&mut state, SmartShelfMessage::SaveConfirmed);
        assert_eq!(state.phase, SmartShelfPhase::Saving);
        assert!(matches!(
            command.commands.as_slice(),
            [SmartShelfCommand::SaveSmartShelf { artifact_id, request }]
                if *artifact_id == Uuid::from_u128(100) && request.items.len() == 2
        ));

        let response = save_response();
        let collection_id = response.collection_id;
        let success =
            reduce(&mut state, SmartShelfMessage::SaveSucceeded(response));
        assert_eq!(state.phase, SmartShelfPhase::Saved);
        assert_eq!(state.save.status, SmartShelfSaveStatus::Saved);
        assert_eq!(
            state.draft.as_ref().unwrap().saved_collection_id,
            Some(collection_id)
        );
        assert_eq!(
            success.intents,
            vec![SmartShelfIntent::OpenSavedCollection(collection_id)]
        );
    }

    #[test]
    fn save_conflict_can_reload_retry_edit_or_discard() {
        let mut state = ready_state();
        reduce(&mut state, SmartShelfMessage::DraftLoaded(draft_response()));
        reduce(&mut state, SmartShelfMessage::SaveConfirmed);
        let request = state.save.last_request.clone().expect("save request");
        let conflict = SmartShelfFailure::from(SmartShelfError {
            code: SmartShelfErrorCode::CollectionConflict,
            message: "collection write conflict".to_string(),
            retryable: true,
            details: Value::Null,
        });

        let failed =
            reduce(&mut state, SmartShelfMessage::SaveFailed(conflict));
        assert_eq!(state.phase, SmartShelfPhase::SaveConflict);
        assert_eq!(state.save.status, SmartShelfSaveStatus::Conflict);
        assert!(matches!(
            failed.intents.as_slice(),
            [SmartShelfIntent::ShowSaveConflict(conflict)] if conflict.recovery_actions.len() == 4
        ));

        let retry = reduce(
            &mut state,
            SmartShelfMessage::RecoverSaveConflict(
                SmartShelfSaveConflictRecovery::RetrySave,
            ),
        );
        assert_eq!(state.phase, SmartShelfPhase::Saving);
        assert_eq!(
            retry.commands,
            vec![SmartShelfCommand::SaveSmartShelf {
                artifact_id: Uuid::from_u128(100),
                request: request.clone(),
            }]
        );

        let conflict = SmartShelfFailure::new(
            SmartShelfFailureCode::Conflict,
            "stale draft",
            true,
        );
        reduce(&mut state, SmartShelfMessage::SaveFailed(conflict));
        let reload = reduce(
            &mut state,
            SmartShelfMessage::RecoverSaveConflict(
                SmartShelfSaveConflictRecovery::ReloadDraft,
            ),
        );
        assert_eq!(
            reload.commands,
            vec![SmartShelfCommand::FetchDraft {
                artifact_id: Uuid::from_u128(100),
            }]
        );

        reduce(&mut state, SmartShelfMessage::DraftLoaded(draft_response()));
        reduce(&mut state, SmartShelfMessage::SaveConfirmed);
        reduce(
            &mut state,
            SmartShelfMessage::SaveFailed(SmartShelfFailure::new(
                SmartShelfFailureCode::Conflict,
                "stale draft",
                true,
            )),
        );
        let edit = reduce(
            &mut state,
            SmartShelfMessage::RecoverSaveConflict(
                SmartShelfSaveConflictRecovery::EditSelection,
            ),
        );
        assert_eq!(state.phase, SmartShelfPhase::DraftReady);
        assert!(matches!(
            edit.intents.as_slice(),
            [SmartShelfIntent::ShowNotice(_)]
        ));

        reduce(&mut state, SmartShelfMessage::SaveConfirmed);
        reduce(
            &mut state,
            SmartShelfMessage::SaveFailed(SmartShelfFailure::new(
                SmartShelfFailureCode::Conflict,
                "stale draft",
                true,
            )),
        );
        let discard = reduce(
            &mut state,
            SmartShelfMessage::RecoverSaveConflict(
                SmartShelfSaveConflictRecovery::Discard,
            ),
        );
        assert_eq!(state.phase, SmartShelfPhase::Idle);
        assert!(state.draft.is_none());
        assert_eq!(discard.intents, vec![SmartShelfIntent::CloseSmartShelf]);
    }

    #[test]
    fn discard_confirmation_cancels_active_run_and_resets_state() {
        let mut state = ready_state();
        state.composer = SmartShelfComposer::default();
        state.composer.prompt = "rain shelf".to_string();
        state.run =
            Some(SmartShelfRunState::from_start(SmartShelfStartResponse {
                run_id: Uuid::from_u128(10),
                status: IntelligenceRunStatus::Running,
                provider: None,
                model: None,
                queued_at_epoch_seconds: None,
                draft_schema_version: 1,
            }));
        state.phase = SmartShelfPhase::Running;

        let confirm = reduce(&mut state, SmartShelfMessage::DiscardRequested);
        assert_eq!(confirm.intents, vec![SmartShelfIntent::ConfirmDiscard]);

        let discarded = reduce(&mut state, SmartShelfMessage::DiscardConfirmed);
        assert_eq!(state.phase, SmartShelfPhase::Idle);
        assert!(state.run.is_none());
        assert_eq!(
            discarded.commands,
            vec![SmartShelfCommand::CancelRun {
                run_id: Uuid::from_u128(10),
                request: IntelligenceRunCancelRequest {
                    reason: Some(
                        "User discarded smart-shelf generation".to_string()
                    ),
                },
            }]
        );
        assert_eq!(discarded.intents, vec![SmartShelfIntent::CloseSmartShelf]);
    }

    #[test]
    fn template_selection_populates_composer_start_request() {
        let mut state = ready_state();
        let first_template = state.composer.templates[0].clone();
        reduce(
            &mut state,
            SmartShelfMessage::TemplateSelected(first_template.id.clone()),
        );
        let transition = reduce(&mut state, SmartShelfMessage::StartRequested);
        assert!(matches!(
            transition.commands.as_slice(),
            [SmartShelfCommand::StartSmartShelf(request)]
                if request.template_id == Some(first_template.id)
                    && request.prompt == first_template.prompt
                    && request.constraints == first_template.constraints
        ));
    }

    #[test]
    fn unknown_provider_start_checks_status_before_generating() {
        let mut state = SmartShelfState::default();
        state.composer.prompt = "rain shelf".to_string();
        let transition = reduce(&mut state, SmartShelfMessage::StartRequested);
        assert_eq!(state.phase, SmartShelfPhase::ProviderUnavailable);
        assert_eq!(
            transition.commands,
            vec![SmartShelfCommand::FetchProviderStatus]
        );
        assert!(matches!(
            transition.intents.as_slice(),
            [SmartShelfIntent::ShowProviderFallback {
                retryable: true,
                ..
            }]
        ));
    }

    #[test]
    fn failed_run_surfaces_typed_intelligence_error() {
        let mut state = ready_state();
        let mut failed = progress(IntelligenceRunStatus::Failed, Vec::new());
        failed.error = Some(IntelligenceError {
            code: IntelligenceErrorCode::ProviderTimeout,
            message: "provider timed out".to_string(),
            retryable: true,
            details: Value::Null,
        });

        let transition =
            reduce(&mut state, SmartShelfMessage::RunProgressLoaded(failed));
        assert_eq!(state.phase, SmartShelfPhase::DraftError);
        assert!(matches!(
            transition.intents.as_slice(),
            [SmartShelfIntent::ShowDraftError(failure)]
                if failure.code == SmartShelfFailureCode::Intelligence(IntelligenceErrorCode::ProviderTimeout)
        ));
    }

    #[test]
    fn item_count_is_clamped_before_start() {
        let mut state = ready_state();
        reduce(
            &mut state,
            SmartShelfMessage::PromptChanged("rain shelf".to_string()),
        );
        reduce(&mut state, SmartShelfMessage::ItemCountChanged(u16::MAX));
        let transition = reduce(&mut state, SmartShelfMessage::StartRequested);
        assert!(matches!(
            transition.commands.as_slice(),
            [SmartShelfCommand::StartSmartShelf(request)]
                if request.item_count == ferrex_player_api::api_types::MAX_SMART_SHELF_ITEM_COUNT
                    && request.caps == IntelligenceCaps::default()
        ));
    }
}
