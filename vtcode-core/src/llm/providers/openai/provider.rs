#![allow(
    clippy::collapsible_if,
    clippy::manual_contains,
    clippy::nonminimal_bool,
    clippy::single_match,
    clippy::result_large_err,
    unused_imports
)]

use crate::config::TimeoutsConfig;
use crate::config::constants::{env_vars, models, urls};
use crate::config::core::{AnthropicConfig, OpenAIPromptCacheSettings, PromptCachingConfig};
use crate::config::types::ReasoningEffortLevel;
use crate::llm::client::LLMClient;
use crate::llm::error_display;
use crate::llm::provider::{self, LLMProvider};
use crate::llm::types as llm_types;
use async_stream::try_stream;
use async_trait::async_trait;
use reqwest::Client as HttpClient;
use reqwest::StatusCode;
use reqwest::header::HeaderMap;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
#[cfg(debug_assertions)]
use std::time::Instant;
use tokio::task::spawn_blocking;
use tracing::debug;

use openai_harmony::chat::{
    Author as HarmonyAuthor, Content as HarmonyContent, Conversation, DeveloperContent,
    Message as HarmonyMessage, ReasoningEffort, Role as HarmonyRole, SystemContent,
    ToolDescription,
};
use openai_harmony::{HarmonyEncodingName, load_harmony_encoding};

// Import from extracted modules
use super::errors::{
    fallback_model_if_not_found, format_openai_error, is_model_not_found,
    is_responses_api_unsupported,
};
use super::headers;
use crate::llm::providers::error_handling::is_rate_limit_error;
use super::responses_api::parse_responses_payload;
use super::message_parser;
use super::harmony;
use super::request_builder;
use super::response_parser;
use super::stream_decoder;
use super::types::{MAX_COMPLETION_TOKENS_FIELD, OpenAIResponsesPayload, ResponsesApiState};

use super::super::{
    common::{
        extract_prompt_cache_settings, override_base_url, parse_client_prompt_common, resolve_model,
    },
    extract_reasoning_trace,
};
use crate::prompts::system::default_system_prompt;

pub struct OpenAIProvider {
    api_key: Arc<str>,
    http_client: HttpClient,
    base_url: Arc<str>,
    model: Arc<str>,
    responses_api_modes: Mutex<HashMap<String, ResponsesApiState>>,
    prompt_cache_enabled: bool,
    prompt_cache_settings: OpenAIPromptCacheSettings,
}

impl OpenAIProvider {
    fn is_gpt5_codex_model(model: &str) -> bool {
        model == models::openai::GPT_5_CODEX
            || model == models::openai::GPT_5_1_CODEX
            || model == models::openai::GPT_5_1_CODEX_MAX
    }

    fn is_responses_api_model(model: &str) -> bool {
        models::openai::RESPONSES_API_MODELS.contains(&model)
    }

    fn uses_harmony(model: &str) -> bool {
        harmony::uses_harmony(model)
    }

    fn convert_to_harmony_conversation(
        &self,
        request: &provider::LLMRequest,
    ) -> Result<Conversation, provider::LLMError> {
        let mut harmony_messages = Vec::with_capacity(request.messages.len() + 4); // +4 for system, developer, and potential splits
        let mut tool_call_authors: HashMap<String, String> = HashMap::with_capacity(16);

        // 1. Add standard system message as per Harmony spec
        let current_date = chrono::Local::now().format("%Y-%m-%d").to_string();
        let reasoning_effort = match request.reasoning_effort {
            Some(ReasoningEffortLevel::Low) => ReasoningEffort::Low,
            Some(ReasoningEffortLevel::Medium) => ReasoningEffort::Medium,
            Some(ReasoningEffortLevel::High) => ReasoningEffort::High,
            _ => ReasoningEffort::Medium,
        };

        let system_content = SystemContent::new()
            .with_conversation_start_date(&current_date)
            .with_reasoning_effort(reasoning_effort);

        // Note: The identity and valid channels are typically handled by the SystemContent renderer
        // in openai-harmony, but we can also add them to instructions if needed.

        harmony_messages.push(HarmonyMessage::from_role_and_content(
            HarmonyRole::System,
            system_content,
        ));

        // 2. Add developer message (instructions + tools)
        let mut developer_content = DeveloperContent::new();
        if let Some(system_prompt) = &request.system_prompt {
            developer_content = developer_content.with_instructions(system_prompt);
        }

        if let Some(tools) = &request.tools {
            let tool_descriptions: Vec<ToolDescription> = tools
                .iter()
                .filter_map(|tool| {
                    if tool.tool_type != "function" {
                        return None;
                    }
                    let func = tool.function.as_ref()?;
                    Some(ToolDescription::new(
                        &func.name,
                        &func.description,
                        Some(func.parameters.clone()),
                    ))
                })
                .collect();

            if !tool_descriptions.is_empty() {
                developer_content = developer_content.with_function_tools(tool_descriptions);
            }
        }

        harmony_messages.push(HarmonyMessage::from_role_and_content(
            HarmonyRole::Developer,
            developer_content,
        ));

        // Convert messages
        for (i, msg) in request.messages.iter().enumerate() {
            match msg.role {
                provider::MessageRole::System => {
                    // Additional system messages (rare in vtcode)
                    harmony_messages.push(HarmonyMessage::from_role_and_content(
                        HarmonyRole::System,
                        msg.content.as_text(),
                    ));
                }
                provider::MessageRole::User => {
                    harmony_messages.push(HarmonyMessage::from_role_and_content(
                        HarmonyRole::User,
                        msg.content.as_text(),
                    ));
                }
                provider::MessageRole::Assistant => {
                    let has_final = !msg.content.as_text().is_empty();
                    let is_last = i == request.messages.len() - 1;

                    // Spec: Drop CoT (analysis) if the response ended in a 'final' message,
                    // as it's no longer needed for subsequent turns.
                    // Keep it if there are tool calls (as they are part of the CoT flow)
                    // or if it's the last message and has no final content yet.
                    let should_keep_analysis = msg.tool_calls.is_some() || (is_last && !has_final);

                    // 1. Handle reasoning (analysis channel)
                    if let Some(reasoning) = &msg.reasoning {
                        if should_keep_analysis {
                            harmony_messages.push(
                                HarmonyMessage::from_role_and_content(
                                    HarmonyRole::Assistant,
                                    reasoning.clone(),
                                )
                                .with_channel("analysis"),
                            );
                        }
                    }

                    // 2. Handle tool calls (commentary channel)
                    if let Some(tool_calls) = &msg.tool_calls {
                        for call in tool_calls {
                            if let Some(ref func) = call.function {
                                let recipient = format!("functions.{}", func.name);
                                tool_call_authors.insert(call.id.clone(), recipient.clone());

                                harmony_messages.push(
                                    HarmonyMessage::from_role_and_content(
                                        HarmonyRole::Assistant,
                                        func.arguments.clone(),
                                    )
                                    .with_channel("commentary")
                                    .with_recipient(&recipient)
                                    .with_content_type("<|constrain|> json"),
                                );
                            }
                        }
                    } else {
                        // 3. Handle final content (final channel)
                        let text = msg.content.as_text();
                        if !text.is_empty() {
                            harmony_messages.push(
                                HarmonyMessage::from_role_and_content(HarmonyRole::Assistant, text)
                                    .with_channel("final"),
                            );
                        }
                    }
                }
                provider::MessageRole::Tool => {
                    let author_name = msg
                        .tool_call_id
                        .as_ref()
                        .and_then(|id| tool_call_authors.get(id))
                        .cloned()
                        .or_else(|| msg.tool_call_id.clone());

                    let author = author_name
                        .map(|name| HarmonyAuthor::new(HarmonyRole::Tool, name))
                        .unwrap_or_else(|| HarmonyAuthor::from(HarmonyRole::Tool));

                    harmony_messages.push(
                        HarmonyMessage::from_author_and_content(author, msg.content.as_text())
                            .with_channel("commentary")
                            .with_recipient("assistant"),
                    );
                }
            }
        }

        Ok(Conversation::from_messages(harmony_messages))
    }

    fn requires_responses_api(model: &str) -> bool {
        model == models::openai::GPT_5
            || model == models::openai::GPT_5_CODEX
            || model == models::openai::GPT_5_1_CODEX
            || model == models::openai::GPT_5_1_CODEX_MAX
    }

    fn default_responses_state(model: &str) -> ResponsesApiState {
        if Self::requires_responses_api(model) {
            ResponsesApiState::Required
        } else if Self::is_responses_api_model(model) {
            ResponsesApiState::Allowed
        } else {
            ResponsesApiState::Disabled
        }
    }

    pub fn new(api_key: String) -> Self {
        Self::with_model_internal(
            api_key,
            models::openai::DEFAULT_MODEL.to_string(),
            None,
            None,
        )
    }

    pub fn with_model(api_key: String, model: String) -> Self {
        Self::with_model_internal(api_key, model, None, None)
    }

    pub fn new_with_client(
        api_key: String,
        model: String,
        http_client: reqwest::Client,
        base_url: String,
        _timeouts: TimeoutsConfig,
    ) -> Self {
        use std::collections::HashMap;
        use std::sync::Arc;
        use std::sync::Mutex;

        Self {
            api_key: Arc::from(api_key.as_str()),
            http_client,
            base_url: Arc::from(base_url.as_str()),
            model: Arc::from(model.as_str()),
            prompt_cache_enabled: false,
            prompt_cache_settings: Default::default(),
            responses_api_modes: Mutex::new(HashMap::new()),
        }
    }

    pub fn from_config(
        api_key: Option<String>,
        model: Option<String>,
        base_url: Option<String>,
        prompt_cache: Option<PromptCachingConfig>,
        _timeouts: Option<TimeoutsConfig>,
        _anthropic: Option<AnthropicConfig>,
    ) -> Self {
        let api_key_value = api_key.unwrap_or_default();
        let model_value = resolve_model(model, models::openai::DEFAULT_MODEL);

        Self::with_model_internal(api_key_value, model_value, prompt_cache, base_url)
    }

    fn with_model_internal(
        api_key: String,
        model: String,
        prompt_cache: Option<PromptCachingConfig>,
        base_url: Option<String>,
    ) -> Self {
        let (prompt_cache_enabled, prompt_cache_settings) = extract_prompt_cache_settings(
            prompt_cache,
            |providers| &providers.openai,
            |cfg, provider_settings| cfg.enabled && provider_settings.enabled,
        );

        let resolved_base_url = override_base_url(
            urls::OPENAI_API_BASE,
            base_url,
            Some(env_vars::OPENAI_BASE_URL),
        );

        let mut responses_api_modes = HashMap::new();
        let default_state = Self::default_responses_state(&model);
        let is_native_openai = resolved_base_url.contains("api.openai.com");
        let is_xai = resolved_base_url.contains("api.x.ai");

        // Non-native OpenAI providers (like xAI) may not support all OpenAI features
        let initial_state = if is_xai || !is_native_openai {
            ResponsesApiState::Disabled
        } else {
            default_state
        };
        responses_api_modes.insert(model.clone(), initial_state);

        // Use centralized HTTP client factory for consistent timeout handling
        use crate::llm::http_client::HttpClientFactory;
        let http_client =
            HttpClientFactory::with_timeouts(Duration::from_secs(120), Duration::from_secs(30));

        Self {
            api_key: Arc::from(api_key.as_str()),
            http_client,
            base_url: Arc::from(resolved_base_url.as_str()),
            model: Arc::from(model.as_str()),
            responses_api_modes: Mutex::new(responses_api_modes),
            prompt_cache_enabled,
            prompt_cache_settings,
        }
    }

    fn authorize(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if self.api_key.trim().is_empty() {
            builder
        } else {
            builder.bearer_auth(&self.api_key)
        }
    }

    fn supports_temperature_parameter(model: &str) -> bool {
        // GPT-5.0 variants don't support temperature
        // GPT-5.1 Codex variants also don't support temperature (API confirmed)
        if model == models::openai::GPT_5
            || model == models::openai::GPT_5_CODEX
            || model == models::openai::GPT_5_MINI
            || model == models::openai::GPT_5_NANO
            || model == models::openai::GPT_5_1_CODEX
            || model == models::openai::GPT_5_1_CODEX_MAX
        {
            return false;
        }
        true
    }

    fn responses_api_state(&self, model: &str) -> ResponsesApiState {
        let mut modes = match self.responses_api_modes.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                tracing::warn!("OpenAI responses_api_modes mutex poisoned, recovering");
                poisoned.into_inner()
            }
        };
        *modes
            .entry(model.to_string())
            .or_insert_with(|| Self::default_responses_state(model))
    }

    fn set_responses_api_state(&self, model: &str, state: ResponsesApiState) {
        let mut modes = match self.responses_api_modes.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                tracing::warn!("OpenAI responses_api_modes mutex poisoned, recovering");
                poisoned.into_inner()
            }
        };
        modes.insert(model.to_string(), state);
    }

    fn parse_client_prompt(&self, prompt: &str) -> provider::LLMRequest {
        parse_client_prompt_common(prompt, &self.model, |value| {
            message_parser::parse_chat_request(value, &self.model)
        })
    }

    fn convert_to_openai_format(
        &self,
        request: &provider::LLMRequest,
    ) -> Result<Value, provider::LLMError> {
        let ctx = request_builder::ChatRequestContext {
            model: &self.model,
            base_url: &self.base_url,
            supports_tools: self.supports_tools(&request.model),
            supports_parallel_tool_config: self.supports_parallel_tool_config(&request.model),
            supports_temperature: Self::supports_temperature_parameter(&request.model),
        };

        request_builder::build_chat_request(request, &ctx)
    }

    fn convert_to_openai_responses_format(
        &self,
        request: &provider::LLMRequest,
    ) -> Result<Value, provider::LLMError> {
        let ctx = request_builder::ResponsesRequestContext {
            supports_tools: self.supports_tools(&request.model),
            supports_parallel_tool_config: self.supports_parallel_tool_config(&request.model),
            supports_temperature: Self::supports_temperature_parameter(&request.model),
            supports_reasoning_effort: self.supports_reasoning_effort(&request.model),
            supports_reasoning: self.supports_reasoning(&request.model),
            is_gpt5_codex_model: Self::is_gpt5_codex_model(&request.model),
            is_responses_api_model: Self::is_responses_api_model(&request.model),
            prompt_cache_retention: self.prompt_cache_settings.prompt_cache_retention.as_deref(),
        };

        request_builder::build_responses_request(request, &ctx)
    }

    fn parse_openai_response(
        &self,
        response_json: Value,
    ) -> Result<provider::LLMResponse, provider::LLMError> {
        let include_cached_prompt_tokens =
            self.prompt_cache_enabled && self.prompt_cache_settings.surface_metrics;
        response_parser::parse_chat_response(response_json, include_cached_prompt_tokens)
    }

    fn parse_openai_responses_response(
        &self,
        response_json: Value,
    ) -> Result<provider::LLMResponse, provider::LLMError> {
        let include_metrics =
            self.prompt_cache_enabled && self.prompt_cache_settings.surface_metrics;
        parse_responses_payload(response_json, include_metrics)
    }

    async fn generate_with_harmony(
        &self,
        request: provider::LLMRequest,
    ) -> Result<provider::LLMResponse, provider::LLMError> {
        // Load harmony encoding off the async runtime to avoid blocking drop panics
        let encoding = spawn_blocking(|| load_harmony_encoding(HarmonyEncodingName::HarmonyGptOss))
            .await
            .map_err(|join_err| {
                let formatted_error = error_display::format_llm_error(
                    "OpenAI",
                    &format!("Failed to load harmony encoding (task join): {}", join_err),
                );
                provider::LLMError::Provider {
                    message: formatted_error,
                    metadata: None,
                }
            })?
            .map_err(|e| {
                let formatted_error = error_display::format_llm_error(
                    "OpenAI",
                    &format!("Failed to load harmony encoding: {}", e),
                );
                provider::LLMError::Provider {
                    message: formatted_error,
                    metadata: None,
                }
            })?;

        // Convert to harmony conversation
        let conversation = self.convert_to_harmony_conversation(&request)?;

        // Render conversation for completion
        let prompt_tokens = encoding
            .render_conversation_for_completion(&conversation, HarmonyRole::Assistant, None)
            .map_err(|e| {
                let formatted_error = error_display::format_llm_error(
                    "OpenAI",
                    &format!("Failed to render conversation: {}", e),
                );
                provider::LLMError::Provider {
                    message: formatted_error,
                    metadata: None,
                }
            })?;

        // Send tokens to inference server
        let completion_tokens = self
            .send_harmony_tokens_to_inference_server(&prompt_tokens, request.temperature)
            .await?;

        // Parse completion tokens back into messages
        let parsed_messages = encoding
            .parse_messages_from_completion_tokens(
                completion_tokens.clone(),
                Some(HarmonyRole::Assistant),
            )
            .map_err(|e| {
                let formatted_error = error_display::format_llm_error(
                    "OpenAI",
                    &format!("Failed to parse completion tokens: {}", e),
                );
                provider::LLMError::Provider {
                    message: formatted_error,
                    metadata: None,
                }
            })?;

        // Extract content from parsed messages
        let mut content = None;
        let mut tool_calls = Vec::with_capacity(8); // Typical tool call count in harmony responses

        let extract_text_content = |parts: &[HarmonyContent]| -> Option<String> {
            let text = parts
                .iter()
                .filter_map(|part| match part {
                    HarmonyContent::Text(text_part) => Some(text_part.text.clone()),
                    _ => None,
                })
                .collect::<String>();

            if text.is_empty() { None } else { Some(text) }
        };

        let normalize_json_arguments = |raw: String| -> String {
            match serde_json::from_str::<Value>(&raw) {
                Ok(parsed) => parsed.to_string(),
                Err(_) => raw,
            }
        };

        for message in parsed_messages {
            match message.author.role {
                HarmonyRole::Assistant => {
                    if let Some(channel) = &message.channel {
                        match channel.as_str() {
                            "final" => {
                                // This is the final response content
                                // Extract text from content Vec<Content>
                                if let Some(text_content) = extract_text_content(&message.content) {
                                    content = Some(text_content);
                                }
                            }
                            "commentary" => {
                                // Check if this is a tool call
                                if let Some(recipient) = &message.recipient {
                                    if recipient.starts_with("functions.") {
                                        // This is a tool call with functions. prefix
                                        let function_name = recipient
                                            .strip_prefix("functions.")
                                            .unwrap_or(recipient);
                                        let arguments = extract_text_content(&message.content)
                                            .map(normalize_json_arguments)
                                            .unwrap_or_else(|| "{}".to_owned());

                                        tool_calls.push(provider::ToolCall::function(
                                            format!("call_{}", tool_calls.len()),
                                            function_name.to_string(),
                                            arguments,
                                        ));
                                    } else {
                                        // Check if this is a harmony format tool call (to=tool_name)
                                        // The recipient might be the tool name directly
                                        let tool_name = Self::parse_harmony_tool_name(recipient);
                                        if !tool_name.is_empty() {
                                            let arguments = extract_text_content(&message.content)
                                                .map(normalize_json_arguments)
                                                .unwrap_or_else(|| "{}".to_string());

                                            tool_calls.push(provider::ToolCall::function(
                                                format!("call_{}", tool_calls.len()),
                                                tool_name,
                                                arguments,
                                            ));
                                        }
                                    }
                                } else {
                                    // Check if the content itself contains harmony tool call format
                                    if let Some(text_content) =
                                        extract_text_content(&message.content)
                                    {
                                        if let Some((tool_name, args)) =
                                            Self::parse_harmony_tool_call_from_text(&text_content)
                                        {
                                            let arguments = serde_json::to_string(&args)
                                                .unwrap_or_else(|_| "{}".to_string());

                                            tool_calls.push(provider::ToolCall::function(
                                                format!("call_{}", tool_calls.len()),
                                                tool_name,
                                                arguments,
                                            ));
                                        }
                                    }
                                }
                            }
                            _ => {} // Other channels like "analysis" are for reasoning
                        }
                    }
                }
                _ => {} // Skip other message types for now
            }
        }

        let tool_calls = if tool_calls.is_empty() {
            None
        } else {
            Some(tool_calls)
        };

        Ok(provider::LLMResponse {
            content,
            tool_calls,
            usage: Some(crate::llm::provider::Usage {
                prompt_tokens: prompt_tokens.len().try_into().unwrap_or(u32::MAX),
                completion_tokens: completion_tokens.len().try_into().unwrap_or(u32::MAX),
                total_tokens: (prompt_tokens.len() + completion_tokens.len())
                    .try_into()
                    .unwrap_or(u32::MAX),
                cached_prompt_tokens: None,
                cache_creation_tokens: None,
                cache_read_tokens: None,
            }),
            finish_reason: crate::llm::provider::FinishReason::Stop,
            reasoning: None,
            reasoning_details: None,
            tool_references: Vec::new(),
            request_id: None,
            organization_id: None,
        })
    }
}

impl OpenAIProvider {
    /// Sends harmony-formatted tokens to an inference server for GPT-OSS models.
    ///
    /// This method handles the HTTP communication with inference servers that support
    /// harmony-formatted token inputs (such as vLLM or Transformers serve).
    ///
    /// # Configuration
    ///
    /// Set the `HARMONY_INFERENCE_SERVER_URL` environment variable to configure
    /// the inference server endpoint. Defaults to `http://localhost:8000` for local vLLM.
    ///
    /// # Supported Servers
    ///
    /// - **vLLM**: Set `HARMONY_INFERENCE_SERVER_URL=http://localhost:8000`
    /// - **Transformers serve**: Configure appropriate endpoint URL
    /// - **Custom servers**: Any server accepting `{"prompt_token_ids": [...], "max_tokens": N, ...}`
    ///
    /// # Example
    ///
    /// ```bash
    /// export HARMONY_INFERENCE_SERVER_URL=http://localhost:8000
    /// vtcode ask --model openai/gpt-oss-20b "Explain quantum computing"
    /// ```
    async fn send_harmony_tokens_to_inference_server(
        &self,
        tokens: &[u32],
        temperature: Option<f32>,
    ) -> Result<Vec<u32>, provider::LLMError> {
        // Get harmony inference server URL from environment variable
        // Default to localhost vLLM server if not configured
        let server_url = std::env::var("HARMONY_INFERENCE_SERVER_URL")
            .unwrap_or_else(|_| "http://localhost:8000".to_owned());

        // Load harmony encoding to get stop tokens
        let encoding = load_harmony_encoding(HarmonyEncodingName::HarmonyGptOss).map_err(|e| {
            let formatted_error = error_display::format_llm_error(
                "OpenAI",
                &format!("Failed to load harmony encoding for stop tokens: {}", e),
            );
            provider::LLMError::Provider {
                message: formatted_error,
                metadata: None,
            }
        })?;

        let stop_token_ids = encoding.stop_tokens_for_assistant_actions().map_err(|e| {
            let formatted_error = error_display::format_llm_error(
                "OpenAI",
                &format!("Failed to get stop tokens: {}", e),
            );
            provider::LLMError::Provider {
                message: formatted_error,
                metadata: None,
            }
        })?;

        // Convert HashSet to Vec for JSON serialization
        let stop_token_ids_vec: Vec<u32> = stop_token_ids.into_iter().collect();

        // Prepare request body for vLLM-style inference server
        let request_body = json!({
            "prompt_token_ids": tokens,
            "temperature": temperature.unwrap_or(0.7),
            "stop_token_ids": stop_token_ids_vec,
            // Additional parameters that might be needed
            "stream": false,
            "logprobs": null,
            "echo": false
        });

        // Send HTTP request to inference server
        let response = headers::apply_json_content_type(
            self.http_client
                .post(format!("{}/generate", server_url)),
        )
            .json(&request_body)
            .send()
            .await
            .map_err(|e| {
                let formatted_error = error_display::format_llm_error(
                    "OpenAI",
                    &format!(
                        "Failed to send request to harmony inference server at {}: {}",
                        server_url, e
                    ),
                );
                provider::LLMError::Network {
                    message: formatted_error,
                    metadata: None,
                }
            })?;

        // Check response status
        if !response.status().is_success() {
            let status = response.status();
            let headers = response.headers().clone();
            let error_text = response.text().await.unwrap_or_default();
            let formatted_error = error_display::format_llm_error(
                "OpenAI",
                &format_openai_error(
                    status,
                    &error_text,
                    &headers,
                    "Harmony inference server error",
                ),
            );
            return Err(provider::LLMError::Provider {
                message: formatted_error,
                metadata: None,
            });
        }

        // Parse response JSON
        let response_json: Value = response.json().await.map_err(|e| {
            let formatted_error = error_display::format_llm_error(
                "OpenAI",
                &format!("Failed to parse harmony inference response: {}", e),
            );
            provider::LLMError::Provider {
                message: formatted_error,
                metadata: None,
            }
        })?;

        // Extract completion tokens from response
        // vLLM returns tokens in different formats depending on the response structure
        let completion_tokens =
            if let Some(tokens_array) = response_json.get("tokens").and_then(|t| t.as_array()) {
                // Direct tokens array
                tokens_array
                    .iter()
                    .filter_map(|v| v.as_u64().and_then(|u| u32::try_from(u).ok()))
                    .collect::<Vec<u32>>()
            } else if let Some(outputs) = response_json.get("outputs").and_then(|o| o.as_array()) {
                // vLLM nested outputs format
                outputs
                    .first()
                    .and_then(|output| output.get("token_ids"))
                    .and_then(|token_ids| token_ids.as_array())
                    .map(|token_ids| {
                        token_ids
                            .iter()
                            .filter_map(|v| v.as_u64().and_then(|u| u32::try_from(u).ok()))
                            .collect::<Vec<u32>>()
                    })
                    .unwrap_or_default()
            } else {
                // Fallback: try to find tokens in any nested structure
                let mut found_tokens = Vec::new();
                if let Some(obj) = response_json.as_object() {
                    for (_, value) in obj {
                        if let Some(arr) = value.as_array() {
                            if arr.iter().all(|v| v.is_u64()) {
                                found_tokens = arr
                                    .iter()
                                    .filter_map(|v| v.as_u64().and_then(|u| u32::try_from(u).ok()))
                                    .collect();
                                break;
                            }
                        }
                    }
                }
                found_tokens
            };

        if completion_tokens.is_empty() {
            let formatted_error = error_display::format_llm_error(
                "OpenAI",
                "No completion tokens received from harmony inference server",
            );
            return Err(provider::LLMError::Provider {
                message: formatted_error,
                metadata: None,
            });
        }

        Ok(completion_tokens)
    }

    /// Parse harmony tool name from recipient or tool reference
    fn parse_harmony_tool_name(recipient: &str) -> String {
        harmony::parse_harmony_tool_name(recipient)
    }

    /// Parse harmony tool call from raw text content
    fn parse_harmony_tool_call_from_text(text: &str) -> Option<(String, serde_json::Value)> {
        harmony::parse_harmony_tool_call_from_text(text)
    }
}

#[cfg(test)]
mod tests;

#[async_trait]
impl provider::LLMProvider for OpenAIProvider {
    fn name(&self) -> &str {
        "openai"
    }

    fn supports_streaming(&self) -> bool {
        // OpenAI requires ID verification for GPT-5 models, so we must disable streaming
        if matches!(
            self.model.as_ref(),
            models::openai::GPT_5
                | models::openai::GPT_5_CODEX
                | models::openai::GPT_5_MINI
                | models::openai::GPT_5_NANO
        ) {
            return false;
        }

        // Even if Responses API is disabled (e.g., Hugging Face router), we can stream via Chat Completions.
        true
    }

    fn supports_reasoning(&self, model: &str) -> bool {
        let requested = if model.trim().is_empty() {
            self.model.as_ref()
        } else {
            model
        };

        models::openai::REASONING_MODELS.contains(&requested)
    }

    fn supports_reasoning_effort(&self, model: &str) -> bool {
        let requested = if model.trim().is_empty() {
            self.model.as_ref()
        } else {
            model
        };
        models::openai::REASONING_MODELS
            .iter()
            .any(|candidate| *candidate == requested)
    }

    fn supports_tools(&self, model: &str) -> bool {
        let requested = if model.trim().is_empty() {
            self.model.as_ref()
        } else {
            model
        };

        !models::openai::TOOL_UNAVAILABLE_MODELS.contains(&requested)
    }

    async fn stream(
        &self,
        mut request: provider::LLMRequest,
    ) -> Result<provider::LLMStream, provider::LLMError> {
        if request.model.trim().is_empty() {
            request.model = self.model.to_string();
        }

        if !self.supports_parallel_tool_config(&request.model) {
            request.parallel_tool_config = None;
        }

        let responses_state = self.responses_api_state(&request.model);

        let prefer_responses_stream = matches!(responses_state, ResponsesApiState::Required)
            || (matches!(responses_state, ResponsesApiState::Allowed)
                && request.tools.as_ref().is_none_or(Vec::is_empty));

        if !prefer_responses_stream {
            #[cfg(debug_assertions)]
            debug!(
                target = "vtcode::llm::openai",
                model = %request.model,
                "Using standard Chat Completions for streaming"
            );
            let mut openai_request = self.convert_to_openai_format(&request)?;
            openai_request["stream"] = Value::Bool(true);
            // Request usage stats in the stream (compatible with newer OpenAI models)
            // Note: Some proxies do not support stream_options and will return 400.
            let is_native_openai = self.base_url.contains("api.openai.com");
            if is_native_openai {
                openai_request["stream_options"] = json!({ "include_usage": true });
            }
            let url = format!("{}/chat/completions", self.base_url);

            let response = self
                .authorize(self.http_client.post(&url))
                .json(&openai_request)
                .send()
                .await
                .map_err(|e| {
                    let formatted_error =
                        error_display::format_llm_error("OpenAI", &format!("Network error: {}", e));
                    provider::LLMError::Network {
                        message: formatted_error,
                        metadata: None,
                    }
                })?;

            if !response.status().is_success() {
                let status = response.status();
                let error_text = response.text().await.unwrap_or_default();

                if is_rate_limit_error(status.as_u16(), &error_text) {
                    return Err(provider::LLMError::RateLimit { metadata: None });
                }

                let formatted_error = error_display::format_llm_error(
                    "OpenAI",
                    &format!("HTTP {}: {}", status, error_text),
                );
                return Err(provider::LLMError::Provider {
                    message: formatted_error,
                    metadata: None,
                });
            }

            return Ok(stream_decoder::create_chat_stream(response));
        }

        let include_metrics =
            self.prompt_cache_enabled && self.prompt_cache_settings.surface_metrics;

        let mut openai_request = self.convert_to_openai_responses_format(&request)?;

        openai_request["stream"] = Value::Bool(true);
        #[cfg(debug_assertions)]
        let debug_model = Some(request.model.clone());
        #[cfg(not(debug_assertions))]
        let debug_model: Option<String> = None;
        #[cfg(debug_assertions)]
        let request_timer = Some(std::time::Instant::now());
        #[cfg(not(debug_assertions))]
        let request_timer: Option<std::time::Instant> = None;
        #[cfg(debug_assertions)]
        {
            let tool_count = request.tools.as_ref().map_or(0, |tools| tools.len());
            debug!(
                target = "vtcode::llm::openai",
                model = %request.model,
                stream = true,
                messages = request.messages.len(),
                tools = tool_count,
                "Dispatching streaming Responses request"
            );
        }

        let url = format!("{}/responses", self.base_url);

        let response = headers::apply_responses_beta(
            self.authorize(self.http_client.post(&url)),
        )
            .json(&openai_request)
            .send()
            .await
            .map_err(|e| {
                let formatted_error =
                    error_display::format_llm_error("OpenAI", &format!("Network error: {}", e));
                provider::LLMError::Network {
                    message: formatted_error,
                    metadata: None,
                }
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let headers = response.headers().clone();
            let error_text = response.text().await.unwrap_or_default();

            if matches!(responses_state, ResponsesApiState::Allowed)
                && is_responses_api_unsupported(status, &error_text)
            {
                #[cfg(debug_assertions)]
                debug!(
                    target = "vtcode::llm::openai",
                    model = %request.model,
                    "Responses API unsupported; falling back to Chat Completions for streaming"
                );
                self.set_responses_api_state(&request.model, ResponsesApiState::Disabled);
                request.stream = true;
                return self.stream(request).await;
            }

            if is_rate_limit_error(status.as_u16(), &error_text) {
                return Err(provider::LLMError::RateLimit { metadata: None });
            }

            if is_model_not_found(status, &error_text) {
                if let Some(fallback_model) = fallback_model_if_not_found(&request.model)
                    && fallback_model != request.model
                {
                    #[cfg(debug_assertions)]
                    debug!(
                        target = "vtcode::llm::openai",
                        requested = %request.model,
                        fallback = %fallback_model,
                        "Model not found while streaming; retrying with fallback"
                    );
                    let mut retry_request = request.clone();
                    retry_request.model = fallback_model;
                    retry_request.stream = false;
                    let response = self.generate(retry_request).await?;
                    let stream = try_stream! {
                        yield provider::LLMStreamEvent::Completed { response };
                    };
                    return Ok(Box::pin(stream));
                }
                let formatted_error = error_display::format_llm_error(
                    "OpenAI",
                    &format_openai_error(status, &error_text, &headers, "Model not available"),
                );
                return Err(provider::LLMError::Provider {
                    message: formatted_error,
                    metadata: None,
                });
            }

            let formatted_error = error_display::format_llm_error(
                "OpenAI",
                &format_openai_error(status, &error_text, &headers, "Responses API error"),
            );
            return Err(provider::LLMError::Provider {
                message: formatted_error,
                metadata: None,
            });
        }

        #[cfg(debug_assertions)]
        {
            if let Some(ref debug_model) = debug_model {
                if let Some(request_timer) = request_timer.as_ref() {
                    debug!(
                        target = "vtcode::llm::openai",
                        model = %debug_model,
                        status = %response.status(),
                        handshake_ms = request_timer.elapsed().as_millis(),
                        "Streaming response headers received"
                    );
                }
            }
        }

        Ok(stream_decoder::create_responses_stream(
            response,
            include_metrics,
            debug_model,
            request_timer,
        ))
    }

    async fn generate(
        &self,
        request: provider::LLMRequest,
    ) -> Result<provider::LLMResponse, provider::LLMError> {
        let mut request = request;

        if request.model.trim().is_empty() {
            request.model = self.model.to_string();
        }

        if !self.supports_parallel_tool_config(&request.model) {
            request.parallel_tool_config = None;
        }

        // Check if this is a harmony model (GPT-OSS)
        if Self::uses_harmony(&request.model) {
            return self.generate_with_harmony(request).await;
        }

        let responses_state = self.responses_api_state(&request.model);
        let attempt_responses = !matches!(responses_state, ResponsesApiState::Disabled)
            && (matches!(responses_state, ResponsesApiState::Required)
                || request.tools.as_ref().is_none_or(Vec::is_empty));
        #[cfg(debug_assertions)]
        let request_timer = Instant::now();
        #[cfg(debug_assertions)]
        {
            let tool_count = request.tools.as_ref().map_or(0, |tools| tools.len());
            debug!(
                target = "vtcode::llm::openai",
                model = %request.model,
                responses_api = attempt_responses,
                messages = request.messages.len(),
                tools = tool_count,
                "Dispatching non-streaming OpenAI request"
            );
        }

        if attempt_responses {
            let openai_request = self.convert_to_openai_responses_format(&request)?;
            let url = format!("{}/responses", self.base_url);

            let response = headers::apply_responses_beta(
                self.authorize(self.http_client.post(&url)),
            )
                .json(&openai_request)
                .send()
                .await
                .map_err(|e| {
                    let formatted_error =
                        error_display::format_llm_error("OpenAI", &format!("Network error: {}", e));
                    provider::LLMError::Network {
                        message: formatted_error,
                        metadata: None,
                    }
                })?;

            if !response.status().is_success() {
                let status = response.status();
                let headers = response.headers().clone();
                let error_text = response.text().await.unwrap_or_default();

                if matches!(responses_state, ResponsesApiState::Allowed)
                    && is_responses_api_unsupported(status, &error_text)
                {
                    #[cfg(debug_assertions)]
                    debug!(
                        target = "vtcode::llm::openai",
                        model = %request.model,
                        "Responses API unsupported; falling back to Chat Completions"
                    );
                    self.set_responses_api_state(&request.model, ResponsesApiState::Disabled);
                    return self.generate(request).await;
                } else if is_rate_limit_error(status.as_u16(), &error_text) {
                    return Err(provider::LLMError::RateLimit { metadata: None });
                } else if is_model_not_found(status, &error_text) {
                    if let Some(fallback_model) = fallback_model_if_not_found(&request.model) {
                        if fallback_model != request.model {
                            #[cfg(debug_assertions)]
                            debug!(
                                target = "vtcode::llm::openai",
                                requested = %request.model,
                                fallback = %fallback_model,
                                "Model not found; retrying with fallback"
                            );
                            let mut retry_request = request.clone();
                            retry_request.model = fallback_model;
                            let retry_openai =
                                self.convert_to_openai_responses_format(&retry_request)?;
                            let retry_response = headers::apply_responses_beta(
                                self.authorize(self.http_client.post(&url)),
                            )
                                .json(&retry_openai)
                                .send()
                                .await
                                .map_err(|e| {
                                    let formatted_error = error_display::format_llm_error(
                                        "OpenAI",
                                        &format!("Network error: {}", e),
                                    );
                                    provider::LLMError::Network {
                                        message: formatted_error,
                                        metadata: None,
                                    }
                                })?;
                            if retry_response.status().is_success() {
                                let openai_response: Value =
                                    retry_response.json().await.map_err(|e| {
                                        let formatted_error = error_display::format_llm_error(
                                            "OpenAI",
                                            &format!("Failed to parse response: {}", e),
                                        );
                                        provider::LLMError::Provider {
                                            message: formatted_error,
                                            metadata: None,
                                        }
                                    })?;
                                let response =
                                    self.parse_openai_responses_response(openai_response)?;
                                return Ok(response);
                            }
                        }
                    }
                    let formatted_error = error_display::format_llm_error(
                        "OpenAI",
                        &format_openai_error(status, &error_text, &headers, "Model not available"),
                    );
                    return Err(provider::LLMError::Provider {
                        message: formatted_error,
                        metadata: None,
                    });
                } else {
                    let formatted_error = error_display::format_llm_error(
                        "OpenAI",
                        &format_openai_error(status, &error_text, &headers, "Responses API error"),
                    );
                    return Err(provider::LLMError::Provider {
                        message: formatted_error,
                        metadata: None,
                    });
                }
            } else {
                let openai_response: Value = response.json().await.map_err(|e| {
                    let formatted_error = error_display::format_llm_error(
                        "OpenAI",
                        &format!("Failed to parse response: {}", e),
                    );
                    provider::LLMError::Provider {
                        message: formatted_error,
                        metadata: None,
                    }
                })?;

                let response = self.parse_openai_responses_response(openai_response)?;
                #[cfg(debug_assertions)]
                {
                    let content_len = response.content.as_ref().map_or(0, |c| c.len());
                    debug!(
                        target = "vtcode::llm::openai",
                        model = %request.model,
                        responses_api = true,
                        elapsed_ms = request_timer.elapsed().as_millis(),
                        content_len = content_len,
                        finish_reason = ?response.finish_reason,
                        "Completed non-streaming OpenAI request"
                    );
                }
                return Ok(response);
            }
        } else {
            #[cfg(debug_assertions)]
            debug!(
                target = "vtcode::llm::openai",
                model = %request.model,
                "Skipping Responses API (disabled); using Chat Completions"
            );
        }

        let openai_request = self.convert_to_openai_format(&request)?;
        let url = format!("{}/chat/completions", self.base_url);

        let response = self
            .authorize(self.http_client.post(&url))
            .json(&openai_request)
            .send()
            .await
            .map_err(|e| {
                let formatted_error =
                    error_display::format_llm_error("OpenAI", &format!("Network error: {}", e));
                provider::LLMError::Network {
                    message: formatted_error,
                    metadata: None,
                }
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();

            if is_rate_limit_error(status.as_u16(), &error_text) {
                return Err(provider::LLMError::RateLimit { metadata: None });
            }

            let formatted_error = error_display::format_llm_error(
                "OpenAI",
                &format!("HTTP {}: {}", status, error_text),
            );
            return Err(provider::LLMError::Provider {
                message: formatted_error,
                metadata: None,
            });
        }

        let openai_response: Value = response.json().await.map_err(|e| {
            let formatted_error = error_display::format_llm_error(
                "OpenAI",
                &format!("Failed to parse response: {}", e),
            );
            provider::LLMError::Provider {
                message: formatted_error,
                metadata: None,
            }
        })?;

        let response = self.parse_openai_response(openai_response)?;
        #[cfg(debug_assertions)]
        {
            let content_len = response.content.as_ref().map_or(0, |c| c.len());
            debug!(
                target = "vtcode::llm::openai",
                model = %request.model,
                responses_api = false,
                elapsed_ms = request_timer.elapsed().as_millis(),
                content_len = content_len,
                finish_reason = ?response.finish_reason,
                "Completed non-streaming OpenAI request"
            );
        }
        Ok(response)
    }

    fn supported_models(&self) -> Vec<String> {
        models::openai::SUPPORTED_MODELS
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    fn validate_request(&self, request: &provider::LLMRequest) -> Result<(), provider::LLMError> {
        if request.messages.is_empty() {
            let formatted_error =
                error_display::format_llm_error("OpenAI", "Messages cannot be empty");
            return Err(provider::LLMError::InvalidRequest {
                message: formatted_error,
                metadata: None,
            });
        }

        if !models::openai::SUPPORTED_MODELS
            .iter()
            .any(|m| *m == request.model)
        {
            let formatted_error = error_display::format_llm_error(
                "OpenAI",
                &format!("Unsupported model: {}", request.model),
            );
            return Err(provider::LLMError::InvalidRequest {
                message: formatted_error,
                metadata: None,
            });
        }

        for message in &request.messages {
            if let Err(err) = message.validate_for_provider("openai") {
                let formatted = error_display::format_llm_error("OpenAI", &err);
                return Err(provider::LLMError::InvalidRequest {
                    message: formatted,
                    metadata: None,
                });
            }
        }

        Ok(())
    }
}

#[async_trait]
impl LLMClient for OpenAIProvider {
    async fn generate(
        &mut self,
        prompt: &str,
    ) -> Result<llm_types::LLMResponse, provider::LLMError> {
        let request = self.parse_client_prompt(prompt);
        let request_model = request.model.to_string();
        let response = provider::LLMProvider::generate(self, request).await?;

        Ok(llm_types::LLMResponse {
            content: response.content.unwrap_or_default(),
            model: request_model,
            usage: response
                .usage
                .map(crate::llm::providers::common::convert_usage_to_llm_types),
            reasoning: response.reasoning,
            reasoning_details: response.reasoning_details,
            request_id: response.request_id,
            organization_id: response.organization_id,
        })
    }

    fn backend_kind(&self) -> llm_types::BackendKind {
        llm_types::BackendKind::OpenAI
    }

    fn model_id(&self) -> &str {
        &self.model
    }
}
