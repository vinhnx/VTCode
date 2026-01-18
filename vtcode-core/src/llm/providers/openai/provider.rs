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
use crate::config::models::Provider as ModelProvider;
use crate::config::types::ReasoningEffortLevel;
use crate::llm::client::LLMClient;
use crate::llm::error_display;
use crate::llm::provider::{self, LLMProvider};
use crate::llm::providers::tag_sanitizer::TagStreamSanitizer;
use crate::llm::rig_adapter::reasoning_parameters_for;
use crate::llm::types as llm_types;
use async_stream::try_stream;
use async_trait::async_trait;
use futures::StreamExt;
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
use super::responses_api::{
    build_codex_responses_payload, build_standard_responses_payload, parse_responses_payload,
};
use super::streaming::OpenAIStreamTelemetry;
use super::types::{MAX_COMPLETION_TOKENS_FIELD, OpenAIResponsesPayload, ResponsesApiState};

use super::super::{
    ReasoningBuffer,
    common::{
        extract_prompt_cache_settings, override_base_url, parse_client_prompt_common, resolve_model,
    },
    extract_reasoning_trace,
    shared::{
        StreamAssemblyError, StreamTelemetry, append_reasoning_segments, extract_data_payload,
        find_sse_boundary,
    },
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
    fn serialize_tools(&self, tools: &[provider::ToolDefinition]) -> Option<Value> {
        if tools.is_empty() {
            return None;
        }

        let mut seen_names = std::collections::HashSet::new();
        let serialized_tools = tools
            .iter()
            .filter_map(|tool| {
                // Use function name when present, otherwise tool type to dedupe apply_patch duplicates
                let canonical_name = tool
                    .function
                    .as_ref()
                    .map(|f| f.name.as_str())
                    .unwrap_or(tool.tool_type.as_str());
                if !seen_names.insert(canonical_name.to_string()) {
                    return None;
                }

                Some(match tool.tool_type.as_str() {
                    "function" => {
                        let func = tool.function.as_ref()?;
                        let name = &func.name;
                        let description = &func.description;
                        let parameters = &func.parameters;

                        json!({
                            "type": &tool.tool_type,
                            "name": name,
                            "description": description,
                            "parameters": parameters,
                            "function": {
                                "name": name,
                                "description": description,
                                "parameters": parameters,
                            }
                        })
                    }
                    "apply_patch" | "shell" | "custom" | "grammar" => {
                        // For GPT-5.1 native tool types and GPT-5 features, use direct serialization
                        json!(tool)
                    }
                    _ => {
                        // Fallback for unknown tool types
                        json!(tool)
                    }
                })
            })
            .collect::<Vec<Value>>();

        Some(Value::Array(serialized_tools))
    }

    /// Serialize tools for the Responses API format (used by GPT-5.1 Codex models)
    /// Per OpenAI documentation (see GIT_TOOL example):
    /// - Function tools use FLAT format: {"type": "function", "name": "...", "description": "...", "parameters": {...}}
    /// - Built-in tools like apply_patch use: {"type": "apply_patch"} (no other fields)
    ///   Note: Cannot have both built-in apply_patch AND a function named "apply_patch"
    fn serialize_tools_for_responses(tools: &[provider::ToolDefinition]) -> Option<Value> {
        if tools.is_empty() {
            return None;
        }

        let mut seen_names = std::collections::HashSet::new();
        let serialized_tools = tools
            .iter()
            .filter_map(|tool| {
                match tool.tool_type.as_str() {
                    "function" => {
                        let func = tool.function.as_ref()?;
                        if !seen_names.insert(func.name.clone()) {
                            return None;
                        }
                        // GPT-5.1 Codex uses FLAT function format (name at top level)
                        Some(json!({
                            "type": "function",
                            "name": &func.name,
                            "description": &func.description,
                            "parameters": &func.parameters
                        }))
                    }
                    "apply_patch" => {
                        // GPT-5.2 Responses API does not accept apply_patch type; serialize as function
                        let name = tool
                            .function
                            .as_ref()
                            .map(|f| f.name.as_str())
                            .unwrap_or("apply_patch");
                        if !seen_names.insert(name.to_string()) {
                            return None;
                        }
                        if let Some(func) = tool.function.as_ref() {
                            Some(json!({
                                "type": "function",
                                "name": &func.name,
                                "description": &func.description,
                                "parameters": &func.parameters
                            }))
                        } else {
                            Some(json!({
                                "type": "function",
                                "name": "apply_patch",
                                "description": "Apply a unified diff patch to a file.",
                                "parameters": json!({
                                    "type": "object",
                                    "properties": {
                                        "file_path": { "type": "string" },
                                        "patch": { "type": "string" }
                                    },
                                    "required": ["file_path", "patch"]
                                })
                            }))
                        }
                    }
                    "shell" => {
                        // For shell, treat as function tool with flat format
                        tool.function.as_ref().map(|func| json!({
                                "type": "function",
                                "name": &func.name,
                                "description": &func.description,
                                "parameters": &func.parameters
                            }))
                    }
                    "custom" => {
                        // GPT-5 custom tool - use custom format
                        if let Some(func) = &tool.function {
                            if !seen_names.insert(func.name.clone()) {
                                return None;
                            }
                            Some(json!({
                                "type": "custom",
                                "name": &func.name,
                                "description": &func.description,
                                "format": func.parameters.get("format")
                            }))
                        } else {
                            None
                        }
                    }
                    "grammar" => {
                        let name = tool
                            .function
                            .as_ref()
                            .map(|f| f.name.as_str())
                            .unwrap_or("apply_patch_grammar");
                        if !seen_names.insert(name.to_string()) {
                            return None;
                        }
                        tool.grammar.as_ref().map(|grammar| json!({
                                "type": "custom",
                                "name": "apply_patch_grammar",
                                "description": "Use the `apply_patch` tool to edit files. This is a FREEFORM tool.",
                                "format": {
                                    "type": "grammar",
                                    "syntax": &grammar.syntax,
                                    "definition": &grammar.definition
                                }
                            }))
                    }
                    _ => {
                        // Unknown tool type - treat as function tool with flat format
                        if let Some(func) = &tool.function {
                            if !seen_names.insert(func.name.clone()) {
                                return None;
                            }
                            Some(json!({
                                "type": "function",
                                "name": &func.name,
                                "description": &func.description,
                                "parameters": &func.parameters
                            }))
                        } else {
                            None
                        }
                    }
                }
            })
            .collect::<Vec<Value>>();

        Some(Value::Array(serialized_tools))
    }

    fn is_gpt5_codex_model(model: &str) -> bool {
        model == models::openai::GPT_5_CODEX
            || model == models::openai::GPT_5_1_CODEX
            || model == models::openai::GPT_5_1_CODEX_MAX
    }

    fn is_responses_api_model(model: &str) -> bool {
        models::openai::RESPONSES_API_MODELS.contains(&model)
    }

    fn normalized_harmony_model(model: &str) -> String {
        let trimmed = model.trim();
        if trimmed.is_empty() {
            return String::new();
        }

        let without_provider = trimmed.rsplit('/').next().unwrap_or(trimmed);
        let without_annotation = without_provider
            .split('@')
            .next()
            .unwrap_or(without_provider);
        let without_variant = without_annotation
            .split(':')
            .next()
            .unwrap_or(without_annotation);

        without_variant.to_ascii_lowercase()
    }

    fn uses_harmony(model: &str) -> bool {
        let normalized = Self::normalized_harmony_model(model);
        if normalized.is_empty() {
            return false;
        }

        models::openai::HARMONY_MODELS
            .iter()
            .any(|candidate| *candidate == normalized)
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
        parse_client_prompt_common(prompt, &self.model, |value| self.parse_chat_request(value))
    }

    fn parse_chat_request(&self, value: &Value) -> Option<provider::LLMRequest> {
        let messages_value = value.get("messages")?.as_array()?;
        let mut system_prompt = None;
        let mut messages = Vec::with_capacity(messages_value.len());

        for entry in messages_value {
            let role = entry
                .get("role")
                .and_then(|r| r.as_str())
                .unwrap_or(crate::config::constants::message_roles::USER);
            let content = entry.get("content");
            let text_content = content.map(Self::extract_content_text).unwrap_or_default();

            match role {
                "system" => {
                    if system_prompt.is_none() && !text_content.is_empty() {
                        system_prompt = Some(text_content);
                    }
                }
                "assistant" => {
                    let tool_calls = entry
                        .get("tool_calls")
                        .and_then(|tc| tc.as_array())
                        .map(|calls| {
                            calls
                                .iter()
                                .filter_map(|call| {
                                    let id = call.get("id").and_then(|v| v.as_str())?;
                                    let function = call.get("function")?;
                                    let name = function.get("name").and_then(|v| v.as_str())?;
                                    let arguments = function.get("arguments");
                                    let serialized = arguments.map_or("{}".to_owned(), |value| {
                                        if value.is_string() {
                                            value.as_str().unwrap_or("").to_string()
                                        } else {
                                            value.to_string()
                                        }
                                    });
                                    Some(provider::ToolCall::function(
                                        id.to_string(),
                                        name.to_string(),
                                        serialized,
                                    ))
                                })
                                .collect::<Vec<_>>()
                        })
                        .filter(|calls| !calls.is_empty());

                    let message = if let Some(calls) = tool_calls {
                        provider::Message::assistant_with_tools(text_content, calls)
                    } else {
                        provider::Message::assistant(text_content)
                    };
                    messages.push(message);
                }
                "tool" => {
                    let tool_call_id = entry
                        .get("tool_call_id")
                        .and_then(|id| id.as_str())
                        .map(|s| s.to_string());
                    let content_value = entry
                        .get("content")
                        .map(|value| {
                            if text_content.is_empty() {
                                value.to_string()
                            } else {
                                text_content.clone()
                            }
                        })
                        .unwrap_or_else(|| text_content.clone());
                    messages.push(if let Some(id) = tool_call_id {
                        provider::Message::tool_response(id, content_value)
                    } else {
                        // Fallback: create a tool message without tool_call_id (invalid but graceful)
                        provider::Message {
                            role: provider::MessageRole::Tool,
                            content: provider::MessageContent::Text(content_value),
                            reasoning: None,
                            reasoning_details: None,
                            tool_calls: None,
                            tool_call_id: None,
                            origin_tool: None,
                        }
                    });
                }
                _ => {
                    messages.push(provider::Message::user(text_content));
                }
            }
        }

        if messages.is_empty() {
            return None;
        }

        let tools = value.get("tools").and_then(|tools_value| {
            let tools_array = tools_value.as_array()?;
            let converted: Vec<_> = tools_array
                .iter()
                .filter_map(|tool| {
                    let function = tool.get("function")?;
                    let name = function.get("name").and_then(|n| n.as_str())?;
                    let description = function
                        .get("description")
                        .and_then(|d| d.as_str())
                        .unwrap_or("")
                        .to_string();
                    let parameters = function
                        .get("parameters")
                        .cloned()
                        .unwrap_or_else(|| json!({}));
                    Some(provider::ToolDefinition::function(
                        name.to_string(),
                        description,
                        parameters,
                    ))
                })
                .collect();

            if converted.is_empty() {
                None
            } else {
                Some(converted)
            }
        });
        let temperature = value
            .get("temperature")
            .and_then(|v| v.as_f64())
            .map(|v| v as f32);
        let stream = value
            .get("stream")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let tool_choice = value.get("tool_choice").and_then(Self::parse_tool_choice);
        let _parallel_tool_calls = value.get("parallel_tool_calls").and_then(|v| v.as_bool());
        let _reasoning_effort = value
            .get("reasoning_effort")
            .and_then(|v| v.as_str())
            .and_then(ReasoningEffortLevel::parse)
            .or_else(|| {
                value
                    .get("reasoning")
                    .and_then(|r| r.get("effort"))
                    .and_then(|effort| effort.as_str())
                    .and_then(ReasoningEffortLevel::parse)
            });

        let model = value
            .get("model")
            .and_then(|m| m.as_str())
            .unwrap_or(&self.model)
            .to_string();

        Some(provider::LLMRequest {
            messages,
            system_prompt,
            tools,
            model,
            temperature,
            stream,
            tool_choice,
            ..Default::default()
        })
    }

    fn extract_content_text(content: &Value) -> String {
        match content {
            Value::String(text) => text.to_string(),
            Value::Array(parts) => parts
                .iter()
                .filter_map(|part| {
                    part.get("text")
                        .and_then(|t| t.as_str())
                        .map(|s| s.to_string())
                        .or_else(|| {
                            part.get("content")
                                .and_then(|c| c.as_str())
                                .map(|s| s.to_string())
                        })
                })
                .collect::<Vec<_>>()
                .join(""),
            _ => String::new(),
        }
    }

    fn parse_tool_choice(choice: &Value) -> Option<provider::ToolChoice> {
        match choice {
            Value::String(value) => match value.as_str() {
                "auto" => Some(provider::ToolChoice::auto()),
                "none" => Some(provider::ToolChoice::none()),
                "required" => Some(provider::ToolChoice::any()),
                _ => None,
            },
            Value::Object(map) => {
                let choice_type = map.get("type").and_then(|t| t.as_str())?;
                match choice_type {
                    "function" => map
                        .get("function")
                        .and_then(|f| f.get("name"))
                        .and_then(|n| n.as_str())
                        .map(|name| provider::ToolChoice::function(name.to_string())),
                    "auto" => Some(provider::ToolChoice::auto()),
                    "none" => Some(provider::ToolChoice::none()),
                    "any" | "required" => Some(provider::ToolChoice::any()),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn convert_to_openai_format(
        &self,
        request: &provider::LLMRequest,
    ) -> Result<Value, provider::LLMError> {
        let mut messages = Vec::with_capacity(request.messages.len() + 1); // +1 for system prompt
        let mut active_tool_call_ids: HashSet<String> = HashSet::with_capacity(16); // Typical tool call count

        if let Some(system_prompt) = &request.system_prompt {
            messages.push(json!({
                "role": crate::config::constants::message_roles::SYSTEM,
                "content": system_prompt
            }));
        }

        for msg in &request.messages {
            let role = msg.role.as_openai_str();
            let mut message = json!({
                "role": role,
                "content": msg.content
            });
            let mut skip_message = false;

            if msg.role == provider::MessageRole::Assistant
                && let Some(tool_calls) = &msg.tool_calls
                && !tool_calls.is_empty()
            {
                let tool_calls_json: Vec<Value> = tool_calls
                    .iter()
                    .filter_map(|tc| {
                        tc.function.as_ref().map(|func| {
                            active_tool_call_ids.insert(tc.id.clone());
                            json!({
                                "id": tc.id,
                                "type": "function",
                                "function": {
                                    "name": func.name,
                                    "arguments": func.arguments
                                }
                            })
                        })
                    })
                    .collect();

                message["tool_calls"] = Value::Array(tool_calls_json);
            }

            if msg.role == provider::MessageRole::Tool {
                match &msg.tool_call_id {
                    Some(tool_call_id) if active_tool_call_ids.contains(tool_call_id) => {
                        message["tool_call_id"] = Value::String(tool_call_id.clone());
                        active_tool_call_ids.remove(tool_call_id);
                    }
                    Some(_) | None => {
                        skip_message = true;
                    }
                }
            }

            if !skip_message {
                messages.push(message);
            }
        }

        if messages.is_empty() {
            let formatted_error = error_display::format_llm_error("OpenAI", "No messages provided");
            return Err(provider::LLMError::InvalidRequest {
                message: formatted_error,
                metadata: None,
            });
        }

        let mut openai_request = json!({
            "model": request.model,
            "messages": messages,
            "stream": request.stream
        });

        let is_native_openai = self.base_url.contains("api.openai.com");
        let _max_tokens_field = if !is_native_openai {
            "max_tokens"
        } else {
            MAX_COMPLETION_TOKENS_FIELD
        };

        if let Some(temperature) = request.temperature
            && Self::supports_temperature_parameter(&request.model)
        {
            openai_request["temperature"] = json!(temperature);
        }

        if self.supports_tools(&request.model) {
            if let Some(tools) = &request.tools
                && let Some(serialized) = self.serialize_tools(tools)
            {
                openai_request["tools"] = serialized;

                // Check if any tools are custom types - if so, disable parallel tool calls
                // as per GPT-5 specification: "custom tool type does NOT support parallel tool calling"
                let has_custom_tool = tools.iter().any(|tool| tool.tool_type == "custom");
                if has_custom_tool {
                    // Override parallel tool calls to false if custom tools are present
                    openai_request["parallel_tool_calls"] = Value::Bool(false);
                }

                // Only add tool_choice when tools are present
                if let Some(tool_choice) = &request.tool_choice {
                    openai_request["tool_choice"] = tool_choice.to_provider_format("openai");
                }

                // Only set parallel tool calls if not overridden due to custom tools
                if request.parallel_tool_calls.is_some()
                    && openai_request.get("parallel_tool_calls").is_none()
                {
                    if let Some(parallel) = request.parallel_tool_calls {
                        openai_request["parallel_tool_calls"] = Value::Bool(parallel);
                    }
                }

                // Only add parallel_tool_config when tools are present
                if self.supports_parallel_tool_config(&request.model) {
                    if let Some(config) = &request.parallel_tool_config {
                        if let Ok(config_value) = serde_json::to_value(config) {
                            openai_request["parallel_tool_config"] = config_value;
                        }
                    }
                }
            }
        }

        // NOTE: The 'reasoning' parameter is NOT supported in Chat Completions API.
        // It's only valid in the Responses API. Since this function builds Chat Completions
        // requests, we explicitly skip adding the reasoning parameter here.
        // Reasoning parameters are only added in convert_to_openai_responses_format().

        Ok(openai_request)
    }

    fn convert_to_openai_responses_format(
        &self,
        request: &provider::LLMRequest,
    ) -> Result<Value, provider::LLMError> {
        let responses_payload = if Self::is_gpt5_codex_model(&request.model) {
            build_codex_responses_payload(request)?
        } else {
            build_standard_responses_payload(request)?
        };

        if responses_payload.input.is_empty() {
            let formatted_error =
                error_display::format_llm_error("OpenAI", "No messages provided for Responses API");
            return Err(provider::LLMError::InvalidRequest {
                message: formatted_error,
                metadata: None,
            });
        }

        let mut openai_request = json!({
            "model": request.model,
            "input": responses_payload.input,
            "stream": request.stream,
        });

        // 'output_types' is part of the GPT-5 Responses API spec
        openai_request["output_types"] = json!(["message", "tool_call"]);

        if let Some(instructions) = responses_payload.instructions {
            if !instructions.trim().is_empty() {
                openai_request["instructions"] = json!(instructions);
            }
        }

        let mut sampling_parameters = json!({});
        let mut has_sampling = false;

        if let Some(temperature) = request.temperature {
            if Self::supports_temperature_parameter(&request.model) {
                sampling_parameters["temperature"] = json!(temperature);
                has_sampling = true;
            }
        }

        if let Some(top_p) = request.top_p {
            sampling_parameters["top_p"] = json!(top_p);
            has_sampling = true;
        }

        if let Some(presence_penalty) = request.presence_penalty {
            sampling_parameters["presence_penalty"] = json!(presence_penalty);
            has_sampling = true;
        }

        if let Some(frequency_penalty) = request.frequency_penalty {
            sampling_parameters["frequency_penalty"] = json!(frequency_penalty);
            has_sampling = true;
        }

        if has_sampling {
            openai_request["sampling_parameters"] = sampling_parameters;
        }

        if self.supports_tools(&request.model) {
            if let Some(tools) = &request.tools {
                if let Some(serialized) = Self::serialize_tools_for_responses(tools) {
                    openai_request["tools"] = serialized;

                    // Check if any tools are custom types - if so, disable parallel tool calls
                    // as per GPT-5 specification: "custom tool type does NOT support parallel tool calling"
                    let has_custom_tool = tools.iter().any(|tool| tool.tool_type == "custom");
                    if has_custom_tool {
                        // Override parallel tool calls to false if custom tools are present
                        openai_request["parallel_tool_calls"] = Value::Bool(false);
                    }

                    // Only add tool_choice when tools are present
                    if let Some(tool_choice) = &request.tool_choice {
                        openai_request["tool_choice"] = tool_choice.to_provider_format("openai");
                    }

                    // Only set parallel tool calls if not overridden due to custom tools
                    if request.parallel_tool_calls.is_some()
                        && !openai_request.get("parallel_tool_calls").is_some()
                    {
                        if let Some(parallel) = request.parallel_tool_calls {
                            openai_request["parallel_tool_calls"] = Value::Bool(parallel);
                        }
                    }

                    // Only add parallel_tool_config when tools are present
                    if self.supports_parallel_tool_config(&request.model) {
                        if let Some(config) = &request.parallel_tool_config {
                            if let Ok(config_value) = serde_json::to_value(config) {
                                openai_request["parallel_tool_config"] = config_value;
                            }
                        }
                    }
                }
            }
        }

        if self.supports_reasoning_effort(&request.model) {
            if let Some(effort) = request.reasoning_effort {
                if let Some(payload) = reasoning_parameters_for(ModelProvider::OpenAI, effort) {
                    openai_request["reasoning"] = payload;
                } else {
                    openai_request["reasoning"] = json!({ "effort": effort.as_str() });
                }
            } else if openai_request.get("reasoning").is_none() {
                // Use the default reasoning effort level (medium) for native OpenAI models
                let default_effort = ReasoningEffortLevel::default().as_str();
                openai_request["reasoning"] = json!({ "effort": default_effort });
            }
        }

        // Enable reasoning summaries if supported (OpenAI GPT-5 only)
        if self.supports_reasoning(&request.model) {
            if let Some(map) = openai_request.as_object_mut() {
                let reasoning_value = map.entry("reasoning").or_insert(json!({}));
                if let Some(reasoning_obj) = reasoning_value.as_object_mut() {
                    if !reasoning_obj.contains_key("summary") {
                        reasoning_obj.insert("summary".to_string(), json!("auto"));
                    }
                }
            }
        }

        // Add text formatting options for GPT-5 and compatible models, including verbosity and grammar
        let mut text_format = json!({});
        let mut has_format_options = false;

        if let Some(verbosity) = request.verbosity {
            text_format["verbosity"] = json!(verbosity.as_str());
            has_format_options = true;
        }

        // Add grammar constraint if tools include grammar definitions
        if let Some(ref tools) = request.tools {
            let grammar_tools: Vec<&provider::ToolDefinition> = tools
                .iter()
                .filter(|tool| tool.tool_type == "grammar")
                .collect();

            if !grammar_tools.is_empty() {
                // Use the first grammar definition found
                if let Some(grammar_tool) = grammar_tools.first() {
                    if let Some(ref grammar) = grammar_tool.grammar {
                        text_format["format"] = json!({
                            "type": "grammar",
                            "syntax": grammar.syntax,
                            "definition": grammar.definition
                        });
                        has_format_options = true;
                    }
                }
            }
        }

        // Set default verbosity for GPT-5.1/5.2 models if no format options specified
        if !has_format_options
            && (request.model.starts_with("gpt-5.1") || request.model.starts_with("gpt-5.2"))
        {
            text_format["verbosity"] = json!("medium");
            has_format_options = true;
        }

        if has_format_options {
            openai_request["text"] = text_format;
        }

        // If configured, include the `prompt_cache_retention` value in the Responses API
        // request. This allows the user to extend the server-side prompt cache window
        // (e.g., "24h") to increase cache reuse and reduce cost/latency on GPT-5.1.
        // Only include prompt_cache_retention when both configured and when the selected
        // model uses the OpenAI Responses API.
        if OpenAIProvider::is_responses_api_model(&request.model) {
            if let Some(ref retention) = self.prompt_cache_settings.prompt_cache_retention {
                if !retention.trim().is_empty() {
                    openai_request["prompt_cache_retention"] = json!(retention);
                }
            }
        }

        Ok(openai_request)
    }

    fn parse_openai_response(
        &self,
        response_json: Value,
    ) -> Result<provider::LLMResponse, provider::LLMError> {
        let choices = response_json
            .get("choices")
            .and_then(|c| c.as_array())
            .ok_or_else(|| {
                let formatted_error = error_display::format_llm_error(
                    "OpenAI",
                    "Invalid response format: missing choices",
                );
                provider::LLMError::Provider {
                    message: formatted_error,
                    metadata: None,
                }
            })?;

        if choices.is_empty() {
            let formatted_error =
                error_display::format_llm_error("OpenAI", "No choices in response");
            return Err(provider::LLMError::Provider {
                message: formatted_error,
                metadata: None,
            });
        }

        let choice = &choices[0];
        let message = choice.get("message").ok_or_else(|| {
            let formatted_error = error_display::format_llm_error(
                "OpenAI",
                "Invalid response format: missing message",
            );
            provider::LLMError::Provider {
                message: formatted_error,
                metadata: None,
            }
        })?;

        let content = match message.get("content") {
            Some(Value::String(text)) => Some(text.to_string()),
            Some(Value::Array(parts)) => {
                let text = parts
                    .iter()
                    .filter_map(|part| part.get("text").and_then(|t| t.as_str()))
                    .collect::<Vec<_>>()
                    .join("");
                if text.is_empty() { None } else { Some(text) }
            }
            _ => None,
        };

        let tool_calls = message
            .get("tool_calls")
            .and_then(|tc| tc.as_array())
            .map(|calls| {
                calls
                    .iter()
                    .filter_map(|call| {
                        let id = call.get("id").and_then(|v| v.as_str())?;
                        let function = call.get("function")?;
                        let name = function.get("name").and_then(|v| v.as_str())?;
                        let arguments = function.get("arguments");
                        let serialized = arguments.map_or("{}".to_string(), |value| {
                            if value.is_string() {
                                value.as_str().unwrap_or("").to_string()
                            } else {
                                value.to_string()
                            }
                        });
                        Some(provider::ToolCall::function(
                            id.to_string(),
                            name.to_string(),
                            serialized,
                        ))
                    })
                    .collect::<Vec<_>>()
            })
            .filter(|calls| !calls.is_empty());

        let reasoning = message
            .get("reasoning_content")
            .and_then(extract_reasoning_trace)
            .or_else(|| message.get("reasoning").and_then(extract_reasoning_trace))
            .or_else(|| {
                choice
                    .get("reasoning_content")
                    .and_then(extract_reasoning_trace)
            })
            .or_else(|| choice.get("reasoning").and_then(extract_reasoning_trace))
            .or_else(|| {
                content.as_ref().and_then(|c| {
                    let (reasoning_parts, _) = crate::llm::utils::extract_reasoning_content(c);
                    if reasoning_parts.is_empty() {
                        None
                    } else {
                        Some(reasoning_parts.join("\n\n"))
                    }
                })
            });

        let finish_reason = choice
            .get("finish_reason")
            .and_then(|fr| fr.as_str())
            .map(|fr| match fr {
                "stop" => crate::llm::provider::FinishReason::Stop,
                "length" => crate::llm::provider::FinishReason::Length,
                "tool_calls" => crate::llm::provider::FinishReason::ToolCalls,
                "content_filter" => crate::llm::provider::FinishReason::ContentFilter,
                other => crate::llm::provider::FinishReason::Error(other.to_string()),
            })
            .unwrap_or(crate::llm::provider::FinishReason::Stop);

        Ok(provider::LLMResponse {
            content,
            tool_calls,
            usage: response_json.get("usage").map(|usage_value| {
                let cached_prompt_tokens =
                    if self.prompt_cache_enabled && self.prompt_cache_settings.surface_metrics {
                        usage_value
                            .get("prompt_tokens_details")
                            .and_then(|details| details.get("cached_tokens"))
                            .and_then(|value| value.as_u64())
                            .map(|value| value as u32)
                    } else {
                        None
                    };

                crate::llm::provider::Usage {
                    prompt_tokens: usage_value
                        .get("prompt_tokens")
                        .and_then(|pt| pt.as_u64())
                        .and_then(|v| u32::try_from(v).ok())
                        .unwrap_or(0),
                    completion_tokens: usage_value
                        .get("completion_tokens")
                        .and_then(|ct| ct.as_u64())
                        .and_then(|v| u32::try_from(v).ok())
                        .unwrap_or(0),
                    total_tokens: usage_value
                        .get("total_tokens")
                        .and_then(|tt| tt.as_u64())
                        .and_then(|v| u32::try_from(v).ok())
                        .unwrap_or(0),
                    cached_prompt_tokens,
                    cache_creation_tokens: None,
                    cache_read_tokens: None,
                }
            }),
            finish_reason,
            reasoning,
            reasoning_details: None,
            tool_references: Vec::new(),
            request_id: None,
            organization_id: None,
        })
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
        let response = self
            .http_client
            .post(format!("{}/generate", server_url))
            .header("Content-Type", "application/json")
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
        // Handle harmony format namespace mappings (e.g., "repo_browser.list_files" -> "list_files")
        // Direct tool name aliases are handled by canonical_tool_name() in the registry
        match recipient {
            "repo_browser.list_files" => "list_files".to_string(),
            "repo_browser.read_file" => "read_file".to_string(),
            "repo_browser.write_file" => "write_file".to_string(),
            "container.exec" => "run_pty_cmd".to_string(),
            "bash" => "bash".to_string(),
            "grep" => "grep_file".to_string(),
            _ => {
                // Try to extract the function name after the last dot
                if let Some(dot_pos) = recipient.rfind('.') {
                    recipient[dot_pos + 1..].to_string()
                } else {
                    recipient.to_string()
                }
            }
        }
    }

    /// Parse harmony tool call from raw text content
    fn parse_harmony_tool_call_from_text(text: &str) -> Option<(String, serde_json::Value)> {
        // Look for harmony format: to=tool_name followed by JSON
        if let Some(to_pos) = text.find("to=") {
            let after_to = &text[to_pos + 3..];
            if let Some(space_pos) = after_to.find(' ') {
                let tool_ref = &after_to[..space_pos];
                let tool_name = Self::parse_harmony_tool_name(tool_ref);

                // Look for JSON in the remaining text
                let remaining = &after_to[space_pos..];
                if let Some(json_start) = remaining.find('{') {
                    if let Some(json_end) = remaining.rfind('}') {
                        let json_text = &remaining[json_start..=json_end];
                        if let Ok(args) = serde_json::from_str(json_text) {
                            return Some((tool_name, args));
                        }
                    }
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::provider::ParallelToolConfig;

    fn sample_tool() -> provider::ToolDefinition {
        provider::ToolDefinition::function(
            "search_workspace".to_owned(),
            "Search project files".to_owned(),
            json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string"}
                },
                "required": ["query"],
                "additionalProperties": false
            }),
        )
    }

    fn sample_request(model: &str) -> provider::LLMRequest {
        provider::LLMRequest {
            messages: vec![provider::Message::user("Hello".to_owned())],
            tools: Some(vec![sample_tool()]),
            model: model.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn serialize_tools_wraps_function_definition() {
        let tools = vec![sample_tool()];
        let provider = OpenAIProvider::new(String::new());
        let serialized = provider
            .serialize_tools(&tools)
            .expect("tools should serialize");
        let serialized_tools = serialized
            .as_array()
            .expect("serialized tools should be an array");
        assert_eq!(serialized_tools.len(), 1);

        let tool_value = serialized_tools[0]
            .as_object()
            .expect("tool should be serialized as object");
        assert_eq!(
            tool_value.get("type").and_then(Value::as_str),
            Some("function")
        );
        assert!(tool_value.contains_key("function"));
        assert_eq!(
            tool_value.get("name").and_then(Value::as_str),
            Some("search_workspace")
        );
        assert_eq!(
            tool_value
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            "Search project files"
        );

        let function_value = tool_value
            .get("function")
            .and_then(Value::as_object)
            .expect("function payload missing");
        assert_eq!(
            function_value.get("name").and_then(Value::as_str),
            Some("search_workspace")
        );
        assert!(function_value.contains_key("parameters"));
        assert_eq!(
            tool_value.get("parameters").and_then(Value::as_object),
            function_value.get("parameters").and_then(Value::as_object)
        );
    }

    #[test]
    fn chat_completions_payload_uses_function_wrapper() {
        let provider =
            OpenAIProvider::with_model(String::new(), models::openai::DEFAULT_MODEL.to_string());
        let request = sample_request(models::openai::DEFAULT_MODEL);
        let payload = provider
            .convert_to_openai_format(&request)
            .expect("conversion should succeed");

        let tools = payload
            .get("tools")
            .and_then(Value::as_array)
            .expect("tools should exist on payload");
        let tool_object = tools[0].as_object().expect("tool entry should be object");
        assert!(tool_object.contains_key("function"));
        assert_eq!(
            tool_object.get("name").and_then(Value::as_str),
            Some("search_workspace")
        );
    }

    #[test]
    fn responses_payload_uses_function_wrapper() {
        let provider =
            OpenAIProvider::with_model(String::new(), models::openai::GPT_5_CODEX.to_string());
        let request = sample_request(models::openai::GPT_5_CODEX);
        let payload = provider
            .convert_to_openai_responses_format(&request)
            .expect("conversion should succeed");

        let instructions = payload
            .get("instructions")
            .and_then(Value::as_str)
            .expect("instructions should be set for codex");
        assert!(instructions.contains("You are Codex, based on GPT-5"));

        let tools = payload
            .get("tools")
            .and_then(Value::as_array)
            .expect("tools should exist on payload");
        let tool_object = tools[0].as_object().expect("tool entry should be object");
        assert!(tool_object.contains_key("function"));
        assert_eq!(
            tool_object.get("name").and_then(Value::as_str),
            Some("search_workspace")
        );
    }

    #[test]
    fn serialize_tools_dedupes_duplicate_names() {
        let duplicate = provider::ToolDefinition::function(
            "search_workspace".to_owned(),
            "dup".to_owned(),
            json!({"type": "object"}),
        );
        let tools = vec![sample_tool(), duplicate];
        let provider = OpenAIProvider::new(String::new());
        let serialized = provider
            .serialize_tools(&tools)
            .expect("tools should serialize cleanly");
        let arr = serialized.as_array().expect("array");
        assert_eq!(arr.len(), 1, "duplicate names should be dropped");
    }

    #[test]
    fn responses_tools_dedupes_apply_patch_and_function() {
        let apply_builtin = provider::ToolDefinition::apply_patch("Apply patches".to_owned());
        let apply_function = provider::ToolDefinition::function(
            "apply_patch".to_owned(),
            "alt apply".to_owned(),
            json!({"type": "object"}),
        );
        let tools = vec![apply_builtin, apply_function];
        let serialized = OpenAIProvider::serialize_tools_for_responses(&tools)
            .expect("responses tools should serialize");
        let arr = serialized.as_array().expect("array");
        assert_eq!(arr.len(), 1, "apply_patch should be deduped");
        let tool = arr[0].as_object().expect("object");
        assert_eq!(tool.get("type").and_then(Value::as_str), Some("function"));
        assert_eq!(
            tool.get("name").and_then(Value::as_str),
            Some("apply_patch")
        );
    }

    #[test]
    fn responses_payload_sets_instructions_from_system_prompt() {
        let provider = OpenAIProvider::with_model(String::new(), models::openai::GPT_5.to_string());
        let mut request = sample_request(models::openai::GPT_5);
        request.system_prompt = Some("You are a helpful assistant.".to_owned());

        let payload = provider
            .convert_to_openai_responses_format(&request)
            .expect("conversion should succeed");

        let instructions = payload
            .get("instructions")
            .and_then(Value::as_str)
            .expect("instructions should be present");
        assert!(instructions.contains("You are a helpful assistant."));

        let input = payload
            .get("input")
            .and_then(Value::as_array)
            .expect("input should be serialized as array");
        assert_eq!(
            input
                .first()
                .and_then(|value| value.get("role"))
                .and_then(Value::as_str),
            Some("user")
        );
    }

    #[test]
    fn harmony_detection_handles_common_variants() {
        assert!(OpenAIProvider::uses_harmony("gpt-oss-20b"));
        assert!(OpenAIProvider::uses_harmony("openai/gpt-oss-20b"));
        assert!(OpenAIProvider::uses_harmony("openai/gpt-oss-20b:free"));
        assert!(OpenAIProvider::uses_harmony("OPENAI/GPT-OSS-120B"));
        assert!(OpenAIProvider::uses_harmony("gpt-oss-120b@openrouter"));

        assert!(!OpenAIProvider::uses_harmony("gpt-5"));
        assert!(!OpenAIProvider::uses_harmony("gpt-oss:20b"));
    }

    #[test]
    fn responses_payload_includes_prompt_cache_retention() {
        let mut pc = PromptCachingConfig::default();
        pc.providers.openai.prompt_cache_retention = Some("24h".to_owned());

        let provider = OpenAIProvider::from_config(
            Some("key".to_owned()),
            Some(models::openai::GPT_5_1.to_string()),
            None,
            Some(pc),
            None,
            None,
        );

        let request = sample_request(models::openai::GPT_5_1);
        let payload = provider
            .convert_to_openai_responses_format(&request)
            .expect("conversion should succeed");

        assert_eq!(
            payload
                .get("prompt_cache_retention")
                .and_then(Value::as_str),
            Some("24h")
        );
    }

    #[test]
    fn responses_payload_excludes_prompt_cache_retention_when_not_set() {
        let pc = PromptCachingConfig::default(); // default is Some("24h"); ram: to simulate none, set to None
        let mut pc = pc;
        pc.providers.openai.prompt_cache_retention = None;
        let provider = OpenAIProvider::from_config(
            Some("key".to_string()),
            Some(models::openai::GPT_5_1.to_string()),
            None,
            Some(pc),
            None,
            None,
        );

        let mut request = sample_request(models::openai::GPT_5_1);
        request.stream = true;
        let payload = provider
            .convert_to_openai_responses_format(&request)
            .expect("conversion should succeed");

        assert!(payload.get("prompt_cache_retention").is_none());
    }

    #[test]
    fn responses_payload_includes_prompt_cache_retention_streaming() {
        let mut pc = PromptCachingConfig::default();
        pc.providers.openai.prompt_cache_retention = Some("12h".to_owned());

        let provider = OpenAIProvider::from_config(
            Some("key".to_string()),
            Some(models::openai::GPT_5_1.to_string()),
            None,
            Some(pc),
            None,
            None,
        );

        let mut request = sample_request(models::openai::GPT_5_1);
        request.stream = true;
        let payload = provider
            .convert_to_openai_responses_format(&request)
            .expect("conversion should succeed");

        assert_eq!(
            payload
                .get("prompt_cache_retention")
                .and_then(Value::as_str),
            Some("12h")
        );
    }

    #[test]
    fn responses_payload_excludes_retention_for_non_responses_model() {
        let mut pc = PromptCachingConfig::default();
        pc.providers.openai.prompt_cache_retention = Some("9999s".to_string());

        let provider = OpenAIProvider::from_config(
            Some("key".to_string()),
            Some(models::openai::CODEX_MINI_LATEST.to_string()),
            None,
            Some(pc),
            None,
            None,
        );

        let request = sample_request(models::openai::CODEX_MINI_LATEST);
        let payload = provider
            .convert_to_openai_responses_format(&request)
            .expect("conversion should succeed");

        assert!(payload.get("prompt_cache_retention").is_none());
    }

    #[test]
    fn provider_from_config_respects_prompt_cache_retention() {
        let mut pc = PromptCachingConfig::default();
        pc.providers.openai.prompt_cache_retention = Some("72h".to_owned());
        let provider = OpenAIProvider::from_config(
            Some("key".to_string()),
            Some(models::openai::GPT_5_1.to_string()),
            None,
            Some(pc.clone()),
            None,
            None,
        );

        assert_eq!(
            provider.prompt_cache_settings.prompt_cache_retention,
            Some("72h".to_owned())
        );
    }

    #[test]
    fn test_parse_harmony_tool_name() {
        assert_eq!(
            OpenAIProvider::parse_harmony_tool_name("repo_browser.list_files"),
            "list_files"
        );
        assert_eq!(
            OpenAIProvider::parse_harmony_tool_name("container.exec"),
            "run_pty_cmd"
        );
        assert_eq!(
            OpenAIProvider::parse_harmony_tool_name("unknown.tool"),
            "tool"
        );
        // Direct tool names (not harmony namespaces) pass through
        // Alias resolution happens in canonical_tool_name()
        assert_eq!(OpenAIProvider::parse_harmony_tool_name("exec"), "exec");
        assert_eq!(
            OpenAIProvider::parse_harmony_tool_name("exec_pty_cmd"),
            "exec_pty_cmd"
        );
        assert_eq!(
            OpenAIProvider::parse_harmony_tool_name("exec_code"),
            "exec_code"
        );
        assert_eq!(OpenAIProvider::parse_harmony_tool_name("simple"), "simple");
    }

    #[test]
    fn test_parse_harmony_tool_call_from_text() {
        let text = r#"to=repo_browser.list_files {"path":"", "recursive":"true"}"#;
        let result = OpenAIProvider::parse_harmony_tool_call_from_text(text);
        assert!(result.is_some());

        let (tool_name, args) = result.unwrap();
        assert_eq!(tool_name, "list_files");
        assert_eq!(args["path"], serde_json::json!(""));
        assert_eq!(args["recursive"], serde_json::json!("true"));
    }

    #[test]
    fn test_parse_harmony_tool_call_from_text_container_exec() {
        let text = r#"to=container.exec {"cmd":["ls", "-la"]}"#;
        let result = OpenAIProvider::parse_harmony_tool_call_from_text(text);
        assert!(result.is_some());

        let (tool_name, args) = result.unwrap();
        assert_eq!(tool_name, "run_pty_cmd");
        assert_eq!(args["cmd"], serde_json::json!(["ls", "-la"]));
    }

    #[test]
    fn chat_completions_uses_max_completion_tokens_field() {
        let provider =
            OpenAIProvider::with_model(String::new(), models::openai::DEFAULT_MODEL.to_string());
        let request = sample_request(models::openai::DEFAULT_MODEL);

        let payload = provider
            .convert_to_openai_format(&request)
            .expect("conversion should succeed");

        let max_tokens_value = payload
            .get(MAX_COMPLETION_TOKENS_FIELD)
            .and_then(Value::as_u64)
            .expect("max completion tokens should be set");
        assert_eq!(max_tokens_value, 512);
        assert!(payload.get("max_tokens").is_none());
    }

    #[test]
    fn chat_completions_applies_temperature_independent_of_max_tokens() {
        let provider = OpenAIProvider::with_model(
            String::new(),
            models::openai::CODEX_MINI_LATEST.to_string(),
        );
        let mut request = sample_request(models::openai::CODEX_MINI_LATEST);
        request.temperature = Some(0.4);

        let payload = provider
            .convert_to_openai_format(&request)
            .expect("conversion should succeed");

        assert!(payload.get(MAX_COMPLETION_TOKENS_FIELD).is_none());
        let temperature_value = payload
            .get("temperature")
            .and_then(Value::as_f64)
            .expect("temperature should be present");
        assert!((temperature_value - 0.4).abs() < 1e-6);
    }

    #[test]
    fn responses_payload_omits_parallel_tool_config_when_not_supported() {
        let provider =
            OpenAIProvider::with_model(String::new(), models::openai::GPT_5_CODEX.to_string());
        let mut request = sample_request(models::openai::GPT_5_CODEX);
        request.parallel_tool_calls = Some(true);
        request.parallel_tool_config = Some(ParallelToolConfig {
            disable_parallel_tool_use: true,
            max_parallel_tools: Some(2),
            encourage_parallel: false,
        });

        let payload = provider
            .convert_to_openai_responses_format(&request)
            .expect("conversion should succeed");

        assert_eq!(payload.get("parallel_tool_calls"), Some(&Value::Bool(true)));
        assert!(
            payload.get("parallel_tool_config").is_none(),
            "OpenAI payload should not include parallel_tool_config"
        );
    }
}

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

                if status.as_u16() == 429
                    || error_text.contains("insufficient_quota")
                    || error_text.contains("quota")
                    || error_text.contains("rate limit")
                {
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

            let stream = try_stream! {
                let mut body_stream = response.bytes_stream();
                let mut buffer = String::new();
                let mut aggregated_content = String::new();
                let mut reasoning_buffer = ReasoningBuffer::default();
                let mut sanitizer = TagStreamSanitizer::new();
                let mut tool_builders = Vec::new();
                let mut finish_reason = provider::FinishReason::Stop;
                let mut accumulated_usage = None;
                let telemetry = OpenAIStreamTelemetry;

                while let Some(chunk_result) = body_stream.next().await {
                    let chunk = chunk_result.map_err(|err| {
                        let formatted_error = error_display::format_llm_error(
                            "OpenAI",
                            &format!("Streaming error: {}", err),
                        );
                        provider::LLMError::Network { message: formatted_error, metadata: None }
                    })?;

                    buffer.push_str(&String::from_utf8_lossy(&chunk));

                    while let Some((split_idx, delimiter_len)) = find_sse_boundary(&buffer) {
                        let event = buffer[..split_idx].to_string();
                        buffer.drain(..split_idx + delimiter_len);

                        if let Some(data_payload) = extract_data_payload(&event) {
                            let trimmed_payload = data_payload.trim();
                            if trimmed_payload.is_empty() || trimmed_payload == "[DONE]" {
                                continue;
                            }

                            let payload: Value = serde_json::from_str(trimmed_payload).map_err(|err| {
                                StreamAssemblyError::InvalidPayload(err.to_string())
                                    .into_llm_error("OpenAI")
                            })?;

                            // Capture usage if present (stream_options: include_usage)
                            if let Some(usage_val) = payload.get("usage") {
                                if let Ok(u) = serde_json::from_value::<provider::Usage>(usage_val.clone()) {
                                    accumulated_usage = Some(u);
                                }
                            }

                            if let Some(choices) = payload.get("choices").and_then(|v| v.as_array()) {
                                if let Some(choice) = choices.first() {
                                    if let Some(delta) = choice.get("delta") {
                                        // 1. Content
                                        if let Some(content) = delta.get("content").and_then(|v| v.as_str()) {
                                            aggregated_content.push_str(content);
                                            telemetry.on_content_delta(content);
                                            for event in sanitizer.process_chunk(content) {
                                                match &event {
                                                    provider::LLMStreamEvent::Token { delta } => {
                                                        yield provider::LLMStreamEvent::Token { delta: delta.clone() };
                                                    }
                                                    provider::LLMStreamEvent::Reasoning { delta } => {
                                                        yield provider::LLMStreamEvent::Reasoning { delta: delta.clone() };
                                                    }
                                                    _ => {}
                                                }
                                            }
                                        }

                                        // 2. Reasoning (DeepSeek / O1 format)
                                        if let Some(reasoning) = delta.get("reasoning_content").and_then(|v| v.as_str()) {
                                            for fragment in append_reasoning_segments(&mut reasoning_buffer, reasoning, &telemetry) {
                                                yield provider::LLMStreamEvent::Reasoning { delta: fragment };
                                            }
                                        }

                                        // 3. Tool Calls
                                        if let Some(tool_deltas) = delta.get("tool_calls").and_then(|v| v.as_array()) {
                                            crate::llm::providers::shared::update_tool_calls(&mut tool_builders, tool_deltas);
                                            telemetry.on_tool_call_delta();
                                        }
                                    }

                                    if let Some(reason) = choice.get("finish_reason").and_then(|v| v.as_str()) {
                                        finish_reason = match reason {
                                            "stop" => provider::FinishReason::Stop,
                                            "length" => provider::FinishReason::Length,
                                            "tool_calls" => provider::FinishReason::ToolCalls,
                                            "content_filter" => provider::FinishReason::ContentFilter,
                                            _ => provider::FinishReason::Stop,
                                        };
                                    }
                                }
                            }
                        }
                    }
                }

                for event in sanitizer.finalize() {
                    yield event;
                }

                let response = provider::LLMResponse {
                    content: if aggregated_content.is_empty() { None } else { Some(aggregated_content) },
                    tool_calls: crate::llm::providers::shared::finalize_tool_calls(tool_builders),
                    usage: accumulated_usage,
                    finish_reason,
                    reasoning: reasoning_buffer.finalize(),
                    reasoning_details: None,
                    tool_references: Vec::new(),
                    request_id: None,
                    organization_id: None,
                };

                yield provider::LLMStreamEvent::Completed { response };
            };

            return Ok(Box::pin(stream));
        }

        let include_metrics =
            self.prompt_cache_enabled && self.prompt_cache_settings.surface_metrics;

        let mut openai_request = self.convert_to_openai_responses_format(&request)?;

        openai_request["stream"] = Value::Bool(true);
        #[cfg(debug_assertions)]
        let debug_model = request.model.clone();
        #[cfg(debug_assertions)]
        let request_timer = Instant::now();
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

        let response = self
            .authorize(self.http_client.post(&url))
            .header("OpenAI-Beta", "responses=v1")
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

            if status.as_u16() == 429
                || error_text.contains("insufficient_quota")
                || error_text.contains("quota")
                || error_text.contains("rate limit")
            {
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
            debug!(
                target = "vtcode::llm::openai",
                model = %request.model,
                status = %response.status(),
                handshake_ms = request_timer.elapsed().as_millis(),
                "Streaming response headers received"
            );
        }

        let stream = try_stream! {
            let mut body_stream = response.bytes_stream();
            let mut buffer = String::new();
            let mut aggregated_content = String::new();
            let mut reasoning_buffer = ReasoningBuffer::default();
            let mut final_response: Option<Value> = None;
            let mut done = false;
            let mut sanitizer = TagStreamSanitizer::new();
            #[cfg(debug_assertions)]
            let mut streamed_events_counter: usize = 0;
            let telemetry = OpenAIStreamTelemetry;

            while let Some(chunk_result) = body_stream.next().await {
                let chunk = chunk_result.map_err(|err| {
                    let formatted_error = error_display::format_llm_error(
                        "OpenAI",
                        &format!("Streaming error: {}", err),
                    );
                    provider::LLMError::Network { message: formatted_error, metadata: None }
                })?;

                buffer.push_str(&String::from_utf8_lossy(&chunk));

                while let Some((split_idx, delimiter_len)) = find_sse_boundary(&buffer) {
                    let event = buffer[..split_idx].to_string();
                    buffer.drain(..split_idx + delimiter_len);
                    #[cfg(debug_assertions)]
                    {
                        streamed_events_counter = streamed_events_counter.saturating_add(1);
                    }

                    if let Some(data_payload) = extract_data_payload(&event) {
                        let trimmed_payload = data_payload.trim();
                        if trimmed_payload.is_empty() {
                            continue;
                        }

                        if trimmed_payload == "[DONE]" {
                            done = true;
                            break;
                        }

                        let payload: Value = serde_json::from_str(trimmed_payload).map_err(|err| {
                            StreamAssemblyError::InvalidPayload(err.to_string())
                                .into_llm_error("OpenAI")
                        })?;

                        if let Some(event_type) = payload.get("type").and_then(|value| value.as_str()) {
                            match event_type {
                                // Per Responses API spec: text content streaming
                                "response.output_text.delta" => {
                                    let delta = payload
                                        .get("delta")
                                        .and_then(|value| value.as_str())
                                        .ok_or_else(|| {
                                            StreamAssemblyError::MissingField("delta")
                                                .into_llm_error("OpenAI")
                                        })?;
                                    aggregated_content.push_str(delta);
                                    telemetry.on_content_delta(delta);

                                    for event in sanitizer.process_chunk(delta) {
                                        match &event {
                                            provider::LLMStreamEvent::Token { delta } => {
                                                yield provider::LLMStreamEvent::Token { delta: delta.clone() };
                                            }
                                            provider::LLMStreamEvent::Reasoning { delta } => {
                                                yield provider::LLMStreamEvent::Reasoning { delta: delta.clone() };
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                                // Refusal content (model declined to respond)
                                "response.refusal.delta" => {
                                    let delta = payload
                                        .get("delta")
                                        .and_then(|value| value.as_str())
                                        .ok_or_else(|| {
                                            StreamAssemblyError::MissingField("delta")
                                                .into_llm_error("OpenAI")
                                        })?;
                                    aggregated_content.push_str(delta);
                                    telemetry.on_content_delta(delta);
                                }
                                // Reasoning streams (thinking models like GPT-5 with reasoning)
                                "response.reasoning_text.delta" => {
                                    let delta = payload
                                        .get("delta")
                                        .and_then(|value| value.as_str())
                                        .ok_or_else(|| {
                                            StreamAssemblyError::MissingField("delta")
                                                .into_llm_error("OpenAI")
                                        })?;
                                    for fragment in append_reasoning_segments(&mut reasoning_buffer, delta, &telemetry) {
                                        yield provider::LLMStreamEvent::Reasoning { delta: fragment };
                                    }
                                }
                                "response.reasoning_summary_text.delta" => {
                                    let delta = payload
                                        .get("delta")
                                        .and_then(|value| value.as_str())
                                        .ok_or_else(|| {
                                            StreamAssemblyError::MissingField("delta")
                                                .into_llm_error("OpenAI")
                                        })?;
                                    // Treat summary the same as reasoning for now
                                    for fragment in append_reasoning_segments(&mut reasoning_buffer, delta, &telemetry) {
                                        yield provider::LLMStreamEvent::Reasoning { delta: fragment };
                                    }
                                }
                                // Function/tool call arguments streaming
                                "response.function_call_arguments.delta" => {
                                    // Tool arguments are streamed but we accumulate in response.completed's final object only.
                                    // We DO NOT push to aggregated_content to avoid polluting the text stream.
                                    // If strict tool call streaming is needed later, we can implement LLMStreamEvent::ToolCall here.
                                }
                                // Response completion with final state
                                "response.completed" => {
                                    if let Some(response_value) = payload.get("response") {
                                        final_response = Some(response_value.clone());
                                    }
                                    done = true;
                                }
                                // Error states
                                "response.failed" | "response.incomplete" => {
                                    let error_message = if let Some(err) = payload.get("response")
                                        .and_then(|r| r.get("error"))
                                    {
                                        err.get("message")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("Unknown error")
                                    } else {
                                        "Unknown error from Responses API"
                                    };
                                    let formatted_error = error_display::format_llm_error("OpenAI", error_message);
                                    Err(provider::LLMError::Provider {
                                        message: formatted_error,
                                        metadata: None,
                                    })?;
                                }
                                // Other stream events (in_progress, created, queued, etc.) - just ignore for now
                                _ => {}
                            }
                        }
                    }

                    if done {
                        break;
                    }
                }

                if done {
                    break;
                }
            }

            // Finalize sanitizer and yield leftover events
            for event in sanitizer.finalize() {
                yield event;
            }

            let response_value = match final_response {
                Some(value) => value,
                None => {
                    let formatted_error = error_display::format_llm_error(
                        "OpenAI",
                        "Stream ended without a completion event",
                    );
                    Err(provider::LLMError::Provider { message: formatted_error, metadata: None })?
                }
            };

            let mut response = parse_responses_payload(response_value, include_metrics)?;

            if response.content.is_none() && !aggregated_content.is_empty() {
                response.content = Some(aggregated_content.clone());
            }

            if let Some(reasoning_text) = reasoning_buffer.finalize() {
                response.reasoning = Some(reasoning_text);
            }

            #[cfg(debug_assertions)]
            {
                debug!(
                    target = "vtcode::llm::openai",
                    model = %debug_model,
                    elapsed_ms = request_timer.elapsed().as_millis(),
                    events = streamed_events_counter,
                    content_len = aggregated_content.len(),
                    "Completed streaming response"
                );
            }

            yield provider::LLMStreamEvent::Completed { response };
        };

        Ok(Box::pin(stream))
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

            let response = self
                .authorize(self.http_client.post(&url))
                .header("OpenAI-Beta", "responses=v1")
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
                } else if status.as_u16() == 429
                    || error_text.contains("insufficient_quota")
                    || error_text.contains("quota")
                    || error_text.contains("rate limit")
                {
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
                            let retry_response = self
                                .authorize(self.http_client.post(&url))
                                .header("OpenAI-Beta", "responses=v1")
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

            if status.as_u16() == 429
                || error_text.contains("insufficient_quota")
                || error_text.contains("quota")
                || error_text.contains("rate limit")
            {
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

#[cfg(test)]
mod streaming_tests {
    use super::*;

    #[test]
    fn test_gpt5_models_disable_streaming() {
        // Test that GPT-5 models return false for supports_streaming
        let test_models = [
            models::openai::GPT_5,
            models::openai::GPT_5_CODEX,
            models::openai::GPT_5_MINI,
            models::openai::GPT_5_NANO,
        ];

        for &model in &test_models {
            let provider = OpenAIProvider::with_model("test-key".to_owned(), model.to_owned());
            assert_eq!(
                provider.supports_streaming(),
                false,
                "Model {} should not support streaming",
                model
            );
        }
    }
}
#[cfg(test)]
mod caching_tests {
    use super::*;
    use crate::config::core::PromptCachingConfig;
    use serde_json::json;

    #[test]
    fn test_openai_prompt_cache_retention() {
        // Setup configuration with retention
        let mut config = PromptCachingConfig::default();
        config.enabled = true;
        config.providers.openai.enabled = true;
        config.providers.openai.prompt_cache_retention = Some("24h".to_string());

        // Initialize provider
        let provider =
            OpenAIProvider::from_config(Some("key".into()), None, None, Some(config), None, None);

        // Create a dummy request for a Responses API model
        // Must use an exact model name from RESPONSES_API_MODELS
        let request = provider::LLMRequest {
            messages: vec![provider::Message::user("Hello".to_string())],
            model: crate::config::constants::models::openai::GPT_5_1_CODEX.to_string(),
            ..Default::default()
        };

        // We need to access private method `convert_to_openai_responses_format`
        // OR we can test `convert_to_openai_format` if it calls it, but `convert_to_openai_format`
        // is for Chat Completions. The Responses API conversion is private.
        // However, since we are inside the module (submodule), we can access private methods of parent if we import them?
        // No, `mod caching_tests` is a child module. Parent private items are visible to child modules
        // in Rust 2018+ if we use `super::`.

        // Let's verify visibility. `convert_to_openai_responses_format` is private `fn`.
        // Child modules can verify it.

        let json_result = provider.convert_to_openai_responses_format(&request);

        assert!(json_result.is_ok());
        let json = json_result.unwrap();

        // Verify the field is present
        assert_eq!(json["prompt_cache_retention"], json!("24h"));
    }

    #[test]
    fn test_openai_prompt_cache_retention_skipped_for_chat_api() {
        // Setup configuration with retention
        let mut config = PromptCachingConfig::default();
        config.enabled = true;
        config.providers.openai.enabled = true;
        config.providers.openai.prompt_cache_retention = Some("24h".to_string());

        let provider =
            OpenAIProvider::from_config(Some("key".into()), None, None, Some(config), None, None);

        // Standard GPT-4o model (Chat Completions API)
        let request = provider::LLMRequest {
            messages: vec![provider::Message::user("Hello".to_string())],
            model: "gpt-4o".to_string(),
            ..Default::default()
        };

        // This uses the standard chat format conversion
        let json_result = provider.convert_to_openai_format(&request);
        assert!(json_result.is_ok());
        let json = json_result.unwrap();

        // Should NOT have prompt_cache_retention
        assert!(json.get("prompt_cache_retention").is_none());
    }
}
