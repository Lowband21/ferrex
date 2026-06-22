//! OpenAI-compatible local chat-completion provider.

use std::{fmt, sync::Arc, time::Duration};

use async_trait::async_trait;
use chrono::Utc;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use tokio::time;

use crate::{
    api::types::intelligence::{
        IntelligenceModelStatus, IntelligenceProviderState,
        IntelligenceProviderStatus,
    },
    domain::intelligence::{
        IntelligenceActionCompletion, IntelligenceActionCompletionRequest,
        IntelligenceActionSpec, IntelligenceChatCompletion,
        IntelligenceChatCompletionRequest, IntelligenceChatMessage,
        IntelligenceJsonSchema, IntelligenceModelProvider,
        IntelligenceProviderError, IntelligenceProviderRequestOptions,
        IntelligenceProviderResult,
    },
};

const DEFAULT_PROVIDER_NAME: &str = "openai-compatible";
const DEFAULT_BASE_URL: &str = "http://localhost:8081/v1";
const DEFAULT_MODEL: &str = "gemma-4-12b";
const LOCAL_NOOP_API_KEY: &str = "sk-noop";
const CHAT_COMPLETIONS_PATH: &str = "chat/completions";
const MODELS_PATH: &str = "models";

/// Configuration for a local OpenAI-compatible provider.
#[derive(Debug, Clone)]
pub struct OpenAiCompatibleProviderConfig {
    pub provider_name: String,
    pub base_url: String,
    pub api_key: Option<String>,
    pub default_model: Option<String>,
    pub request_timeout: Duration,
    pub max_retries: u32,
    pub max_output_bytes: usize,
    pub prefer_native_tools: bool,
}

impl Default for OpenAiCompatibleProviderConfig {
    fn default() -> Self {
        Self {
            provider_name: DEFAULT_PROVIDER_NAME.to_string(),
            base_url: DEFAULT_BASE_URL.to_string(),
            api_key: None,
            default_model: Some(DEFAULT_MODEL.to_string()),
            request_timeout: Duration::from_secs(60),
            max_retries: 1,
            max_output_bytes: 64 * 1024,
            prefer_native_tools: true,
        }
    }
}

impl OpenAiCompatibleProviderConfig {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            ..Self::default()
        }
    }

    fn bearer_token(&self) -> String {
        self.api_key
            .as_deref()
            .map(str::trim)
            .filter(|key| !key.is_empty())
            .unwrap_or(LOCAL_NOOP_API_KEY)
            .to_string()
    }

    fn api_key_configured(&self) -> bool {
        self.api_key
            .as_deref()
            .map(str::trim)
            .is_some_and(|key| !key.is_empty())
    }
}

/// Provider implementation for llama.cpp and other local OpenAI-compatible
/// `/v1/models` and `/v1/chat/completions` servers.
#[derive(Clone)]
pub struct OpenAiCompatibleProvider {
    config: OpenAiCompatibleProviderConfig,
    transport: Arc<dyn OpenAiTransport>,
}

impl fmt::Debug for OpenAiCompatibleProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenAiCompatibleProvider")
            .field("provider_name", &self.config.provider_name)
            .field("base_url", &self.config.base_url)
            .field("api_key_configured", &self.config.api_key_configured())
            .field("default_model", &self.config.default_model)
            .field("request_timeout", &self.config.request_timeout)
            .field("max_retries", &self.config.max_retries)
            .field("max_output_bytes", &self.config.max_output_bytes)
            .field("prefer_native_tools", &self.config.prefer_native_tools)
            .finish()
    }
}

impl OpenAiCompatibleProvider {
    pub fn new(
        config: OpenAiCompatibleProviderConfig,
    ) -> IntelligenceProviderResult<Self> {
        let transport = ReqwestOpenAiTransport::new(config.request_timeout)?;
        Ok(Self::with_transport(config, Arc::new(transport)))
    }

    fn with_transport(
        config: OpenAiCompatibleProviderConfig,
        transport: Arc<dyn OpenAiTransport>,
    ) -> Self {
        Self { config, transport }
    }

    fn model_for_request(
        &self,
        requested: Option<&str>,
    ) -> IntelligenceProviderResult<String> {
        requested
            .map(str::trim)
            .filter(|model| !model.is_empty())
            .map(ToOwned::to_owned)
            .or_else(|| self.config.default_model.clone())
            .ok_or_else(|| IntelligenceProviderError::NotConfigured {
                message: "FERREX_INTELLIGENCE_MODEL is not configured"
                    .to_string(),
            })
    }

    fn endpoint(&self, path: &str) -> IntelligenceProviderResult<String> {
        let base = self.config.base_url.trim_end_matches('/');
        let path = path.trim_start_matches('/');
        let url = format!("{base}/{path}");
        url::Url::parse(&url)
            .map(|url| url.to_string())
            .map_err(|err| IntelligenceProviderError::InvalidRequest {
                message: format!(
                    "invalid intelligence provider base URL `{}`: {err}",
                    self.config.base_url
                ),
            })
    }

    async fn send(
        &self,
        method: OpenAiHttpMethod,
        path: &str,
        body: Option<Value>,
        options: &IntelligenceProviderRequestOptions,
    ) -> IntelligenceProviderResult<OpenAiHttpResponse> {
        let timeout = options.timeout.unwrap_or(self.config.request_timeout);
        let request = OpenAiHttpRequest {
            method,
            url: self.endpoint(path)?,
            bearer_token: self.config.bearer_token(),
            body,
            timeout,
        };
        let request_future = self.transport.send(request);

        let result = if let Some(token) = options.cancellation_token.clone() {
            tokio::select! {
                _ = token.cancelled() => {
                    return Err(IntelligenceProviderError::Cancelled {
                        message: "provider request was cancelled".to_string(),
                    });
                }
                result = time::timeout(timeout, request_future) => result,
            }
        } else {
            time::timeout(timeout, request_future).await
        };

        result.map_err(|_| IntelligenceProviderError::Timeout {
            message: format!(
                "provider request exceeded {} ms",
                timeout.as_millis()
            ),
        })?
    }

    async fn get_json<T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        options: &IntelligenceProviderRequestOptions,
    ) -> IntelligenceProviderResult<T> {
        let response = self
            .send(OpenAiHttpMethod::Get, path, None, options)
            .await?;
        decode_success_response(response)
    }

    async fn post_chat(
        &self,
        body: Value,
        options: &IntelligenceProviderRequestOptions,
    ) -> IntelligenceProviderResult<OpenAiChatResponse> {
        let response = self
            .send(
                OpenAiHttpMethod::Post,
                CHAT_COMPLETIONS_PATH,
                Some(body),
                options,
            )
            .await?;
        decode_success_response(response)
    }
}

#[async_trait]
impl IntelligenceModelProvider for OpenAiCompatibleProvider {
    async fn discover_models(
        &self,
        options: IntelligenceProviderRequestOptions,
    ) -> IntelligenceProviderResult<Vec<IntelligenceModelStatus>> {
        let response: OpenAiModelsResponse =
            self.get_json(MODELS_PATH, &options).await?;
        let default_model = self.config.default_model.as_deref();
        Ok(response
            .data
            .into_iter()
            .map(|model| model.into_status(default_model))
            .collect())
    }

    async fn status(
        &self,
        options: IntelligenceProviderRequestOptions,
    ) -> IntelligenceProviderResult<IntelligenceProviderStatus> {
        let models = self.discover_models(options).await?;
        let default_model = self.config.default_model.clone();
        let selected_missing = default_model.as_ref().is_some_and(|model| {
            !models.iter().any(|status| status.name == *model)
        });
        let error = selected_missing.then(|| {
            IntelligenceProviderError::ModelUnavailable {
                model: default_model.clone().unwrap_or_default(),
            }
            .to_intelligence_error()
        });
        let state = if models.is_empty() {
            IntelligenceProviderState::Unavailable
        } else if selected_missing {
            IntelligenceProviderState::Degraded
        } else {
            IntelligenceProviderState::Ready
        };

        Ok(IntelligenceProviderStatus {
            enabled: true,
            provider_name: self.config.provider_name.clone(),
            base_url: self.config.base_url.clone(),
            api_key_configured: self.config.api_key_configured(),
            default_model,
            state,
            models,
            checked_at_epoch_seconds: Some(Utc::now().timestamp()),
            error,
        })
    }

    async fn complete_chat(
        &self,
        request: IntelligenceChatCompletionRequest,
        options: IntelligenceProviderRequestOptions,
    ) -> IntelligenceProviderResult<IntelligenceChatCompletion> {
        validate_chat_request(&request)?;
        let model = self.model_for_request(request.model.as_deref())?;
        let max_retries =
            options.max_retries.unwrap_or(self.config.max_retries);
        let mut mode = JsonCompletionMode::JsonSchema;
        let mut messages = request.messages.clone();
        let mut attempts = 0_u32;
        let mut malformed_retries = 0_u32;

        loop {
            attempts += 1;
            let body = chat_completion_body(
                &model,
                &messages,
                request.temperature,
                Some((&request.response_schema, mode)),
                None,
                None,
            );
            match self.post_chat(body, &options).await {
                Ok(response) => {
                    match parse_chat_json_completion(
                        response,
                        &request.response_schema,
                        self.config.max_output_bytes,
                        attempts,
                    ) {
                        Ok(completion) => return Ok(completion),
                        Err(err) if malformed_retries < max_retries => {
                            malformed_retries += 1;
                            messages.push(IntelligenceChatMessage::user(
                                retry_instruction(
                                    &request.response_schema,
                                    &err,
                                ),
                            ));
                        }
                        Err(err) => {
                            return Err(
                                IntelligenceProviderError::RetryExhausted {
                                    attempts,
                                    last_error: err.to_string(),
                                },
                            );
                        }
                    }
                }
                Err(err) if should_fallback_options(&err) => {
                    if let Some(next) = mode.fallback() {
                        mode = next;
                        messages = messages_with_schema_instruction(
                            &request.messages,
                            &request.response_schema,
                        );
                        continue;
                    }
                    return Err(err);
                }
                Err(err) => return Err(err),
            }
        }
    }

    async fn complete_action(
        &self,
        request: IntelligenceActionCompletionRequest,
        options: IntelligenceProviderRequestOptions,
    ) -> IntelligenceProviderResult<IntelligenceActionCompletion> {
        validate_action_request(&request)?;
        let model = self.model_for_request(request.model.as_deref())?;
        let max_retries =
            options.max_retries.unwrap_or(self.config.max_retries);
        let mut mode = if self.config.prefer_native_tools {
            ActionCompletionMode::NativeTools
        } else {
            ActionCompletionMode::JsonSchema
        };
        let action_schema = action_response_schema(&request.actions);
        let mut messages = if matches!(mode, ActionCompletionMode::NativeTools)
        {
            request.messages.clone()
        } else {
            messages_with_action_instruction(&request)
        };
        let mut attempts = 0_u32;
        let mut malformed_retries = 0_u32;

        loop {
            attempts += 1;
            let schema = IntelligenceJsonSchema::new(
                "ferrex_action_completion",
                action_schema.clone(),
            );
            let (response_schema, tools, tool_choice) = match mode {
                ActionCompletionMode::NativeTools => (
                    None,
                    Some(openai_tools(&request.actions)),
                    Some(openai_tool_choice(request.force_action.as_deref())),
                ),
                ActionCompletionMode::JsonSchema => (
                    Some((&schema, JsonCompletionMode::JsonSchema)),
                    None,
                    None,
                ),
                ActionCompletionMode::JsonObject => (
                    Some((&schema, JsonCompletionMode::JsonObject)),
                    None,
                    None,
                ),
                ActionCompletionMode::PromptOnly => (
                    Some((&schema, JsonCompletionMode::PromptOnly)),
                    None,
                    None,
                ),
            };
            let body = chat_completion_body(
                &model,
                &messages,
                request.temperature,
                response_schema,
                tools,
                tool_choice,
            );

            match self.post_chat(body, &options).await {
                Ok(response) => {
                    match parse_action_completion(
                        response,
                        &request,
                        mode,
                        self.config.max_output_bytes,
                        attempts,
                    ) {
                        Ok(completion) => return Ok(completion),
                        Err(err) if malformed_retries < max_retries => {
                            malformed_retries += 1;
                            messages.push(IntelligenceChatMessage::user(
                                action_retry_instruction(&request, &err),
                            ));
                        }
                        Err(err) => {
                            return Err(
                                IntelligenceProviderError::RetryExhausted {
                                    attempts,
                                    last_error: err.to_string(),
                                },
                            );
                        }
                    }
                }
                Err(err) if should_fallback_options(&err) => {
                    if let Some(next) = mode.fallback() {
                        mode = next;
                        messages = messages_with_action_instruction(&request);
                        continue;
                    }
                    return Err(err);
                }
                Err(err) => return Err(err),
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JsonCompletionMode {
    JsonSchema,
    JsonObject,
    PromptOnly,
}

impl JsonCompletionMode {
    const fn fallback(self) -> Option<Self> {
        match self {
            Self::JsonSchema => Some(Self::JsonObject),
            Self::JsonObject => Some(Self::PromptOnly),
            Self::PromptOnly => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActionCompletionMode {
    NativeTools,
    JsonSchema,
    JsonObject,
    PromptOnly,
}

impl ActionCompletionMode {
    const fn fallback(self) -> Option<Self> {
        match self {
            Self::NativeTools => Some(Self::JsonSchema),
            Self::JsonSchema => Some(Self::JsonObject),
            Self::JsonObject => Some(Self::PromptOnly),
            Self::PromptOnly => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OpenAiHttpMethod {
    Get,
    Post,
}

#[derive(Debug, Clone)]
struct OpenAiHttpRequest {
    method: OpenAiHttpMethod,
    url: String,
    bearer_token: String,
    body: Option<Value>,
    timeout: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OpenAiHttpResponse {
    status: u16,
    body: String,
}

#[async_trait]
trait OpenAiTransport: Send + Sync {
    async fn send(
        &self,
        request: OpenAiHttpRequest,
    ) -> IntelligenceProviderResult<OpenAiHttpResponse>;
}

#[derive(Debug, Clone)]
struct ReqwestOpenAiTransport {
    client: reqwest::Client,
}

impl ReqwestOpenAiTransport {
    fn new(timeout: Duration) -> IntelligenceProviderResult<Self> {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|err| IntelligenceProviderError::Internal {
                message: format!("failed to build provider HTTP client: {err}"),
            })?;
        Ok(Self { client })
    }
}

#[async_trait]
impl OpenAiTransport for ReqwestOpenAiTransport {
    async fn send(
        &self,
        request: OpenAiHttpRequest,
    ) -> IntelligenceProviderResult<OpenAiHttpResponse> {
        let builder = match request.method {
            OpenAiHttpMethod::Get => self.client.get(&request.url),
            OpenAiHttpMethod::Post => self.client.post(&request.url),
        }
        .bearer_auth(request.bearer_token)
        .timeout(request.timeout);

        let builder = if let Some(body) = request.body {
            builder.json(&body)
        } else {
            builder
        };

        let response =
            builder.send().await.map_err(reqwest_error_to_provider)?;
        let status = response.status().as_u16();
        let body = response.text().await.map_err(reqwest_error_to_provider)?;

        Ok(OpenAiHttpResponse { status, body })
    }
}

fn reqwest_error_to_provider(err: reqwest::Error) -> IntelligenceProviderError {
    if err.is_timeout() {
        IntelligenceProviderError::Timeout {
            message: "provider request timed out".to_string(),
        }
    } else if err.is_connect() || err.is_request() {
        IntelligenceProviderError::Unavailable {
            message: "provider endpoint could not be reached".to_string(),
        }
    } else {
        IntelligenceProviderError::Internal {
            message: format!("provider HTTP client failed: {err}"),
        }
    }
}

fn decode_success_response<T: for<'de> Deserialize<'de>>(
    response: OpenAiHttpResponse,
) -> IntelligenceProviderResult<T> {
    if !(200..=299).contains(&response.status) {
        return Err(classify_status(response.status, &response.body));
    }

    serde_json::from_str(&response.body).map_err(|err| {
        IntelligenceProviderError::MalformedOutput {
            message: format!("provider response was not valid JSON: {err}"),
        }
    })
}

fn classify_status(status: u16, body: &str) -> IntelligenceProviderError {
    let message = provider_error_message(body);
    match status {
        400 if is_option_rejection(&message) => {
            IntelligenceProviderError::ProviderRejectedOptions { message }
        }
        400 => IntelligenceProviderError::InvalidRequest { message },
        401 | 403 => IntelligenceProviderError::Unauthorized { message },
        404 => IntelligenceProviderError::ProviderStatus { status, message },
        408 | 504 => IntelligenceProviderError::Timeout { message },
        429 => IntelligenceProviderError::RateLimited { message },
        500..=599 => IntelligenceProviderError::Unavailable { message },
        _ => IntelligenceProviderError::ProviderStatus { status, message },
    }
}

fn provider_error_message(body: &str) -> String {
    if body.trim().is_empty() {
        return "provider returned an empty error body".to_string();
    }
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|value| {
            value
                .pointer("/error/message")
                .and_then(Value::as_str)
                .or_else(|| value.get("message").and_then(Value::as_str))
                .map(ToOwned::to_owned)
        })
        .unwrap_or_else(|| body.trim().chars().take(512).collect())
}

fn is_option_rejection(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    let mentions_option = [
        "response_format",
        "json_schema",
        "schema",
        "tools",
        "tool_choice",
        "function",
    ]
    .iter()
    .any(|needle| lower.contains(needle));
    let rejects_option = [
        "unsupported",
        "not support",
        "not implemented",
        "unknown",
        "unrecognized",
        "invalid parameter",
        "extra inputs are not permitted",
    ]
    .iter()
    .any(|needle| lower.contains(needle));

    mentions_option && rejects_option
}

fn should_fallback_options(err: &IntelligenceProviderError) -> bool {
    matches!(
        err,
        IntelligenceProviderError::ProviderRejectedOptions { .. }
    )
}

#[derive(Debug, Deserialize)]
struct OpenAiModelsResponse {
    #[serde(default)]
    data: Vec<OpenAiModel>,
}

#[derive(Debug, Deserialize)]
struct OpenAiModel {
    id: String,
    #[serde(default, flatten)]
    extra: Map<String, Value>,
}

impl OpenAiModel {
    fn into_status(
        self,
        default_model: Option<&str>,
    ) -> IntelligenceModelStatus {
        let supports_tools = self
            .extra
            .get("supports_tools")
            .and_then(Value::as_bool)
            .or_else(|| {
                self.extra
                    .get("capabilities")
                    .and_then(|capabilities| capabilities.get("tools"))
                    .and_then(Value::as_bool)
            })
            .unwrap_or(false);
        let context_window_tokens = [
            "context_window_tokens",
            "context_window",
            "context_length",
            "max_context_length",
            "n_ctx",
        ]
        .iter()
        .find_map(|key| self.extra.get(*key).and_then(Value::as_u64))
        .and_then(|value| u32::try_from(value).ok());
        let selected = default_model.is_some_and(|model| model == self.id);

        IntelligenceModelStatus {
            name: self.id,
            selected,
            available: true,
            supports_tools,
            context_window_tokens,
        }
    }
}

#[derive(Debug, Deserialize)]
struct OpenAiChatResponse {
    model: Option<String>,
    #[serde(default)]
    choices: Vec<OpenAiChoice>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChoice {
    message: OpenAiMessage,
}

#[derive(Debug, Deserialize)]
struct OpenAiMessage {
    #[serde(default)]
    content: Option<Value>,
    #[serde(default)]
    tool_calls: Vec<OpenAiToolCall>,
}

impl OpenAiMessage {
    fn content_text(&self) -> Option<String> {
        match self.content.as_ref()? {
            Value::String(text) => Some(text.clone()),
            Value::Null => None,
            other => Some(other.to_string()),
        }
    }
}

#[derive(Debug, Deserialize)]
struct OpenAiToolCall {
    #[serde(default)]
    function: OpenAiToolFunctionCall,
}

#[derive(Debug, Default, Deserialize)]
struct OpenAiToolFunctionCall {
    #[serde(default)]
    name: String,
    #[serde(default)]
    arguments: String,
}

fn validate_chat_request(
    request: &IntelligenceChatCompletionRequest,
) -> IntelligenceProviderResult<()> {
    if request.messages.is_empty() {
        return Err(IntelligenceProviderError::InvalidRequest {
            message: "chat completion requires at least one message"
                .to_string(),
        });
    }
    if request.response_schema.name.trim().is_empty() {
        return Err(IntelligenceProviderError::InvalidRequest {
            message: "chat completion schema name cannot be empty".to_string(),
        });
    }
    Ok(())
}

fn validate_action_request(
    request: &IntelligenceActionCompletionRequest,
) -> IntelligenceProviderResult<()> {
    if request.messages.is_empty() {
        return Err(IntelligenceProviderError::InvalidRequest {
            message: "action completion requires at least one message"
                .to_string(),
        });
    }
    if request.actions.is_empty() {
        return Err(IntelligenceProviderError::InvalidRequest {
            message: "action completion requires at least one action"
                .to_string(),
        });
    }
    for action in &request.actions {
        if action.name.trim().is_empty() {
            return Err(IntelligenceProviderError::InvalidRequest {
                message: "action names cannot be empty".to_string(),
            });
        }
    }
    if let Some(force_action) = request.force_action.as_deref()
        && !request
            .actions
            .iter()
            .any(|action| action.name == force_action)
    {
        return Err(IntelligenceProviderError::InvalidRequest {
            message: format!("forced action `{force_action}` is not defined"),
        });
    }
    Ok(())
}

fn chat_completion_body(
    model: &str,
    messages: &[IntelligenceChatMessage],
    temperature: Option<f32>,
    response_schema: Option<(&IntelligenceJsonSchema, JsonCompletionMode)>,
    tools: Option<Value>,
    tool_choice: Option<Value>,
) -> Value {
    let mut body = Map::new();
    body.insert("model".to_string(), Value::String(model.to_string()));
    body.insert(
        "messages".to_string(),
        Value::Array(
            messages
                .iter()
                .map(|message| {
                    json!({
                        "role": message.role.as_openai_role(),
                        "content": message.content,
                    })
                })
                .collect(),
        ),
    );
    if let Some(temperature) = temperature {
        body.insert("temperature".to_string(), json!(temperature));
    }
    if let Some((schema, mode)) = response_schema {
        match mode {
            JsonCompletionMode::JsonSchema => {
                body.insert(
                    "response_format".to_string(),
                    json!({
                        "type": "json_schema",
                        "json_schema": {
                            "name": provider_safe_schema_name(&schema.name),
                            "strict": schema.strict,
                            "schema": schema.schema,
                        }
                    }),
                );
            }
            JsonCompletionMode::JsonObject => {
                body.insert(
                    "response_format".to_string(),
                    json!({ "type": "json_object" }),
                );
            }
            JsonCompletionMode::PromptOnly => {}
        }
    }
    if let Some(tools) = tools {
        body.insert("tools".to_string(), tools);
    }
    if let Some(tool_choice) = tool_choice {
        body.insert("tool_choice".to_string(), tool_choice);
    }

    Value::Object(body)
}

fn provider_safe_schema_name(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
                ch
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = sanitized.trim_matches('_');
    if trimmed.is_empty() {
        "ferrex_response".to_string()
    } else {
        trimmed.chars().take(64).collect()
    }
}

fn parse_chat_json_completion(
    response: OpenAiChatResponse,
    schema: &IntelligenceJsonSchema,
    max_output_bytes: usize,
    attempts: u32,
) -> IntelligenceProviderResult<IntelligenceChatCompletion> {
    let model = response
        .model
        .clone()
        .unwrap_or_else(|| "unknown".to_string());
    let content =
        first_message(&response)?.content_text().ok_or_else(|| {
            IntelligenceProviderError::MalformedOutput {
                message: "chat completion did not include message content"
                    .to_string(),
            }
        })?;
    enforce_output_limit(&content, max_output_bytes)?;
    let value = parse_json_text(&content)?;
    schema.validate(&value)?;
    Ok(IntelligenceChatCompletion {
        model,
        content: value,
        attempts,
    })
}

fn parse_action_completion(
    response: OpenAiChatResponse,
    request: &IntelligenceActionCompletionRequest,
    mode: ActionCompletionMode,
    max_output_bytes: usize,
    attempts: u32,
) -> IntelligenceProviderResult<IntelligenceActionCompletion> {
    let model = response
        .model
        .clone()
        .unwrap_or_else(|| "unknown".to_string());
    let message = first_message(&response)?;
    let (action_name, arguments);
    if matches!(mode, ActionCompletionMode::NativeTools)
        && !message.tool_calls.is_empty()
    {
        let function = &message.tool_calls[0].function;
        action_name = function.name.clone();
        arguments = if function.arguments.trim().is_empty() {
            Value::Object(Map::new())
        } else {
            enforce_output_limit(&function.arguments, max_output_bytes)?;
            parse_json_text(&function.arguments)?
        };
    } else {
        let content = message.content_text().ok_or_else(|| {
            IntelligenceProviderError::MalformedOutput {
                message:
                    "action completion did not include content or tool calls"
                        .to_string(),
            }
        })?;
        enforce_output_limit(&content, max_output_bytes)?;
        let value = parse_json_text(&content)?;
        action_name = value
            .get("action")
            .and_then(Value::as_str)
            .ok_or_else(|| IntelligenceProviderError::SchemaViolation {
                path: "$.action".to_string(),
                message: "action completion must include an action string"
                    .to_string(),
            })?
            .to_string();
        arguments = value.get("arguments").cloned().ok_or_else(|| {
            IntelligenceProviderError::SchemaViolation {
                path: "$.arguments".to_string(),
                message: "action completion must include arguments".to_string(),
            }
        })?;
    }

    let spec = validate_selected_action(request, &action_name)?;
    spec.validate_arguments(&arguments)?;
    Ok(IntelligenceActionCompletion {
        model,
        action_name,
        arguments,
        attempts,
    })
}

fn first_message(
    response: &OpenAiChatResponse,
) -> IntelligenceProviderResult<&OpenAiMessage> {
    response
        .choices
        .first()
        .map(|choice| &choice.message)
        .ok_or_else(|| IntelligenceProviderError::MalformedOutput {
            message: "chat completion did not include choices".to_string(),
        })
}

fn validate_selected_action<'a>(
    request: &'a IntelligenceActionCompletionRequest,
    action_name: &str,
) -> IntelligenceProviderResult<&'a IntelligenceActionSpec> {
    if let Some(force_action) = request.force_action.as_deref()
        && force_action != action_name
    {
        return Err(IntelligenceProviderError::SchemaViolation {
            path: "$.action".to_string(),
            message: format!(
                "model selected `{action_name}` but `{force_action}` was required"
            ),
        });
    }

    request
        .actions
        .iter()
        .find(|action| action.name == action_name)
        .ok_or_else(|| IntelligenceProviderError::SchemaViolation {
            path: "$.action".to_string(),
            message: format!("model selected unknown action `{action_name}`"),
        })
}

fn enforce_output_limit(
    text: &str,
    max_output_bytes: usize,
) -> IntelligenceProviderResult<()> {
    if text.len() > max_output_bytes {
        Err(IntelligenceProviderError::MalformedOutput {
            message: format!(
                "provider output exceeded max_output_bytes ({max_output_bytes})"
            ),
        })
    } else {
        Ok(())
    }
}

fn parse_json_text(text: &str) -> IntelligenceProviderResult<Value> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(IntelligenceProviderError::MalformedOutput {
            message: "provider output was empty".to_string(),
        });
    }
    if let Ok(value) = serde_json::from_str(trimmed) {
        return Ok(value);
    }
    if let Some(fenced) = extract_fenced_json(trimmed)
        && let Ok(value) = serde_json::from_str(fenced)
    {
        return Ok(value);
    }
    if let Some(candidate) = extract_json_object_or_array(trimmed)
        && let Ok(value) = serde_json::from_str(candidate)
    {
        return Ok(value);
    }

    Err(IntelligenceProviderError::MalformedOutput {
        message: "provider output was not valid JSON".to_string(),
    })
}

fn extract_fenced_json(text: &str) -> Option<&str> {
    let start = text.find("```")?;
    let after_start = &text[start + 3..];
    let after_language = after_start
        .strip_prefix("json")
        .or_else(|| after_start.strip_prefix("JSON"))
        .unwrap_or(after_start)
        .trim_start_matches(|ch: char| ch.is_whitespace());
    let end = after_language.find("```")?;
    Some(after_language[..end].trim())
}

fn extract_json_object_or_array(text: &str) -> Option<&str> {
    let object_start = text.find('{');
    let array_start = text.find('[');
    let start = match (object_start, array_start) {
        (Some(obj), Some(arr)) => obj.min(arr),
        (Some(obj), None) => obj,
        (None, Some(arr)) => arr,
        (None, None) => return None,
    };
    let end = text.rfind('}').max(text.rfind(']'))?;
    (start <= end).then_some(text[start..=end].trim())
}

fn messages_with_schema_instruction(
    messages: &[IntelligenceChatMessage],
    schema: &IntelligenceJsonSchema,
) -> Vec<IntelligenceChatMessage> {
    let mut out = Vec::with_capacity(messages.len() + 1);
    out.push(IntelligenceChatMessage::system(format!(
        "Return only one JSON object matching this JSON Schema named `{}`. Do not include markdown, prose, or comments. Schema: {}",
        schema.name, schema.schema
    )));
    out.extend_from_slice(messages);
    out
}

fn retry_instruction(
    schema: &IntelligenceJsonSchema,
    err: &IntelligenceProviderError,
) -> String {
    format!(
        "The previous response was invalid ({err}). Return only corrected JSON matching schema `{}`: {}",
        schema.name, schema.schema
    )
}

fn messages_with_action_instruction(
    request: &IntelligenceActionCompletionRequest,
) -> Vec<IntelligenceChatMessage> {
    let action_docs: Vec<Value> = request
        .actions
        .iter()
        .map(|action| {
            json!({
                "name": action.name,
                "description": action.description,
                "parameters_schema": action.parameters_schema,
            })
        })
        .collect();
    let force = request
        .force_action
        .as_deref()
        .map(|name| format!(" You must choose `{name}`."))
        .unwrap_or_default();
    let mut out = Vec::with_capacity(request.messages.len() + 1);
    out.push(IntelligenceChatMessage::system(format!(
        "Select exactly one action and return only JSON in the form {{\"action\":\"<name>\",\"arguments\":{{...}}}}. Available actions: {}.{} Do not include markdown, prose, or comments.",
        Value::Array(action_docs),
        force
    )));
    out.extend_from_slice(&request.messages);
    out
}

fn action_retry_instruction(
    request: &IntelligenceActionCompletionRequest,
    err: &IntelligenceProviderError,
) -> String {
    let action_names: Vec<&str> = request
        .actions
        .iter()
        .map(|action| action.name.as_str())
        .collect();
    format!(
        "The previous action response was invalid ({err}). Return only corrected JSON with action in {:?} and arguments matching that action schema.",
        action_names
    )
}

fn action_response_schema(actions: &[IntelligenceActionSpec]) -> Value {
    let names: Vec<Value> = actions
        .iter()
        .map(|action| Value::String(action.name.clone()))
        .collect();
    json!({
        "type": "object",
        "required": ["action", "arguments"],
        "properties": {
            "action": {"type": "string", "enum": names},
            "arguments": {"type": "object"}
        },
        "additionalProperties": false
    })
}

fn openai_tools(actions: &[IntelligenceActionSpec]) -> Value {
    Value::Array(
        actions
            .iter()
            .map(|action| {
                json!({
                    "type": "function",
                    "function": {
                        "name": action.name,
                        "description": action.description,
                        "parameters": action.parameters_schema,
                    }
                })
            })
            .collect(),
    )
}

fn openai_tool_choice(force_action: Option<&str>) -> Value {
    force_action
        .map(|name| {
            json!({
                "type": "function",
                "function": { "name": name }
            })
        })
        .unwrap_or_else(|| Value::String("auto".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::VecDeque, sync::Mutex};

    use tokio_util::sync::CancellationToken;

    #[derive(Debug)]
    struct FakeTransport {
        responses: Mutex<VecDeque<FakeResponse>>,
        requests: Mutex<Vec<OpenAiHttpRequest>>,
    }

    impl FakeTransport {
        fn new(responses: Vec<FakeResponse>) -> Arc<Self> {
            Arc::new(Self {
                responses: Mutex::new(responses.into()),
                requests: Mutex::new(Vec::new()),
            })
        }

        fn requests(&self) -> Vec<OpenAiHttpRequest> {
            self.requests.lock().expect("request log poisoned").clone()
        }
    }

    #[derive(Debug)]
    enum FakeResponse {
        Response { status: u16, body: Value },
        Raw { status: u16, body: String },
        Error(IntelligenceProviderError),
        Pending,
    }

    #[async_trait]
    impl OpenAiTransport for FakeTransport {
        async fn send(
            &self,
            request: OpenAiHttpRequest,
        ) -> IntelligenceProviderResult<OpenAiHttpResponse> {
            self.requests
                .lock()
                .expect("request log poisoned")
                .push(request);
            let response = self
                .responses
                .lock()
                .expect("response queue poisoned")
                .pop_front()
                .expect("fake response missing");
            match response {
                FakeResponse::Response { status, body } => {
                    Ok(OpenAiHttpResponse {
                        status,
                        body: body.to_string(),
                    })
                }
                FakeResponse::Raw { status, body } => {
                    Ok(OpenAiHttpResponse { status, body })
                }
                FakeResponse::Error(err) => Err(err),
                FakeResponse::Pending => std::future::pending().await,
            }
        }
    }

    fn provider_with(
        responses: Vec<FakeResponse>,
    ) -> (OpenAiCompatibleProvider, Arc<FakeTransport>) {
        let transport = FakeTransport::new(responses);
        let config = OpenAiCompatibleProviderConfig {
            request_timeout: Duration::from_millis(250),
            ..OpenAiCompatibleProviderConfig::default()
        };
        (
            OpenAiCompatibleProvider::with_transport(config, transport.clone()),
            transport,
        )
    }

    fn test_schema() -> IntelligenceJsonSchema {
        IntelligenceJsonSchema::new(
            "movie_answer",
            json!({
                "type": "object",
                "required": ["title"],
                "properties": {"title": {"type": "string"}},
                "additionalProperties": false
            }),
        )
    }

    fn chat_request() -> IntelligenceChatCompletionRequest {
        IntelligenceChatCompletionRequest::new(
            vec![IntelligenceChatMessage::user("name a movie")],
            test_schema(),
        )
    }

    fn chat_body(content: &str) -> Value {
        json!({
            "model": "gemma-4-12b",
            "choices": [{"message": {"content": content}}]
        })
    }

    #[tokio::test]
    async fn discovers_models_with_local_noop_api_key() {
        let (provider, transport) = provider_with(vec![
            FakeResponse::Response {
                status: 200,
                body: json!({"data": [{"id": "gemma-4-12b", "context_length": 4096, "supports_tools": true}]}),
            },
        ]);

        let models = provider
            .discover_models(IntelligenceProviderRequestOptions::default())
            .await
            .expect("model discovery succeeds");

        assert_eq!(models[0].name, "gemma-4-12b");
        assert!(models[0].selected);
        assert!(models[0].supports_tools);
        assert_eq!(models[0].context_window_tokens, Some(4096));
        let requests = transport.requests();
        assert_eq!(requests[0].url, "http://localhost:8081/v1/models");
        assert_eq!(requests[0].bearer_token, LOCAL_NOOP_API_KEY);
    }

    #[tokio::test]
    async fn provider_unavailable_is_typed() {
        let (provider, _) = provider_with(vec![FakeResponse::Error(
            IntelligenceProviderError::Unavailable {
                message: "connection refused".to_string(),
            },
        )]);

        let err = provider
            .discover_models(IntelligenceProviderRequestOptions::default())
            .await
            .expect_err("provider should be unreachable");

        assert!(matches!(err, IntelligenceProviderError::Unavailable { .. }));
        assert_eq!(
            err.to_intelligence_error().code,
            crate::api::types::intelligence::IntelligenceErrorCode::ProviderUnavailable
        );
    }

    #[tokio::test]
    async fn chat_completion_retries_malformed_json_then_succeeds() {
        let (provider, transport) = provider_with(vec![
            FakeResponse::Response {
                status: 200,
                body: chat_body("not json"),
            },
            FakeResponse::Response {
                status: 200,
                body: chat_body(r#"{"title":"Arrival"}"#),
            },
        ]);

        let completion = provider
            .complete_chat(
                chat_request(),
                IntelligenceProviderRequestOptions::default(),
            )
            .await
            .expect("retry should recover");

        assert_eq!(completion.content, json!({"title": "Arrival"}));
        assert_eq!(completion.attempts, 2);
        assert_eq!(transport.requests().len(), 2);
    }

    #[tokio::test]
    async fn chat_completion_reports_schema_retry_exhaustion() {
        let (provider, _) = provider_with(vec![
            FakeResponse::Response {
                status: 200,
                body: chat_body(r#"{}"#),
            },
            FakeResponse::Response {
                status: 200,
                body: chat_body(r#"{}"#),
            },
        ]);

        let err = provider
            .complete_chat(
                chat_request(),
                IntelligenceProviderRequestOptions::default(),
            )
            .await
            .expect_err("invalid output should exhaust retries");

        assert!(matches!(
            err,
            IntelligenceProviderError::RetryExhausted { attempts: 2, .. }
        ));
    }

    #[tokio::test]
    async fn chat_completion_falls_back_when_json_schema_rejected() {
        let (provider, transport) = provider_with(vec![
            FakeResponse::Raw {
                status: 400,
                body: json!({"error": {"message": "response_format json_schema is unsupported"}}).to_string(),
            },
            FakeResponse::Response {
                status: 200,
                body: chat_body(r#"{"title":"Moon"}"#),
            },
        ]);

        let completion = provider
            .complete_chat(
                chat_request(),
                IntelligenceProviderRequestOptions::default(),
            )
            .await
            .expect("json_object fallback should recover");

        assert_eq!(completion.content, json!({"title": "Moon"}));
        let requests = transport.requests();
        assert_eq!(
            requests[0]
                .body
                .as_ref()
                .and_then(|body| body.pointer("/response_format/type"))
                .and_then(Value::as_str),
            Some("json_schema")
        );
        assert_eq!(
            requests[1]
                .body
                .as_ref()
                .and_then(|body| body.pointer("/response_format/type"))
                .and_then(Value::as_str),
            Some("json_object")
        );
    }

    #[tokio::test]
    async fn action_completion_falls_back_when_native_tools_rejected() {
        let action = IntelligenceActionSpec::new(
            "recommend",
            "Recommend a title",
            json!({
                "type": "object",
                "required": ["title"],
                "properties": {"title": {"type": "string"}},
                "additionalProperties": false
            }),
        );
        let request = IntelligenceActionCompletionRequest::new(
            vec![IntelligenceChatMessage::user("recommend one")],
            vec![action],
        );
        let (provider, transport) = provider_with(vec![
            FakeResponse::Raw {
                status: 400,
                body: json!({"error": {"message": "tools are not supported by this server"}}).to_string(),
            },
            FakeResponse::Response {
                status: 200,
                body: chat_body(r#"{"action":"recommend","arguments":{"title":"Arrival"}}"#),
            },
        ]);

        let completion = provider
            .complete_action(
                request,
                IntelligenceProviderRequestOptions::default(),
            )
            .await
            .expect("tool fallback should recover");

        assert_eq!(completion.action_name, "recommend");
        assert_eq!(completion.arguments, json!({"title": "Arrival"}));
        let requests = transport.requests();
        assert!(
            requests[0]
                .body
                .as_ref()
                .and_then(|body| body.get("tools"))
                .is_some()
        );
        assert!(
            requests[1]
                .body
                .as_ref()
                .and_then(|body| body.get("tools"))
                .is_none()
        );
    }

    #[tokio::test]
    async fn cancellation_stops_pending_provider_request() {
        let (provider, _) = provider_with(vec![FakeResponse::Pending]);
        let token = CancellationToken::new();
        let mut options = IntelligenceProviderRequestOptions::default();
        options.timeout = Some(Duration::from_secs(5));
        options.cancellation_token = Some(token.clone());

        let task = tokio::spawn(async move {
            provider.complete_chat(chat_request(), options).await
        });
        tokio::time::sleep(Duration::from_millis(10)).await;
        token.cancel();

        let err = task
            .await
            .expect("task should not panic")
            .expect_err("request should be cancelled");
        assert!(matches!(err, IntelligenceProviderError::Cancelled { .. }));
    }

    #[tokio::test]
    async fn timeout_stops_pending_provider_request() {
        let (provider, _) = provider_with(vec![FakeResponse::Pending]);
        let options = IntelligenceProviderRequestOptions {
            timeout: Some(Duration::from_millis(10)),
            ..IntelligenceProviderRequestOptions::default()
        };

        let err = provider
            .complete_chat(chat_request(), options)
            .await
            .expect_err("request should time out");

        assert!(matches!(err, IntelligenceProviderError::Timeout { .. }));
    }

    #[test]
    fn parses_fenced_json_output() {
        let parsed = parse_json_text("```json\n{\"title\":\"Arrival\"}\n```")
            .expect("fenced JSON should parse");
        assert_eq!(parsed, json!({"title": "Arrival"}));
    }

    #[test]
    fn validates_prompt_action_arguments_against_selected_schema() {
        let action = IntelligenceActionSpec::new(
            "search",
            "Search library",
            json!({
                "type": "object",
                "required": ["query"],
                "properties": {"query": {"type": "string"}}
            }),
        );
        let request = IntelligenceActionCompletionRequest::new(
            vec![IntelligenceChatMessage::user("find sci fi")],
            vec![action],
        );
        let response = OpenAiChatResponse {
            model: Some("fake".to_string()),
            choices: vec![OpenAiChoice {
                message: OpenAiMessage {
                    content: Some(json!(
                        r#"{"action":"search","arguments":{}}"#
                    )),
                    tool_calls: Vec::new(),
                },
            }],
        };

        let err = parse_action_completion(
            response,
            &request,
            ActionCompletionMode::JsonSchema,
            4096,
            1,
        )
        .expect_err("missing query should violate action schema");

        assert!(matches!(
            err,
            IntelligenceProviderError::SchemaViolation { ref path, .. }
                if path == "$.query"
        ));
    }

    #[test]
    fn validates_response_schema_before_building_response_format() {
        assert_eq!(provider_safe_schema_name("movie answer!"), "movie_answer");
        assert_eq!(provider_safe_schema_name("!!!"), "ferrex_response");
    }
}
