#![allow(clippy::result_large_err)]
use crate::config::TimeoutsConfig;
use crate::config::constants::{env_vars, models, urls};
use crate::config::core::{AnthropicConfig, ModelConfig, PromptCachingConfig};
use crate::llm::client::LLMClient;
use crate::llm::provider::{
    ContentPart, FinishReason, LLMError, LLMProvider, LLMRequest, LLMResponse, LLMStream,
    LLMStreamEvent, Message, MessageContent, MessageRole, ToolCall, ToolChoice, ToolDefinition,
    Usage,
};
use crate::llm::types as llm_types;
use crate::utils::http_client;
use anyhow::Result;
use async_stream::try_stream;
use async_trait::async_trait;
use futures::StreamExt;
use reqwest::Client as HttpClient;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashMap;

pub mod client;
pub mod parser;
pub mod pull;
pub mod url;

pub use client::OllamaClient;
pub use parser::pull_events_from_value;
pub use pull::{
    CliPullProgressReporter, OllamaPullEvent, OllamaPullProgressReporter, TuiPullProgressReporter,
};
pub use url::{base_url_to_host_root, is_openai_compatible_base_url};

use semver::Version;

use super::common::{override_base_url, parse_client_prompt_common, resolve_model};
use super::error_handling::{format_network_error, format_parse_error};

// ============================================================================
// Wire API Detection (adapted from OpenAI Codex's codex-ollama/src/lib.rs)
// ============================================================================

/// Wire protocol that the Ollama server supports.
/// Based on OpenAI Codex's WireApi enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OllamaWireApi {
    /// The Responses API (OpenAI-compatible at `/v1/responses`).
    Responses,
    /// Regular Chat Completions compatible with `/v1/chat/completions`.
    Chat,
}

/// Result of detecting which wire API the Ollama server supports.
pub struct WireApiDetection {
    pub wire_api: OllamaWireApi,
    pub version: Option<Version>,
}

/// Minimum Ollama version that supports the Responses API.
/// Ollama versions >= 0.13.4 support the Responses API.
fn min_responses_version() -> Version {
    Version::new(0, 13, 4)
}

/// Determine which wire API to use based on the Ollama server version.
fn wire_api_for_version(version: &Version) -> OllamaWireApi {
    // Version 0.0.0 is used for development builds, which typically support latest features
    if *version == Version::new(0, 0, 0) || *version >= min_responses_version() {
        OllamaWireApi::Responses
    } else {
        OllamaWireApi::Chat
    }
}

/// Detect which wire API the running Ollama server supports based on its version.
/// Returns `Ok(None)` when the version endpoint is missing or unparsable; callers
/// should keep the configured default in that case.
///
/// Adapted from OpenAI Codex's codex-ollama/src/lib.rs
pub async fn detect_wire_api(
    base_url: Option<String>,
) -> std::io::Result<Option<WireApiDetection>> {
    let resolved_base_url = override_base_url(
        urls::OLLAMA_API_BASE,
        base_url,
        Some(env_vars::OLLAMA_BASE_URL),
    );

    let client = match OllamaClient::try_from_base_url(&resolved_base_url).await {
        Ok(c) => c,
        Err(e) => {
            tracing::debug!("Failed to connect to Ollama server for version detection: {e}");
            return Ok(None);
        }
    };

    let Some(version) = client.fetch_version().await? else {
        return Ok(None);
    };

    let wire_api = wire_api_for_version(&version);

    Ok(Some(WireApiDetection {
        wire_api,
        version: Some(version),
    }))
}

/// Prepare the local OSS environment when using Ollama.
///
/// - Ensures a local Ollama server is reachable.
/// - Checks if the model exists locally and pulls it if missing.
///
/// Adapted from OpenAI Codex's codex-ollama/src/lib.rs
pub async fn ensure_oss_ready(
    model: Option<&str>,
    base_url: Option<String>,
) -> std::io::Result<()> {
    let target_model = model.unwrap_or(models::ollama::DEFAULT_MODEL);

    let resolved_base_url = override_base_url(
        urls::OLLAMA_API_BASE,
        base_url,
        Some(env_vars::OLLAMA_BASE_URL),
    );

    // Verify local Ollama is reachable
    let ollama_client = OllamaClient::try_from_base_url(&resolved_base_url).await?;

    // If the model is not present locally, pull it
    match ollama_client.fetch_models().await {
        Ok(existing_models) => {
            if !existing_models.iter().any(|m| m == target_model) {
                tracing::info!("Model '{target_model}' not found locally, pulling...");
                let mut reporter = CliPullProgressReporter::new();
                ollama_client
                    .pull_with_reporter(target_model, &mut reporter)
                    .await?;
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
    name: String,
    model: String,
    modified_at: String,
    size: u64,
    digest: String,
    details: OllamaModelDetails,
}

#[derive(Debug, Deserialize, Serialize)]
struct OllamaModelDetails {
    format: String,
    family: String,
    families: Option<Vec<String>>,
    parameter_size: String,
    quantization_level: String,
}

const OLLAMA_CONNECTION_ERROR: &str = "No running Ollama server detected. Start it with: `ollama serve` (after installing)\
     Install instructions: https://github.com/ollama/ollama?tab=readme-ov-file";

/// Fetches available local Ollama models from the Ollama API endpoint
pub async fn fetch_ollama_models(base_url: Option<String>) -> Result<Vec<String>, anyhow::Error> {
    use crate::config::constants::{env_vars, urls};

    let resolved_base_url = override_base_url(
        urls::OLLAMA_API_BASE,
        base_url,
        Some(env_vars::OLLAMA_BASE_URL),
    );

    // Construct the tags endpoint URL
    let tags_url = format!("{}/api/tags", resolved_base_url);

    // Create HTTP client with connection timeout
    let client = http_client::create_client_with_timeout(std::time::Duration::from_secs(5));

    // Make GET request to fetch models
    let response = client
        .get(&tags_url)
        .header("Content-Type", "application/json")
        .send()
        .await
        .map_err(|e| {
            tracing::warn!("Failed to connect to Ollama server: {e:?}");
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
        .map_err(|e| anyhow::anyhow!("Failed to parse Ollama models response: {}", e))?;

    // Extract model names
    let model_names: Vec<String> = tags_response
        .models
        .into_iter()
        .map(|model| model.name) // Use 'name' field which is the full model name including tag
        .collect();

    Ok(model_names)
}

pub struct OllamaProvider {
    http_client: HttpClient,
    base_url: String,
    model: String,
    api_key: Option<String>,
    model_behavior: Option<ModelConfig>,
}

impl OllamaProvider {
    pub fn new(api_key: String) -> Self {
        Self::with_model(api_key, models::ollama::DEFAULT_MODEL.to_string())
    }

    pub fn with_model(api_key: String, model: String) -> Self {
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

    fn is_local_base_url(base_url: &str) -> bool {
        let lowered = base_url.trim().to_ascii_lowercase();
        const LOCAL_PREFIXES: &[&str] = &[
            "http://localhost",
            "https://localhost",
            "http://127.",
            "https://127.",
            "http://0.0.0.0",
            "https://0.0.0.0",
            "http://[::1]",
            "https://[::1]",
        ];

        LOCAL_PREFIXES
            .iter()
            .any(|prefix| lowered.starts_with(prefix))
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

        let resolved_base =
            override_base_url(default_base, base_url, Some(env_vars::OLLAMA_BASE_URL));
        let target_is_local = Self::is_local_base_url(&resolved_base);

        // Never send API keys to local endpoints; keep keys for cloud/remote targets
        let effective_api_key = if target_is_local {
            None
        } else {
            normalized_api_key
        };

        Self {
            http_client: http_client::create_default_client(),
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
                .unwrap_or(crate::config::constants::message_roles::USER);
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
            messages,
            system_prompt: system_prompt.map(std::sync::Arc::new),
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
            stream: value
                .get("stream")
                .and_then(|entry| entry.as_bool())
                .unwrap_or(false),
            ..Default::default()
        })
    }

    fn build_payload(
        &self,
        request: &LLMRequest,
        stream: bool,
    ) -> Result<OllamaChatRequest, LLMError> {
        let mut messages = Vec::new();
        let mut tool_names: HashMap<String, String> = HashMap::new();

        if let Some(system) = &request.system_prompt
            && !system.trim().is_empty()
        {
            messages.push(OllamaChatMessage {
                role: "system".to_string(),
                content: Some(system.to_string()),
                tool_calls: None,
                tool_call_id: None,
                tool_name: None,
                images: None,
            });
        }

        for message in &request.messages {
            let (content_text, images) = Self::extract_content_and_images(&message.content);
            match message.role {
                MessageRole::Tool => {
                    let tool_name = message
                        .tool_call_id
                        .as_ref()
                        .and_then(|id| tool_names.get(id).cloned());
                    messages.push(OllamaChatMessage {
                        role: "tool".to_string(),
                        content: Some(content_text),
                        tool_calls: None,
                        tool_call_id: message.tool_call_id.clone(),
                        tool_name,
                        images: None,
                    });
                }
                _ => {
                    let mut payload_message = OllamaChatMessage {
                        role: message.role.as_generic_str().to_string(),
                        content: Some(content_text),
                        tool_calls: None,
                        tool_call_id: None,
                        tool_name: None,
                        images,
                    };

                    if let Some(tool_calls) = message.get_tool_calls() {
                        let mut converted = Vec::new();
                        for (index, tool_call) in tool_calls.iter().enumerate() {
                            if let Some(ref func) = tool_call.function {
                                if !tool_call.id.is_empty() {
                                    tool_names
                                        .entry(tool_call.id.clone())
                                        .or_insert_with(|| func.name.clone());
                                }

                                let arguments = Self::parse_tool_arguments(&func.arguments)?;
                                converted.push(OllamaToolCall {
                                    call_type: tool_call.call_type.clone(),
                                    function: OllamaToolFunctionCall {
                                        name: func.name.clone(),
                                        arguments: Some(arguments),
                                        index: Some(index as u32),
                                    },
                                });
                            }
                        }

                        if !converted.is_empty() {
                            payload_message.tool_calls = Some(converted);
                            if payload_message.content.is_none() {
                                payload_message.content = Some(String::new());
                            }
                        }
                    }

                    messages.push(payload_message);
                }
            }
        }

        let options = if request.temperature.is_some() || request.max_tokens.is_some() {
            Some(OllamaChatOptions {
                temperature: request.temperature,
                num_predict: request.max_tokens,
            })
        } else {
            None
        };

        let tools = match request.tool_choice {
            Some(ToolChoice::None) => None,
            _ => request.tools.as_ref().map(|tools| {
                tools
                    .iter()
                    .filter_map(|tool| {
                        // Normalize all tools to function type for Ollama compatibility
                        tool.function.as_ref().map(|func| {
                            ToolDefinition::function(
                                func.name.clone(),
                                func.description.clone(),
                                func.parameters.clone(),
                            )
                        })
                    })
                    .collect()
            }),
        };

        Ok(OllamaChatRequest {
            model: request.model.clone(),
            messages,
            stream,
            format: request.output_format.clone(),
            options,
            tools,
            think: Self::think_value(request),
        })
    }

    fn extract_content_and_images(content: &MessageContent) -> (String, Option<Vec<String>>) {
        let mut images = Vec::new();
        if let MessageContent::Parts(parts) = content {
            for part in parts {
                if let ContentPart::Image { data, .. } = part {
                    images.push(data.clone());
                }
            }
        }

        let text = content.as_text().into_owned();
        let images = if images.is_empty() {
            None
        } else {
            Some(images)
        };
        (text, images)
    }

    fn parse_tool_arguments(arguments: &str) -> Result<Value, LLMError> {
        if arguments.trim().is_empty() {
            return Ok(Value::Object(Map::new()));
        }

        serde_json::from_str(arguments).map_err(|err| LLMError::InvalidRequest {
            message: format!("Failed to parse tool arguments for Ollama: {err}"),
            metadata: None,
        })
    }

    fn think_value(request: &LLMRequest) -> Option<Value> {
        let model_id = request.model.as_str();
        if !models::ollama::REASONING_MODELS.contains(&model_id) {
            return None;
        }

        if models::ollama::REASONING_LEVEL_MODELS.contains(&model_id) {
            request
                .reasoning_effort
                .map(|effort| Value::String(effort.to_string()))
        } else {
            Some(Value::Bool(true))
        }
    }

    fn convert_tool_calls(
        tool_calls: Option<Vec<OllamaResponseToolCall>>,
    ) -> Result<Option<Vec<ToolCall>>, LLMError> {
        let Some(tool_calls) = tool_calls else {
            return Ok(None);
        };

        if tool_calls.is_empty() {
            return Ok(None);
        }

        let mut converted = Vec::new();
        for (index, call) in tool_calls.into_iter().enumerate() {
            let function = call.function.ok_or_else(|| LLMError::Provider {
                message: "Ollama response missing function details for tool call".to_string(),
                metadata: None,
            })?;

            let name = function.name.ok_or_else(|| LLMError::Provider {
                message: "Ollama response missing tool function name".to_string(),
                metadata: None,
            })?;

            let arguments_value = function
                .arguments
                .unwrap_or_else(|| Value::Object(Map::new()));
            let arguments = match arguments_value {
                Value::String(raw) => raw,
                other => serde_json::to_string(&other).map_err(|err| LLMError::Provider {
                    message: format!("Failed to serialize Ollama tool arguments: {err}"),
                    metadata: None,
                })?,
            };

            let id = function
                .index
                .map(|value| format!("tool_call_{value}"))
                .unwrap_or_else(|| format!("tool_call_{index}"));

            converted.push(ToolCall::function(id, name, arguments));
        }

        Ok(Some(converted))
    }

    fn usage_from_counts(
        prompt_tokens: Option<u32>,
        completion_tokens: Option<u32>,
    ) -> Option<Usage> {
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
            reasoning_details: None,
            tool_references: Vec::new(),
            request_id: None,
            organization_id: None,
        }
    }

    fn response_from_chat_payload(
        model: String,
        parsed: OllamaChatResponse,
    ) -> Result<LLMResponse, LLMError> {
        if let Some(error) = parsed.error {
            return Err(LLMError::Provider {
                message: error,
                metadata: None,
            });
        }

        let (content, reasoning, tool_calls) = if let Some(message) = parsed.message {
            let content = message
                .content
                .and_then(|value| (!value.is_empty()).then_some(value));
            let reasoning = message
                .thinking
                .and_then(|value| (!value.is_empty()).then_some(value));
            let tool_calls = Self::convert_tool_calls(message.tool_calls)?;
            (content, reasoning, tool_calls)
        } else {
            (None, None, None)
        };

        Ok(Self::build_response(
            content,
            tool_calls,
            reasoning,
            model,
            parsed.done_reason.as_deref(),
            parsed.prompt_eval_count,
            parsed.eval_count,
        ))
    }

    fn authorized_post_with_key(
        http_client: &HttpClient,
        url: &str,
        api_key: Option<&str>,
    ) -> reqwest::RequestBuilder {
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
            let body = response.text().await.unwrap_or_default();
            let error_message = Self::extract_error(&body)
                .unwrap_or_else(|| format!("Ollama request failed ({status}): {body}"));
            return Err(LLMError::Provider {
                message: error_message,
                metadata: None,
            });
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
    #[allow(dead_code)]
    role: Option<String>,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    thinking: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<OllamaResponseToolCall>>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct OllamaResponseToolCall {
    #[serde(default)]
    #[serde(rename = "type")]
    #[allow(dead_code)]
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

    async fn count_prompt_tokens_exact(
        &self,
        request: &LLMRequest,
    ) -> Result<Option<u32>, LLMError> {
        let mut request = request.clone();
        self.validate_request(&request)?;
        if request.model.is_empty() {
            request.model = self.model.clone();
        }

        let mut payload = self.build_payload(&request, false)?;
        let options = payload.options.get_or_insert(OllamaChatOptions {
            temperature: None,
            num_predict: None,
        });
        options.num_predict = Some(0);
        options.temperature = None;

        let response = self
            .authorized_post(self.chat_url())
            .json(&payload)
            .send()
            .await
            .map_err(|e| format_network_error("Ollama", &e))?;

        if !response.status().is_success() {
            return Ok(None);
        }

        let parsed = response
            .json::<OllamaChatResponse>()
            .await
            .map_err(|e| format_parse_error("Ollama", &e))?;

        Ok(parsed.prompt_eval_count)
    }

    async fn generate(&self, mut request: LLMRequest) -> Result<LLMResponse, LLMError> {
        self.validate_request(&request)?;
        if request.model.is_empty() {
            request.model = self.model.clone();
        }
        let model = request.model.clone();
        let payload = self.build_payload(&request, false)?;
        let url = self.chat_url();
        Self::request_non_stream_response(
            &self.http_client,
            &url,
            self.api_key.as_deref(),
            &payload,
            model,
        )
        .await
    }

    async fn stream(&self, mut request: LLMRequest) -> Result<LLMStream, LLMError> {
        self.validate_request(&request)?;
        if request.model.is_empty() {
            request.model = self.model.clone();
        }
        let model = request.model.clone();
        let payload = self.build_payload(&request, true)?;
        let fallback_payload = self.build_payload(&request, false)?;
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
            let body = response.text().await.unwrap_or_default();
            let error_message = Self::extract_error(&body)
                .unwrap_or_else(|| format!("Ollama streaming request failed ({status}): {body}"));
            return Err(LLMError::Provider {
                message: error_message,
                metadata: None,
            });
        }

        let byte_stream = response.bytes_stream();
        let mut buffer: Vec<u8> = Vec::new();
        let mut aggregator = crate::llm::providers::shared::StreamAggregator::new(model.clone());
        let fallback_http_client = self.http_client.clone();
        let fallback_api_key = self.api_key.clone();
        let fallback_model = model.clone();
        let fallback_url = url.clone();
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
                    Err(err) if !saw_stream_chunk => {
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
                    let line_bytes: Vec<u8> = buffer.drain(..=pos).collect();
                    let line = std::str::from_utf8(&line_bytes)
                        .map_err(|err| LLMError::Provider {
                            message: format!("Invalid UTF-8 in Ollama stream: {err}"),
                            metadata: None,
                        })?;
                    let line = line.trim();

                    if line.is_empty() {
                        continue;
                    }

                    let parsed = parse_stream_chunk(line)?;

                    if let Some(error) = parsed.error {
                        Err(LLMError::Provider {
                            message: error,
                            metadata: None,
                        })?;
                    }

                    if let Some(message) = parsed.message {
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
                                    LLMStreamEvent::Reasoning { .. } if has_explicit_thinking => {
                                    }
                                    _ => yield event,
                                }
                            }
                        }

                        if let Some(tool_calls) = message.tool_calls {
                            let tool_calls_json: Vec<serde_json::Value> = tool_calls
                                .into_iter()
                                .map(|tc| serde_json::to_value(tc).unwrap_or(serde_json::Value::Null))
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
                response.finish_reason = crate::llm::providers::common::map_finish_reason_common(&fr);
            }

            yield LLMStreamEvent::Completed { response: Box::new(response) };
        };

        Ok(Box::pin(stream))
    }

    fn supported_models(&self) -> Vec<String> {
        models::ollama::SUPPORTED_MODELS
            .iter()
            .map(|model| model.to_string())
            .collect()
    }

    fn validate_request(&self, request: &LLMRequest) -> Result<(), LLMError> {
        if let Some(tool_choice) = &request.tool_choice {
            match tool_choice {
                ToolChoice::Auto | ToolChoice::None => {}
                _ => {
                    return Err(LLMError::InvalidRequest {
                        message: "Ollama does not support explicit tool_choice overrides"
                            .to_string(),
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

        for message in &request.messages {
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
    async fn generate(&mut self, prompt: &str) -> Result<llm_types::LLMResponse, LLMError> {
        let mut request = self.parse_client_prompt(prompt);
        if request.model.is_empty() {
            request.model = self.model.clone();
        }
        Ok(LLMProvider::generate(self, request).await?)
    }

    fn backend_kind(&self) -> llm_types::BackendKind {
        llm_types::BackendKind::Ollama
    }

    fn model_id(&self) -> &str {
        &self.model
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::provider::{ContentPart, Message, MessageContent};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_provider() -> OllamaProvider {
        OllamaProvider::from_config(
            None,
            Some("test-model".to_string()),
            Some("http://localhost".to_string()),
            None,
            None,
            None,
            None,
        )
    }

    #[test]
    fn build_payload_includes_images() {
        let provider = test_provider();
        let parts = vec![
            ContentPart::text("see ".to_string()),
            ContentPart::image("BASE64DATA".to_string(), "image/png".to_string()),
        ];
        let request = LLMRequest {
            model: "test-model".to_string(),
            messages: vec![Message::user_with_parts(parts)],
            ..Default::default()
        };

        let payload = provider.build_payload(&request, false).unwrap();
        assert_eq!(payload.messages.len(), 1);
        let message = &payload.messages[0];
        assert_eq!(message.content.as_deref(), Some("see "));
        assert_eq!(
            message.images.as_ref(),
            Some(&vec!["BASE64DATA".to_string()])
        );
    }

    #[test]
    fn build_payload_omits_images_when_none_present() {
        let provider = test_provider();
        let content = MessageContent::text("no images".to_string());
        let request = LLMRequest {
            model: "test-model".to_string(),
            messages: vec![Message::user(content.as_text().into_owned())],
            ..Default::default()
        };

        let payload = provider.build_payload(&request, false).unwrap();
        assert_eq!(payload.messages.len(), 1);
        let message = &payload.messages[0];
        assert_eq!(message.content.as_deref(), Some("no images"));
        assert!(message.images.is_none());
    }

    #[tokio::test]
    async fn exact_count_uses_chat_zero_predict_prompt_eval_count() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "done": true,
                "prompt_eval_count": 42,
                "eval_count": 0,
                "message": { "role": "assistant", "content": "" }
            })))
            .mount(&server)
            .await;

        let provider = OllamaProvider::new_with_client(
            String::new(),
            "test-model".to_string(),
            reqwest::Client::new(),
            server.uri(),
            TimeoutsConfig::default(),
        );

        let request = LLMRequest {
            model: "test-model".to_string(),
            messages: vec![Message::user("hello".to_string())],
            ..Default::default()
        };

        let count = <OllamaProvider as LLMProvider>::count_prompt_tokens_exact(&provider, &request)
            .await
            .expect("count should succeed");
        assert_eq!(count, Some(42));
    }

    #[tokio::test]
    async fn exact_count_returns_none_when_unavailable() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let provider = OllamaProvider::new_with_client(
            String::new(),
            "test-model".to_string(),
            reqwest::Client::new(),
            server.uri(),
            TimeoutsConfig::default(),
        );

        let request = LLMRequest {
            model: "test-model".to_string(),
            messages: vec![Message::user("hello".to_string())],
            ..Default::default()
        };

        let count = <OllamaProvider as LLMProvider>::count_prompt_tokens_exact(&provider, &request)
            .await
            .expect("count should succeed");
        assert_eq!(count, None);
    }
}
