//! Reducer input messages, side-effect commands, and UI/application intents.

use ferrex_player_api::api_types::{
    CollectionId, IntelligenceError, IntelligenceErrorCode,
    IntelligenceProviderStatus, IntelligenceRunCancelRequest,
    IntelligenceRunCancelResponse, IntelligenceRunStatusResponse,
    SmartShelfDraftResponse, SmartShelfError, SmartShelfErrorCode,
    SmartShelfSaveRequest, SmartShelfSaveResponse, SmartShelfStartRequest,
    SmartShelfStartResponse,
};
use uuid::Uuid;

use crate::state::{
    SmartShelfSaveConfirmation, SmartShelfSaveConflict,
    SmartShelfSaveConflictRecovery,
};

/// Messages accepted by the smart-shelf reducer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SmartShelfMessage {
    /// Ask the application shell to refresh provider readiness.
    ProviderRefreshRequested,
    /// Provider readiness was loaded from the API.
    ProviderStatusLoaded(IntelligenceProviderStatus),
    /// Provider readiness could not be loaded.
    ProviderStatusFailed(SmartShelfFailure),
    /// User changed the free-form composer prompt.
    PromptChanged(String),
    /// User selected a built-in or supplied composer template.
    TemplateSelected(String),
    /// User cleared the active composer template while keeping prompt text.
    TemplateCleared,
    /// User chose a library scope for the generated shelf.
    LibrarySelected(Option<ferrex_player_api::api_types::LibraryId>),
    /// User changed the requested output item count.
    ItemCountChanged(u16),
    /// User changed the optional model override.
    ModelChanged(Option<String>),
    /// User asked to start a smart-shelf run.
    StartRequested,
    /// Smart-shelf run start was accepted by the API.
    StartAccepted(SmartShelfStartResponse),
    /// Smart-shelf run start failed.
    StartFailed(SmartShelfFailure),
    /// Runtime progress was loaded for the active run.
    RunProgressLoaded(IntelligenceRunStatusResponse),
    /// Runtime progress polling failed.
    RunProgressFailed(SmartShelfFailure),
    /// User asked to cancel the active run.
    CancelRequested,
    /// Runtime cancel request completed.
    CancelFinished(IntelligenceRunCancelResponse),
    /// Runtime cancel request failed.
    CancelFailed(SmartShelfFailure),
    /// A typed draft was loaded for review/edit/save.
    DraftLoaded(SmartShelfDraftResponse),
    /// Typed draft loading failed.
    DraftLoadFailed(SmartShelfFailure),
    /// User toggled the lock state of a draft item.
    ToggleLock(ferrex_player_api::api_types::MediaID),
    /// User accepted an alternate to replace a selected draft item.
    ReplaceWithAlternate {
        /// Currently selected item to replace.
        target_media_id: ferrex_player_api::api_types::MediaID,
        /// Alternate item to move into the selected list.
        alternate_media_id: ferrex_player_api::api_types::MediaID,
    },
    /// User asked to regenerate only unlocked items.
    RegenerateUnlockedRequested,
    /// User asked to retry the most recent recoverable operation.
    RetryRequested,
    /// User chose to edit the prompt after an error or validation failure.
    EditPromptRequested,
    /// User initiated a discard flow.
    DiscardRequested,
    /// User confirmed discard/reset.
    DiscardConfirmed,
    /// User initiated save and should see a confirmation first.
    SaveRequested,
    /// User confirmed the save dialog.
    SaveConfirmed,
    /// Smart-shelf save succeeded.
    SaveSucceeded(SmartShelfSaveResponse),
    /// Smart-shelf save failed.
    SaveFailed(SmartShelfFailure),
    /// User selected a recovery action for a save conflict.
    RecoverSaveConflict(SmartShelfSaveConflictRecovery),
}

/// Side-effect commands emitted by the reducer for an app shell to execute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SmartShelfCommand {
    /// Load provider/model readiness from the API.
    FetchProviderStatus,
    /// Start a smart-shelf run with a DTO-shaped API request.
    StartSmartShelf(SmartShelfStartRequest),
    /// Poll a running intelligence run.
    PollRun { run_id: Uuid },
    /// Cancel a running intelligence run.
    CancelRun {
        run_id: Uuid,
        request: IntelligenceRunCancelRequest,
    },
    /// Read a typed smart-shelf draft.
    FetchDraft { artifact_id: Uuid },
    /// Save the accepted smart-shelf draft.
    SaveSmartShelf {
        artifact_id: Uuid,
        request: SmartShelfSaveRequest,
    },
}

/// UI/application intents emitted by the reducer without depending on a UI toolkit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SmartShelfIntent {
    /// Move input focus to the composer prompt.
    FocusPrompt,
    /// Render a provider fallback/recovery surface instead of starting a run.
    ShowProviderFallback { message: String, retryable: bool },
    /// Show a transient notice.
    ShowNotice(SmartShelfNotice),
    /// Present draft validation issues to the user.
    ShowDraftValidation(
        Vec<ferrex_player_api::api_types::SmartShelfDraftValidationIssue>,
    ),
    /// Present a draft/runtime error.
    ShowDraftError(SmartShelfFailure),
    /// Ask the user to confirm save.
    ShowSaveConfirmation(SmartShelfSaveConfirmation),
    /// Present a non-conflict save error.
    ShowSaveError(SmartShelfFailure),
    /// Present conflict recovery choices.
    ShowSaveConflict(SmartShelfSaveConflict),
    /// Navigate/open the saved collection.
    OpenSavedCollection(CollectionId),
    /// Ask the user to confirm discarding local state.
    ConfirmDiscard,
    /// Close the smart-shelf surface after a discard/reset.
    CloseSmartShelf,
    /// Inform the shell that a regenerate-unlocked run was requested.
    RegenerateUnlocked {
        locked_media_ids: Vec<ferrex_player_api::api_types::MediaID>,
    },
}

/// Severity for displayable smart-shelf notices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SmartShelfNoticeLevel {
    /// Informational notice.
    Info,
    /// Warning notice.
    Warning,
    /// Error notice.
    Error,
}

/// Displayable notice emitted by the reducer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmartShelfNotice {
    /// Notice severity.
    pub level: SmartShelfNoticeLevel,
    /// Human-readable message safe for UI display.
    pub message: String,
}

impl SmartShelfNotice {
    /// Build an informational notice.
    pub fn info(message: impl Into<String>) -> Self {
        Self {
            level: SmartShelfNoticeLevel::Info,
            message: message.into(),
        }
    }

    /// Build a warning notice.
    pub fn warning(message: impl Into<String>) -> Self {
        Self {
            level: SmartShelfNoticeLevel::Warning,
            message: message.into(),
        }
    }

    /// Build an error notice.
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            level: SmartShelfNoticeLevel::Error,
            message: message.into(),
        }
    }
}

/// Stable reducer-level failure code preserving API error classes when present.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SmartShelfFailureCode {
    /// Failure came from an intelligence runtime/provider endpoint.
    Intelligence(IntelligenceErrorCode),
    /// Failure came from a smart-shelf draft/save endpoint.
    SmartShelf(SmartShelfErrorCode),
    /// Local composer or draft validation failed before an API call.
    Validation,
    /// The provider is not ready enough to start a run.
    ProviderUnavailable,
    /// The expected active run was missing.
    MissingRun,
    /// The expected draft was missing.
    MissingDraft,
    /// A replacement target or alternate was not present in the draft.
    ReplacementUnavailable,
    /// The requested operation conflicts with current server or local state.
    Conflict,
    /// Unknown or transport-level failure without a typed API code.
    Unknown,
}

/// UI-safe failure value used by reducer state and intents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmartShelfFailure {
    /// Stable failure code.
    pub code: SmartShelfFailureCode,
    /// Human-readable message safe for UI display.
    pub message: String,
    /// Whether retry is reasonable without editing state first.
    pub retryable: bool,
}

impl SmartShelfFailure {
    /// Build a failure with an explicit code.
    pub fn new(
        code: SmartShelfFailureCode,
        message: impl Into<String>,
        retryable: bool,
    ) -> Self {
        Self {
            code,
            message: message.into(),
            retryable,
        }
    }

    /// Build a validation failure.
    pub fn validation(message: impl Into<String>) -> Self {
        Self::new(SmartShelfFailureCode::Validation, message, false)
    }

    /// Build a provider-unavailable failure.
    pub fn provider_unavailable(
        message: impl Into<String>,
        retryable: bool,
    ) -> Self {
        Self::new(
            SmartShelfFailureCode::ProviderUnavailable,
            message,
            retryable,
        )
    }

    /// Build an unknown retryable/non-retryable failure.
    pub fn unknown(message: impl Into<String>, retryable: bool) -> Self {
        Self::new(SmartShelfFailureCode::Unknown, message, retryable)
    }

    /// Whether this save failure should show conflict recovery actions.
    pub const fn is_save_conflict(&self) -> bool {
        matches!(
            self.code,
            SmartShelfFailureCode::SmartShelf(
                SmartShelfErrorCode::AlreadySaved
                    | SmartShelfErrorCode::DraftStale
                    | SmartShelfErrorCode::CollectionConflict
            ) | SmartShelfFailureCode::Conflict
        )
    }

    /// Whether this failure means the provider readiness fallback should be shown.
    pub const fn is_provider_unavailable(&self) -> bool {
        matches!(
            self.code,
            SmartShelfFailureCode::ProviderUnavailable
                | SmartShelfFailureCode::Intelligence(
                    IntelligenceErrorCode::FeatureDisabled
                        | IntelligenceErrorCode::ProviderNotConfigured
                        | IntelligenceErrorCode::ProviderUnavailable
                        | IntelligenceErrorCode::ProviderUnauthorized
                        | IntelligenceErrorCode::ModelUnavailable
                )
        )
    }
}

impl From<IntelligenceError> for SmartShelfFailure {
    fn from(value: IntelligenceError) -> Self {
        Self {
            code: SmartShelfFailureCode::Intelligence(value.code),
            message: value.message,
            retryable: value.retryable,
        }
    }
}

impl From<SmartShelfError> for SmartShelfFailure {
    fn from(value: SmartShelfError) -> Self {
        Self {
            code: SmartShelfFailureCode::SmartShelf(value.code),
            message: value.message,
            retryable: value.retryable,
        }
    }
}
