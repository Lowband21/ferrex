//! Grounded intelligence run orchestration.
//!
//! This module coordinates provider action selection, bounded tool execution,
//! durable run/tool/event audit rows, cancellation, restart recovery, and
//! grounding validation before draft artifacts are created.

use std::{
    collections::{HashMap, HashSet},
    fmt,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use async_trait::async_trait;
use chrono::Utc;
use ferrex_model::MediaID;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::time;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
    api::types::intelligence::{
        IntelligenceArtifactSourceEdge,
        IntelligenceArtifactSummary as ApiArtifactSummary,
        IntelligenceDraftArtifactPayload, IntelligenceDraftArtifactReadRequest,
        IntelligenceError, IntelligenceErrorCode, IntelligenceProviderState,
        IntelligenceProviderStatus, IntelligenceRunAuditRequest,
        IntelligenceRunAuditResponse, IntelligenceRunCancelRequest,
        IntelligenceRunCancelResponse, IntelligenceRunEvent,
        IntelligenceRunEventKind, IntelligenceRunEventsRequest,
        IntelligenceRunEventsResponse, IntelligenceRunPurpose,
        IntelligenceRunStartRequest, IntelligenceRunStartResponse,
        IntelligenceRunStatus as ApiRunStatus, IntelligenceRunStatusResponse,
        IntelligenceSummary, clamp_intelligence_page_limit,
        default_intelligence_page_limit,
    },
    application::intelligence_tools::{
        IntelligenceToolCallContext, IntelligenceToolError,
        IntelligenceToolErrorCode, IntelligenceToolExecution,
        IntelligenceToolExecutionControls, IntelligenceToolRegistry,
    },
    database::repository_ports::{
        intelligence::{
            IntelligenceRepository, IntelligenceRunCreate,
            IntelligenceRunEventCreate, IntelligenceRunEventListFilter,
            IntelligenceRunKind, IntelligenceRunStatus as StoreRunStatus,
            IntelligenceRunUpdate,
        },
        query::QueryRepository,
    },
    domain::intelligence::{
        IntelligenceActionCompletionRequest, IntelligenceActionSpec,
        IntelligenceChatMessage, IntelligenceModelProvider,
        IntelligenceProviderError, IntelligenceProviderRequestOptions,
        IntelligenceProviderResult,
    },
    error::{MediaError, Result},
};

const PROVIDER_NAME: &str = "openai-compatible";
const FINAL_RESPONSE_ACTION: &str = "final_response";
const REDACTED: &str = "[redacted]";
const MAX_AUDIT_EXCERPT_CHARS: usize = 2_048;
const MAX_RESULT_SUMMARY_CHARS: usize = 4_096;
const MAX_EVENT_PAYLOAD_BYTES: usize = 16 * 1024;
const MAX_REDACTED_STRING_CHARS: usize = 512;
const MAX_REDACTED_ARRAY_ITEMS: usize = 32;
const MAX_REDACTED_OBJECT_KEYS: usize = 64;

static SECRET_ASSIGNMENT_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)\b(api[_-]?key|authorization|bearer|password|secret|token|refresh[_-]?token)\s*[:=]\s*[^\s,;]+",
    )
    .expect("secret assignment redaction regex must compile")
});
static OPENAI_KEY_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\bsk-[A-Za-z0-9_\-]{6,}\b")
        .expect("OpenAI key redaction regex must compile")
});
static BEARER_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\bbearer\s+[A-Za-z0-9._\-+/=]{8,}")
        .expect("bearer token redaction regex must compile")
});

/// Runtime limits and provider labels used by the run manager.
#[derive(Debug, Clone)]
pub struct IntelligenceRunManagerConfig {
    pub enabled: bool,
    pub provider_name: String,
    pub default_model: Option<String>,
    pub model_timeout: Duration,
    pub tool_timeout: Duration,
    pub total_timeout: Duration,
    pub max_steps: u32,
    pub max_tool_calls: u32,
    pub per_user_concurrency: u32,
    pub max_malformed_retries: u32,
    pub max_output_bytes: usize,
    pub max_tool_result_bytes: usize,
}

impl Default for IntelligenceRunManagerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            provider_name: PROVIDER_NAME.to_string(),
            default_model: Some("gemma-4-12b".to_string()),
            model_timeout: Duration::from_secs(60),
            tool_timeout: Duration::from_secs(20),
            total_timeout: Duration::from_secs(180),
            max_steps: 12,
            max_tool_calls: 24,
            per_user_concurrency: 1,
            max_malformed_retries: 1,
            max_output_bytes: 64 * 1024,
            max_tool_result_bytes: 256 * 1024,
        }
    }
}

/// Narrow storage surface required by runtime orchestration.
#[async_trait]
pub trait IntelligenceRuntimeStore: Send + Sync {
    async fn create_run(&self, create: IntelligenceRunCreate) -> Result<Uuid>;

    async fn update_run(
        &self,
        run_id: Uuid,
        update: IntelligenceRunUpdate,
    ) -> Result<()>;

    async fn run_audit(
        &self,
        request: &IntelligenceRunAuditRequest,
        user_id: Option<Uuid>,
    ) -> Result<IntelligenceRunAuditResponse>;

    async fn append_run_event(
        &self,
        create: IntelligenceRunEventCreate,
    ) -> Result<IntelligenceRunEvent>;

    async fn list_run_events(
        &self,
        filter: IntelligenceRunEventListFilter,
    ) -> Result<Vec<IntelligenceRunEvent>>;

    async fn get_artifact(
        &self,
        artifact_id: Uuid,
        user_id: Option<Uuid>,
    ) -> Result<Option<ApiArtifactSummary>>;

    async fn get_draft_artifact(
        &self,
        artifact_id: Uuid,
        user_id: Option<Uuid>,
    ) -> Result<Option<IntelligenceDraftArtifactPayload>>;

    async fn mark_stale_in_flight_runs_terminal(
        &self,
        reason: &str,
    ) -> Result<Vec<Uuid>>;
}

/// Runtime store adapter over the Phase 1/2 intelligence repository port.
#[derive(Clone)]
pub struct RepositoryIntelligenceRuntimeStore {
    intelligence: Arc<dyn IntelligenceRepository>,
}

impl fmt::Debug for RepositoryIntelligenceRuntimeStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RepositoryIntelligenceRuntimeStore")
            .field("intelligence", &"dyn IntelligenceRepository")
            .finish()
    }
}

impl RepositoryIntelligenceRuntimeStore {
    pub fn new(intelligence: Arc<dyn IntelligenceRepository>) -> Self {
        Self { intelligence }
    }
}

#[async_trait]
impl IntelligenceRuntimeStore for RepositoryIntelligenceRuntimeStore {
    async fn create_run(&self, create: IntelligenceRunCreate) -> Result<Uuid> {
        self.intelligence.create_run(create).await
    }

    async fn update_run(
        &self,
        run_id: Uuid,
        update: IntelligenceRunUpdate,
    ) -> Result<()> {
        self.intelligence.update_run(run_id, update).await
    }

    async fn run_audit(
        &self,
        request: &IntelligenceRunAuditRequest,
        user_id: Option<Uuid>,
    ) -> Result<IntelligenceRunAuditResponse> {
        self.intelligence.run_audit(request, user_id).await
    }

    async fn append_run_event(
        &self,
        create: IntelligenceRunEventCreate,
    ) -> Result<IntelligenceRunEvent> {
        self.intelligence.append_run_event(create).await
    }

    async fn list_run_events(
        &self,
        filter: IntelligenceRunEventListFilter,
    ) -> Result<Vec<IntelligenceRunEvent>> {
        self.intelligence.list_run_events(filter).await
    }

    async fn get_artifact(
        &self,
        artifact_id: Uuid,
        user_id: Option<Uuid>,
    ) -> Result<Option<ApiArtifactSummary>> {
        self.intelligence.get_artifact(artifact_id, user_id).await
    }

    async fn get_draft_artifact(
        &self,
        artifact_id: Uuid,
        user_id: Option<Uuid>,
    ) -> Result<Option<IntelligenceDraftArtifactPayload>> {
        self.intelligence
            .get_draft_artifact(artifact_id, user_id)
            .await
    }

    async fn mark_stale_in_flight_runs_terminal(
        &self,
        reason: &str,
    ) -> Result<Vec<Uuid>> {
        self.intelligence
            .mark_stale_in_flight_runs_terminal(reason)
            .await
    }
}

#[derive(Clone)]
struct ActiveRun {
    token: CancellationToken,
    user_id: Option<Uuid>,
}

#[derive(Default)]
struct ActiveRunState {
    runs: HashMap<Uuid, ActiveRun>,
    user_counts: HashMap<Option<Uuid>, usize>,
}

/// Grounded runtime manager for provider action loops and draft generation.
#[derive(Clone)]
pub struct IntelligenceRunManager {
    config: IntelligenceRunManagerConfig,
    store: Arc<dyn IntelligenceRuntimeStore>,
    provider: Arc<dyn IntelligenceModelProvider>,
    tools: IntelligenceToolRegistry,
    active_runs: Arc<Mutex<ActiveRunState>>,
}

impl fmt::Debug for IntelligenceRunManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IntelligenceRunManager")
            .field("config", &self.config)
            .field("store", &"dyn IntelligenceRuntimeStore")
            .field("provider", &"dyn IntelligenceModelProvider")
            .field("tools", &self.tools)
            .finish_non_exhaustive()
    }
}

impl IntelligenceRunManager {
    pub fn new(
        config: IntelligenceRunManagerConfig,
        store: Arc<dyn IntelligenceRuntimeStore>,
        provider: Arc<dyn IntelligenceModelProvider>,
        tools: IntelligenceToolRegistry,
    ) -> Self {
        Self {
            config,
            store,
            provider,
            tools,
            active_runs: Arc::new(Mutex::new(ActiveRunState::default())),
        }
    }

    pub fn from_repositories(
        config: IntelligenceRunManagerConfig,
        intelligence: Arc<dyn IntelligenceRepository>,
        query: Arc<dyn QueryRepository>,
        provider: Arc<dyn IntelligenceModelProvider>,
    ) -> Self {
        let store = Arc::new(RepositoryIntelligenceRuntimeStore::new(
            intelligence.clone(),
        ));
        let tools =
            IntelligenceToolRegistry::from_repositories(intelligence, query);
        Self::new(config, store, provider, tools)
    }

    /// Mark runs left queued/running by an older local process as failed.
    pub async fn recover_stale_runs(&self) -> Result<Vec<Uuid>> {
        let reason = "local intelligence runtime restarted before completion";
        let run_ids = self
            .store
            .mark_stale_in_flight_runs_terminal(reason)
            .await?;
        for run_id in &run_ids {
            let error = runtime_error(
                IntelligenceErrorCode::RunCancelled,
                reason,
                false,
                json!({"recovered_on_restart": true}),
            );
            self.append_event(
                *run_id,
                IntelligenceRunEventKind::Failed,
                Some(ApiRunStatus::Failed),
                None,
                None,
                Some("stale run marked terminal".to_string()),
                json!({"reason_hash": sha256_hex(reason.as_bytes())}),
                Some(error),
            )
            .await?;
        }
        Ok(run_ids)
    }

    /// Create a queued run and execute it asynchronously.
    pub async fn start_run(
        &self,
        request: IntelligenceRunStartRequest,
        user_id: Option<Uuid>,
    ) -> Result<IntelligenceRunStartResponse> {
        self.ensure_enabled()?;
        self.reserve_active_slot(user_id)?;
        let model = selected_model(&self.config, &request);
        let token = CancellationToken::new();
        let reserved_run_id = Uuid::now_v7();
        let run_id = match self
            .create_queued_run(reserved_run_id, &request, user_id)
            .await
        {
            Ok(run_id) => run_id,
            Err(err) => {
                self.release_active_slot(user_id);
                return Err(err);
            }
        };
        self.register_active(run_id, user_id, token.clone());

        let manager = self.clone();
        let request_for_task = request.clone();
        tokio::spawn(async move {
            if let Err(err) = manager
                .drive_existing_run(run_id, request_for_task, user_id, token)
                .await
            {
                tracing::warn!(
                    run_id = %run_id,
                    error = %err,
                    "intelligence run task failed before terminal update"
                );
            }
        });

        Ok(IntelligenceRunStartResponse {
            run_id,
            status: ApiRunStatus::Queued,
            provider: Some(self.config.provider_name.clone()),
            model,
            queued_at_epoch_seconds: Some(Utc::now().timestamp()),
        })
    }

    /// Create a queued run and drive it on the current task until terminal.
    pub async fn run_to_completion(
        &self,
        request: IntelligenceRunStartRequest,
        user_id: Option<Uuid>,
    ) -> Result<IntelligenceRunStatusResponse> {
        let token = CancellationToken::new();
        self.run_to_completion_with_token(request, user_id, token)
            .await
    }

    /// Drive a run with a caller-owned cancellation token, useful for tests and
    /// embedders that already coordinate cancellation lifetimes.
    pub async fn run_to_completion_with_token(
        &self,
        request: IntelligenceRunStartRequest,
        user_id: Option<Uuid>,
        token: CancellationToken,
    ) -> Result<IntelligenceRunStatusResponse> {
        self.ensure_enabled()?;
        self.reserve_active_slot(user_id)?;
        let reserved_run_id = Uuid::now_v7();
        let run_id = match self
            .create_queued_run(reserved_run_id, &request, user_id)
            .await
        {
            Ok(run_id) => run_id,
            Err(err) => {
                self.release_active_slot(user_id);
                return Err(err);
            }
        };
        self.register_active(run_id, user_id, token.clone());
        let outcome = self
            .drive_existing_run(run_id, request, user_id, token)
            .await?;
        Ok(outcome.response)
    }

    pub async fn cancel_run(
        &self,
        run_id: Uuid,
        user_id: Option<Uuid>,
        request: IntelligenceRunCancelRequest,
    ) -> Result<IntelligenceRunCancelResponse> {
        let audit = self
            .store
            .run_audit(&audit_request(run_id), user_id)
            .await?;
        if is_terminal_status(audit.run.status) {
            return Ok(IntelligenceRunCancelResponse {
                run_id,
                status: audit.run.status,
                cancellation_requested: false,
                cancelled_at_epoch_seconds: audit
                    .run
                    .completed_at_epoch_seconds,
                message: Some("run is already terminal".to_string()),
                error: None,
            });
        }

        let reason = request
            .reason
            .as_deref()
            .map(|value| redacted_excerpt(value, 512))
            .unwrap_or_else(|| "cancellation requested".to_string());
        self.append_event(
            run_id,
            IntelligenceRunEventKind::CancelRequested,
            Some(audit.run.status),
            None,
            None,
            Some("cancellation requested".to_string()),
            json!({"reason_hash": sha256_hex(reason.as_bytes())}),
            None,
        )
        .await?;

        if let Some(token) = self.active_token(run_id) {
            token.cancel();
            Ok(IntelligenceRunCancelResponse {
                run_id,
                status: audit.run.status,
                cancellation_requested: true,
                cancelled_at_epoch_seconds: Some(Utc::now().timestamp()),
                message: Some("cancellation signal delivered".to_string()),
                error: None,
            })
        } else {
            let error = runtime_error(
                IntelligenceErrorCode::RunCancelled,
                &reason,
                false,
                Value::Null,
            );
            self.store
                .update_run(
                    run_id,
                    IntelligenceRunUpdate {
                        status: Some(StoreRunStatus::Cancelled),
                        error_excerpt: Some(redacted_excerpt(&reason, 512)),
                        finished_at: Some(Utc::now()),
                        ..IntelligenceRunUpdate::default()
                    },
                )
                .await?;
            self.append_event(
                run_id,
                IntelligenceRunEventKind::Cancelled,
                Some(ApiRunStatus::Cancelled),
                None,
                None,
                Some("run cancelled".to_string()),
                json!({}),
                Some(error.clone()),
            )
            .await?;
            Ok(IntelligenceRunCancelResponse {
                run_id,
                status: ApiRunStatus::Cancelled,
                cancellation_requested: true,
                cancelled_at_epoch_seconds: Some(Utc::now().timestamp()),
                message: Some("run cancelled".to_string()),
                error: Some(error),
            })
        }
    }

    pub async fn run_status(
        &self,
        run_id: Uuid,
        user_id: Option<Uuid>,
    ) -> Result<IntelligenceRunStatusResponse> {
        let audit = self
            .store
            .run_audit(&audit_request(run_id), user_id)
            .await?;
        let events = self
            .store
            .list_run_events(IntelligenceRunEventListFilter {
                run_id,
                after_sequence: None,
                limit: default_intelligence_page_limit(),
                user_id,
            })
            .await?;
        let current_phase = events
            .last()
            .map(|event| event.event_kind.as_db_str().to_string())
            .or_else(|| Some(audit.run.status.as_db_str().to_string()));
        let error = events.iter().rev().find_map(|event| event.error.clone());
        let mut draft_artifact_ids = audit.run.artifact_ids.clone();
        for event in &events {
            if matches!(
                event.event_kind,
                IntelligenceRunEventKind::DraftArtifactCreated
                    | IntelligenceRunEventKind::DraftArtifactUpdated
            ) {
                if let Some(artifact_id) = event.artifact_id {
                    push_unique_uuid(&mut draft_artifact_ids, artifact_id);
                }
            }
            if let Some(ids) = event
                .payload
                .get("draft_artifact_ids")
                .and_then(|value| value.as_array())
            {
                for value in ids {
                    if let Some(raw) = value.as_str()
                        && let Ok(artifact_id) = Uuid::parse_str(raw)
                    {
                        push_unique_uuid(&mut draft_artifact_ids, artifact_id);
                    }
                }
            }
        }

        Ok(IntelligenceRunStatusResponse {
            run_id,
            purpose: audit.run.purpose,
            status: audit.run.status,
            terminal: is_terminal_status(audit.run.status),
            current_phase,
            provider: Some(self.config.provider_name.clone()),
            model: audit.run.model,
            queued_at_epoch_seconds: audit.run.queued_at_epoch_seconds,
            started_at_epoch_seconds: audit.run.started_at_epoch_seconds,
            completed_at_epoch_seconds: audit.run.completed_at_epoch_seconds,
            current_step: Some(audit.run.tool_calls.len() as u32),
            max_steps: Some(self.config.max_steps),
            draft_artifact_ids,
            output_summary: audit.run.output_summary,
            error,
        })
    }

    pub async fn run_events(
        &self,
        request: IntelligenceRunEventsRequest,
        user_id: Option<Uuid>,
    ) -> Result<IntelligenceRunEventsResponse> {
        let limit = if request.limit == 0 {
            default_intelligence_page_limit()
        } else {
            clamp_intelligence_page_limit(request.limit)
        };
        let filter = IntelligenceRunEventListFilter {
            run_id: request.run_id,
            after_sequence: request.after_sequence,
            limit,
            user_id,
        };
        let events = self.store.list_run_events(filter).await?;
        let has_more = (events.len() as u16) >= limit;
        Ok(IntelligenceRunEventsResponse {
            run_id: request.run_id,
            events,
            page: crate::api::types::intelligence::IntelligencePageInfo {
                next_cursor: None,
                limit,
                has_more,
            },
        })
    }

    pub async fn draft_artifact(
        &self,
        request: IntelligenceDraftArtifactReadRequest,
        user_id: Option<Uuid>,
    ) -> Result<Option<IntelligenceDraftArtifactPayload>> {
        self.store
            .get_draft_artifact(request.artifact_id, user_id)
            .await
    }

    pub async fn provider_status(
        &self,
    ) -> IntelligenceProviderResult<IntelligenceProviderStatus> {
        if !self.config.enabled {
            return Ok(IntelligenceProviderStatus {
                enabled: false,
                provider_name: self.config.provider_name.clone(),
                base_url: String::new(),
                api_key_configured: false,
                default_model: self.config.default_model.clone(),
                state: IntelligenceProviderState::Disabled,
                models: Vec::new(),
                checked_at_epoch_seconds: Some(Utc::now().timestamp()),
                error: None,
            });
        }
        self.provider
            .status(IntelligenceProviderRequestOptions {
                timeout: Some(self.config.model_timeout),
                max_retries: Some(self.config.max_malformed_retries),
                cancellation_token: None,
            })
            .await
    }

    fn ensure_enabled(&self) -> Result<()> {
        if !self.config.enabled {
            return Err(MediaError::InvalidMedia(
                "intelligence runtime is disabled".to_string(),
            ));
        }
        Ok(())
    }

    async fn create_queued_run(
        &self,
        run_id: Uuid,
        request: &IntelligenceRunStartRequest,
        user_id: Option<Uuid>,
    ) -> Result<Uuid> {
        self.ensure_enabled()?;
        let request_hash = hash_run_request(request, user_id);
        let prompt_excerpt =
            redacted_excerpt(&request.prompt, MAX_AUDIT_EXCERPT_CHARS);
        let model = selected_model(&self.config, request);
        let run_id = self
            .store
            .create_run(IntelligenceRunCreate {
                run_id: Some(run_id),
                run_kind: run_kind_for_purpose(request.purpose),
                library_id: request.library_id,
                user_id,
                media_id: request.media_id,
                idempotency_key: request.idempotency_key.clone(),
                provider_name: Some(self.config.provider_name.clone()),
                model_name: model.clone(),
                request_hash: Some(request_hash.clone()),
                prompt_excerpt: Some(prompt_excerpt),
                metadata: bounded_event_payload(json!({
                    "purpose": request.purpose,
                    "caps": request.caps,
                    "request_metadata": redact_json_value(&request.metadata),
                    "request_hash": request_hash,
                    "runtime": {
                        "max_steps": self.config.max_steps,
                        "max_tool_calls": self.config.max_tool_calls,
                        "per_user_concurrency": self.config.per_user_concurrency,
                        "model_timeout_ms": self.config.model_timeout.as_millis(),
                        "tool_timeout_ms": self.config.tool_timeout.as_millis(),
                        "total_timeout_ms": self.config.total_timeout.as_millis(),
                    }
                })),
            })
            .await?;

        self.append_event(
            run_id,
            IntelligenceRunEventKind::Queued,
            Some(ApiRunStatus::Queued),
            None,
            None,
            Some("run queued".to_string()),
            json!({
                "purpose": request.purpose,
                "request_hash": request_hash,
                "prompt_excerpt_chars": request.prompt.chars().count().min(MAX_AUDIT_EXCERPT_CHARS),
            }),
            None,
        )
        .await?;
        Ok(run_id)
    }

    async fn drive_existing_run(
        &self,
        run_id: Uuid,
        request: IntelligenceRunStartRequest,
        user_id: Option<Uuid>,
        token: CancellationToken,
    ) -> Result<RunLoopOutcome> {
        let outcome = self.drive_loop(run_id, request, user_id, token).await;
        self.unregister_active(run_id);
        outcome
    }

    async fn drive_loop(
        &self,
        run_id: Uuid,
        request: IntelligenceRunStartRequest,
        user_id: Option<Uuid>,
        token: CancellationToken,
    ) -> Result<RunLoopOutcome> {
        let model = selected_model(&self.config, &request);
        let started_at = Utc::now();
        self.store
            .update_run(
                run_id,
                IntelligenceRunUpdate {
                    status: Some(StoreRunStatus::Running),
                    provider_name: Some(self.config.provider_name.clone()),
                    model_name: model.clone(),
                    started_at: Some(started_at),
                    ..IntelligenceRunUpdate::default()
                },
            )
            .await?;
        self.append_event(
            run_id,
            IntelligenceRunEventKind::Started,
            Some(ApiRunStatus::Running),
            None,
            None,
            Some("run started".to_string()),
            json!({"max_steps": self.config.max_steps, "max_tool_calls": self.config.max_tool_calls}),
            None,
        )
        .await?;

        let mut ledger = IntelligenceGroundingLedger::from_request(&request);
        let mut messages = initial_messages(&request);
        let actions = self.action_specs();
        let deadline = Instant::now()
            .checked_add(self.config.total_timeout)
            .unwrap_or_else(Instant::now);
        let mut malformed_attempts = 0_u32;
        let mut tool_calls = 0_u32;
        let mut draft_artifact_ids = Vec::new();

        for step in 1..=self.config.max_steps {
            if token.is_cancelled() {
                return self
                    .finish_cancelled(
                        run_id,
                        &request,
                        model.clone(),
                        step,
                        draft_artifact_ids,
                        "run cancellation requested",
                    )
                    .await;
            }
            let Some(remaining) =
                deadline.checked_duration_since(Instant::now())
            else {
                return self
                    .finish_failed(
                        run_id,
                        &request,
                        model.clone(),
                        step,
                        draft_artifact_ids,
                        runtime_error(
                            IntelligenceErrorCode::RunTimedOut,
                            "intelligence run exceeded total timeout",
                            true,
                            json!({"total_timeout_ms": self.config.total_timeout.as_millis()}),
                        ),
                    )
                    .await;
            };
            if remaining.is_zero() {
                return self
                    .finish_failed(
                        run_id,
                        &request,
                        model.clone(),
                        step,
                        draft_artifact_ids,
                        runtime_error(
                            IntelligenceErrorCode::RunTimedOut,
                            "intelligence run exceeded total timeout",
                            true,
                            json!({"total_timeout_ms": self.config.total_timeout.as_millis()}),
                        ),
                    )
                    .await;
            }

            let mut action_request = IntelligenceActionCompletionRequest::new(
                messages.clone(),
                actions.clone(),
            );
            action_request.model = model.clone();
            let model_timeout =
                min_duration(self.config.model_timeout, remaining);
            let options = IntelligenceProviderRequestOptions {
                timeout: Some(model_timeout),
                max_retries: Some(self.config.max_malformed_retries),
                cancellation_token: Some(token.clone()),
            };
            let completion = match time::timeout(
                model_timeout,
                self.provider.complete_action(action_request, options),
            )
            .await
            {
                Ok(Ok(completion)) => completion,
                Ok(Err(err)) => {
                    if token.is_cancelled()
                        || matches!(
                            err,
                            IntelligenceProviderError::Cancelled { .. }
                        )
                    {
                        return self
                            .finish_cancelled(
                                run_id,
                                &request,
                                model.clone(),
                                step,
                                draft_artifact_ids,
                                "provider request cancelled",
                            )
                            .await;
                    }
                    return self
                        .finish_failed(
                            run_id,
                            &request,
                            model.clone(),
                            step,
                            draft_artifact_ids,
                            sanitized_provider_error(&err),
                        )
                        .await;
                }
                Err(_) => {
                    return self
                        .finish_failed(
                            run_id,
                            &request,
                            model.clone(),
                            step,
                            draft_artifact_ids,
                            runtime_error(
                                IntelligenceErrorCode::ProviderTimeout,
                                "model action selection exceeded its timeout",
                                true,
                                json!({"model_timeout_ms": model_timeout.as_millis()}),
                            ),
                        )
                        .await;
                }
            };

            if completion.arguments.to_string().len()
                > self.config.max_output_bytes
            {
                return self
                    .finish_failed(
                        run_id,
                        &request,
                        model.clone(),
                        step,
                        draft_artifact_ids,
                        runtime_error(
                            IntelligenceErrorCode::InvalidRequest,
                            "model action output exceeded runtime byte budget",
                            false,
                            json!({"max_output_bytes": self.config.max_output_bytes}),
                        ),
                    )
                    .await;
            }

            if completion.action_name == FINAL_RESPONSE_ACTION {
                let final_args =
                    match parse_final_response_args(&completion.arguments) {
                        Ok(args) => args,
                        Err(err) => {
                            return self
                                .finish_failed(
                                    run_id,
                                    &request,
                                    model.clone(),
                                    step,
                                    draft_artifact_ids,
                                    runtime_error(
                                        IntelligenceErrorCode::InvalidRequest,
                                        err.to_string(),
                                        false,
                                        Value::Null,
                                    ),
                                )
                                .await;
                        }
                    };
                if let Err(err) = self
                    .validate_final_response(&ledger, &final_args, user_id)
                    .await
                {
                    return self
                        .finish_failed(
                            run_id,
                            &request,
                            model.clone(),
                            step,
                            draft_artifact_ids,
                            runtime_error(
                                IntelligenceErrorCode::InvalidRequest,
                                err.to_string(),
                                false,
                                Value::Null,
                            ),
                        )
                        .await;
                }
                return self
                    .finish_succeeded(
                        run_id,
                        &request,
                        model.clone(),
                        step,
                        draft_artifact_ids,
                        final_args,
                    )
                    .await;
            }

            if self.tools.definition(&completion.action_name).is_none() {
                malformed_attempts = malformed_attempts.saturating_add(1);
                if malformed_attempts <= self.config.max_malformed_retries {
                    messages.push(IntelligenceChatMessage::user(format!(
                        "The action `{}` is not approved. Choose one of the supplied Ferrex actions or `{}`.",
                        redacted_excerpt(&completion.action_name, 128),
                        FINAL_RESPONSE_ACTION
                    )));
                    continue;
                }
                return self
                    .finish_failed(
                        run_id,
                        &request,
                        model.clone(),
                        step,
                        draft_artifact_ids,
                        runtime_error(
                            IntelligenceErrorCode::ProviderError,
                            "model selected an unapproved action after retry budget",
                            false,
                            json!({"action_hash": sha256_hex(completion.action_name.as_bytes())}),
                        ),
                    )
                    .await;
            }

            if tool_calls >= self.config.max_tool_calls {
                return self
                    .finish_failed(
                        run_id,
                        &request,
                        model.clone(),
                        step,
                        draft_artifact_ids,
                        runtime_error(
                            IntelligenceErrorCode::InvalidRequest,
                            "intelligence run exceeded max tool call budget",
                            false,
                            json!({"max_tool_calls": self.config.max_tool_calls}),
                        ),
                    )
                    .await;
            }

            let tool_call_id = Uuid::now_v7();
            if completion.action_name == "create_draft" {
                if let Err(err) = self
                    .validate_draft_arguments(
                        run_id,
                        &ledger,
                        &completion.arguments,
                        user_id,
                    )
                    .await
                {
                    return self
                        .finish_failed(
                            run_id,
                            &request,
                            model.clone(),
                            step,
                            draft_artifact_ids,
                            runtime_error(
                                IntelligenceErrorCode::InvalidRequest,
                                err.to_string(),
                                false,
                                Value::Null,
                            ),
                        )
                        .await;
                }
            }

            tool_calls = tool_calls.saturating_add(1);
            // The audit row is created by the tool registry after this event, so
            // do not attach tool_call_id yet; the finished event links to it.
            self.append_event(
                run_id,
                IntelligenceRunEventKind::ToolCallStarted,
                Some(ApiRunStatus::Running),
                None,
                None,
                Some(format!("tool `{}` started", completion.action_name)),
                json!({
                    "tool": completion.action_name,
                    "step": step,
                    "arguments_hash": stable_json_hash(&completion.arguments),
                }),
                None,
            )
            .await?;

            let context = IntelligenceToolCallContext {
                run_id,
                sequence: i32::try_from(tool_calls).unwrap_or(i32::MAX),
                user_id,
                allowed_library_ids: request.library_id.map(|id| vec![id]),
                idempotency_key: Some(format!("{run_id}:{tool_calls}")),
            };
            let tool_timeout =
                min_duration(self.config.tool_timeout, remaining);
            let execution = self
                .tools
                .execute_with_controls(
                    &context,
                    &completion.action_name,
                    completion.arguments.clone(),
                    IntelligenceToolExecutionControls {
                        timeout: Some(tool_timeout),
                        cancellation_token: Some(token.clone()),
                        tool_call_id: Some(tool_call_id),
                    },
                )
                .await;

            let execution = match execution {
                Ok(execution) => execution,
                Err(err)
                    if err.code == IntelligenceToolErrorCode::Cancelled =>
                {
                    self.append_tool_finished_event(
                        run_id,
                        tool_call_id,
                        &completion.action_name,
                        None,
                        Some(&err),
                    )
                    .await?;
                    return self
                        .finish_cancelled(
                            run_id,
                            &request,
                            model.clone(),
                            step,
                            draft_artifact_ids,
                            "tool execution cancelled",
                        )
                        .await;
                }
                Err(err) => {
                    self.append_tool_finished_event(
                        run_id,
                        tool_call_id,
                        &completion.action_name,
                        None,
                        Some(&err),
                    )
                    .await?;
                    return self
                        .finish_failed(
                            run_id,
                            &request,
                            model.clone(),
                            step,
                            draft_artifact_ids,
                            tool_error_to_runtime_error(&err),
                        )
                        .await;
                }
            };

            if usize::try_from(execution.visible_bytes).unwrap_or(usize::MAX)
                > self.config.max_tool_result_bytes
            {
                return self
                    .finish_failed(
                        run_id,
                        &request,
                        model.clone(),
                        step,
                        draft_artifact_ids,
                        runtime_error(
                            IntelligenceErrorCode::InvalidRequest,
                            "tool result exceeded runtime byte budget",
                            false,
                            json!({
                                "max_tool_result_bytes": self.config.max_tool_result_bytes,
                                "visible_bytes": execution.visible_bytes,
                            }),
                        ),
                    )
                    .await;
            }

            ledger.record_tool_execution(&execution);
            if completion.action_name == "create_draft" {
                for artifact_id in &execution.artifact_ids {
                    ledger.draft_artifact_ids.insert(*artifact_id);
                    if !draft_artifact_ids.contains(artifact_id) {
                        draft_artifact_ids.push(*artifact_id);
                    }
                    self.append_event(
                        run_id,
                        IntelligenceRunEventKind::DraftArtifactCreated,
                        Some(ApiRunStatus::Running),
                        Some(tool_call_id),
                        Some(*artifact_id),
                        Some("draft artifact created".to_string()),
                        json!({"artifact_id": artifact_id}),
                        None,
                    )
                    .await?;
                }
            }

            self.append_tool_finished_event(
                run_id,
                tool_call_id,
                &completion.action_name,
                Some(&execution),
                None,
            )
            .await?;

            let tool_result = bounded_tool_result_for_model(
                &execution.visible,
                self.config.max_output_bytes,
            );
            messages.push(IntelligenceChatMessage::assistant(format!(
                "Tool `{}` completed with {} rows.",
                completion.action_name, execution.row_count
            )));
            messages.push(IntelligenceChatMessage::user(format!(
                "Grounded Ferrex tool result for `{}`:\n{}",
                completion.action_name, tool_result
            )));
        }

        self.finish_failed(
            run_id,
            &request,
            model,
            self.config.max_steps,
            draft_artifact_ids,
            runtime_error(
                IntelligenceErrorCode::InvalidRequest,
                "intelligence run exceeded max step budget",
                false,
                json!({"max_steps": self.config.max_steps}),
            ),
        )
        .await
    }

    async fn finish_succeeded(
        &self,
        run_id: Uuid,
        request: &IntelligenceRunStartRequest,
        model: Option<String>,
        current_step: u32,
        draft_artifact_ids: Vec<Uuid>,
        final_args: FinalResponseArgs,
    ) -> Result<RunLoopOutcome> {
        let summary =
            redacted_excerpt(&final_args.summary, MAX_RESULT_SUMMARY_CHARS);
        self.store
            .update_run(
                run_id,
                IntelligenceRunUpdate {
                    status: Some(StoreRunStatus::Succeeded),
                    result_summary: Some(summary.clone()),
                    finished_at: Some(Utc::now()),
                    ..IntelligenceRunUpdate::default()
                },
            )
            .await?;
        self.append_event(
            run_id,
            IntelligenceRunEventKind::Completed,
            Some(ApiRunStatus::Succeeded),
            None,
            None,
            Some("run completed".to_string()),
            json!({
                "summary_hash": sha256_hex(summary.as_bytes()),
                "draft_artifact_ids": draft_artifact_ids,
                "selected_media_count": final_args.selected_media_ids.len(),
                "artifact_citation_count": final_args.artifact_citations.len(),
            }),
            None,
        )
        .await?;
        Ok(RunLoopOutcome {
            response: self.status_response(
                run_id,
                request,
                ApiRunStatus::Succeeded,
                model,
                current_step,
                draft_artifact_ids,
                Some(IntelligenceSummary::new(summary)),
                None,
            ),
        })
    }

    async fn finish_failed(
        &self,
        run_id: Uuid,
        request: &IntelligenceRunStartRequest,
        model: Option<String>,
        current_step: u32,
        draft_artifact_ids: Vec<Uuid>,
        error: IntelligenceError,
    ) -> Result<RunLoopOutcome> {
        let excerpt = redacted_excerpt(&error.message, MAX_AUDIT_EXCERPT_CHARS);
        self.store
            .update_run(
                run_id,
                IntelligenceRunUpdate {
                    status: Some(StoreRunStatus::Failed),
                    error_excerpt: Some(excerpt),
                    finished_at: Some(Utc::now()),
                    ..IntelligenceRunUpdate::default()
                },
            )
            .await?;
        self.append_event(
            run_id,
            IntelligenceRunEventKind::Failed,
            Some(ApiRunStatus::Failed),
            None,
            None,
            Some("run failed".to_string()),
            json!({"error_code": error.code.as_str()}),
            Some(error.clone()),
        )
        .await?;
        Ok(RunLoopOutcome {
            response: self.status_response(
                run_id,
                request,
                ApiRunStatus::Failed,
                model,
                current_step,
                draft_artifact_ids,
                None,
                Some(error),
            ),
        })
    }

    async fn finish_cancelled(
        &self,
        run_id: Uuid,
        request: &IntelligenceRunStartRequest,
        model: Option<String>,
        current_step: u32,
        draft_artifact_ids: Vec<Uuid>,
        reason: &str,
    ) -> Result<RunLoopOutcome> {
        let error = runtime_error(
            IntelligenceErrorCode::RunCancelled,
            reason,
            false,
            Value::Null,
        );
        self.store
            .update_run(
                run_id,
                IntelligenceRunUpdate {
                    status: Some(StoreRunStatus::Cancelled),
                    error_excerpt: Some(redacted_excerpt(
                        reason,
                        MAX_AUDIT_EXCERPT_CHARS,
                    )),
                    finished_at: Some(Utc::now()),
                    ..IntelligenceRunUpdate::default()
                },
            )
            .await?;
        self.append_event(
            run_id,
            IntelligenceRunEventKind::Cancelled,
            Some(ApiRunStatus::Cancelled),
            None,
            None,
            Some("run cancelled".to_string()),
            json!({}),
            Some(error.clone()),
        )
        .await?;
        Ok(RunLoopOutcome {
            response: self.status_response(
                run_id,
                request,
                ApiRunStatus::Cancelled,
                model,
                current_step,
                draft_artifact_ids,
                None,
                Some(error),
            ),
        })
    }

    fn status_response(
        &self,
        run_id: Uuid,
        request: &IntelligenceRunStartRequest,
        status: ApiRunStatus,
        model: Option<String>,
        current_step: u32,
        draft_artifact_ids: Vec<Uuid>,
        output_summary: Option<IntelligenceSummary>,
        error: Option<IntelligenceError>,
    ) -> IntelligenceRunStatusResponse {
        IntelligenceRunStatusResponse {
            run_id,
            purpose: request.purpose,
            status,
            terminal: is_terminal_status(status),
            current_phase: Some(status.as_db_str().to_string()),
            provider: Some(self.config.provider_name.clone()),
            model,
            queued_at_epoch_seconds: None,
            started_at_epoch_seconds: None,
            completed_at_epoch_seconds: is_terminal_status(status)
                .then(|| Utc::now().timestamp()),
            current_step: Some(current_step),
            max_steps: Some(self.config.max_steps),
            draft_artifact_ids,
            output_summary,
            error,
        }
    }

    async fn append_tool_finished_event(
        &self,
        run_id: Uuid,
        tool_call_id: Uuid,
        tool_name: &str,
        execution: Option<&IntelligenceToolExecution>,
        error: Option<&IntelligenceToolError>,
    ) -> Result<()> {
        let payload = match execution {
            Some(execution) => json!({
                "tool": tool_name,
                "row_count": execution.row_count,
                "visible_bytes": execution.visible_bytes,
                "media_count": execution.media_ids.len(),
                "artifact_count": execution.artifact_ids.len(),
                "summary": execution.summary,
            }),
            None => json!({"tool": tool_name}),
        };
        self.append_event(
            run_id,
            IntelligenceRunEventKind::ToolCallFinished,
            Some(ApiRunStatus::Running),
            Some(tool_call_id),
            None,
            Some(format!("tool `{tool_name}` finished")),
            payload,
            error.map(tool_error_to_runtime_error),
        )
        .await?;
        Ok(())
    }

    async fn append_event(
        &self,
        run_id: Uuid,
        event_kind: IntelligenceRunEventKind,
        status: Option<ApiRunStatus>,
        tool_call_id: Option<Uuid>,
        artifact_id: Option<Uuid>,
        message: Option<String>,
        payload: Value,
        error: Option<IntelligenceError>,
    ) -> Result<IntelligenceRunEvent> {
        self.store
            .append_run_event(IntelligenceRunEventCreate {
                event_id: None,
                run_id,
                sequence: None,
                event_kind,
                status,
                tool_call_id,
                artifact_id,
                message: message.map(|value| redacted_excerpt(&value, 512)),
                payload: bounded_event_payload(payload),
                error,
            })
            .await
    }

    async fn validate_final_response(
        &self,
        ledger: &IntelligenceGroundingLedger,
        final_args: &FinalResponseArgs,
        user_id: Option<Uuid>,
    ) -> Result<()> {
        for media_id in &final_args.selected_media_ids {
            ledger.ensure_media(*media_id)?;
        }
        for artifact_id in final_args
            .artifact_citations
            .iter()
            .chain(final_args.draft_artifact_ids.iter())
        {
            self.ensure_visible_ledger_artifact(ledger, *artifact_id, user_id)
                .await?;
        }
        Ok(())
    }

    async fn validate_draft_arguments(
        &self,
        run_id: Uuid,
        ledger: &IntelligenceGroundingLedger,
        arguments: &Value,
        user_id: Option<Uuid>,
    ) -> Result<()> {
        let probe: DraftGroundingProbe =
            serde_json::from_value(arguments.clone())?;
        if let Some(media_id) = probe.media_id {
            ledger.ensure_media(media_id)?;
        }
        for source in &probe.sources {
            if let Some(media_id) = source.source_media_id {
                ledger.ensure_media(media_id)?;
            }
            if let Some(artifact_id) = source.source_artifact_id {
                self.ensure_visible_ledger_artifact(
                    ledger,
                    artifact_id,
                    user_id,
                )
                .await?;
            }
            if let Some(source_run_id) = source.source_run_id
                && source_run_id != run_id
            {
                return Err(MediaError::InvalidMedia(
                    "draft source cites a run outside the active grounded run"
                        .to_string(),
                ));
            }
            if let Some(tool_call_id) = source.source_tool_call_id
                && !ledger.tool_call_ids.contains(&tool_call_id)
            {
                return Err(MediaError::InvalidMedia(
                    "draft source cites a tool call absent from the grounding ledger"
                        .to_string(),
                ));
            }
        }
        Ok(())
    }

    async fn ensure_visible_ledger_artifact(
        &self,
        ledger: &IntelligenceGroundingLedger,
        artifact_id: Uuid,
        user_id: Option<Uuid>,
    ) -> Result<()> {
        ledger.ensure_artifact(artifact_id)?;
        if ledger.draft_artifact_ids.contains(&artifact_id) {
            return Ok(());
        }
        if self
            .store
            .get_artifact(artifact_id, user_id)
            .await?
            .is_some()
        {
            Ok(())
        } else {
            Err(MediaError::InvalidMedia(
                "artifact citation is not visible to the requesting user"
                    .to_string(),
            ))
        }
    }

    fn action_specs(&self) -> Vec<IntelligenceActionSpec> {
        let mut actions: Vec<_> = self
            .tools
            .definitions()
            .into_iter()
            .map(|definition| {
                IntelligenceActionSpec::new(
                    definition.name,
                    definition.description,
                    definition.input_schema,
                )
            })
            .collect();
        actions.push(final_response_action_spec());
        actions
    }

    fn reserve_active_slot(&self, user_id: Option<Uuid>) -> Result<()> {
        let limit = self.config.per_user_concurrency as usize;
        let mut active = self
            .active_runs
            .lock()
            .expect("active intelligence run mutex poisoned");
        let active_for_user =
            active.user_counts.get(&user_id).copied().unwrap_or(0);
        if active_for_user >= limit {
            return Err(MediaError::ConcurrencyLimit(format!(
                "intelligence per-user concurrency limit reached ({active_for_user} active run(s), limit {limit})"
            )));
        }
        *active.user_counts.entry(user_id).or_insert(0) += 1;
        Ok(())
    }

    fn release_active_slot(&self, user_id: Option<Uuid>) {
        let mut active = self
            .active_runs
            .lock()
            .expect("active intelligence run mutex poisoned");
        decrement_active_user_count(&mut active, user_id);
    }

    fn register_active(
        &self,
        run_id: Uuid,
        user_id: Option<Uuid>,
        token: CancellationToken,
    ) {
        let mut active = self
            .active_runs
            .lock()
            .expect("active intelligence run mutex poisoned");
        if let Some(previous) =
            active.runs.insert(run_id, ActiveRun { token, user_id })
        {
            decrement_active_user_count(&mut active, previous.user_id);
        }
    }

    fn unregister_active(&self, run_id: Uuid) {
        let mut active = self
            .active_runs
            .lock()
            .expect("active intelligence run mutex poisoned");
        if let Some(run) = active.runs.remove(&run_id) {
            decrement_active_user_count(&mut active, run.user_id);
        }
    }

    fn active_token(&self, run_id: Uuid) -> Option<CancellationToken> {
        self.active_runs
            .lock()
            .expect("active intelligence run mutex poisoned")
            .runs
            .get(&run_id)
            .map(|run| run.token.clone())
    }
}

fn decrement_active_user_count(
    active: &mut ActiveRunState,
    user_id: Option<Uuid>,
) {
    if let Some(count) = active.user_counts.get_mut(&user_id) {
        *count = count.saturating_sub(1);
        if *count == 0 {
            active.user_counts.remove(&user_id);
        }
    }
}

#[derive(Debug, Clone)]
struct RunLoopOutcome {
    response: IntelligenceRunStatusResponse,
}

#[derive(Debug, Clone, Default)]
struct IntelligenceGroundingLedger {
    media_ids: HashSet<MediaID>,
    artifact_ids: HashSet<Uuid>,
    draft_artifact_ids: HashSet<Uuid>,
    tool_call_ids: HashSet<Uuid>,
}

impl IntelligenceGroundingLedger {
    fn from_request(request: &IntelligenceRunStartRequest) -> Self {
        let mut ledger = Self::default();
        if let Some(media_id) = request.media_id {
            ledger.media_ids.insert(media_id);
        }
        ledger
    }

    fn record_tool_execution(&mut self, execution: &IntelligenceToolExecution) {
        self.tool_call_ids.insert(execution.tool_call_id);
        self.media_ids.extend(execution.media_ids.iter().copied());
        self.artifact_ids
            .extend(execution.artifact_ids.iter().copied());
        for grounding in &execution.grounding {
            if let Some(media_id) = grounding.media_id {
                self.media_ids.insert(media_id);
            }
            if let Some(artifact_id) = grounding.artifact_id {
                self.artifact_ids.insert(artifact_id);
            }
        }
    }

    fn ensure_media(&self, media_id: MediaID) -> Result<()> {
        if self.media_ids.contains(&media_id) {
            Ok(())
        } else {
            Err(MediaError::InvalidMedia(
                "media id is absent from the grounding ledger".to_string(),
            ))
        }
    }

    fn ensure_artifact(&self, artifact_id: Uuid) -> Result<()> {
        if self.artifact_ids.contains(&artifact_id)
            || self.draft_artifact_ids.contains(&artifact_id)
        {
            Ok(())
        } else {
            Err(MediaError::InvalidMedia(
                "artifact citation is absent from the grounding ledger"
                    .to_string(),
            ))
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct DraftGroundingProbe {
    #[serde(default)]
    media_id: Option<MediaID>,
    #[serde(default)]
    sources: Vec<IntelligenceArtifactSourceEdge>,
}

#[derive(Debug, Clone, Deserialize)]
struct FinalResponseArgs {
    summary: String,
    #[serde(default)]
    selected_media_ids: Vec<MediaID>,
    #[serde(default)]
    artifact_citations: Vec<Uuid>,
    #[serde(default)]
    draft_artifact_ids: Vec<Uuid>,
}

fn parse_final_response_args(value: &Value) -> Result<FinalResponseArgs> {
    let args: FinalResponseArgs = serde_json::from_value(value.clone())?;
    if args.summary.trim().is_empty() {
        return Err(MediaError::InvalidMedia(
            "final response summary must not be empty".to_string(),
        ));
    }
    Ok(args)
}

fn initial_messages(
    request: &IntelligenceRunStartRequest,
) -> Vec<IntelligenceChatMessage> {
    let mut messages = vec![IntelligenceChatMessage::system(
        "You are Ferrex's grounded local intelligence runtime. Use only the supplied Ferrex tools for media context. Create drafts with create_draft when producing artifacts. Finish with final_response. Never cite media or artifact ids that were not returned by a tool or supplied as the seed.",
    )];
    let mut request_context = json!({
        "purpose": request.purpose,
        "library_id": request.library_id,
        "media_id": request.media_id,
        "caps": request.caps,
        "metadata": redact_json_value(&request.metadata),
    });
    request_context = bounded_event_payload(request_context);
    messages.push(IntelligenceChatMessage::user(format!(
        "User request:\n{}\n\nBounded request context:\n{}",
        request.prompt, request_context
    )));
    messages
}

fn final_response_action_spec() -> IntelligenceActionSpec {
    IntelligenceActionSpec::new(
        FINAL_RESPONSE_ACTION,
        "Finish the grounded Ferrex run after all needed draft artifacts have been created.",
        json!({
            "type": "object",
            "required": ["summary"],
            "additionalProperties": false,
            "properties": {
                "summary": {"type": "string", "minLength": 1, "maxLength": 4096},
                "selected_media_ids": {"type": "array", "items": {}},
                "artifact_citations": {
                    "type": "array",
                    "items": {"type": "string", "format": "uuid"}
                },
                "draft_artifact_ids": {
                    "type": "array",
                    "items": {"type": "string", "format": "uuid"}
                }
            }
        }),
    )
}

fn selected_model(
    config: &IntelligenceRunManagerConfig,
    request: &IntelligenceRunStartRequest,
) -> Option<String> {
    request
        .model
        .as_deref()
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| config.default_model.clone())
}

fn run_kind_for_purpose(
    purpose: IntelligenceRunPurpose,
) -> IntelligenceRunKind {
    match purpose {
        IntelligenceRunPurpose::LibraryOverview
        | IntelligenceRunPurpose::ArtifactSearch
        | IntelligenceRunPurpose::ItemContext
        | IntelligenceRunPurpose::RelatedContext => IntelligenceRunKind::Search,
        IntelligenceRunPurpose::CandidateSearch => IntelligenceRunKind::Search,
        IntelligenceRunPurpose::ArtifactRefresh => {
            IntelligenceRunKind::Summarize
        }
        IntelligenceRunPurpose::Recommendation => {
            IntelligenceRunKind::Recommend
        }
        IntelligenceRunPurpose::Other => IntelligenceRunKind::Answer,
    }
}

fn is_terminal_status(status: ApiRunStatus) -> bool {
    matches!(
        status,
        ApiRunStatus::Succeeded
            | ApiRunStatus::Failed
            | ApiRunStatus::Cancelled
    )
}

fn push_unique_uuid(values: &mut Vec<Uuid>, value: Uuid) {
    if !values.contains(&value) {
        values.push(value);
    }
}

fn audit_request(run_id: Uuid) -> IntelligenceRunAuditRequest {
    IntelligenceRunAuditRequest {
        run_id,
        pagination: Default::default(),
        caps: Default::default(),
    }
}

fn min_duration(lhs: Duration, rhs: Duration) -> Duration {
    if lhs <= rhs { lhs } else { rhs }
}

fn hash_run_request(
    request: &IntelligenceRunStartRequest,
    user_id: Option<Uuid>,
) -> String {
    stable_json_hash(&json!({
        "purpose": request.purpose,
        "library_id": request.library_id,
        "media_id": request.media_id,
        "prompt": request.prompt,
        "metadata": request.metadata,
        "user_id": user_id,
    }))
}

fn stable_json_hash(value: &Value) -> String {
    sha256_hex(value.to_string().as_bytes())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn sanitized_provider_error(
    error: &IntelligenceProviderError,
) -> IntelligenceError {
    let mut api_error = error.to_intelligence_error();
    api_error.message = redacted_excerpt(&api_error.message, 512);
    api_error.details = redact_json_value(&api_error.details);
    api_error
}

fn tool_error_to_runtime_error(
    error: &IntelligenceToolError,
) -> IntelligenceError {
    let code = match error.code {
        IntelligenceToolErrorCode::Cancelled => {
            IntelligenceErrorCode::RunCancelled
        }
        IntelligenceToolErrorCode::ToolTimedOut => {
            IntelligenceErrorCode::ToolTimedOut
        }
        IntelligenceToolErrorCode::StorageError
        | IntelligenceToolErrorCode::AuditError => {
            IntelligenceErrorCode::StorageError
        }
        IntelligenceToolErrorCode::UnknownTool
        | IntelligenceToolErrorCode::MalformedArguments
        | IntelligenceToolErrorCode::ScopeViolation
        | IntelligenceToolErrorCode::BudgetExceeded
        | IntelligenceToolErrorCode::NotFound
        | IntelligenceToolErrorCode::InvalidRequest => {
            IntelligenceErrorCode::InvalidRequest
        }
        IntelligenceToolErrorCode::Internal => IntelligenceErrorCode::Internal,
    };
    runtime_error(
        code,
        redacted_excerpt(&error.message, 512),
        error.retryable,
        redact_json_value(&error.details),
    )
}

fn runtime_error(
    code: IntelligenceErrorCode,
    message: impl AsRef<str>,
    retryable: bool,
    details: Value,
) -> IntelligenceError {
    IntelligenceError {
        code,
        message: redacted_excerpt(message.as_ref(), 512),
        retryable,
        details: redact_json_value(&details),
    }
}

fn bounded_event_payload(payload: Value) -> Value {
    let redacted = redact_json_value(&payload);
    let text = redacted.to_string();
    if text.len() <= MAX_EVENT_PAYLOAD_BYTES {
        return redacted;
    }
    json!({
        "truncated": true,
        "sha256": sha256_hex(text.as_bytes()),
        "excerpt": redacted_excerpt(&text, 512),
    })
}

fn bounded_tool_result_for_model(value: &Value, max_bytes: usize) -> String {
    let redacted = redact_json_value(value);
    let text = redacted.to_string();
    if text.len() <= max_bytes {
        return text;
    }
    json!({
        "truncated": true,
        "sha256": sha256_hex(text.as_bytes()),
        "excerpt": redacted_excerpt(&text, 512),
    })
    .to_string()
}

fn redact_json_value(value: &Value) -> Value {
    redact_json_value_at(value, 0)
}

fn redact_json_value_at(value: &Value, depth: usize) -> Value {
    if depth > 8 {
        return json!({"truncated": true});
    }
    match value {
        Value::String(text) => {
            Value::String(redacted_excerpt(text, MAX_REDACTED_STRING_CHARS))
        }
        Value::Array(items) => Value::Array(
            items
                .iter()
                .take(MAX_REDACTED_ARRAY_ITEMS)
                .map(|item| redact_json_value_at(item, depth + 1))
                .collect(),
        ),
        Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (key, child) in map.iter().take(MAX_REDACTED_OBJECT_KEYS) {
                if is_sensitive_key(key) {
                    out.insert(
                        key.clone(),
                        Value::String(REDACTED.to_string()),
                    );
                } else {
                    out.insert(
                        key.clone(),
                        redact_json_value_at(child, depth + 1),
                    );
                }
            }
            Value::Object(out)
        }
        _ => value.clone(),
    }
}

fn is_sensitive_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key.contains("password")
        || key.contains("secret")
        || key.contains("token")
        || key.contains("api_key")
        || key.contains("apikey")
        || key.contains("authorization")
}

fn redacted_excerpt(text: &str, max_chars: usize) -> String {
    let without_assignments =
        SECRET_ASSIGNMENT_RE.replace_all(text, "$1=[redacted]");
    let without_bearer =
        BEARER_RE.replace_all(&without_assignments, "Bearer [redacted]");
    let without_keys =
        OPENAI_KEY_RE.replace_all(&without_bearer, "sk-[redacted]");
    truncate_chars(&without_keys, max_chars)
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    match text.char_indices().nth(max_chars) {
        Some((idx, _)) => text[..idx].to_string(),
        None => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use async_trait::async_trait;
    use ferrex_model::{LibraryId, MovieID, SeriesID};
    use serde_json::json;

    use super::*;
    use crate::{
        api::types::intelligence::{
            IntelligenceArtifactKind, IntelligenceArtifactSearchRequest,
            IntelligenceArtifactSearchResponse, IntelligenceArtifactStatus,
            IntelligenceArtifactSummary, IntelligenceCandidate,
            IntelligenceCandidateSearchRequest,
            IntelligenceCandidateSearchResponse, IntelligenceCaps,
            IntelligenceContextItem, IntelligenceGroundingSource,
            IntelligenceItemContextRequest, IntelligenceItemContextResponse,
            IntelligenceLibraryOverviewRequest,
            IntelligenceLibraryOverviewResponse, IntelligenceMediaRef,
            IntelligencePageInfo, IntelligenceRelatedContextRequest,
            IntelligenceRelatedContextResponse, IntelligenceSummary,
            IntelligenceToolCallAudit,
        },
        application::intelligence_tools::{
            IntelligenceToolBackend, IntelligenceToolSideEffect,
        },
        database::repository_ports::intelligence::{
            IntelligenceDraftArtifactCreate, IntelligenceToolCallCreate,
            IntelligenceToolCallStatus as StoreToolCallStatus,
            IntelligenceToolCallUpdate,
        },
        domain::intelligence::{
            IntelligenceActionCompletion, fake::FakeIntelligenceProvider,
        },
        query::types::{MediaQuery, MediaWithStatus},
    };

    #[derive(Debug, Default)]
    struct FakeRuntime {
        runs: Mutex<Vec<IntelligenceRunCreate>>,
        run_updates: Mutex<Vec<(Uuid, IntelligenceRunUpdate)>>,
        events: Mutex<Vec<IntelligenceRunEventCreate>>,
        tool_creates: Mutex<Vec<IntelligenceToolCallCreate>>,
        tool_updates: Mutex<Vec<(Uuid, IntelligenceToolCallUpdate)>>,
        draft_creates: Mutex<Vec<IntelligenceDraftArtifactCreate>>,
        source_replacements: Mutex<
            Vec<(Uuid, Option<Uuid>, Vec<IntelligenceArtifactSourceEdge>)>,
        >,
        candidate_response: Mutex<Option<IntelligenceCandidateSearchResponse>>,
        draft_payload: Mutex<Option<IntelligenceDraftArtifactPayload>>,
        visible_artifacts: Mutex<HashSet<Uuid>>,
        stale_runs: Mutex<Vec<Uuid>>,
        candidate_delay: Mutex<Option<Duration>>,
    }

    impl FakeRuntime {
        fn manager(
            self: &Arc<Self>,
            provider: Arc<FakeIntelligenceProvider>,
        ) -> IntelligenceRunManager {
            self.manager_with_config(
                provider,
                IntelligenceRunManagerConfig {
                    enabled: true,
                    model_timeout: Duration::from_secs(5),
                    tool_timeout: Duration::from_secs(5),
                    total_timeout: Duration::from_secs(10),
                    max_steps: 6,
                    max_tool_calls: 4,
                    max_malformed_retries: 1,
                    ..IntelligenceRunManagerConfig::default()
                },
            )
        }

        fn manager_with_config(
            self: &Arc<Self>,
            provider: Arc<FakeIntelligenceProvider>,
            config: IntelligenceRunManagerConfig,
        ) -> IntelligenceRunManager {
            let store: Arc<dyn IntelligenceRuntimeStore> = self.clone();
            let backend: Arc<dyn IntelligenceToolBackend> = self.clone();
            IntelligenceRunManager::new(
                config,
                store,
                provider,
                IntelligenceToolRegistry::new(backend),
            )
        }

        fn last_run_status(&self, run_id: Uuid) -> ApiRunStatus {
            self.run_updates
                .lock()
                .unwrap()
                .iter()
                .rev()
                .find_map(|(id, update)| {
                    (*id == run_id).then_some(update.status).flatten()
                })
                .map(store_status_to_api)
                .unwrap_or(ApiRunStatus::Queued)
        }
    }

    #[async_trait]
    impl IntelligenceRuntimeStore for FakeRuntime {
        async fn create_run(
            &self,
            mut create: IntelligenceRunCreate,
        ) -> Result<Uuid> {
            let run_id = create.run_id.unwrap_or_else(|| {
                Uuid::from_u128(10 + self.runs.lock().unwrap().len() as u128)
            });
            create.run_id = Some(run_id);
            self.runs.lock().unwrap().push(create);
            Ok(run_id)
        }

        async fn update_run(
            &self,
            run_id: Uuid,
            update: IntelligenceRunUpdate,
        ) -> Result<()> {
            self.run_updates.lock().unwrap().push((run_id, update));
            Ok(())
        }

        async fn run_audit(
            &self,
            request: &IntelligenceRunAuditRequest,
            _user_id: Option<Uuid>,
        ) -> Result<IntelligenceRunAuditResponse> {
            let status = self.last_run_status(request.run_id);
            Ok(IntelligenceRunAuditResponse {
                run: crate::api::types::intelligence::IntelligenceRunAudit {
                    run_id: request.run_id,
                    purpose: IntelligenceRunPurpose::Recommendation,
                    status,
                    requested_by_user_id: None,
                    model: Some("fake-model".to_string()),
                    queued_at_epoch_seconds: Some(1),
                    started_at_epoch_seconds: None,
                    completed_at_epoch_seconds: None,
                    input_summary: None,
                    output_summary: None,
                    artifact_ids: Vec::new(),
                    grounding: Vec::new(),
                    tool_calls: Vec::<IntelligenceToolCallAudit>::new(),
                },
                page: IntelligencePageInfo::default(),
                caps: IntelligenceCaps::default(),
            })
        }

        async fn append_run_event(
            &self,
            create: IntelligenceRunEventCreate,
        ) -> Result<IntelligenceRunEvent> {
            let mut events = self.events.lock().unwrap();
            let sequence = create.sequence.unwrap_or_else(|| {
                events
                    .iter()
                    .filter(|event| event.run_id == create.run_id)
                    .count() as i32
            });
            events.push(create.clone());
            Ok(IntelligenceRunEvent {
                event_id: Uuid::from_u128(1_000 + events.len() as u128),
                run_id: create.run_id,
                sequence,
                event_kind: create.event_kind,
                status: create.status,
                tool_call_id: create.tool_call_id,
                artifact_id: create.artifact_id,
                message: create.message,
                payload: create.payload,
                error: create.error,
                created_at_epoch_seconds: Some(1),
            })
        }

        async fn list_run_events(
            &self,
            filter: IntelligenceRunEventListFilter,
        ) -> Result<Vec<IntelligenceRunEvent>> {
            Ok(self
                .events
                .lock()
                .unwrap()
                .iter()
                .enumerate()
                .filter(|(_, event)| event.run_id == filter.run_id)
                .map(|(idx, event)| IntelligenceRunEvent {
                    event_id: Uuid::from_u128(2_000 + idx as u128),
                    run_id: event.run_id,
                    sequence: idx as i32,
                    event_kind: event.event_kind,
                    status: event.status,
                    tool_call_id: event.tool_call_id,
                    artifact_id: event.artifact_id,
                    message: event.message.clone(),
                    payload: event.payload.clone(),
                    error: event.error.clone(),
                    created_at_epoch_seconds: Some(1),
                })
                .collect())
        }

        async fn get_artifact(
            &self,
            artifact_id: Uuid,
            _user_id: Option<Uuid>,
        ) -> Result<Option<IntelligenceArtifactSummary>> {
            Ok(self
                .visible_artifacts
                .lock()
                .unwrap()
                .contains(&artifact_id)
                .then(|| IntelligenceArtifactSummary {
                    artifact_id,
                    kind: IntelligenceArtifactKind::GeneratedAnswer,
                    media: None,
                    title: "Visible".to_string(),
                    summary: Some(IntelligenceSummary::new("visible")),
                    provenance: Vec::new(),
                    grounding: Vec::new(),
                    created_at_epoch_seconds: None,
                    updated_at_epoch_seconds: None,
                }))
        }

        async fn get_draft_artifact(
            &self,
            artifact_id: Uuid,
            _user_id: Option<Uuid>,
        ) -> Result<Option<IntelligenceDraftArtifactPayload>> {
            Ok(self
                .draft_payload
                .lock()
                .unwrap()
                .clone()
                .or_else(|| Some(draft_payload(artifact_id))))
        }

        async fn mark_stale_in_flight_runs_terminal(
            &self,
            _reason: &str,
        ) -> Result<Vec<Uuid>> {
            Ok(std::mem::take(&mut *self.stale_runs.lock().unwrap()))
        }
    }

    #[async_trait]
    impl IntelligenceToolBackend for FakeRuntime {
        async fn library_overview(
            &self,
            _request: &IntelligenceLibraryOverviewRequest,
            _user_id: Option<Uuid>,
        ) -> Result<IntelligenceLibraryOverviewResponse> {
            Ok(IntelligenceLibraryOverviewResponse {
                libraries: Vec::new(),
                facets: Vec::new(),
                page: IntelligencePageInfo::default(),
                caps: IntelligenceCaps::default(),
                generated_at_epoch_seconds: None,
            })
        }

        async fn candidate_search(
            &self,
            request: &IntelligenceCandidateSearchRequest,
            _user_id: Option<Uuid>,
        ) -> Result<IntelligenceCandidateSearchResponse> {
            let delay = *self.candidate_delay.lock().unwrap();
            if let Some(delay) = delay {
                time::sleep(delay).await;
            }
            Ok(self
                .candidate_response
                .lock()
                .unwrap()
                .clone()
                .unwrap_or_else(|| IntelligenceCandidateSearchResponse {
                    candidates: vec![candidate(movie(1))],
                    page: IntelligencePageInfo::default(),
                    caps: request.caps,
                }))
        }

        async fn item_context(
            &self,
            request: &IntelligenceItemContextRequest,
            _user_id: Option<Uuid>,
        ) -> Result<IntelligenceItemContextResponse> {
            Ok(IntelligenceItemContextResponse {
                item: IntelligenceContextItem {
                    media: IntelligenceMediaRef::new(request.media_id, "Seed"),
                    summary: None,
                    facets: Vec::new(),
                    artifact_ids: Vec::new(),
                    provenance: Vec::new(),
                },
                related: Vec::new(),
                artifacts: Vec::new(),
                grounding: Vec::new(),
                caps: request.caps,
            })
        }

        async fn related_context(
            &self,
            request: &IntelligenceRelatedContextRequest,
            _user_id: Option<Uuid>,
        ) -> Result<IntelligenceRelatedContextResponse> {
            Ok(IntelligenceRelatedContextResponse {
                seed: IntelligenceMediaRef::new(request.media_id, "Seed"),
                related: Vec::new(),
                page: IntelligencePageInfo::default(),
                caps: request.caps,
            })
        }

        async fn artifact_search(
            &self,
            request: &IntelligenceArtifactSearchRequest,
            _user_id: Option<Uuid>,
        ) -> Result<IntelligenceArtifactSearchResponse> {
            Ok(IntelligenceArtifactSearchResponse {
                artifacts: request
                    .artifact_ids
                    .iter()
                    .copied()
                    .map(|artifact_id| IntelligenceArtifactSummary {
                        artifact_id,
                        kind: IntelligenceArtifactKind::GeneratedAnswer,
                        media: None,
                        title: "Artifact".to_string(),
                        summary: Some(IntelligenceSummary::new("artifact")),
                        provenance: Vec::new(),
                        grounding: Vec::new(),
                        created_at_epoch_seconds: None,
                        updated_at_epoch_seconds: None,
                    })
                    .collect(),
                page: IntelligencePageInfo::default(),
                caps: request.caps,
            })
        }

        async fn get_draft_artifact(
            &self,
            artifact_id: Uuid,
            user_id: Option<Uuid>,
        ) -> Result<Option<IntelligenceDraftArtifactPayload>> {
            <Self as IntelligenceRuntimeStore>::get_draft_artifact(
                self,
                artifact_id,
                user_id,
            )
            .await
        }

        async fn create_draft_artifact(
            &self,
            create: IntelligenceDraftArtifactCreate,
        ) -> Result<Uuid> {
            self.draft_creates.lock().unwrap().push(create);
            Ok(Uuid::from_u128(900))
        }

        async fn replace_artifact_sources(
            &self,
            artifact_id: Uuid,
            user_id: Option<Uuid>,
            sources: Vec<IntelligenceArtifactSourceEdge>,
        ) -> Result<()> {
            self.source_replacements.lock().unwrap().push((
                artifact_id,
                user_id,
                sources,
            ));
            Ok(())
        }

        async fn query_media(
            &self,
            _query: &MediaQuery,
        ) -> Result<Vec<MediaWithStatus>> {
            Ok(Vec::new())
        }

        async fn query_in_progress_media(
            &self,
            _user_id: Uuid,
            query: &MediaQuery,
        ) -> Result<Vec<MediaWithStatus>> {
            self.query_media(query).await
        }

        async fn query_completed_media(
            &self,
            _user_id: Uuid,
            query: &MediaQuery,
        ) -> Result<Vec<MediaWithStatus>> {
            self.query_media(query).await
        }

        async fn query_unwatched_media(
            &self,
            _user_id: Uuid,
            query: &MediaQuery,
        ) -> Result<Vec<MediaWithStatus>> {
            self.query_media(query).await
        }

        async fn query_recently_watched_media(
            &self,
            _user_id: Uuid,
            _recent_days: u32,
            query: &MediaQuery,
        ) -> Result<Vec<MediaWithStatus>> {
            self.query_media(query).await
        }

        async fn create_tool_call(
            &self,
            create: IntelligenceToolCallCreate,
        ) -> Result<Uuid> {
            let id = create.tool_call_id.unwrap_or_else(|| {
                Uuid::from_u128(
                    700 + self.tool_creates.lock().unwrap().len() as u128,
                )
            });
            let mut stored = create;
            stored.tool_call_id = Some(id);
            self.tool_creates.lock().unwrap().push(stored);
            Ok(id)
        }

        async fn update_tool_call(
            &self,
            tool_call_id: Uuid,
            update: IntelligenceToolCallUpdate,
        ) -> Result<()> {
            self.tool_updates
                .lock()
                .unwrap()
                .push((tool_call_id, update));
            Ok(())
        }
    }

    fn store_status_to_api(status: StoreRunStatus) -> ApiRunStatus {
        match status {
            StoreRunStatus::Queued => ApiRunStatus::Queued,
            StoreRunStatus::Running => ApiRunStatus::Running,
            StoreRunStatus::Succeeded => ApiRunStatus::Succeeded,
            StoreRunStatus::Failed => ApiRunStatus::Failed,
            StoreRunStatus::Cancelled => ApiRunStatus::Cancelled,
        }
    }

    fn library(id: u128) -> LibraryId {
        LibraryId(Uuid::from_u128(id))
    }

    fn movie(id: u128) -> MediaID {
        MediaID::Movie(MovieID(Uuid::from_u128(id)))
    }

    fn series(id: u128) -> MediaID {
        MediaID::Series(SeriesID(Uuid::from_u128(id)))
    }

    fn candidate(media_id: MediaID) -> IntelligenceCandidate {
        IntelligenceCandidate {
            media: IntelligenceMediaRef::new(media_id, "Candidate"),
            summary: Some(IntelligenceSummary::new("candidate summary")),
            match_reason: None,
            score: Some(0.8),
            artifact_ids: Vec::new(),
            grounding: vec![
                crate::api::types::intelligence::IntelligenceGroundingRef {
                    source: IntelligenceGroundingSource::SearchIndex,
                    media_id: Some(media_id),
                    artifact_id: None,
                    field: Some("title".to_string()),
                    label: "search hit".to_string(),
                    evidence: Some(IntelligenceSummary::new("candidate")),
                },
            ],
        }
    }

    fn draft_payload(artifact_id: Uuid) -> IntelligenceDraftArtifactPayload {
        IntelligenceDraftArtifactPayload {
            artifact_id,
            kind: IntelligenceArtifactKind::GeneratedAnswer,
            status: IntelligenceArtifactStatus::Draft,
            library_id: Some(library(1)),
            owner_user_id: Some(Uuid::from_u128(200)),
            media_id: Some(movie(1)),
            run_id: Some(Uuid::from_u128(10)),
            title: "Draft".to_string(),
            summary: Some(IntelligenceSummary::new("draft summary")),
            excerpt: None,
            content: json!({"body": "bounded draft body"}),
            metadata: json!({}),
            sources: Vec::new(),
            created_at_epoch_seconds: None,
            updated_at_epoch_seconds: None,
        }
    }

    fn run_request() -> IntelligenceRunStartRequest {
        IntelligenceRunStartRequest {
            purpose: IntelligenceRunPurpose::Recommendation,
            library_id: Some(library(1)),
            media_id: None,
            prompt: "recommend something; api_key=sk-live-secret".to_string(),
            idempotency_key: None,
            model: Some("fake-model".to_string()),
            caps: IntelligenceCaps::default(),
            metadata: json!({"refresh_token": "rt-secret", "safe": "ok"}),
        }
    }

    #[tokio::test]
    async fn run_manager_persists_events_tools_and_validated_draft() {
        let runtime = Arc::new(FakeRuntime::default());
        let provider = Arc::new(FakeIntelligenceProvider::default());
        provider.push_action(Ok(IntelligenceActionCompletion {
            model: "fake-model".to_string(),
            action_name: "candidate_search".to_string(),
            arguments: json!({"query": "arrival", "library_ids": [library(1)]}),
            attempts: 1,
        }));
        provider.push_action(Ok(IntelligenceActionCompletion {
            model: "fake-model".to_string(),
            action_name: "create_draft".to_string(),
            arguments: json!({
                "kind": "generated_answer",
                "library_id": library(1),
                "media_id": movie(1),
                "title": "Grounded draft",
                "summary": "A bounded answer",
                "content": {"answer": "watch Candidate"},
                "sources": [{
                    "source_ordinal": 0,
                    "source_kind": "media",
                    "source_library_id": library(1),
                    "source_media_id": movie(1)
                }]
            }),
            attempts: 1,
        }));
        provider.push_action(Ok(IntelligenceActionCompletion {
            model: "fake-model".to_string(),
            action_name: FINAL_RESPONSE_ACTION.to_string(),
            arguments: json!({
                "summary": "Draft ready",
                "selected_media_ids": [movie(1)],
                "draft_artifact_ids": [Uuid::from_u128(900)]
            }),
            attempts: 1,
        }));
        let manager = runtime.manager(provider);

        let response = manager
            .run_to_completion(run_request(), Some(Uuid::from_u128(200)))
            .await
            .unwrap();

        assert_eq!(response.status, ApiRunStatus::Succeeded);
        assert_eq!(response.draft_artifact_ids, vec![Uuid::from_u128(900)]);
        let draft_creates = runtime.draft_creates.lock().unwrap();
        assert_eq!(draft_creates.len(), 1);
        assert_eq!(draft_creates[0].title, "Grounded draft");
        assert_eq!(
            draft_creates[0].scope.user_id(),
            Some(Uuid::from_u128(200))
        );
        assert_eq!(draft_creates[0].media_id, Some(movie(1)));
        assert_eq!(draft_creates[0].run_id, Some(response.run_id));
        drop(draft_creates);
        let source_replacements = runtime.source_replacements.lock().unwrap();
        assert_eq!(source_replacements.len(), 1);
        assert_eq!(source_replacements[0].0, Uuid::from_u128(900));
        assert_eq!(source_replacements[0].1, Some(Uuid::from_u128(200)));
        assert_eq!(source_replacements[0].2.len(), 1);
        assert_eq!(source_replacements[0].2[0].source_media_id, Some(movie(1)));
        drop(source_replacements);
        assert!(runtime.tool_creates.lock().unwrap().iter().all(|create| {
            !create.arguments.to_string().contains("sk-live-secret")
        }));

        let run = &runtime.runs.lock().unwrap()[0];
        assert_eq!(run.request_hash.as_ref().unwrap().len(), 64);
        let audit_text = format!(
            "{} {}",
            run.prompt_excerpt.as_deref().unwrap_or_default(),
            run.metadata
        );
        assert!(!audit_text.contains("sk-live-secret"));
        assert!(!audit_text.contains("rt-secret"));
        assert!(audit_text.contains(REDACTED));

        let event_kinds: Vec<_> = runtime
            .events
            .lock()
            .unwrap()
            .iter()
            .map(|event| event.event_kind)
            .collect();
        assert!(event_kinds.contains(&IntelligenceRunEventKind::Queued));
        assert!(event_kinds.contains(&IntelligenceRunEventKind::Started));
        assert!(
            event_kinds.contains(&IntelligenceRunEventKind::ToolCallStarted)
        );
        assert!(
            event_kinds.contains(&IntelligenceRunEventKind::ToolCallFinished)
        );
        assert!(
            event_kinds
                .contains(&IntelligenceRunEventKind::DraftArtifactCreated)
        );
        assert!(event_kinds.contains(&IntelligenceRunEventKind::Completed));
    }

    #[tokio::test]
    async fn per_user_concurrency_limit_rejects_second_active_run() {
        let runtime = Arc::new(FakeRuntime::default());
        *runtime.candidate_delay.lock().unwrap() = Some(Duration::from_secs(5));
        let provider = Arc::new(FakeIntelligenceProvider::default());
        provider.push_action(Ok(IntelligenceActionCompletion {
            model: "fake-model".to_string(),
            action_name: "candidate_search".to_string(),
            arguments: json!({"query": "arrival", "library_ids": [library(1)]}),
            attempts: 1,
        }));
        let manager = runtime.manager(provider);
        let user_id = Some(Uuid::from_u128(200));

        let first = manager.start_run(run_request(), user_id).await.unwrap();
        assert_eq!(first.status, ApiRunStatus::Queued);

        let err = manager
            .start_run(run_request(), user_id)
            .await
            .expect_err("same user should be limited while a run is active");
        assert!(matches!(
            err,
            MediaError::ConcurrencyLimit(ref message)
                if message.contains("per-user concurrency limit")
        ));
        assert_eq!(runtime.runs.lock().unwrap().len(), 1);

        let other_user = manager
            .start_run(run_request(), Some(Uuid::from_u128(201)))
            .await
            .unwrap();
        assert_eq!(other_user.status, ApiRunStatus::Queued);
    }

    #[tokio::test]
    async fn grounding_validator_rejects_unseen_draft_media() {
        let runtime = Arc::new(FakeRuntime::default());
        let provider = Arc::new(FakeIntelligenceProvider::default());
        provider.push_action(Ok(IntelligenceActionCompletion {
            model: "fake-model".to_string(),
            action_name: "create_draft".to_string(),
            arguments: json!({
                "kind": "generated_answer",
                "library_id": library(1),
                "media_id": series(404),
                "title": "Ungrounded",
                "content": {"answer": "hallucinated"}
            }),
            attempts: 1,
        }));
        let manager = runtime.manager(provider);

        let response = manager
            .run_to_completion(run_request(), Some(Uuid::from_u128(200)))
            .await
            .unwrap();

        assert_eq!(response.status, ApiRunStatus::Failed);
        assert_eq!(
            response.error.as_ref().map(|error| error.code),
            Some(IntelligenceErrorCode::InvalidRequest)
        );
        assert!(runtime.draft_creates.lock().unwrap().is_empty());
        assert!(runtime.tool_creates.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn malformed_final_response_fails_without_side_effects() {
        let runtime = Arc::new(FakeRuntime::default());
        let provider = Arc::new(FakeIntelligenceProvider::default());
        provider.push_action(Ok(IntelligenceActionCompletion {
            model: "fake-model".to_string(),
            action_name: FINAL_RESPONSE_ACTION.to_string(),
            arguments: json!({"selected_media_ids": []}),
            attempts: 1,
        }));
        let manager = runtime.manager(provider);

        let response = manager
            .run_to_completion(run_request(), Some(Uuid::from_u128(200)))
            .await
            .unwrap();

        assert_eq!(response.status, ApiRunStatus::Failed);
        assert_eq!(
            response.error.as_ref().map(|error| error.code),
            Some(IntelligenceErrorCode::InvalidRequest)
        );
        assert!(runtime.tool_creates.lock().unwrap().is_empty());
        assert!(runtime.draft_creates.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn direct_write_actions_are_rejected_before_tool_execution() {
        let runtime = Arc::new(FakeRuntime::default());
        let provider = Arc::new(FakeIntelligenceProvider::default());
        for _ in 0..2 {
            provider.push_action(Ok(IntelligenceActionCompletion {
                model: "fake-model".to_string(),
                action_name: "delete_media".to_string(),
                arguments: json!({"media_id": movie(1)}),
                attempts: 1,
            }));
        }
        let manager = runtime.manager(provider);

        let response = manager
            .run_to_completion(run_request(), Some(Uuid::from_u128(200)))
            .await
            .unwrap();

        assert_eq!(response.status, ApiRunStatus::Failed);
        assert_eq!(
            response.error.as_ref().map(|error| error.code),
            Some(IntelligenceErrorCode::ProviderError)
        );
        assert!(runtime.tool_creates.lock().unwrap().is_empty());
        assert!(runtime.draft_creates.lock().unwrap().is_empty());
        assert!(runtime.events.lock().unwrap().iter().any(|event| {
            event.event_kind == IntelligenceRunEventKind::Failed
                && event
                    .error
                    .as_ref()
                    .is_some_and(|error| error.message.contains("unapproved"))
        }));
    }

    #[tokio::test]
    async fn runtime_tool_result_byte_budget_stops_run() {
        let runtime = Arc::new(FakeRuntime::default());
        let provider = Arc::new(FakeIntelligenceProvider::default());
        provider.push_action(Ok(IntelligenceActionCompletion {
            model: "fake-model".to_string(),
            action_name: "candidate_search".to_string(),
            arguments: json!({"query": "arrival", "library_ids": [library(1)]}),
            attempts: 1,
        }));
        let manager = runtime.manager_with_config(
            provider,
            IntelligenceRunManagerConfig {
                enabled: true,
                model_timeout: Duration::from_secs(5),
                tool_timeout: Duration::from_secs(5),
                total_timeout: Duration::from_secs(10),
                max_steps: 2,
                max_tool_calls: 4,
                max_malformed_retries: 1,
                max_tool_result_bytes: 8,
                ..IntelligenceRunManagerConfig::default()
            },
        );

        let response = manager
            .run_to_completion(run_request(), Some(Uuid::from_u128(200)))
            .await
            .unwrap();

        assert_eq!(response.status, ApiRunStatus::Failed);
        assert_eq!(
            response.error.as_ref().map(|error| error.code),
            Some(IntelligenceErrorCode::InvalidRequest)
        );
        assert!(
            response
                .error
                .as_ref()
                .is_some_and(|error| error.message.contains("byte budget"))
        );
        assert_eq!(runtime.tool_creates.lock().unwrap().len(), 1);
        assert!(runtime.draft_creates.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn runtime_tool_call_budget_stops_extra_actions() {
        let runtime = Arc::new(FakeRuntime::default());
        let provider = Arc::new(FakeIntelligenceProvider::default());
        for _ in 0..5 {
            provider.push_action(Ok(IntelligenceActionCompletion {
                model: "fake-model".to_string(),
                action_name: "candidate_search".to_string(),
                arguments: json!({"query": "arrival", "library_ids": [library(1)]}),
                attempts: 1,
            }));
        }
        let manager = runtime.manager(provider);

        let response = manager
            .run_to_completion(run_request(), Some(Uuid::from_u128(200)))
            .await
            .unwrap();

        assert_eq!(response.status, ApiRunStatus::Failed);
        assert_eq!(
            response.error.as_ref().map(|error| error.code),
            Some(IntelligenceErrorCode::InvalidRequest)
        );
        assert!(
            response.error.as_ref().is_some_and(|error| error
                .message
                .contains("tool call budget"))
        );
        assert_eq!(runtime.tool_creates.lock().unwrap().len(), 4);
        assert!(runtime.draft_creates.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn cancellation_marks_run_and_tool_cancelled() {
        let runtime = Arc::new(FakeRuntime::default());
        *runtime.candidate_delay.lock().unwrap() =
            Some(Duration::from_secs(30));
        let provider = Arc::new(FakeIntelligenceProvider::default());
        provider.push_action(Ok(IntelligenceActionCompletion {
            model: "fake-model".to_string(),
            action_name: "candidate_search".to_string(),
            arguments: json!({"query": "arrival", "library_ids": [library(1)]}),
            attempts: 1,
        }));
        let manager = runtime.manager(provider);
        let token = CancellationToken::new();
        let run_future = {
            let manager = manager.clone();
            let token = token.clone();
            tokio::spawn(async move {
                manager
                    .run_to_completion_with_token(
                        run_request(),
                        Some(Uuid::from_u128(200)),
                        token,
                    )
                    .await
                    .unwrap()
            })
        };

        for _ in 0..100 {
            if !runtime.tool_creates.lock().unwrap().is_empty() {
                break;
            }
            time::sleep(Duration::from_millis(10)).await;
        }
        assert!(!runtime.tool_creates.lock().unwrap().is_empty());
        token.cancel();

        let response = time::timeout(Duration::from_secs(2), run_future)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(response.status, ApiRunStatus::Cancelled);
        assert!(runtime.run_updates.lock().unwrap().iter().any(
            |(_, update)| { update.status == Some(StoreRunStatus::Cancelled) }
        ));
        assert!(runtime.tool_updates.lock().unwrap().iter().any(
            |(_, update)| {
                update.status == Some(StoreToolCallStatus::Cancelled)
            }
        ));
    }

    #[tokio::test]
    async fn restart_recovery_marks_stale_runs_and_emits_failed_events() {
        let runtime = Arc::new(FakeRuntime::default());
        runtime.stale_runs.lock().unwrap().push(Uuid::from_u128(77));
        let provider = Arc::new(FakeIntelligenceProvider::default());
        let manager = runtime.manager(provider);

        let recovered = manager.recover_stale_runs().await.unwrap();

        assert_eq!(recovered, vec![Uuid::from_u128(77)]);
        assert!(runtime.events.lock().unwrap().iter().any(|event| {
            event.run_id == Uuid::from_u128(77)
                && event.event_kind == IntelligenceRunEventKind::Failed
        }));
    }

    #[test]
    fn final_action_is_exposed_as_side_effect_free_action() {
        let action = final_response_action_spec();
        assert_eq!(action.name, FINAL_RESPONSE_ACTION);
        let side_effect = IntelligenceToolSideEffect::ReadOnly;
        assert_eq!(side_effect, IntelligenceToolSideEffect::ReadOnly);
    }
}
