//! Typed smart-shelf DTOs layered over generic intelligence drafts.
//!
//! Smart shelves are intentionally narrow: they start a constrained grounded
//! intelligence run, read a draft artifact as a typed ordered shelf, validate
//! recoverable draft issues, and save an accepted draft as a private manual
//! collection.

use std::collections::HashSet;

use ferrex_model::{LibraryId, MediaID};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use uuid::Uuid;

use super::{
    collections::{CollectionId, CollectionSummary},
    intelligence::{
        IntelligenceArtifactSourceEdge, IntelligenceCaps,
        IntelligenceDraftArtifactPayload, IntelligenceMediaKind,
        IntelligenceRunStartResponse, IntelligenceRunStatus,
        IntelligenceSummary,
    },
};

/// Schema version used by smart-shelf draft artifact content.
pub const SMART_SHELF_DRAFT_SCHEMA_VERSION: u16 = 1;
/// Default number of items requested for a smart shelf.
pub const DEFAULT_SMART_SHELF_ITEM_COUNT: u16 = 8;
/// Maximum number of items a smart-shelf request or draft can ask the server to
/// validate/save in one operation.
pub const MAX_SMART_SHELF_ITEM_COUNT: u16 = 50;

fn smart_shelf_draft_schema_version() -> u16 {
    SMART_SHELF_DRAFT_SCHEMA_VERSION
}

fn default_smart_shelf_item_count() -> u16 {
    DEFAULT_SMART_SHELF_ITEM_COUNT
}

fn deserialize_smart_shelf_item_count<'de, D>(
    deserializer: D,
) -> Result<u16, D::Error>
where
    D: Deserializer<'de>,
{
    let value = u16::deserialize(deserializer)?;
    Ok(clamp_smart_shelf_item_count(value))
}

/// Clamp a requested smart-shelf item count to stable server bounds.
pub const fn clamp_smart_shelf_item_count(value: u16) -> u16 {
    if value == 0 {
        DEFAULT_SMART_SHELF_ITEM_COUNT
    } else if value > MAX_SMART_SHELF_ITEM_COUNT {
        MAX_SMART_SHELF_ITEM_COUNT
    } else {
        value
    }
}

/// Narrow start request for a grounded smart-shelf run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SmartShelfStartRequest {
    /// Friendly user prompt or selected template expansion.
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub library_id: Option<LibraryId>,
    /// Media kinds the MVP can safely validate and save. Empty means the server
    /// uses its supported defaults.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub media_kinds: Vec<IntelligenceMediaKind>,
    #[serde(
        default = "default_smart_shelf_item_count",
        deserialize_with = "deserialize_smart_shelf_item_count"
    )]
    pub item_count: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub locked_media_ids: Vec<MediaID>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default)]
    pub caps: IntelligenceCaps,
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub constraints: Value,
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub metadata: Value,
}

/// Smart-shelf start response with the same runtime identifiers as the generic
/// intelligence start route plus the draft schema the provider was constrained
/// to produce.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SmartShelfStartResponse {
    pub run_id: Uuid,
    pub status: IntelligenceRunStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queued_at_epoch_seconds: Option<i64>,
    #[serde(default = "smart_shelf_draft_schema_version")]
    pub draft_schema_version: u16,
}

impl From<IntelligenceRunStartResponse> for SmartShelfStartResponse {
    fn from(value: IntelligenceRunStartResponse) -> Self {
        Self {
            run_id: value.run_id,
            status: value.status,
            provider: value.provider,
            model: value.model,
            queued_at_epoch_seconds: value.queued_at_epoch_seconds,
            draft_schema_version: SMART_SHELF_DRAFT_SCHEMA_VERSION,
        }
    }
}

/// Typed smart-shelf draft content stored in a generic draft artifact body.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SmartShelfDraftContent {
    #[serde(default = "smart_shelf_draft_schema_version")]
    pub schema_version: u16,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interpreted_intent: Option<String>,
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub requested_constraints: Value,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<SmartShelfDraftItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alternates: Vec<SmartShelfDraftAlternate>,
}

/// A selected item in an ordered smart-shelf draft.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SmartShelfDraftItem {
    /// One-based ordinal requested by the model. Save preserves vector order;
    /// the ordinal is retained as provenance for UI/debugging.
    #[serde(default)]
    pub ordinal: u32,
    pub media_id: MediaID,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub year: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<SmartShelfDraftSource>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub locked: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replacement_of: Option<MediaID>,
}

/// Alternate item that can replace a selected smart-shelf item.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SmartShelfDraftAlternate {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_ordinal: Option<u32>,
    pub media_id: MediaID,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub year: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<SmartShelfDraftSource>,
}

impl SmartShelfDraftAlternate {
    pub fn into_item(self, ordinal: u32) -> SmartShelfDraftItem {
        SmartShelfDraftItem {
            ordinal,
            media_id: self.media_id,
            title: self.title,
            subtitle: self.subtitle,
            year: self.year,
            reason: self.reason,
            sources: self.sources,
            locked: false,
            replacement_of: None,
        }
    }
}

/// Bounded provenance chip for an item reason.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SmartShelfDraftSource {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_id: Option<MediaID>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<IntelligenceSummary>,
}

/// Validation severity for recoverable smart-shelf draft issues.
#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum SmartShelfDraftValidationSeverity {
    #[default]
    Error,
    Warning,
}

/// Stable validation issue codes returned by typed draft reads.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum SmartShelfDraftValidationIssueCode {
    MalformedContent,
    EmptyDraft,
    DuplicateMedia,
    UnsupportedMedia,
    UngroundedItem,
    MissingReason,
    MissingSource,
}

/// Stable smart-shelf save/read error codes.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum SmartShelfErrorCode {
    DraftHidden,
    DraftStale,
    DraftMalformed,
    DraftEmpty,
    DuplicateMedia,
    UnsupportedMedia,
    UngroundedItem,
    MissingReason,
    MissingSource,
    AlreadySaved,
    Unauthorized,
    InvalidRequest,
    CollectionConflict,
    CollectionStorageError,
    Internal,
}

impl SmartShelfDraftValidationIssueCode {
    pub const fn save_error_code(self) -> SmartShelfErrorCode {
        match self {
            Self::MalformedContent => SmartShelfErrorCode::DraftMalformed,
            Self::EmptyDraft => SmartShelfErrorCode::DraftEmpty,
            Self::DuplicateMedia => SmartShelfErrorCode::DuplicateMedia,
            Self::UnsupportedMedia => SmartShelfErrorCode::UnsupportedMedia,
            Self::UngroundedItem => SmartShelfErrorCode::UngroundedItem,
            Self::MissingReason => SmartShelfErrorCode::MissingReason,
            Self::MissingSource => SmartShelfErrorCode::MissingSource,
        }
    }
}

/// A single recoverable validation issue in a typed draft.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SmartShelfDraftValidationIssue {
    pub code: SmartShelfDraftValidationIssueCode,
    #[serde(default)]
    pub severity: SmartShelfDraftValidationSeverity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ordinal: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_id: Option<MediaID>,
    pub message: String,
}

impl SmartShelfDraftValidationIssue {
    pub fn error(
        code: SmartShelfDraftValidationIssueCode,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code,
            severity: SmartShelfDraftValidationSeverity::Error,
            ordinal: None,
            media_id: None,
            message: message.into(),
        }
    }

    pub fn for_item(
        code: SmartShelfDraftValidationIssueCode,
        ordinal: u32,
        media_id: MediaID,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code,
            severity: SmartShelfDraftValidationSeverity::Error,
            ordinal: Some(ordinal),
            media_id: Some(media_id),
            message: message.into(),
        }
    }
}

/// Aggregate validation report for a typed draft.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SmartShelfDraftValidation {
    pub valid: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub issues: Vec<SmartShelfDraftValidationIssue>,
}

impl SmartShelfDraftValidation {
    pub fn from_issues(issues: Vec<SmartShelfDraftValidationIssue>) -> Self {
        let valid = !issues.iter().any(|issue| {
            issue.severity == SmartShelfDraftValidationSeverity::Error
        });
        Self { valid, issues }
    }

    pub fn first_save_error_code(&self) -> Option<SmartShelfErrorCode> {
        self.issues
            .iter()
            .find(|issue| {
                issue.severity == SmartShelfDraftValidationSeverity::Error
            })
            .map(|issue| issue.code.save_error_code())
    }
}

/// Typed draft read response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SmartShelfDraftResponse {
    pub artifact_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_user_id: Option<Uuid>,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<IntelligenceSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft: Option<SmartShelfDraftContent>,
    pub validation: SmartShelfDraftValidation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub saved_collection_id: Option<CollectionId>,
}

impl SmartShelfDraftResponse {
    /// Parse a generic intelligence draft artifact into the typed smart-shelf
    /// response. Malformed content is returned as validation issues rather than
    /// an error so clients can display recoverable draft failures.
    pub fn from_draft_artifact(
        payload: IntelligenceDraftArtifactPayload,
    ) -> Self {
        let saved_collection_id =
            saved_collection_id_from_metadata(&payload.metadata)
                .map(CollectionId);
        let artifact_id = payload.artifact_id;
        let run_id = payload.run_id;
        let owner_user_id = payload.owner_user_id;
        let title = payload.title;
        let summary = payload.summary;
        let content = payload.content.clone();
        let source_media_ids =
            grounded_media_ids(payload.media_id, payload.sources.as_slice());

        match serde_json::from_value::<SmartShelfDraftContent>(content) {
            Ok(draft) => {
                let validation = validate_smart_shelf_draft_items(
                    &draft.items,
                    &source_media_ids,
                );
                Self {
                    artifact_id,
                    run_id,
                    owner_user_id,
                    title,
                    summary,
                    draft: Some(draft),
                    validation,
                    saved_collection_id,
                }
            }
            Err(error) => Self {
                artifact_id,
                run_id,
                owner_user_id,
                title,
                summary,
                draft: None,
                validation: SmartShelfDraftValidation::from_issues(vec![
                    SmartShelfDraftValidationIssue::error(
                        SmartShelfDraftValidationIssueCode::MalformedContent,
                        format!(
                            "smart-shelf draft content is malformed: {error}"
                        ),
                    ),
                ]),
                saved_collection_id,
            },
        }
    }
}

/// Save request for accepting a typed smart-shelf draft as a private manual
/// collection. Empty `items` means save the validated draft order as-is;
/// otherwise the provided order is treated as the accepted replacement/lock
/// state and each selected media id must come from the draft item/alternate set.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SmartShelfSaveRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<SmartShelfSaveItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
}

/// Accepted item state supplied by the UI at save time.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SmartShelfSaveItem {
    pub media_id: MediaID,
    #[serde(default, skip_serializing_if = "is_false")]
    pub locked: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replacement_of: Option<MediaID>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<SmartShelfDraftSource>,
}

/// Save response returned after a private manual collection is created.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SmartShelfSaveResponse {
    pub draft_artifact_id: Uuid,
    pub collection_id: CollectionId,
    pub collection: CollectionSummary,
    pub item_count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saved_at_epoch_seconds: Option<i64>,
}

/// Error envelope used by smart-shelf-specific routes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SmartShelfError {
    pub code: SmartShelfErrorCode,
    pub message: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub retryable: bool,
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub details: Value,
}

/// Build the grounded media-id set from durable artifact source edges.
pub fn grounded_media_ids(
    seed_media_id: Option<MediaID>,
    sources: &[IntelligenceArtifactSourceEdge],
) -> HashSet<MediaID> {
    let mut media_ids = HashSet::new();
    if let Some(media_id) = seed_media_id {
        media_ids.insert(media_id);
    }
    for source in sources {
        if let Some(media_id) = source.source_media_id {
            media_ids.insert(media_id);
        }
    }
    media_ids
}

/// Validate the selected smart-shelf item list against durable grounding.
pub fn validate_smart_shelf_draft_items(
    items: &[SmartShelfDraftItem],
    grounded_media_ids: &HashSet<MediaID>,
) -> SmartShelfDraftValidation {
    let mut issues = Vec::new();
    if items.is_empty() {
        issues.push(SmartShelfDraftValidationIssue::error(
            SmartShelfDraftValidationIssueCode::EmptyDraft,
            "smart-shelf draft contains no selected items",
        ));
        return SmartShelfDraftValidation::from_issues(issues);
    }

    let mut seen = HashSet::new();
    for (index, item) in items.iter().enumerate() {
        let ordinal = if item.ordinal == 0 {
            u32::try_from(index + 1).unwrap_or(u32::MAX)
        } else {
            item.ordinal
        };

        if !is_supported_smart_shelf_media(item.media_id) {
            issues.push(SmartShelfDraftValidationIssue::for_item(
                SmartShelfDraftValidationIssueCode::UnsupportedMedia,
                ordinal,
                item.media_id,
                "smart-shelf drafts currently support movie and series items only",
            ));
        }

        if !seen.insert(item.media_id) {
            issues.push(SmartShelfDraftValidationIssue::for_item(
                SmartShelfDraftValidationIssueCode::DuplicateMedia,
                ordinal,
                item.media_id,
                "smart-shelf draft contains the same media item more than once",
            ));
        }

        if item
            .reason
            .as_deref()
            .is_none_or(|reason| reason.trim().is_empty())
        {
            issues.push(SmartShelfDraftValidationIssue::for_item(
                SmartShelfDraftValidationIssueCode::MissingReason,
                ordinal,
                item.media_id,
                "smart-shelf draft item is missing a grounded reason",
            ));
        }

        if !item.sources.iter().any(smart_shelf_source_present) {
            issues.push(SmartShelfDraftValidationIssue::for_item(
                SmartShelfDraftValidationIssueCode::MissingSource,
                ordinal,
                item.media_id,
                "smart-shelf draft item is missing a source/provenance indicator",
            ));
        }

        if !grounded_media_ids.contains(&item.media_id) {
            issues.push(SmartShelfDraftValidationIssue::for_item(
                SmartShelfDraftValidationIssueCode::UngroundedItem,
                ordinal,
                item.media_id,
                "smart-shelf draft item is not grounded by the draft artifact sources",
            ));
        }
    }

    SmartShelfDraftValidation::from_issues(issues)
}

/// Smart-shelf MVP supports media kinds that existing collection/detail UX can
/// safely render for this flow.
pub const fn is_supported_smart_shelf_media(media_id: MediaID) -> bool {
    matches!(media_id, MediaID::Movie(_) | MediaID::Series(_))
}

pub fn smart_shelf_source_present(source: &SmartShelfDraftSource) -> bool {
    source
        .label
        .as_deref()
        .is_some_and(|label| !label.trim().is_empty())
        || source.media_id.is_some()
        || source.artifact_id.is_some()
        || source
            .field
            .as_deref()
            .is_some_and(|field| !field.trim().is_empty())
}

pub fn saved_collection_id_from_metadata(metadata: &Value) -> Option<Uuid> {
    metadata
        .get("smart_shelf_save")
        .and_then(|value| value.get("collection_id"))
        .and_then(Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok())
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrex_model::{EpisodeID, MovieID};

    #[test]
    fn validates_duplicate_unsupported_ungrounded_missing_fields() {
        let grounded =
            HashSet::from([MediaID::Movie(MovieID(Uuid::from_u128(1)))]);
        let source = SmartShelfDraftSource {
            label: Some("Library metadata".to_string()),
            media_id: Some(MediaID::Movie(MovieID(Uuid::from_u128(1)))),
            artifact_id: None,
            field: None,
            evidence: None,
        };
        let validation = validate_smart_shelf_draft_items(
            &[
                SmartShelfDraftItem {
                    ordinal: 1,
                    media_id: MediaID::Movie(MovieID(Uuid::from_u128(1))),
                    title: Some("One".to_string()),
                    subtitle: None,
                    year: None,
                    reason: Some("Grounded reason".to_string()),
                    sources: vec![source.clone()],
                    locked: false,
                    replacement_of: None,
                },
                SmartShelfDraftItem {
                    ordinal: 2,
                    media_id: MediaID::Movie(MovieID(Uuid::from_u128(1))),
                    title: Some("Duplicate".to_string()),
                    subtitle: None,
                    year: None,
                    reason: None,
                    sources: Vec::new(),
                    locked: false,
                    replacement_of: None,
                },
                SmartShelfDraftItem {
                    ordinal: 3,
                    media_id: MediaID::Episode(EpisodeID(Uuid::from_u128(3))),
                    title: Some("Unsupported".to_string()),
                    subtitle: None,
                    year: None,
                    reason: Some("Has a reason".to_string()),
                    sources: vec![source],
                    locked: false,
                    replacement_of: None,
                },
            ],
            &grounded,
        );

        let codes = validation
            .issues
            .iter()
            .map(|issue| issue.code)
            .collect::<Vec<_>>();
        assert!(!validation.valid);
        assert!(
            codes.contains(&SmartShelfDraftValidationIssueCode::DuplicateMedia)
        );
        assert!(
            codes.contains(&SmartShelfDraftValidationIssueCode::MissingReason)
        );
        assert!(
            codes.contains(&SmartShelfDraftValidationIssueCode::MissingSource)
        );
        assert!(
            codes.contains(
                &SmartShelfDraftValidationIssueCode::UnsupportedMedia
            )
        );
        assert!(
            codes.contains(&SmartShelfDraftValidationIssueCode::UngroundedItem)
        );
    }
}
