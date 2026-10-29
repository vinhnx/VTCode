//! Ollama provider implementation
//!
//! Adapted from [openai/codex] `codex-ollama` (Apache-2.0).
//! Copyright 2025 OpenAI. See the repository `THIRD-PARTY-NOTICES` file for
//! full attribution.
//!
//! [openai/codex]: https://github.com/openai/codex

use crate::client::LLMClient;
use crate::provider::{
    ContentPart, FinishReason, LLMError, LLMProvider, LLMRequest, LLMResponse, LLMStream, LLMStreamEvent, Message,
    MessageContent, MessageRole, ToolCall, ToolChoice, ToolDefinition, Usage,
};
use anyhow::Result;
use async_stream::try_stream;
use async_trait::async_trait;
use futures::StreamExt;
use hashbrown::HashMap;
use reqwest::Client as HttpClient;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use vtcode_config::TimeoutsConfig;
use vtcode_config::constants::{env_vars, models, urls};
use vtcode_config::core::{AnthropicConfig, ModelConfig, PromptCachingConfig};

pub(crate) mod client;
pub(crate) mod parser;
pub mod pull;
pub(crate) mod url;

pub(crate) use client::OllamaClient;
pub(crate) use parser::pull_events_from_value;
pub(crate) use pull::{CliPullProgressReporter, OllamaPullEvent, OllamaPullProgressReporter, TuiPullProgressReporter};
pub(crate) use url::{base_url_to_host_root, is_openai_compatible_base_url};

use super::common::{
    assistant_interleaved_history_text, collect_history_system_directives, extract_reasoning_text_from_detail_values,
    extract_reasoning_text_from_serialized_details, is_minimax_m2_model, merge_system_prompt_with_history_directives,
    override_base_url, parse_client_prompt_common, resolve_model, serialize_reasoning_detail_values,
};
use super::error_handling::{format_network_error, format_parse_error};
use super::local_readiness::{invalidate_readiness_cache, resolve_local_model};
use super::local_server::LocalProvider;

/// Prepare the local OSS environment when using Ollama.
///
/// - Ensures a local Ollama server is reachable.
/// - Checks if the model exists locally and pulls it if missing.
///
/// Adapted from OpenAI Codex's codex-ollama/src/lib.rs
async fn ensure_oss_ready(model: Option<&str>, base_url: Option<String>) -> std::io::Result<()> {
    let target_model = model.unwrap_or(models::ollama::DEFAULT_MODEL);

    let resolved_base_url = override_base_url(urls::OLLAMA_API_BASE, base_url, Some(env_vars::OLLAMA_BASE_URL));

    // Verify local Ollama is reachable
    let ollama_client = OllamaClient::try_from_base_url(&resolved_base_url).await?;

    // If the model is not present locally, pull it
    match ollama_client.fetch_models().await {
        Ok(existing_models) => {
            if !existing_models.iter().any(|m| m == target_model) {
                tracing::info!("Model '{target_model}' not found locally, pulling...");
                let mut reporter = CliPullProgressReporter::new();
                ollama_client.pull_with_reporter(target_model, &mut reporter).await?;
            }
        }
        Err(e) => {
            tracing::warn!("Failed to list Ollama models: {e}");
            // Continue anyway; model might exist but listing failed
        }
    }

    Ok(())
}

#[derive(Debug, Deserialize, Serialize)]
struct OllamaTagsResponse {
    models: Vec<OllamaTag>,
}

#[derive(Debug, Deserialize, Serialize)]
struct OllamaTag {
    name: Option<String>,
    model: Option<String>,
    modified_at: Option<String>,
    size: Option<u64>,
    digest: Option<String>,
    details: Option<OllamaModelDetails>,
}

#[derive(Debug, Deserialize, Serialize)]
struct OllamaModelDetails {
    format: Option<String>,
    family: Option<String>,
    families: Option<Vec<String>>,
    parameter_size: Option<String>,
    quantization_level: Option<String>,
}

pub(super) fn ollama_model_name_from_fields<'a>(name: Option<&'a str>, model: Option<&'a str>) -> Option<&'a str> {
    name.or(model).map(str::trim).filter(|value| !value.is_empty())
}

pub(super) const OLLAMA_CONNECTION_ERROR: &str = "No running Ollama server detected. Start it with: `ollama serve` (after installing)\n\
     Install instructions: https://github.com/ollama/ollama?tab=readme-ov-file";

/// Fetches available local Ollama models from the Ollama API endpoint
pub async fn fetch_ollama_models(base_url: Option<String>) -> Result<Vec<String>, anyhow::Error> {
    use vtcode_config::constants::{env_vars, urls};

    let resolved_base_url = override_base_url(urls::OLLAMA_API_BASE, base_url, Some(env_vars::OLLAMA_BASE_URL));

    // Construct the tags endpoint URL
    let tags_url = format!("{resolved_base_url}/api/tags");

    // Create HTTP client with connection timeout
    let client = vtcode_commons::http::create_client_with_timeout(std::time::Duration::from_secs(5));

    // Make GET request to fetch models
    let response = client
        .get(&tags_url)
        .header("Content-Type", "application/json")
        .send()
        .await
        .map_err(|e| {
            // Connection refused is the expected state when Ollama isn't
            // running — log at debug so startup stays quiet for users without
            // local servers. The error is still propagated to the caller.
            tracing::debug!("Failed to connect to Ollama server: {e:?}");
            anyhow::anyhow!(OLLAMA_CONNECTION_ERROR)
        })?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "Failed to fetch Ollama models: HTTP {}. {}",
            response.status(),
            if response.status() == reqwest::StatusCode::NOT_FOUND {
                "Ensure Ollama server is running."
            } else {
                ""
            }
        ));
    }

    // Parse the response
    let tags_response: OllamaTagsResponse = response
        .json()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to parse Ollama models response: {e}"))?;

    // Extract model names
    let model_names: Vec<String> = tags_response
        .models
        .into_iter()
        .filter_map(|model| {
            ollama_model_name_from_fields(model.name.as_deref(), model.model.as_deref()).map(str::to_string)
        })
        .collect();

    Ok(model_names)
}
mod payload;

pub struct OllamaProvider {
    http_client: HttpClient,
    base_url: String,
    model: String,
    api_key: Option<String>,
    model_behavior: Option<ModelConfig>,
}

impl OllamaProvider {
    fn merged_system_prompt(request: &LLMRequest) -> Option<String> {
        const HISTORY_DIRECTIVES_SECTION_HEADER: &str = "[History Directives]";
        let directives = collect_history_system_directives(request);
        merge_system_prompt_with_history_directives(
            request.system_prompt.as_ref().map(|prompt| prompt.as_ref()),
            &directives,
            HISTORY_DIRECTIVES_SECTION_HEADER,
        )
    }

    pub fn new(api_key: String) -> Self {
        Self::with_model(api_key, models::ollama::DEFAULT_MODEL.to_string())
    }

    fn with_model(api_key: String, model: String) -> Self {
        Self::with_model_internal(model, None, Some(api_key), None)
    }

    pub fn new_with_client(
        api_key: String,
        model: String,
        http_client: reqwest::Client,
        base_url: String,
        _timeouts: TimeoutsConfig,
    ) -> Self {
        Self {
            http_client,
            base_url,
            model,
            api_key: Some(api_key),
            model_behavior: None,
        }
    }

    pub fn from_config(
        api_key: Option<String>,
        model: Option<String>,
        base_url: Option<String>,
        _prompt_cache: Option<PromptCachingConfig>,
        _timeouts: Option<TimeoutsConfig>,
        _anthropic: Option<AnthropicConfig>,
        model_behavior: Option<ModelConfig>,
    ) -> Self {
        let resolved_model = resolve_model(model, models::ollama::DEFAULT_MODEL);
        Self::with_model_internal(resolved_model, base_url, api_key, model_behavior)
    }

    fn normalize_api_key(api_key: Option<String>) -> Option<String> {
        api_key.and_then(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        })
    }

    fn with_model_internal(
        model: String,
        base_url: Option<String>,
        api_key: Option<String>,
        model_behavior: Option<ModelConfig>,
    ) -> Self {
        let normalized_api_key = Self::normalize_api_key(api_key);
        let is_cloud_model = model.contains(":cloud") || model.contains("-cloud");

        let default_base = if is_cloud_model {
            urls::OLLAMA_CLOUD_API_BASE
        } else {
            urls::OLLAMA_API_BASE
        };

        let resolved_base = override_base_url(default_base, base_url, Some(env_vars::OLLAMA_BASE_URL));
        let target_is_local = super::local_server::is_local_base_url(&resolved_base);

        // Never send API keys to local endpoints; keep keys for cloud/remote targets
        let effective_api_key = if target_is_local { None } else { normalized_api_key };

        Self {
            http_client: vtcode_commons::http::create_default_client(),
            base_url: resolved_base,
            model,
            api_key: effective_api_key,
            model_behavior,
        }
    }

    fn chat_url(&self) -> String {
        format!("{}/api/chat", self.base_url.trim_end_matches('/'))
    }

    fn authorized_post(&self, url: String) -> reqwest::RequestBuilder {
        let builder = self.http_client.post(url);
        if let Some(api_key) = &self.api_key {
            builder.bearer_auth(api_key)
        } else {
            builder
        }
    }

    fn parse_client_prompt(&self, prompt: &str) -> LLMRequest {
        parse_client_prompt_common(prompt, &self.model, |value| self.parse_chat_request(value))
    }

    /// Verify the server is up and the model is available before generating.
    /// Returns the (possibly substituted) model id or a structured error with a
    /// recovery command (`ollama pull <model>` / `/local start ollama`).
    ///
    /// `VTCODE_LOCAL_AUTOPULL` is deliberately strict (`1`/`true`, no
    /// whitespace/casing slack) because enabling it side-effects: a missing
    /// model triggers a network `ollama pull` at generate time. Widening to
    /// the canonical tri-state set (canonical parser, `parse_bool_env_value`)
    /// would silently enable downloads for casual values (`yes`/`on`). Keep a
    /// minimal literal comparison; the canonical parser governs the non-wire,
    /// non-side-effecting flags.
    async fn ensure_ready(&self, requested: &str) -> Result<String, LLMError> {
        let autopull = std::env::var("VTCODE_LOCAL_AUTOPULL")
            .ok()
            .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"));

        match resolve_local_model(LocalProvider::Ollama, requested, Some(&self.base_url)).await {
            Ok(model) => Ok(model),
            Err(err) if autopull => {
                let model = match &err {
                    super::local_readiness::LocalReadinessError::ModelMissing { model, .. } => model.clone(),
                    _ => return Err(err.to_llm_error("Ollama")),
                };
                match ensure_oss_ready(Some(&model), Some(self.base_url.clone())).await {
                    Ok(()) => {
                        invalidate_readiness_cache();
                        Ok(model)
                    }
                    Err(_) => Err(err.to_llm_error("Ollama")),
                }
            }
            Err(err) => Err(err.to_llm_error("Ollama")),
        }
    }

    fn parse_chat_request(&self, value: &Value) -> Option<LLMRequest> {
        let messages_value = value.get("messages")?.as_array()?;
        let mut system_prompt = value
            .get("system")
            .and_then(|entry| entry.as_str())
            .filter(|text| !text.trim().is_empty())
            .map(|text| text.to_string());
        let mut messages = Vec::new();

        for entry in messages_value {
            let role = entry
                .get("role")
                .and_then(|r| r.as_str())
                .unwrap_or(vtcode_config::constants::message_roles::USER);
            let content = entry
                .get("content")
                .map(|c| match c {
                    Value::String(text) => text.to_string(),
                    other => other.to_string(),
                })
                .unwrap_or_default();

            if content.trim().is_empty() {
                continue;
            }

            match role {
                "system" => {
                    if system_prompt.is_none() {
                        system_prompt = Some(content);
                    }
                }
                "assistant" => messages.push(Message::assistant(content)),
                "user" => messages.push(Message::user(content)),
                _ => {}
            }
        }

        if messages.is_empty() {
            return None;
        }

        let tools = value
            .get("tools")
            .and_then(|entry| serde_json::from_value::<Vec<ToolDefinition>>(entry.clone()).ok());

        Some(LLMRequest {
            messages: std::sync::Arc::new(messages),
            system_prompt: system_prompt.map(std::sync::Arc::from),
            tools: tools.map(std::sync::Arc::new),
            model: value
                .get("model")
                .and_then(|m| m.as_str())
                .filter(|m| !m.trim().is_empty())
                .map(|m| m.to_string())
                .unwrap_or_else(|| self.model.clone()),
            max_tokens: value
                .get("max_tokens")
                .and_then(|entry| entry.as_u64())
                .map(|value| value as u32),
            temperature: value
                .get("temperature")
                .and_then(|entry| entry.as_f64())
                .map(|value| value as f32),
            stream: value.get("stream").and_then(|entry| entry.as_bool()).unwrap_or(false),
            ..Default::default()
        })
    }

    fn convert_tool_calls(tool_calls: Option<Vec<OllamaResponseToolCall>>) -> Result<Option<Vec<ToolCall>>, LLMError> {
        let Some(tool_calls) = tool_calls else {
            return Ok(None);
        };

        if tool_calls.is_empty() {
            return Ok(None);
        }

        let mut converted = Vec::new();
        for call in tool_calls.into_iter() {
            let function = call.function.ok_or_else(|| LLMError::Provider {
                message: "Ollama response missing function details for tool call".to_string(),
                metadata: None,
            })?;

            let name = function.name.ok_or_else(|| LLMError::Provider {
                message: "Ollama response missing tool function name".to_string(),
                metadata: None,
            })?;

            let arguments_value = function.arguments.unwrap_or_else(|| Value::Object(Map::new()));
            let arguments = match arguments_value {
                Value::String(raw) => raw,
                other => serde_json::to_string(&other).map_err(|err| LLMError::Provider {
                    message: format!("Failed to serialize Ollama tool arguments: {err}"),
                    metadata: None,
                })?,
            };

            // Ollama omits tool-call ids; index-based fallbacks reset every
            // response and collide across assistant messages downstream.
            let id = crate::providers::shared::generate_tool_call_id();

            converted.push(ToolCall::function(id, name, arguments));
        }

        Ok(Some(converted))
    }

    fn usage_from_counts(prompt_tokens: Option<u32>, completion_tokens: Option<u32>) -> Option<Usage> {
        if prompt_tokens.is_none() && completion_tokens.is_none() {
            return None;
        }

        let prompt = prompt_tokens.unwrap_or_default();
        let completion = completion_tokens.unwrap_or_default();
        Some(Usage {
            prompt_tokens: prompt,
            completion_tokens: completion,
            total_tokens: prompt + completion,
            cached_prompt_tokens: None,
            cache_creation_tokens: None,
            cache_read_tokens: None,
            iterations: None,
        })
    }

    fn finish_reason_from(reason: Option<&str>) -> FinishReason {
        match reason {
            Some("stop") | None => FinishReason::Stop,
            Some("length") => FinishReason::Length,
            Some("tool_calls") => FinishReason::ToolCalls,
            Some(other) => FinishReason::Error(other.to_string()),
        }
    }

    fn build_response(
        content: Option<String>,
        tool_calls: Option<Vec<ToolCall>>,
        reasoning: Option<String>,
        reasoning_details: Option<Vec<String>>,
        model: String,
        finish_reason: Option<&str>,
        prompt_tokens: Option<u32>,
        completion_tokens: Option<u32>,
    ) -> LLMResponse {
        let mut finish = Self::finish_reason_from(finish_reason);
        if tool_calls.as_ref().is_some_and(|calls| !calls.is_empty()) {
            finish = FinishReason::ToolCalls;
        }

        LLMResponse {
            content,
            tool_calls,
            model,
            usage: Self::usage_from_counts(prompt_tokens, completion_tokens),
            finish_reason: finish,
            reasoning,
            reasoning_details,
            tool_references: Vec::new(),
            request_id: None,
            organization_id: None,
            compaction: None,
        }
    }

    /// Tagged private reasoning must never become public content when no final
    /// text remains. Apply the same boundary to both completed wire modes.
    fn normalize_response_reasoning(response: &mut LLMResponse) {
        let Some(content) = response.content.take() else {
            return;
        };
        let (reasoning_parts, cleaned_content) = crate::utils::extract_reasoning_content(&content);
        if reasoning_parts.is_empty() {
            response.content = Some(content);
            return;
        }
        super::common::preserve_interleaved_content_in_reasoning_details(&mut response.reasoning_details, &content);
        if response.reasoning.is_none() {
            response.reasoning = Some(reasoning_parts.join("\n\n"));
        }
        response.content = cleaned_content;
    }

    fn response_from_chat_payload(model: String, parsed: OllamaChatResponse) -> Result<LLMResponse, LLMError> {
        if let Some(error) = parsed.error {
            return Err(LLMError::Provider { message: error, metadata: None });
        }

        let (content, reasoning, tool_calls, native_reasoning_details) = if let Some(message) = parsed.message {
            let content = message.content.and_then(|value| (!value.is_empty()).then_some(value));
            let reasoning = message.thinking.and_then(|value| (!value.is_empty()).then_some(value));
            let tool_calls = Self::convert_tool_calls(message.tool_calls)?;
            let native_reasoning_details = message.reasoning_details.filter(|d| !d.is_empty());
            (content, reasoning, tool_calls, native_reasoning_details)
        } else {
            (None, None, None, None)
        };

        let reasoning = reasoning.or_else(|| {
            native_reasoning_details
                .as_deref()
                .and_then(extract_reasoning_text_from_detail_values)
        });
        let reasoning_details = native_reasoning_details.as_deref().and_then(serialize_reasoning_detail_values);
        let mut response = Self::build_response(
            content,
            tool_calls,
            reasoning,
            reasoning_details,
            model,
            parsed.done_reason.as_deref(),
            parsed.prompt_eval_count,
            parsed.eval_count,
        );
        Self::normalize_response_reasoning(&mut response);
        Ok(response)
    }

    fn authorized_post_with_key(http_client: &HttpClient, url: &str, api_key: Option<&str>) -> reqwest::RequestBuilder {
        let builder = http_client.post(url.to_string());
        if let Some(value) = api_key {
            builder.bearer_auth(value)
        } else {
            builder
        }
    }

    async fn request_non_stream_response(
        http_client: &HttpClient,
        url: &str,
        api_key: Option<&str>,
        payload: &OllamaChatRequest,
        model: String,
    ) -> Result<LLMResponse, LLMError> {
        let response = Self::authorized_post_with_key(http_client, url, api_key)
            .json(payload)
            .send()
            .await
            .map_err(|e| format_network_error("Ollama", &e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = crate::providers::common::read_provider_error_body(response).await;
            let error_message =
                Self::extract_error(&body).unwrap_or_else(|| format!("Ollama request failed ({status}): {body}"));
            return Err(LLMError::Provider { message: error_message, metadata: None });
        }

        let parsed = response
            .json::<OllamaChatResponse>()
            .await
            .map_err(|e| format_parse_error("Ollama", &e))?;
        Self::response_from_chat_payload(model, parsed)
    }

    fn extract_error(body: &str) -> Option<String> {
        serde_json::from_str::<OllamaErrorResponse>(body)
            .ok()
            .and_then(|resp| resp.error)
    }
}

#[derive(Debug, Serialize)]
struct OllamaChatRequest {
    model: String,
    messages: Vec<OllamaChatMessage>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    format: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<OllamaChatOptions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<ToolDefinition>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    think: Option<Value>,
}

#[derive(Debug, Serialize)]
struct OllamaChatMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    images: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OllamaToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_name: Option<String>,
}

#[derive(Debug, Serialize)]
struct OllamaChatOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    num_predict: Option<u32>,
}

#[derive(Debug, Serialize)]
struct OllamaToolCall {
    #[serde(rename = "type")]
    call_type: String,
    function: OllamaToolFunctionCall,
}

#[derive(Debug, Serialize)]
struct OllamaToolFunctionCall {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    arguments: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    index: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct OllamaChatResponse {
    message: Option<OllamaResponseMessage>,
    #[serde(default)]
    done: bool,
    #[serde(default)]
    done_reason: Option<String>,
    #[serde(default)]
    prompt_eval_count: Option<u32>,
    #[serde(default)]
    eval_count: Option<u32>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OllamaResponseMessage {
    #[serde(default)]
    #[expect(
        dead_code,
        reason = "Intentional compatibility, platform, test, or API-shape suppression."
    )]
    role: Option<String>,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    thinking: Option<String>,
    #[serde(default)]
    reasoning_details: Option<Vec<Value>>,
    #[serde(default)]
    tool_calls: Option<Vec<OllamaResponseToolCall>>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct OllamaResponseToolCall {
    #[serde(default)]
    #[serde(rename = "type")]
    call_type: Option<String>,
    #[serde(default)]
    function: Option<OllamaResponseFunctionCall>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct OllamaResponseFunctionCall {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<Value>,
    #[serde(default)]
    index: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct OllamaErrorResponse {
    error: Option<String>,
}

fn parse_stream_chunk(line: &str) -> Result<OllamaChatResponse, LLMError> {
    serde_json::from_str::<OllamaChatResponse>(line).map_err(|err| LLMError::Provider {
        message: format!("Failed to parse Ollama stream chunk: {err}"),
        metadata: None,
    })
}

#[async_trait]
impl LLMProvider for OllamaProvider {
    fn name(&self) -> &str {
        "ollama"
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_non_streaming(&self, _model: &str) -> bool {
        // Pinned so the stream-timeout fallback cannot silently regress.
        true
    }

    fn supports_tools(&self, _model: &str) -> bool {
        true
    }

    fn supports_reasoning(&self, model: &str) -> bool {
        // Codex-inspired robustness: Setting model_supports_reasoning to false
        // does NOT disable it for known reasoning models.
        models::ollama::REASONING_MODELS.contains(&model)
            || self
                .model_behavior
                .as_ref()
                .and_then(|b| b.model_supports_reasoning)
                .unwrap_or(false)
    }

    fn supports_reasoning_effort(&self, model: &str) -> bool {
        // Same robustness logic for reasoning effort
        models::ollama::REASONING_LEVEL_MODELS.contains(&model)
            || self
                .model_behavior
                .as_ref()
                .and_then(|b| b.model_supports_reasoning_effort)
                .unwrap_or(false)
    }

    async fn generate(&self, mut request: LLMRequest) -> Result<LLMResponse, LLMError> {
        self.validate_request(&request)?;
        if request.model.is_empty() {
            request.model = self.model.clone();
        }
        let resolved = self.ensure_ready(&request.model).await?;
        request.model = resolved;
        let model = request.model.clone();
        let payload = self.build_payload(&request, false)?;
        let url = self.chat_url();
        Self::request_non_stream_response(&self.http_client, &url, self.api_key.as_deref(), &payload, model).await
    }

    async fn stream(&self, mut request: LLMRequest) -> Result<LLMStream, LLMError> {
        self.validate_request(&request)?;
        if request.model.is_empty() {
            request.model = self.model.clone();
        }
        let resolved = self.ensure_ready(&request.model).await?;
        request.model = resolved;
        let model = request.model.clone();
        let payload = self.build_payload(&request, true)?;
        let fallback_payload = self.build_payload(&request, false)?;
        // Tool-free recovery owns a shared request allowance in the runloop.
        // Propagate transport failure so that layer admits any corrective call.
        let allow_transport_fallback = !matches!(request.tool_choice, Some(ToolChoice::None));
        let url = self.chat_url();

        let response = self
            .authorized_post(url.clone())
            .header(reqwest::header::ACCEPT_ENCODING, "identity")
            .json(&payload)
            .send()
            .await
            .map_err(|e| format_network_error("Ollama", &e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = crate::providers::common::read_provider_error_body(response).await;
            let error_message = Self::extract_error(&body)
                .unwrap_or_else(|| format!("Ollama streaming request failed ({status}): {body}"));
            return Err(LLMError::Provider { message: error_message, metadata: None });
        }

        let byte_stream = response.bytes_stream();
        let mut buffer: Vec<u8> = Vec::new();
        let mut aggregator = crate::providers::shared::StreamAggregator::new(model.clone());
        let fallback_http_client = self.http_client.clone();
        let fallback_api_key = self.api_key.clone();
        let fallback_model = model.clone();
        let fallback_url = url.clone();
        let any_interleaved = request
            .messages
            .iter()
            .any(|msg| assistant_interleaved_history_text(msg, &request.model).is_some());
        let stream = try_stream! {
            let mut prompt_tokens: Option<u32> = None;
            let mut completion_tokens: Option<u32> = None;
            let mut finish_reason: Option<String> = None;
            let mut completed = false;
            let mut saw_stream_chunk = false;

            futures::pin_mut!(byte_stream);
            while let Some(chunk_result) = byte_stream.next().await {
                let chunk = match chunk_result {
                    Ok(chunk) => {
                        saw_stream_chunk = true;
                        chunk
                    }
                    Err(err) if !saw_stream_chunk && allow_transport_fallback => {
                        tracing::warn!(
                            model = %fallback_model,
                            url = %fallback_url,
                            error = %err,
                            "Ollama stream failed before first chunk; retrying once as non-stream response"
                        );
                        let fallback_response = Self::request_non_stream_response(
                            &fallback_http_client,
                            &fallback_url,
                            fallback_api_key.as_deref(),
                            &fallback_payload,
                            fallback_model.clone(),
                        ).await?;
                        yield LLMStreamEvent::Completed { response: Box::new(fallback_response) };
                        return;
                    }
                    Err(err) => Err(format_network_error("Ollama", &err))?,
                };
                buffer.extend_from_slice(&chunk);

                while let Some(pos) = buffer.iter().position(|b| *b == b'\n') {
                    // Borrow the line directly from `buffer` instead of
                    // draining into a temporary `Vec<u8>` per SSE line.
                    // `parse_stream_chunk` returns an owned deserialized
                    // value, so the borrow ends before we drain.
                    let line = std::str::from_utf8(&buffer[..pos])
                        .map_err(|err| LLMError::Provider {
                            message: format!("Invalid UTF-8 in Ollama stream: {err}"),
                            metadata: None,
                        })?;
                    let line = line.trim();

                    if line.is_empty() {
                        buffer.drain(..=pos);
                        continue;
                    }

                    let parsed = parse_stream_chunk(line)?;
                    // `parsed` is owned — safe to discard the borrowed line.
                    buffer.drain(..=pos);

                    if let Some(error) = parsed.error {
                        Err(LLMError::Provider {
                            message: error,
                            metadata: None,
                        })?;
                    }

                    if let Some(message) = parsed.message {
                        if let Some(reasoning_details) = message.reasoning_details.as_deref() {
                            aggregator.set_reasoning_details(reasoning_details);
                        }

                        let has_explicit_thinking = message
                            .thinking
                            .as_ref()
                            .map(|v| !v.is_empty())
                            .unwrap_or(false);

                        if let Some(thinking) = message.thinking
                            && let Some(delta) = aggregator.handle_reasoning(&thinking) {
                                yield LLMStreamEvent::Reasoning { delta };
                            }

                        if let Some(content) = message.content {
                            for event in aggregator.handle_content(&content) {
                                match &event {
                                    LLMStreamEvent::Reasoning { .. }
                                        if has_explicit_thinking || any_interleaved =>
                                    {
                                    }
                                    _ => yield event,
                                }
                            }
                        }

                        if let Some(tool_calls) = message.tool_calls {
                            let tool_calls_json: Vec<Value> = tool_calls
                                .into_iter()
                                .map(|tc| serde_json::to_value(tc).unwrap_or(Value::Null))
                                .filter(|v| !v.is_null())
                                .collect();
                            aggregator.handle_tool_calls(&tool_calls_json);
                        }
                    }

                    if parsed.done {
                        prompt_tokens = parsed.prompt_eval_count;
                        completion_tokens = parsed.eval_count;
                        finish_reason = parsed.done_reason;
                        completed = true;
                    }
                }

                if completed {
                    break;
                }
            }

            if !completed {
                Err(LLMError::Provider {
                    message: "Ollama stream ended without completion signal".to_string(),
                    metadata: None,
                })?;
            }

            let mut response = aggregator.finalize();
            if let Some(pt) = prompt_tokens {
                let mut usage = response.usage.unwrap_or_default();
                usage.prompt_tokens = pt;
                if let Some(ct) = completion_tokens {
                    usage.completion_tokens = ct;
                    usage.total_tokens = pt + ct;
                }
                response.usage = Some(usage);
            }
            if let Some(fr) = finish_reason {
                response.finish_reason = crate::providers::common::map_finish_reason_common(&fr);
            }
            if response.reasoning.is_none()
                && let Some(details) = response.reasoning_details.as_ref()
            {
                response.reasoning = extract_reasoning_text_from_serialized_details(details);
            }

            Self::normalize_response_reasoning(&mut response);
            yield LLMStreamEvent::Completed { response: Box::new(response) };
        };

        Ok(Box::pin(stream))
    }

    fn supported_models(&self) -> Vec<String> {
        models::ollama::SUPPORTED_MODELS.iter().map(|model| model.to_string()).collect()
    }

    fn validate_request(&self, request: &LLMRequest) -> Result<(), LLMError> {
        if let Some(tool_choice) = &request.tool_choice {
            match tool_choice {
                ToolChoice::Auto | ToolChoice::None | ToolChoice::AllowedTools(_) => {}
                _ => {
                    return Err(LLMError::InvalidRequest {
                        message: "Ollama does not support explicit tool_choice overrides".to_string(),
                        metadata: None,
                    });
                }
            }
        }

        if request.parallel_tool_calls.is_some() || request.parallel_tool_config.is_some() {
            return Err(LLMError::InvalidRequest {
                message: "Ollama does not support parallel tool configuration".to_string(),
                metadata: None,
            });
        }

        for message in request.messages.iter() {
            if matches!(message.role, MessageRole::Tool) && message.tool_call_id.is_none() {
                return Err(LLMError::InvalidRequest {
                    message: "Ollama tool responses must include tool_call_id".to_string(),
                    metadata: None,
                });
            }
        }

        Ok(())
    }
}

#[async_trait]
impl LLMClient for OllamaProvider {
    async fn generate(&mut self, prompt: &str) -> Result<LLMResponse, LLMError> {
        let mut request = self.parse_client_prompt(prompt);
        if request.model.is_empty() {
            request.model = self.model.clone();
        }
        Ok(LLMProvider::generate(self, request).await?)
    }

    fn model_id(&self) -> &str {
        &self.model
    }
}

#[cfg(test)]
mod tests;
