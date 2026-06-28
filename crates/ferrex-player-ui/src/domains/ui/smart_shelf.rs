use std::{sync::Arc, time::Duration};

use ferrex_player_api::{
    api_types::{
        IntelligenceRunCancelRequest, SmartShelfSaveRequest,
        SmartShelfStartRequest,
    },
    services::api::ApiService,
};
use ferrex_player_foundation::repository::RepositoryError;
use ferrex_player_intelligence::{
    ProviderReadiness, SmartShelfCommand, SmartShelfFailure,
    SmartShelfFailureCode, SmartShelfIntent, SmartShelfMessage,
    SmartShelfNotice, SmartShelfSaveConflictRecovery, SmartShelfSaveStatus,
    SmartShelfState, reduce,
};
use iced::Task;

use crate::{
    common::messages::{DomainMessage, DomainUpdateResult},
    domains::ui::{collections, messages::UiMessage},
    state::State,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmartShelfProviderFallbackState {
    pub message: String,
    pub retryable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SmartShelfUiState {
    pub open: bool,
    pub reducer: SmartShelfState,
    pub notice: Option<SmartShelfNotice>,
    pub provider_fallback: Option<SmartShelfProviderFallbackState>,
    pub confirm_discard: bool,
}

impl SmartShelfUiState {
    pub fn reset_transient(&mut self) {
        self.notice = None;
        self.provider_fallback = None;
        self.confirm_discard = false;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SmartShelfUiMessage {
    OpenComposer,
    CloseRequested,
    ConfirmDiscard,
    CancelDiscard,
    CancelSaveConfirmation,
    DismissNotice,
    Reducer(SmartShelfMessage),
}

impl SmartShelfUiMessage {
    pub fn name(&self) -> &'static str {
        match self {
            Self::OpenComposer => "UI::SmartShelf::OpenComposer",
            Self::CloseRequested => "UI::SmartShelf::CloseRequested",
            Self::ConfirmDiscard => "UI::SmartShelf::ConfirmDiscard",
            Self::CancelDiscard => "UI::SmartShelf::CancelDiscard",
            Self::CancelSaveConfirmation => {
                "UI::SmartShelf::CancelSaveConfirmation"
            }
            Self::DismissNotice => "UI::SmartShelf::DismissNotice",
            Self::Reducer(_) => "UI::SmartShelf::Reducer",
        }
    }
}

impl From<SmartShelfUiMessage> for UiMessage {
    fn from(message: SmartShelfUiMessage) -> Self {
        UiMessage::SmartShelf(message)
    }
}

pub fn update_smart_shelf_ui(
    state: &mut State,
    message: SmartShelfUiMessage,
) -> DomainUpdateResult {
    match message {
        SmartShelfUiMessage::OpenComposer => open_smart_shelf(state),
        SmartShelfUiMessage::CloseRequested => close_smart_shelf(state),
        SmartShelfUiMessage::ConfirmDiscard => {
            apply_reducer_message(state, SmartShelfMessage::DiscardConfirmed)
        }
        SmartShelfUiMessage::CancelDiscard => {
            state.domains.ui.state.smart_shelf.confirm_discard = false;
            DomainUpdateResult::task(Task::none())
        }
        SmartShelfUiMessage::CancelSaveConfirmation => {
            let surface = &mut state.domains.ui.state.smart_shelf;
            surface.reducer.save.reset();
            if surface
                .reducer
                .draft
                .as_ref()
                .is_some_and(|draft| draft.can_save())
            {
                surface.reducer.phase =
                    ferrex_player_intelligence::SmartShelfPhase::DraftReady;
            }
            DomainUpdateResult::task(Task::none())
        }
        SmartShelfUiMessage::DismissNotice => {
            state.domains.ui.state.smart_shelf.notice = None;
            DomainUpdateResult::task(Task::none())
        }
        SmartShelfUiMessage::Reducer(message) => {
            apply_reducer_message(state, message)
        }
    }
}

fn open_smart_shelf(state: &mut State) -> DomainUpdateResult {
    state.domains.ui.state.smart_shelf.open = true;
    state.domains.ui.state.smart_shelf.confirm_discard = false;

    if matches!(
        state.domains.ui.state.smart_shelf.reducer.provider,
        ProviderReadiness::Unknown
    ) {
        apply_reducer_message(
            state,
            SmartShelfMessage::ProviderRefreshRequested,
        )
    } else {
        DomainUpdateResult::task(Task::none())
    }
}

fn close_smart_shelf(state: &mut State) -> DomainUpdateResult {
    let has_recoverable_work = state
        .domains
        .ui
        .state
        .smart_shelf
        .reducer
        .has_recoverable_work();

    if has_recoverable_work {
        apply_reducer_message(state, SmartShelfMessage::DiscardRequested)
    } else {
        let surface = &mut state.domains.ui.state.smart_shelf;
        surface.open = false;
        surface.reset_transient();
        DomainUpdateResult::task(Task::none())
    }
}

fn apply_reducer_message(
    state: &mut State,
    message: SmartShelfMessage,
) -> DomainUpdateResult {
    let transition = {
        let surface = &mut state.domains.ui.state.smart_shelf;
        surface.confirm_discard = false;
        surface.provider_fallback = None;
        reduce(&mut surface.reducer, message)
    };

    apply_transition(state, transition)
}

fn apply_transition(
    state: &mut State,
    transition: ferrex_player_intelligence::SmartShelfTransition,
) -> DomainUpdateResult {
    let mut tasks = transition
        .commands
        .into_iter()
        .map(|command| command_task(state.api_service.clone(), command))
        .collect::<Vec<_>>();
    let mut events = Vec::new();

    for intent in transition.intents {
        match intent {
            SmartShelfIntent::FocusPrompt => {}
            SmartShelfIntent::ShowProviderFallback { message, retryable } => {
                let surface = &mut state.domains.ui.state.smart_shelf;
                surface.open = true;
                surface.provider_fallback =
                    Some(SmartShelfProviderFallbackState {
                        message,
                        retryable,
                    });
            }
            SmartShelfIntent::ShowNotice(notice) => {
                state.domains.ui.state.smart_shelf.notice = Some(notice);
            }
            SmartShelfIntent::ShowDraftValidation(_) => {}
            SmartShelfIntent::ShowDraftError(failure)
            | SmartShelfIntent::ShowSaveError(failure) => {
                state.domains.ui.state.smart_shelf.notice =
                    Some(SmartShelfNotice::error(failure.message));
            }
            SmartShelfIntent::ShowSaveConfirmation(_) => {}
            SmartShelfIntent::ShowSaveConflict(_) => {}
            SmartShelfIntent::OpenSavedCollection(collection_id) => {
                let surface = &mut state.domains.ui.state.smart_shelf;
                surface.open = false;
                surface.reset_transient();
                let navigation =
                    collections::open_collection_detail(state, collection_id);
                tasks.push(navigation.task);
                events.extend(navigation.events);
            }
            SmartShelfIntent::ConfirmDiscard => {
                state.domains.ui.state.smart_shelf.confirm_discard = true;
            }
            SmartShelfIntent::CloseSmartShelf => {
                let surface = &mut state.domains.ui.state.smart_shelf;
                surface.open = false;
                surface.reset_transient();
            }
            SmartShelfIntent::RegenerateUnlocked { .. } => {}
        }
    }

    DomainUpdateResult::with_events(Task::batch(tasks), events)
}

fn command_task(
    api_service: Arc<dyn ApiService>,
    command: SmartShelfCommand,
) -> Task<DomainMessage> {
    match command {
        SmartShelfCommand::FetchProviderStatus => Task::perform(
            async move {
                api_service
                    .fetch_intelligence_provider_status()
                    .await
                    .map_err(|error| repository_failure(error, true))
            },
            |result| {
                smart_shelf_domain_message(match result {
                    Ok(status) => {
                        SmartShelfMessage::ProviderStatusLoaded(status)
                    }
                    Err(failure) => {
                        SmartShelfMessage::ProviderStatusFailed(failure)
                    }
                })
            },
        ),
        SmartShelfCommand::StartSmartShelf(request) => {
            Task::perform(start_smart_shelf(api_service, request), |result| {
                smart_shelf_domain_message(match result {
                    Ok(response) => SmartShelfMessage::StartAccepted(response),
                    Err(failure) => SmartShelfMessage::StartFailed(failure),
                })
            })
        }
        SmartShelfCommand::PollRun { run_id } => Task::perform(
            async move {
                tokio::time::sleep(Duration::from_millis(750)).await;
                api_service
                    .fetch_intelligence_run_status(run_id)
                    .await
                    .map_err(|error| repository_failure(error, true))
            },
            |result| {
                smart_shelf_domain_message(match result {
                    Ok(response) => {
                        SmartShelfMessage::RunProgressLoaded(response)
                    }
                    Err(failure) => {
                        SmartShelfMessage::RunProgressFailed(failure)
                    }
                })
            },
        ),
        SmartShelfCommand::CancelRun { run_id, request } => Task::perform(
            cancel_smart_shelf_run(api_service, run_id, request),
            |result| {
                smart_shelf_domain_message(match result {
                    Ok(response) => SmartShelfMessage::CancelFinished(response),
                    Err(failure) => SmartShelfMessage::CancelFailed(failure),
                })
            },
        ),
        SmartShelfCommand::FetchDraft { artifact_id } => Task::perform(
            async move {
                api_service
                    .fetch_smart_shelf_draft(artifact_id)
                    .await
                    .map_err(|error| repository_failure(error, true))
            },
            |result| {
                smart_shelf_domain_message(match result {
                    Ok(response) => SmartShelfMessage::DraftLoaded(response),
                    Err(failure) => SmartShelfMessage::DraftLoadFailed(failure),
                })
            },
        ),
        SmartShelfCommand::SaveSmartShelf {
            artifact_id,
            request,
        } => Task::perform(
            save_smart_shelf(api_service, artifact_id, request),
            |result| {
                smart_shelf_domain_message(match result {
                    Ok(response) => SmartShelfMessage::SaveSucceeded(response),
                    Err(failure) => SmartShelfMessage::SaveFailed(failure),
                })
            },
        ),
    }
}

async fn start_smart_shelf(
    api_service: Arc<dyn ApiService>,
    request: SmartShelfStartRequest,
) -> Result<
    ferrex_player_api::api_types::SmartShelfStartResponse,
    SmartShelfFailure,
> {
    api_service
        .start_smart_shelf(request)
        .await
        .map_err(|error| repository_failure(error, true))
}

async fn cancel_smart_shelf_run(
    api_service: Arc<dyn ApiService>,
    run_id: uuid::Uuid,
    request: IntelligenceRunCancelRequest,
) -> Result<
    ferrex_player_api::api_types::IntelligenceRunCancelResponse,
    SmartShelfFailure,
> {
    api_service
        .cancel_intelligence_run(run_id, request)
        .await
        .map_err(|error| repository_failure(error, true))
}

async fn save_smart_shelf(
    api_service: Arc<dyn ApiService>,
    artifact_id: uuid::Uuid,
    request: SmartShelfSaveRequest,
) -> Result<
    ferrex_player_api::api_types::SmartShelfSaveResponse,
    SmartShelfFailure,
> {
    api_service
        .save_smart_shelf(artifact_id, request)
        .await
        .map_err(|error| repository_failure(error, true))
}

fn smart_shelf_domain_message(message: SmartShelfMessage) -> DomainMessage {
    DomainMessage::Ui(UiMessage::SmartShelf(SmartShelfUiMessage::Reducer(
        message,
    )))
}

fn repository_failure(
    error: RepositoryError,
    default_retryable: bool,
) -> SmartShelfFailure {
    let message = error.to_string();
    let lower = message.to_ascii_lowercase();

    let code = if lower.contains("provider")
        || lower.contains("model")
        || lower.contains("intelligence")
            && (lower.contains("configured")
                || lower.contains("unavailable")
                || lower.contains("unauthorized"))
    {
        SmartShelfFailureCode::ProviderUnavailable
    } else if lower.contains("conflict")
        || lower.contains("already been saved")
        || lower.contains("stale")
        || lower.contains("revision")
        || lower.contains("version")
    {
        SmartShelfFailureCode::Conflict
    } else if lower.contains("validation")
        || lower.contains("duplicate")
        || lower.contains("scope")
        || lower.contains("unsupported")
        || lower.contains("missing")
    {
        SmartShelfFailureCode::Validation
    } else {
        SmartShelfFailureCode::Unknown
    };

    let retryable = matches!(code, SmartShelfFailureCode::Unknown)
        .then_some(default_retryable)
        .unwrap_or_else(|| {
            matches!(
                code,
                SmartShelfFailureCode::ProviderUnavailable
                    | SmartShelfFailureCode::Conflict
            )
        });

    SmartShelfFailure::new(code, message, retryable)
}

pub fn save_conflict_recovery_label(
    action: SmartShelfSaveConflictRecovery,
) -> &'static str {
    match action {
        SmartShelfSaveConflictRecovery::ReloadDraft => "Reload draft",
        SmartShelfSaveConflictRecovery::EditSelection => "Edit selection",
        SmartShelfSaveConflictRecovery::RetrySave => "Retry save",
        SmartShelfSaveConflictRecovery::Discard => "Discard",
    }
}

pub fn save_status_label(status: SmartShelfSaveStatus) -> &'static str {
    match status {
        SmartShelfSaveStatus::Idle => "Ready to save",
        SmartShelfSaveStatus::Confirming => "Confirm save",
        SmartShelfSaveStatus::Saving => "Saving…",
        SmartShelfSaveStatus::Saved => "Saved",
        SmartShelfSaveStatus::Conflict => "Needs recovery",
        SmartShelfSaveStatus::Error => "Save failed",
    }
}
