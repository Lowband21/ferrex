//! Smart-shelf reducer state types.

use ferrex_player_api::api_types::{
    CollectionId, IntelligenceCaps, IntelligenceError, IntelligenceMediaKind,
    IntelligenceProviderState, IntelligenceProviderStatus,
    IntelligenceRunStatus, IntelligenceRunStatusResponse, IntelligenceSummary,
    LibraryId, MediaID, SmartShelfDraftAlternate, SmartShelfDraftContent,
    SmartShelfDraftItem, SmartShelfDraftResponse, SmartShelfDraftSource,
    SmartShelfDraftValidation, SmartShelfErrorCode, SmartShelfSaveItem,
    SmartShelfSaveRequest, SmartShelfSaveResponse, SmartShelfStartRequest,
    SmartShelfStartResponse, clamp_smart_shelf_item_count,
};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    SmartShelfFailure, SmartShelfFailureCode,
    templates::{SmartShelfTemplate, built_in_templates},
};

/// Provider readiness distilled into UI-safe states.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderReadiness {
    /// No provider status has been loaded yet.
    Unknown,
    /// Provider status is currently being refreshed.
    Checking,
    /// Provider and selected/default model are ready.
    Ready {
        provider: String,
        model: Option<String>,
    },
    /// Provider can be used, but the UI should surface degraded readiness.
    Degraded {
        provider: String,
        model: Option<String>,
        message: Option<String>,
    },
    /// Provider cannot currently start smart-shelf runs.
    Unavailable { message: String, retryable: bool },
}

impl ProviderReadiness {
    /// Convert the API provider status into a reducer-friendly readiness value.
    pub fn from_status(status: &IntelligenceProviderStatus) -> Self {
        match status.state {
            IntelligenceProviderState::Ready => Self::Ready {
                provider: status.provider_name.clone(),
                model: status.default_model.clone(),
            },
            IntelligenceProviderState::Degraded => Self::Degraded {
                provider: status.provider_name.clone(),
                model: status.default_model.clone(),
                message: status.error.as_ref().map(|error| error.message.clone()),
            },
            IntelligenceProviderState::Disabled => Self::Unavailable {
                message: "Smart shelves are disabled for this server".to_string(),
                retryable: false,
            },
            IntelligenceProviderState::NotConfigured => Self::Unavailable {
                message: "Configure an intelligence provider before generating smart shelves".to_string(),
                retryable: false,
            },
            IntelligenceProviderState::Unavailable => Self::Unavailable {
                message: status
                    .error
                    .as_ref()
                    .map(|error| error.message.clone())
                    .unwrap_or_else(|| {
                        "The configured intelligence provider is unavailable".to_string()
                    }),
                retryable: status.error.as_ref().is_none_or(|error| error.retryable),
            },
        }
    }

    /// Whether reducer start requests should be allowed to issue API commands.
    pub const fn allows_start(&self) -> bool {
        matches!(self, Self::Ready { .. } | Self::Degraded { .. })
    }

    /// User-displayable fallback message for non-ready states.
    pub fn fallback_message(&self) -> Option<(String, bool)> {
        match self {
            Self::Unknown | Self::Checking => Some((
                "Checking intelligence provider readiness before starting a smart shelf".to_string(),
                true,
            )),
            Self::Unavailable { message, retryable } => {
                Some((message.clone(), *retryable))
            }
            Self::Ready { .. } | Self::Degraded { .. } => None,
        }
    }
}

impl Default for ProviderReadiness {
    fn default() -> Self {
        Self::Unknown
    }
}

/// High-level smart-shelf lifecycle phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SmartShelfPhase {
    /// Composer is idle and ready for input.
    #[default]
    Idle,
    /// Provider fallback is active.
    ProviderUnavailable,
    /// Start command has been emitted and the API response is pending.
    Starting,
    /// Runtime is queued/running/polling.
    Running,
    /// Cancel command has been emitted.
    Cancelling,
    /// Runtime was cancelled.
    Cancelled,
    /// Draft is valid and editable.
    DraftReady,
    /// Draft loaded but has validation errors.
    DraftInvalid,
    /// Draft/runtime failed.
    DraftError,
    /// Save command has been emitted.
    Saving,
    /// Save succeeded.
    Saved,
    /// Save failed with a recoverable conflict/stale state.
    SaveConflict,
    /// Save failed with a non-conflict error.
    SaveError,
}

/// Composer state for prompt/template driven smart-shelf starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmartShelfComposer {
    /// User-edited prompt text.
    pub prompt: String,
    /// Active template id, if the prompt came from a template.
    pub selected_template_id: Option<String>,
    /// Templates offered by the shell.
    pub templates: Vec<SmartShelfTemplate>,
    /// Optional library scope.
    pub library_id: Option<LibraryId>,
    /// Requested media kinds.
    pub media_kinds: Vec<IntelligenceMediaKind>,
    /// Requested result count, clamped to the API bounds.
    pub item_count: u16,
    /// Media ids that should be preserved by the next start/regenerate request.
    pub locked_media_ids: Vec<MediaID>,
    /// Optional model override.
    pub model: Option<String>,
    /// Request caps forwarded to the API.
    pub caps: IntelligenceCaps,
    /// Structured constraints forwarded to the API.
    pub constraints: Value,
    /// Structured metadata forwarded to the API.
    pub metadata: Value,
    /// Last composer-level validation failure.
    pub validation_error: Option<SmartShelfFailure>,
}

impl SmartShelfComposer {
    /// Create a composer with caller-supplied templates.
    pub fn with_templates(templates: Vec<SmartShelfTemplate>) -> Self {
        Self {
            templates,
            ..Self::default()
        }
    }

    /// Apply a template by id. Returns `true` when the template was found.
    pub fn select_template(&mut self, template_id: &str) -> bool {
        let Some(template) = self
            .templates
            .iter()
            .find(|template| template.id == template_id)
            .cloned()
        else {
            return false;
        };

        self.prompt = template.prompt;
        self.selected_template_id = Some(template.id);
        self.media_kinds = template.media_kinds;
        self.item_count = clamp_smart_shelf_item_count(template.item_count);
        self.constraints = template.constraints;
        self.validation_error = None;
        true
    }

    /// Clear template selection without discarding current prompt text.
    pub fn clear_template(&mut self) {
        self.selected_template_id = None;
    }

    /// Build the API start request from current composer state.
    pub fn start_request(
        &mut self,
    ) -> Result<SmartShelfStartRequest, SmartShelfFailure> {
        let prompt = self.prompt.trim().to_string();
        if prompt.is_empty() {
            let failure = SmartShelfFailure::validation(
                "Describe the smart shelf before starting generation",
            );
            self.validation_error = Some(failure.clone());
            return Err(failure);
        }

        self.validation_error = None;
        Ok(SmartShelfStartRequest {
            prompt,
            library_id: self.library_id,
            media_kinds: self.media_kinds.clone(),
            item_count: clamp_smart_shelf_item_count(self.item_count),
            template_id: self.selected_template_id.clone(),
            locked_media_ids: self.locked_media_ids.clone(),
            idempotency_key: None,
            model: self.model.clone(),
            caps: self.caps,
            constraints: self.constraints.clone(),
            metadata: self.metadata.clone(),
        })
    }

    pub(crate) fn start_request_with_regenerate_metadata(
        &mut self,
        draft: &SmartShelfDraftState,
    ) -> Result<SmartShelfStartRequest, SmartShelfFailure> {
        if self.prompt.trim().is_empty() {
            if let Some(intent) = draft.interpreted_intent.as_deref() {
                self.prompt = intent.to_string();
            } else {
                self.prompt = draft.title.clone();
            }
        }

        self.locked_media_ids = draft.locked_media_ids();
        let mut request = self.start_request()?;
        request.locked_media_ids = draft.locked_media_ids();
        request.metadata = merge_regenerate_metadata(
            &request.metadata,
            draft.artifact_id,
            request.locked_media_ids.len(),
        );
        Ok(request)
    }
}

impl Default for SmartShelfComposer {
    fn default() -> Self {
        Self {
            prompt: String::new(),
            selected_template_id: None,
            templates: built_in_templates(),
            library_id: None,
            media_kinds: vec![
                IntelligenceMediaKind::Movie,
                IntelligenceMediaKind::Series,
            ],
            item_count: clamp_smart_shelf_item_count(8),
            locked_media_ids: Vec::new(),
            model: None,
            caps: IntelligenceCaps::default(),
            constraints: Value::Null,
            metadata: Value::Null,
            validation_error: None,
        }
    }
}

/// Runtime state for an active or recently completed smart-shelf run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmartShelfRunState {
    /// Intelligence run id.
    pub run_id: Uuid,
    /// Current runtime status.
    pub status: IntelligenceRunStatus,
    /// Whether the status is terminal.
    pub terminal: bool,
    /// Optional phase label from the runtime.
    pub current_phase: Option<String>,
    /// Current step, when available.
    pub current_step: Option<u32>,
    /// Max step, when available.
    pub max_steps: Option<u32>,
    /// Provider selected by the runtime.
    pub provider: Option<String>,
    /// Model selected by the runtime.
    pub model: Option<String>,
    /// Draft artifacts produced by the run.
    pub draft_artifact_ids: Vec<Uuid>,
    /// Runtime error, when terminal failure occurred.
    pub error: Option<SmartShelfFailure>,
}

impl SmartShelfRunState {
    /// Create run state from a start response.
    pub fn from_start(response: SmartShelfStartResponse) -> Self {
        Self {
            run_id: response.run_id,
            status: response.status,
            terminal: is_terminal_status(response.status),
            current_phase: None,
            current_step: None,
            max_steps: None,
            provider: response.provider,
            model: response.model,
            draft_artifact_ids: Vec::new(),
            error: None,
        }
    }

    /// Create run state from a status response.
    pub fn from_status(response: &IntelligenceRunStatusResponse) -> Self {
        Self {
            run_id: response.run_id,
            status: response.status,
            terminal: response.terminal,
            current_phase: response.current_phase.clone(),
            current_step: response.current_step,
            max_steps: response.max_steps,
            provider: response.provider.clone(),
            model: response.model.clone(),
            draft_artifact_ids: response.draft_artifact_ids.clone(),
            error: response.error.clone().map(SmartShelfFailure::from),
        }
    }

    /// Update this state from a status response.
    pub fn apply_status(&mut self, response: &IntelligenceRunStatusResponse) {
        *self = Self::from_status(response);
    }

    /// Whether the run can still be cancelled.
    pub const fn can_cancel(&self) -> bool {
        !self.terminal
            && matches!(
                self.status,
                IntelligenceRunStatus::Queued | IntelligenceRunStatus::Running
            )
    }
}

/// Editable selected draft item state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmartShelfItemState {
    /// One-based item ordinal.
    pub ordinal: u32,
    /// Selected media id.
    pub media_id: MediaID,
    /// Display title.
    pub title: Option<String>,
    /// Display subtitle.
    pub subtitle: Option<String>,
    /// Release year.
    pub year: Option<u16>,
    /// Grounded selection reason.
    pub reason: Option<String>,
    /// Source/provenance chips.
    pub sources: Vec<SmartShelfDraftSource>,
    /// Whether the user locked this item across replacement/regeneration.
    pub locked: bool,
    /// Original selected item this item replaced, when applicable.
    pub replacement_of: Option<MediaID>,
}

impl SmartShelfItemState {
    /// Build editable item state from an API draft item.
    pub fn from_draft_item(item: SmartShelfDraftItem) -> Self {
        Self {
            ordinal: item.ordinal,
            media_id: item.media_id,
            title: item.title,
            subtitle: item.subtitle,
            year: item.year,
            reason: item.reason,
            sources: item.sources,
            locked: item.locked,
            replacement_of: item.replacement_of,
        }
    }

    /// Convert item state to a save request item.
    pub fn to_save_item(&self) -> SmartShelfSaveItem {
        SmartShelfSaveItem {
            media_id: self.media_id,
            locked: self.locked,
            replacement_of: self.replacement_of,
            reason: self.reason.clone(),
            sources: self.sources.clone(),
        }
    }
}

/// Editable alternate item state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmartShelfAlternateState {
    /// Target ordinal suggested by the provider.
    pub target_ordinal: Option<u32>,
    /// Alternate media id.
    pub media_id: MediaID,
    /// Display title.
    pub title: Option<String>,
    /// Display subtitle.
    pub subtitle: Option<String>,
    /// Release year.
    pub year: Option<u16>,
    /// Grounded alternate reason.
    pub reason: Option<String>,
    /// Source/provenance chips.
    pub sources: Vec<SmartShelfDraftSource>,
}

impl SmartShelfAlternateState {
    /// Build alternate state from an API draft alternate.
    pub fn from_draft_alternate(alternate: SmartShelfDraftAlternate) -> Self {
        Self {
            target_ordinal: alternate.target_ordinal,
            media_id: alternate.media_id,
            title: alternate.title,
            subtitle: alternate.subtitle,
            year: alternate.year,
            reason: alternate.reason,
            sources: alternate.sources,
        }
    }

    fn into_item_replacing(
        self,
        ordinal: u32,
        replaced_media_id: MediaID,
    ) -> SmartShelfItemState {
        SmartShelfItemState {
            ordinal,
            media_id: self.media_id,
            title: self.title,
            subtitle: self.subtitle,
            year: self.year,
            reason: self.reason,
            sources: self.sources,
            locked: false,
            replacement_of: Some(replaced_media_id),
        }
    }
}

/// Typed draft state that the UI can render and edit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmartShelfDraftState {
    /// Draft artifact id.
    pub artifact_id: Uuid,
    /// Producing run id, when available.
    pub run_id: Option<Uuid>,
    /// Draft title.
    pub title: String,
    /// Bounded server summary.
    pub summary: Option<IntelligenceSummary>,
    /// Draft description.
    pub description: Option<String>,
    /// Provider-interpreted prompt/intent.
    pub interpreted_intent: Option<String>,
    /// Selected/editable items.
    pub items: Vec<SmartShelfItemState>,
    /// Alternate items available for replacement.
    pub alternates: Vec<SmartShelfAlternateState>,
    /// Server validation result for the loaded draft.
    pub validation: SmartShelfDraftValidation,
    /// Collection id when the draft has already been saved.
    pub saved_collection_id: Option<CollectionId>,
    /// Whether local item state differs from the loaded draft.
    pub dirty: bool,
}

impl SmartShelfDraftState {
    /// Convert an API draft response into editable reducer state.
    pub fn from_response(response: SmartShelfDraftResponse) -> Self {
        let SmartShelfDraftResponse {
            artifact_id,
            run_id,
            title,
            summary,
            draft,
            validation,
            saved_collection_id,
            ..
        } = response;

        let (description, interpreted_intent, items, alternates) = draft
            .map(draft_parts)
            .unwrap_or_else(|| (None, None, Vec::new(), Vec::new()));

        Self {
            artifact_id,
            run_id,
            title,
            summary,
            description,
            interpreted_intent,
            items,
            alternates,
            validation,
            saved_collection_id,
            dirty: false,
        }
    }

    /// Whether the draft can be saved without first fixing validation errors.
    pub fn can_save(&self) -> bool {
        self.validation.valid
            && !self.items.is_empty()
            && self.saved_collection_id.is_none()
    }

    /// Count locked selected items.
    pub fn locked_count(&self) -> usize {
        self.items.iter().filter(|item| item.locked).count()
    }

    /// Count selected replacement items.
    pub fn replacements_count(&self) -> usize {
        self.items
            .iter()
            .filter(|item| item.replacement_of.is_some())
            .count()
    }

    /// Media ids locked by the user.
    pub fn locked_media_ids(&self) -> Vec<MediaID> {
        self.items
            .iter()
            .filter(|item| item.locked)
            .map(|item| item.media_id)
            .collect()
    }

    /// Toggle lock for a selected item.
    pub fn toggle_lock(
        &mut self,
        media_id: MediaID,
    ) -> Result<bool, SmartShelfFailure> {
        let Some(item) =
            self.items.iter_mut().find(|item| item.media_id == media_id)
        else {
            return Err(SmartShelfFailure::new(
                SmartShelfFailureCode::MissingDraft,
                "The selected smart-shelf item is no longer available",
                false,
            ));
        };

        item.locked = !item.locked;
        self.dirty = true;
        Ok(item.locked)
    }

    /// Replace a selected item with an alternate.
    pub fn replace_with_alternate(
        &mut self,
        target_media_id: MediaID,
        alternate_media_id: MediaID,
    ) -> Result<(), SmartShelfFailure> {
        let Some(target_index) = self
            .items
            .iter()
            .position(|item| item.media_id == target_media_id)
        else {
            return Err(SmartShelfFailure::new(
                SmartShelfFailureCode::ReplacementUnavailable,
                "The selected item is no longer in this draft",
                false,
            ));
        };

        if self.items[target_index].locked {
            return Err(SmartShelfFailure::new(
                SmartShelfFailureCode::Conflict,
                "Unlock this item before replacing it",
                false,
            ));
        }

        if self
            .items
            .iter()
            .any(|item| item.media_id == alternate_media_id)
        {
            return Err(SmartShelfFailure::new(
                SmartShelfFailureCode::Conflict,
                "That alternate is already selected in the shelf",
                false,
            ));
        }

        let Some(alternate_index) = self
            .alternates
            .iter()
            .position(|alternate| alternate.media_id == alternate_media_id)
        else {
            return Err(SmartShelfFailure::new(
                SmartShelfFailureCode::ReplacementUnavailable,
                "The requested alternate is no longer available",
                false,
            ));
        };

        let target = self.items.remove(target_index);
        let alternate = self.alternates.remove(alternate_index);
        let replacement = alternate.into_item_replacing(
            target.ordinal,
            target.replacement_of.unwrap_or(target.media_id),
        );
        self.alternates.push(SmartShelfAlternateState {
            target_ordinal: Some(target.ordinal),
            media_id: target.media_id,
            title: target.title,
            subtitle: target.subtitle,
            year: target.year,
            reason: target.reason,
            sources: target.sources,
        });
        self.items.insert(target_index, replacement);
        self.dirty = true;
        Ok(())
    }

    /// Build a save request from current draft item state.
    pub fn save_request(&self) -> SmartShelfSaveRequest {
        SmartShelfSaveRequest {
            title: Some(self.title.clone()),
            description: self.description.clone(),
            items: self
                .items
                .iter()
                .map(SmartShelfItemState::to_save_item)
                .collect(),
            idempotency_key: None,
        }
    }

    /// Build a save confirmation summary from current draft state.
    pub fn save_confirmation(&self) -> SmartShelfSaveConfirmation {
        SmartShelfSaveConfirmation {
            artifact_id: self.artifact_id,
            title: self.title.clone(),
            item_count: self.items.len(),
            locked_count: self.locked_count(),
            replacements_count: self.replacements_count(),
        }
    }
}

/// Save status tracked independently from the high-level phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SmartShelfSaveStatus {
    /// No save is active.
    #[default]
    Idle,
    /// User is reviewing save confirmation.
    Confirming,
    /// Save command has been emitted.
    Saving,
    /// Save succeeded.
    Saved,
    /// Save failed with recoverable conflict.
    Conflict,
    /// Save failed with non-conflict error.
    Error,
}

/// Summary shown before issuing a save command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmartShelfSaveConfirmation {
    /// Draft artifact being saved.
    pub artifact_id: Uuid,
    /// Collection title to be saved.
    pub title: String,
    /// Number of accepted items.
    pub item_count: usize,
    /// Number of locked accepted items.
    pub locked_count: usize,
    /// Number of accepted replacement items.
    pub replacements_count: usize,
}

/// Recovery choices for save conflict/stale failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SmartShelfSaveConflictRecovery {
    /// Fetch the draft again from the server.
    ReloadDraft,
    /// Return to draft editing with the current local selections.
    EditSelection,
    /// Retry the previous save request.
    RetrySave,
    /// Discard local smart-shelf state.
    Discard,
}

/// Save conflict state and available recovery actions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmartShelfSaveConflict {
    /// Draft artifact id that failed to save.
    pub artifact_id: Uuid,
    /// Conflict failure returned by the API or local reducer.
    pub failure: SmartShelfFailure,
    /// User-facing recovery actions.
    pub recovery_actions: Vec<SmartShelfSaveConflictRecovery>,
}

/// Save state retained for confirmation, retry, success, and recovery.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SmartShelfSaveState {
    /// Current save status.
    pub status: SmartShelfSaveStatus,
    /// Pending confirmation summary.
    pub confirmation: Option<SmartShelfSaveConfirmation>,
    /// Last request emitted by the reducer, used for retry.
    pub last_request: Option<SmartShelfSaveRequest>,
    /// Last successful save response.
    pub last_response: Option<SmartShelfSaveResponse>,
    /// Last non-success failure.
    pub last_error: Option<SmartShelfFailure>,
    /// Active conflict recovery state.
    pub conflict: Option<SmartShelfSaveConflict>,
}

impl SmartShelfSaveState {
    /// Reset save state before a new run/draft is started.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Store a pending confirmation.
    pub fn confirm(&mut self, confirmation: SmartShelfSaveConfirmation) {
        self.status = SmartShelfSaveStatus::Confirming;
        self.confirmation = Some(confirmation);
        self.last_error = None;
        self.conflict = None;
    }

    /// Mark save as in-flight.
    pub fn saving(&mut self, request: SmartShelfSaveRequest) {
        self.status = SmartShelfSaveStatus::Saving;
        self.confirmation = None;
        self.last_request = Some(request);
        self.last_error = None;
        self.conflict = None;
    }

    /// Mark save as successful.
    pub fn succeeded(&mut self, response: SmartShelfSaveResponse) {
        self.status = SmartShelfSaveStatus::Saved;
        self.confirmation = None;
        self.last_response = Some(response);
        self.last_error = None;
        self.conflict = None;
    }

    /// Mark save as failed.
    pub fn failed(
        &mut self,
        artifact_id: Uuid,
        failure: SmartShelfFailure,
    ) -> Option<SmartShelfSaveConflict> {
        self.confirmation = None;
        self.last_error = Some(failure.clone());
        if failure.is_save_conflict() {
            let conflict = SmartShelfSaveConflict {
                artifact_id,
                failure,
                recovery_actions: vec![
                    SmartShelfSaveConflictRecovery::ReloadDraft,
                    SmartShelfSaveConflictRecovery::EditSelection,
                    SmartShelfSaveConflictRecovery::RetrySave,
                    SmartShelfSaveConflictRecovery::Discard,
                ],
            };
            self.status = SmartShelfSaveStatus::Conflict;
            self.conflict = Some(conflict.clone());
            Some(conflict)
        } else {
            self.status = SmartShelfSaveStatus::Error;
            self.conflict = None;
            None
        }
    }
}

/// Complete smart-shelf reducer state.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SmartShelfState {
    /// High-level lifecycle phase.
    pub phase: SmartShelfPhase,
    /// Provider readiness/fallback state.
    pub provider: ProviderReadiness,
    /// Prompt/template composer.
    pub composer: SmartShelfComposer,
    /// Active or most recent run.
    pub run: Option<SmartShelfRunState>,
    /// Loaded editable draft.
    pub draft: Option<SmartShelfDraftState>,
    /// Save confirmation/progress/recovery state.
    pub save: SmartShelfSaveState,
    /// Last failure from any reducer operation.
    pub last_error: Option<SmartShelfFailure>,
    /// Last start request emitted by the reducer, used for retry.
    pub last_start_request: Option<SmartShelfStartRequest>,
    /// Last draft artifact id requested, used for reload/retry.
    pub last_draft_artifact_id: Option<Uuid>,
}

impl SmartShelfState {
    /// Whether the current state contains local/user-visible work.
    pub fn has_recoverable_work(&self) -> bool {
        self.run.is_some()
            || self.draft.is_some()
            || self.save.confirmation.is_some()
            || self.save.last_request.is_some()
            || !self.composer.prompt.trim().is_empty()
    }

    /// Reset all run/draft/save state while keeping provider status and templates.
    pub fn reset_work(&mut self) {
        let provider = self.provider.clone();
        let templates = self.composer.templates.clone();
        let model = self.composer.model.clone();
        *self = Self::default();
        self.provider = provider;
        self.composer.templates = templates;
        self.composer.model = model;
    }

    /// Return active run id when it can be cancelled.
    pub fn cancellable_run_id(&self) -> Option<Uuid> {
        self.run
            .as_ref()
            .filter(|run| run.can_cancel())
            .map(|run| run.run_id)
    }
}

pub(crate) const fn is_terminal_status(status: IntelligenceRunStatus) -> bool {
    matches!(
        status,
        IntelligenceRunStatus::Succeeded
            | IntelligenceRunStatus::Failed
            | IntelligenceRunStatus::Cancelled
    )
}

pub(crate) fn failure_from_run_error(
    error: Option<IntelligenceError>,
    fallback: impl Into<String>,
) -> SmartShelfFailure {
    error
        .map(SmartShelfFailure::from)
        .unwrap_or_else(|| SmartShelfFailure::unknown(fallback, true))
}

pub(crate) fn already_saved_failure(
    collection_id: CollectionId,
) -> SmartShelfFailure {
    SmartShelfFailure::new(
        SmartShelfFailureCode::SmartShelf(SmartShelfErrorCode::AlreadySaved),
        format!(
            "This smart shelf has already been saved as collection {collection_id}"
        ),
        false,
    )
}

fn draft_parts(
    draft: SmartShelfDraftContent,
) -> (
    Option<String>,
    Option<String>,
    Vec<SmartShelfItemState>,
    Vec<SmartShelfAlternateState>,
) {
    (
        draft.description,
        draft.interpreted_intent,
        draft
            .items
            .into_iter()
            .map(SmartShelfItemState::from_draft_item)
            .collect(),
        draft
            .alternates
            .into_iter()
            .map(SmartShelfAlternateState::from_draft_alternate)
            .collect(),
    )
}

fn merge_regenerate_metadata(
    existing: &Value,
    artifact_id: Uuid,
    locked_count: usize,
) -> Value {
    let mut object = existing.as_object().cloned().unwrap_or_default();
    object.insert("regenerate_unlocked".to_string(), Value::Bool(true));
    object.insert("previous_artifact_id".to_string(), json!(artifact_id));
    object.insert("locked_item_count".to_string(), json!(locked_count));
    Value::Object(object)
}
