//! Provider-neutral intelligence runtime contracts.
//!
//! The types in this module define the server-side boundary used by runtime
//! orchestration code to discover local models and request JSON-constrained chat
//! or action completions without depending on a specific provider's HTTP shape.

use std::{fmt, time::Duration};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use thiserror::Error;
use tokio_util::sync::CancellationToken;

use crate::api::types::intelligence::{
    IntelligenceError, IntelligenceErrorCode, IntelligenceModelStatus,
    IntelligenceProviderStatus,
};

/// Result type used by intelligence model providers.
pub type IntelligenceProviderResult<T> =
    std::result::Result<T, IntelligenceProviderError>;

/// Provider-neutral error taxonomy for model discovery and completion calls.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum IntelligenceProviderError {
    /// Runtime requested a provider that has not been configured.
    #[error("intelligence provider is not configured: {message}")]
    NotConfigured { message: String },
    /// Provider endpoint could not be reached or returned an unavailable state.
    #[error("intelligence provider is unavailable: {message}")]
    Unavailable { message: String },
    /// Provider rejected the configured credentials.
    #[error("intelligence provider rejected credentials: {message}")]
    Unauthorized { message: String },
    /// Provider rate-limited the request.
    #[error("intelligence provider rate limited request: {message}")]
    RateLimited { message: String },
    /// Provider or request deadline was exceeded.
    #[error("intelligence provider request timed out: {message}")]
    Timeout { message: String },
    /// Request was cancelled before a terminal provider response.
    #[error("intelligence provider request cancelled: {message}")]
    Cancelled { message: String },
    /// The requested model is not known or not available.
    #[error("intelligence model is unavailable: {model}")]
    ModelUnavailable { model: String },
    /// Caller supplied an invalid request.
    #[error("invalid intelligence provider request: {message}")]
    InvalidRequest { message: String },
    /// Provider returned a body that could not be parsed as the requested JSON.
    #[error("malformed intelligence provider output: {message}")]
    MalformedOutput { message: String },
    /// Provider returned JSON that did not satisfy the requested schema.
    #[error(
        "intelligence provider output violated schema at {path}: {message}"
    )]
    SchemaViolation { path: String, message: String },
    /// Malformed/schema-violating responses exhausted retry budget.
    #[error(
        "intelligence provider output exhausted retry budget after {attempts} attempts: {last_error}"
    )]
    RetryExhausted { attempts: u32, last_error: String },
    /// Provider explicitly rejected native tools or schema/JSON options.
    #[error("intelligence provider rejected requested options: {message}")]
    ProviderRejectedOptions { message: String },
    /// Provider returned a non-success HTTP status that is otherwise classified.
    #[error("intelligence provider returned status {status}: {message}")]
    ProviderStatus { status: u16, message: String },
    /// Internal integration failure without provider-secret material.
    #[error("intelligence provider internal error: {message}")]
    Internal { message: String },
}

impl IntelligenceProviderError {
    /// Convert this provider error into the stable intelligence API error DTO.
    pub fn to_intelligence_error(&self) -> IntelligenceError {
        IntelligenceError {
            code: self.intelligence_error_code(),
            message: self.to_string(),
            retryable: self.is_retryable(),
            details: self.details(),
        }
    }

    /// Stable error code for transport and runtime handlers.
    pub fn intelligence_error_code(&self) -> IntelligenceErrorCode {
        match self {
            Self::NotConfigured { .. } => {
                IntelligenceErrorCode::ProviderNotConfigured
            }
            Self::Unavailable { .. } => {
                IntelligenceErrorCode::ProviderUnavailable
            }
            Self::Unauthorized { .. } => {
                IntelligenceErrorCode::ProviderUnauthorized
            }
            Self::RateLimited { .. } => {
                IntelligenceErrorCode::ProviderRateLimited
            }
            Self::Timeout { .. } => IntelligenceErrorCode::ProviderTimeout,
            Self::Cancelled { .. } => IntelligenceErrorCode::RunCancelled,
            Self::ModelUnavailable { .. } => {
                IntelligenceErrorCode::ModelUnavailable
            }
            Self::InvalidRequest { .. } => {
                IntelligenceErrorCode::InvalidRequest
            }
            Self::MalformedOutput { .. }
            | Self::SchemaViolation { .. }
            | Self::RetryExhausted { .. }
            | Self::ProviderRejectedOptions { .. }
            | Self::ProviderStatus { .. } => {
                IntelligenceErrorCode::ProviderError
            }
            Self::Internal { .. } => IntelligenceErrorCode::Internal,
        }
    }

    /// Whether retrying the same provider operation may succeed later.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::Unavailable { .. }
                | Self::RateLimited { .. }
                | Self::Timeout { .. }
                | Self::MalformedOutput { .. }
                | Self::SchemaViolation { .. }
                | Self::ProviderStatus {
                    status: 500..=599,
                    ..
                }
        )
    }

    fn details(&self) -> Value {
        match self {
            Self::SchemaViolation { path, .. } => json!({ "path": path }),
            Self::RetryExhausted { attempts, .. } => {
                json!({ "attempts": attempts })
            }
            Self::ProviderStatus { status, .. } => json!({ "status": status }),
            Self::ModelUnavailable { model } => json!({ "model": model }),
            _ => Value::Null,
        }
    }
}

/// Chat message role accepted by provider-neutral completion requests.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IntelligenceChatRole {
    System,
    User,
    Assistant,
}

impl IntelligenceChatRole {
    /// OpenAI-compatible role label for adapters.
    pub const fn as_openai_role(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::User => "user",
            Self::Assistant => "assistant",
        }
    }
}

/// Provider-neutral chat message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntelligenceChatMessage {
    pub role: IntelligenceChatRole,
    pub content: String,
}

impl IntelligenceChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: IntelligenceChatRole::System,
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: IntelligenceChatRole::User,
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: IntelligenceChatRole::Assistant,
            content: content.into(),
        }
    }
}

/// JSON schema requested for a chat completion.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntelligenceJsonSchema {
    /// Provider-safe response/schema name.
    pub name: String,
    /// JSON Schema subset used for validation and provider response_format.
    pub schema: Value,
    /// Ask providers to enforce exact schema semantics when supported.
    #[serde(default = "default_true")]
    pub strict: bool,
}

impl IntelligenceJsonSchema {
    pub fn new(name: impl Into<String>, schema: Value) -> Self {
        Self {
            name: name.into(),
            schema,
            strict: true,
        }
    }

    pub fn validate(&self, value: &Value) -> IntelligenceProviderResult<()> {
        validate_json_schema(value, &self.schema)
    }
}

const fn default_true() -> bool {
    true
}

/// Provider-neutral request for a schema-constrained chat completion.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IntelligenceChatCompletionRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub messages: Vec<IntelligenceChatMessage>,
    pub response_schema: IntelligenceJsonSchema,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
}

impl IntelligenceChatCompletionRequest {
    pub fn new(
        messages: Vec<IntelligenceChatMessage>,
        response_schema: IntelligenceJsonSchema,
    ) -> Self {
        Self {
            model: None,
            messages,
            response_schema,
            temperature: Some(0.0),
        }
    }
}

/// A JSON-constrained chat completion returned by a provider.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntelligenceChatCompletion {
    pub model: String,
    pub content: Value,
    pub attempts: u32,
}

/// Action/function callable by an intelligence runtime.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntelligenceActionSpec {
    pub name: String,
    pub description: String,
    /// JSON Schema for action arguments.
    pub parameters_schema: Value,
}

impl IntelligenceActionSpec {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        parameters_schema: Value,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            parameters_schema,
        }
    }

    pub fn validate_arguments(
        &self,
        arguments: &Value,
    ) -> IntelligenceProviderResult<()> {
        validate_json_schema(arguments, &self.parameters_schema)
    }
}

/// Provider-neutral request for selecting one runtime action and arguments.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IntelligenceActionCompletionRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub messages: Vec<IntelligenceChatMessage>,
    pub actions: Vec<IntelligenceActionSpec>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub force_action: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
}

impl IntelligenceActionCompletionRequest {
    pub fn new(
        messages: Vec<IntelligenceChatMessage>,
        actions: Vec<IntelligenceActionSpec>,
    ) -> Self {
        Self {
            model: None,
            messages,
            actions,
            force_action: None,
            temperature: Some(0.0),
        }
    }
}

/// Provider-neutral action selection returned by a model.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntelligenceActionCompletion {
    pub model: String,
    pub action_name: String,
    pub arguments: Value,
    pub attempts: u32,
}

/// Request-scoped provider controls.
#[derive(Clone, Default)]
pub struct IntelligenceProviderRequestOptions {
    pub timeout: Option<Duration>,
    pub max_retries: Option<u32>,
    pub cancellation_token: Option<CancellationToken>,
}

impl fmt::Debug for IntelligenceProviderRequestOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IntelligenceProviderRequestOptions")
            .field("timeout", &self.timeout)
            .field("max_retries", &self.max_retries)
            .field(
                "cancellation_token",
                &self.cancellation_token.as_ref().map(|_| "<token>"),
            )
            .finish()
    }
}

/// Provider-neutral model/runtime boundary.
#[async_trait]
pub trait IntelligenceModelProvider: Send + Sync {
    /// Discover currently advertised models.
    async fn discover_models(
        &self,
        options: IntelligenceProviderRequestOptions,
    ) -> IntelligenceProviderResult<Vec<IntelligenceModelStatus>>;

    /// Read provider readiness/status without exposing provider-native payloads.
    async fn status(
        &self,
        options: IntelligenceProviderRequestOptions,
    ) -> IntelligenceProviderResult<IntelligenceProviderStatus>;

    /// Produce JSON matching `request.response_schema`.
    async fn complete_chat(
        &self,
        request: IntelligenceChatCompletionRequest,
        options: IntelligenceProviderRequestOptions,
    ) -> IntelligenceProviderResult<IntelligenceChatCompletion>;

    /// Select one action and JSON arguments matching that action's schema.
    async fn complete_action(
        &self,
        request: IntelligenceActionCompletionRequest,
        options: IntelligenceProviderRequestOptions,
    ) -> IntelligenceProviderResult<IntelligenceActionCompletion>;
}

/// Validate a JSON value against the schema subset Ferrex sends to providers.
pub fn validate_json_schema(
    value: &Value,
    schema: &Value,
) -> IntelligenceProviderResult<()> {
    let mut path = String::from("$");
    validate_schema_at(value, schema, &mut path)
}

fn validate_schema_at(
    value: &Value,
    schema: &Value,
    path: &mut String,
) -> IntelligenceProviderResult<()> {
    let Some(schema_obj) = schema.as_object() else {
        return Ok(());
    };

    if let Some(const_value) = schema_obj.get("const")
        && value != const_value
    {
        return schema_violation(
            path,
            format!("expected constant {const_value}"),
        );
    }

    if let Some(enum_values) = schema_obj.get("enum") {
        let values = enum_values.as_array().ok_or_else(|| {
            IntelligenceProviderError::InvalidRequest {
                message: "schema enum must be an array".to_string(),
            }
        })?;
        if !values.iter().any(|candidate| candidate == value) {
            return schema_violation(path, "value is not in enum".to_string());
        }
    }

    if let Some(schema_type) = schema_obj.get("type") {
        validate_type(value, schema_type, path)?;
    } else if schema_obj.contains_key("properties")
        || schema_obj.contains_key("required")
    {
        validate_type(value, &Value::String("object".to_string()), path)?;
    } else if schema_obj.contains_key("items") {
        validate_type(value, &Value::String("array".to_string()), path)?;
    }

    if let Some(properties) = schema_obj.get("properties") {
        let Some(properties) = properties.as_object() else {
            return Err(IntelligenceProviderError::InvalidRequest {
                message: "schema properties must be an object".to_string(),
            });
        };
        validate_object_properties(value, schema_obj, properties, path)?;
    }

    if let Some(items_schema) = schema_obj.get("items") {
        validate_array_items(value, items_schema, path)?;
    }

    validate_length_bounds(value, schema_obj, path)?;
    validate_numeric_bounds(value, schema_obj, path)?;

    Ok(())
}

fn validate_type(
    value: &Value,
    schema_type: &Value,
    path: &str,
) -> IntelligenceProviderResult<()> {
    let valid = match schema_type {
        Value::String(t) => json_type_matches(value, t),
        Value::Array(types) => types
            .iter()
            .filter_map(Value::as_str)
            .any(|t| json_type_matches(value, t)),
        _ => {
            return Err(IntelligenceProviderError::InvalidRequest {
                message: "schema type must be a string or array".to_string(),
            });
        }
    };

    if valid {
        Ok(())
    } else {
        Err(IntelligenceProviderError::SchemaViolation {
            path: path.to_string(),
            message: format!("expected type {schema_type}"),
        })
    }
}

fn json_type_matches(value: &Value, expected: &str) -> bool {
    match expected {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "number" => value.is_number(),
        "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        _ => false,
    }
}

fn validate_object_properties(
    value: &Value,
    schema_obj: &Map<String, Value>,
    properties: &Map<String, Value>,
    path: &mut String,
) -> IntelligenceProviderResult<()> {
    let Some(object) = value.as_object() else {
        return Ok(());
    };

    if let Some(required) = schema_obj.get("required") {
        let Some(required) = required.as_array() else {
            return Err(IntelligenceProviderError::InvalidRequest {
                message: "schema required must be an array".to_string(),
            });
        };
        for key in required.iter().filter_map(Value::as_str) {
            if !object.contains_key(key) {
                push_path(path, key);
                let err = IntelligenceProviderError::SchemaViolation {
                    path: path.clone(),
                    message: "required property is missing".to_string(),
                };
                pop_path(path, key);
                return Err(err);
            }
        }
    }

    for (key, property_schema) in properties {
        if let Some(property_value) = object.get(key) {
            push_path(path, key);
            let result =
                validate_schema_at(property_value, property_schema, path);
            pop_path(path, key);
            result?;
        }
    }

    if let Some(additional) = schema_obj.get("additionalProperties") {
        match additional {
            Value::Bool(false) => {
                for key in object.keys() {
                    if !properties.contains_key(key) {
                        push_path(path, key);
                        let err = IntelligenceProviderError::SchemaViolation {
                            path: path.clone(),
                            message: "additional property is not allowed"
                                .to_string(),
                        };
                        pop_path(path, key);
                        return Err(err);
                    }
                }
            }
            Value::Object(_) => {
                for (key, property_value) in object {
                    if !properties.contains_key(key) {
                        push_path(path, key);
                        let result = validate_schema_at(
                            property_value,
                            additional,
                            path,
                        );
                        pop_path(path, key);
                        result?;
                    }
                }
            }
            _ => {}
        }
    }

    Ok(())
}

fn validate_array_items(
    value: &Value,
    items_schema: &Value,
    path: &mut String,
) -> IntelligenceProviderResult<()> {
    let Some(items) = value.as_array() else {
        return Ok(());
    };
    for (idx, item) in items.iter().enumerate() {
        let suffix = format!("[{idx}]");
        path.push_str(&suffix);
        let result = validate_schema_at(item, items_schema, path);
        for _ in 0..suffix.len() {
            path.pop();
        }
        result?;
    }
    Ok(())
}

fn validate_length_bounds(
    value: &Value,
    schema_obj: &Map<String, Value>,
    path: &str,
) -> IntelligenceProviderResult<()> {
    if let Some(text) = value.as_str() {
        if let Some(min) = schema_obj.get("minLength").and_then(Value::as_u64)
            && text.chars().count() < min as usize
        {
            return Err(IntelligenceProviderError::SchemaViolation {
                path: path.to_string(),
                message: format!("string shorter than minLength {min}"),
            });
        }
        if let Some(max) = schema_obj.get("maxLength").and_then(Value::as_u64)
            && text.chars().count() > max as usize
        {
            return Err(IntelligenceProviderError::SchemaViolation {
                path: path.to_string(),
                message: format!("string longer than maxLength {max}"),
            });
        }
    }

    if let Some(array) = value.as_array() {
        if let Some(min) = schema_obj.get("minItems").and_then(Value::as_u64)
            && array.len() < min as usize
        {
            return Err(IntelligenceProviderError::SchemaViolation {
                path: path.to_string(),
                message: format!("array shorter than minItems {min}"),
            });
        }
        if let Some(max) = schema_obj.get("maxItems").and_then(Value::as_u64)
            && array.len() > max as usize
        {
            return Err(IntelligenceProviderError::SchemaViolation {
                path: path.to_string(),
                message: format!("array longer than maxItems {max}"),
            });
        }
    }

    Ok(())
}

fn validate_numeric_bounds(
    value: &Value,
    schema_obj: &Map<String, Value>,
    path: &str,
) -> IntelligenceProviderResult<()> {
    let Some(number) = value.as_f64() else {
        return Ok(());
    };

    if let Some(minimum) = schema_obj.get("minimum").and_then(Value::as_f64)
        && number < minimum
    {
        return Err(IntelligenceProviderError::SchemaViolation {
            path: path.to_string(),
            message: format!("number lower than minimum {minimum}"),
        });
    }
    if let Some(maximum) = schema_obj.get("maximum").and_then(Value::as_f64)
        && number > maximum
    {
        return Err(IntelligenceProviderError::SchemaViolation {
            path: path.to_string(),
            message: format!("number greater than maximum {maximum}"),
        });
    }

    Ok(())
}

fn push_path(path: &mut String, key: &str) {
    path.push('.');
    path.push_str(key);
}

fn pop_path(path: &mut String, key: &str) {
    for _ in 0..=key.len() {
        path.pop();
    }
}

fn schema_violation<T>(
    path: &str,
    message: String,
) -> IntelligenceProviderResult<T> {
    Err(IntelligenceProviderError::SchemaViolation {
        path: path.to_string(),
        message,
    })
}

#[cfg(any(test, feature = "test-utils"))]
pub mod fake {
    //! Fake model provider for deterministic runtime tests.

    use std::{collections::VecDeque, sync::Mutex};

    use super::*;
    use crate::api::types::intelligence::{
        IntelligenceProviderState, IntelligenceProviderStatus,
    };

    /// Queue-backed provider fake for tests that should not speak HTTP.
    #[derive(Debug)]
    pub struct FakeIntelligenceProvider {
        provider_name: String,
        models: Mutex<
            VecDeque<IntelligenceProviderResult<Vec<IntelligenceModelStatus>>>,
        >,
        chat: Mutex<
            VecDeque<IntelligenceProviderResult<IntelligenceChatCompletion>>,
        >,
        actions: Mutex<
            VecDeque<IntelligenceProviderResult<IntelligenceActionCompletion>>,
        >,
    }

    impl Default for FakeIntelligenceProvider {
        fn default() -> Self {
            Self::new("fake-intelligence")
        }
    }

    impl FakeIntelligenceProvider {
        pub fn new(provider_name: impl Into<String>) -> Self {
            Self {
                provider_name: provider_name.into(),
                models: Mutex::new(VecDeque::new()),
                chat: Mutex::new(VecDeque::new()),
                actions: Mutex::new(VecDeque::new()),
            }
        }

        pub fn push_models(
            &self,
            result: IntelligenceProviderResult<Vec<IntelligenceModelStatus>>,
        ) {
            self.models
                .lock()
                .expect("fake model queue poisoned")
                .push_back(result);
        }

        pub fn push_chat(
            &self,
            result: IntelligenceProviderResult<IntelligenceChatCompletion>,
        ) {
            self.chat
                .lock()
                .expect("fake chat queue poisoned")
                .push_back(result);
        }

        pub fn push_action(
            &self,
            result: IntelligenceProviderResult<IntelligenceActionCompletion>,
        ) {
            self.actions
                .lock()
                .expect("fake action queue poisoned")
                .push_back(result);
        }

        fn default_models(&self) -> Vec<IntelligenceModelStatus> {
            vec![IntelligenceModelStatus {
                name: "fake-model".to_string(),
                selected: true,
                available: true,
                supports_tools: true,
                context_window_tokens: Some(8192),
            }]
        }
    }

    #[async_trait]
    impl IntelligenceModelProvider for FakeIntelligenceProvider {
        async fn discover_models(
            &self,
            _options: IntelligenceProviderRequestOptions,
        ) -> IntelligenceProviderResult<Vec<IntelligenceModelStatus>> {
            let next = self
                .models
                .lock()
                .expect("fake model queue poisoned")
                .pop_front();
            next.unwrap_or_else(|| Ok(self.default_models()))
        }

        async fn status(
            &self,
            options: IntelligenceProviderRequestOptions,
        ) -> IntelligenceProviderResult<IntelligenceProviderStatus> {
            let models = self.discover_models(options).await?;
            Ok(IntelligenceProviderStatus {
                enabled: true,
                provider_name: self.provider_name.clone(),
                base_url: "fake://local".to_string(),
                api_key_configured: false,
                default_model: models.first().map(|model| model.name.clone()),
                state: IntelligenceProviderState::Ready,
                models,
                checked_at_epoch_seconds: None,
                error: None,
            })
        }

        async fn complete_chat(
            &self,
            _request: IntelligenceChatCompletionRequest,
            _options: IntelligenceProviderRequestOptions,
        ) -> IntelligenceProviderResult<IntelligenceChatCompletion> {
            self.chat
                .lock()
                .expect("fake chat queue poisoned")
                .pop_front()
                .unwrap_or_else(|| {
                    Err(IntelligenceProviderError::InvalidRequest {
                        message: "fake chat queue is empty".to_string(),
                    })
                })
        }

        async fn complete_action(
            &self,
            _request: IntelligenceActionCompletionRequest,
            _options: IntelligenceProviderRequestOptions,
        ) -> IntelligenceProviderResult<IntelligenceActionCompletion> {
            self.actions
                .lock()
                .expect("fake action queue poisoned")
                .pop_front()
                .unwrap_or_else(|| {
                    Err(IntelligenceProviderError::InvalidRequest {
                        message: "fake action queue is empty".to_string(),
                    })
                })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_validation_accepts_required_object_subset() {
        let schema = json!({
            "type": "object",
            "required": ["title", "score"],
            "properties": {
                "title": {"type": "string"},
                "score": {"type": "number", "minimum": 0, "maximum": 1}
            },
            "additionalProperties": false
        });
        let value = json!({"title": "Arrival", "score": 0.9});

        validate_json_schema(&value, &schema).expect("schema should validate");
    }

    #[test]
    fn schema_validation_reports_path_for_missing_required_property() {
        let schema = json!({
            "type": "object",
            "required": ["title"],
            "properties": {"title": {"type": "string"}}
        });
        let err = validate_json_schema(&json!({}), &schema)
            .expect_err("schema should fail");

        assert!(matches!(
            err,
            IntelligenceProviderError::SchemaViolation { ref path, .. }
                if path == "$.title"
        ));
    }

    #[tokio::test]
    async fn fake_provider_contract_responses_are_queue_backed() {
        let provider = fake::FakeIntelligenceProvider::new("contract-fake");
        let no_native_tools_model = IntelligenceModelStatus {
            name: "fake-no-tools".to_string(),
            selected: true,
            available: true,
            supports_tools: false,
            context_window_tokens: Some(4096),
        };
        provider.push_models(Ok(vec![no_native_tools_model.clone()]));
        provider.push_chat(Ok(IntelligenceChatCompletion {
            model: "fake-no-tools".to_string(),
            content: json!({"title": "Arrival"}),
            attempts: 1,
        }));
        provider.push_action(Ok(IntelligenceActionCompletion {
            model: "fake-no-tools".to_string(),
            action_name: "final_response".to_string(),
            arguments: json!({"summary": "grounded"}),
            attempts: 1,
        }));

        let status = provider
            .status(IntelligenceProviderRequestOptions::default())
            .await
            .expect("fake status should use queued models");
        assert_eq!(status.provider_name, "contract-fake");
        assert_eq!(status.models, vec![no_native_tools_model]);
        assert!(!status.models[0].supports_tools);

        let completion = provider
            .complete_chat(
                IntelligenceChatCompletionRequest::new(
                    vec![IntelligenceChatMessage::user("name a movie")],
                    IntelligenceJsonSchema::new(
                        "movie_answer",
                        json!({
                            "type": "object",
                            "required": ["title"],
                            "properties": {"title": {"type": "string"}},
                        }),
                    ),
                ),
                IntelligenceProviderRequestOptions::default(),
            )
            .await
            .expect("queued chat response should be returned");
        assert_eq!(completion.content, json!({"title": "Arrival"}));

        let action = provider
            .complete_action(
                IntelligenceActionCompletionRequest::new(
                    vec![IntelligenceChatMessage::user("finish")],
                    vec![IntelligenceActionSpec::new(
                        "final_response",
                        "Finish the run",
                        json!({"type": "object"}),
                    )],
                ),
                IntelligenceProviderRequestOptions::default(),
            )
            .await
            .expect("queued action response should be returned");
        assert_eq!(action.action_name, "final_response");

        let err = provider
            .complete_action(
                IntelligenceActionCompletionRequest::new(
                    vec![IntelligenceChatMessage::user("again")],
                    vec![IntelligenceActionSpec::new(
                        "final_response",
                        "Finish the run",
                        json!({"type": "object"}),
                    )],
                ),
                IntelligenceProviderRequestOptions::default(),
            )
            .await
            .expect_err("empty fake action queue is deterministic");
        assert!(matches!(
            err,
            IntelligenceProviderError::InvalidRequest { .. }
        ));
    }
}
