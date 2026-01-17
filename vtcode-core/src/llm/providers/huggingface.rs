#![allow(
    clippy::result_large_err,
    clippy::bind_instead_of_map,
    clippy::collapsible_if
)]

use crate::config::TimeoutsConfig;
use crate::config::constants::{env_vars, models, urls};
use crate::config::core::{AnthropicConfig, PromptCachingConfig};
use crate::llm::client::LLMClient;
use crate::llm::error_display::format_llm_error;
use crate::llm::provider::ToolDefinition;
use crate::llm::provider::{
    LLMError, LLMErrorMetadata, LLMProvider, LLMRequest, LLMResponse, LLMStream, LLMStreamEvent,
    MessageRole,
};
use crate::llm::providers::shared::{
    NoopStreamTelemetry, StreamTelemetry, ToolCallBuilder, finalize_tool_calls, update_tool_calls,
};
use crate::llm::providers::tag_sanitizer::TagStreamSanitizer;
use crate::llm::types as llm_types;
use async_stream::try_stream;
use async_trait::async_trait;
use futures::StreamExt;
use reqwest::{Client as HttpClient, Response, StatusCode};
use serde_json::{Value, json};

use super::common::{
    map_finish_reason_common, override_base_url, parse_response_openai_format, resolve_model,
    serialize_messages_openai_format,
};
use super::error_handling::{format_network_error, format_parse_error};

const PROVIDER_NAME: &str = "HuggingFace";
const PROVIDER_KEY: &str = "huggingface";
const JSON_INSTRUCTION: &str = "Return JSON that matches the provided schema.";

/// Hugging Face Inference Providers - OpenAI-compatible LLM routing service
///
/// Provides unified access to hundreds of LLMs across 15+ inference providers
/// (Cerebras, Cohere, Fireworks, Groq, Together, Replicate, Sambanova, etc.)
///
/// **Base URL**: `https://router.huggingface.co/v1`
/// **Documentation**: https://huggingface.co/docs/inference-providers
///
/// ## Provider Selection
///
/// HuggingFace supports flexible provider routing by appending suffixes to model IDs:
///
/// ```text
/// # Auto-selection (default) - uses your preference order
/// deepseek-ai/DeepSeek-R1
///
/// # Performance-optimized - highest throughput
/// deepseek-ai/DeepSeek-R1:fastest
///
/// # Cost-optimized - lowest price per token
/// deepseek-ai/DeepSeek-R1:cheapest
///
/// # Explicit provider - force specific provider
/// deepseek-ai/DeepSeek-R1:together
/// MiniMaxAI/MiniMax-M2.1:novita
/// ```
///
/// Configure preferences: https://hf.co/settings/inference-providers
///
/// ## HuggingFace-Specific Behaviors (17 Total)
///
/// This provider implements 17 behaviors that differ from standard OpenAI API to ensure
/// compatibility with HuggingFace's router and the diverse set of backend providers:
///
/// 1. **Strict Tool Schema**: Only `{type, function}` fields (no OpenAI extensions)
/// 2. **max_tokens Field**: Uses `max_tokens` vs OpenAI's `max_completion_tokens`
/// 3. **No output_types**: Responses API excludes this (causes 400 errors)
/// 4. **No Penalty Params**: Skips `presence_penalty`/`frequency_penalty`
/// 5. **Flat Sampling**: Params at top level (not nested in `sampling_parameters`)
/// 6. **No Reasoning Params**: Skips OpenAI reasoning controls
/// 7. **No stream_options**: Excluded to avoid 400 errors
/// 8. **GLM Tool Handling**: GLM models disable tools in Chat API only
/// 9. **JSON Instructions**: Explicit "Return JSON" for Responses API schemas
/// 10. **Model Validation**: MiniMax-M2 requires `:novita` suffix
/// 11. **Responses API**: Disabled by default (beta/unstable)
/// 12. **No Model Fallback**: No automatic model rewrites
/// 13. **No Harmony**: Skips GPT-OSS Harmony pathway
/// 14. **Base URL Detection**: Identifies HF via URL patterns
/// 15. **HF-Specific Errors**: References HF docs and model catalogs
/// 16. **Prefer Responses for Tools**: Better tool orchestration
/// 17. **Tool Orchestration**: Built-in coordination and retry logic
///
/// ## API Compatibility
/// Implements both the Chat Completion API and the Responses API (beta):
/// - Chat Completions: https://huggingface.co/docs/inference-providers/tasks/chat-completion
/// - Responses API: https://huggingface.co/docs/inference-providers/guides/responses_api
///
/// ## Supported Features
///
/// ### Chat Completions API
/// - Conversational LLMs and VLMs (Vision-Language Models)
/// - Streaming with Server-Sent Events (SSE)
/// - Tool calling and parallel tool execution
/// - Structured output (JSON mode with `response_format`)
/// - Grammars and constraints
/// - Context caching (model-dependent)
///
/// ### Responses API (Beta)
/// - **Built-in tool orchestration** - Invoke functions, server-side MCP tools, and schema-validated outputs
/// - **Event-driven streaming** - Semantic events like `response.created`, `output_text.delta`, `response.completed`
/// - **Reasoning controls** - Dial up or down reasoning effort with `reasoning.effort` parameter
/// - **Structured outputs** - Require models to return schema-compliant JSON every time
/// - **Remote MCP tools** - Call server-hosted tools that implement the Model Context Protocol
///
/// ## Recommended Models
/// See: https://huggingface.co/docs/inference-providers/tasks/chat-completion#recommended-models
pub struct HuggingFaceProvider {
    api_key: String,
    http_client: HttpClient,
    base_url: String,
    model: String,
    _timeouts: TimeoutsConfig,
}

impl HuggingFaceProvider {
    pub fn new(api_key: String) -> Self {
        Self::with_model_internal(
            api_key,
            models::huggingface::DEFAULT_MODEL.to_string(),
            None,
            None,
        )
    }

    pub fn with_model(api_key: String, model: String) -> Self {
        Self::with_model_internal(api_key, model, None, None)
    }

    pub fn with_timeouts(api_key: String, timeouts: TimeoutsConfig) -> Self {
        Self::with_model_internal(
            api_key,
            models::huggingface::DEFAULT_MODEL.to_string(),
            None,
            Some(timeouts),
        )
    }

    fn with_model_internal(
        api_key: String,
        model: String,
        base_url: Option<String>,
        timeouts: Option<TimeoutsConfig>,
    ) -> Self {
        use crate::llm::http_client::HttpClientFactory;

        let timeouts = timeouts.unwrap_or_default();

        Self {
            api_key,
            http_client: HttpClientFactory::for_llm(&timeouts),
            base_url: override_base_url(
                urls::HUGGINGFACE_API_BASE,
                base_url,
                Some(env_vars::HUGGINGFACE_BASE_URL),
            ),
            model,
            _timeouts: timeouts,
        }
    }

    pub fn from_config(
        api_key: Option<String>,
        model: Option<String>,
        base_url: Option<String>,
        _prompt_cache: Option<PromptCachingConfig>,
        timeouts: Option<TimeoutsConfig>,
        _anthropic: Option<AnthropicConfig>,
    ) -> Self {
        let api_key_value = api_key.unwrap_or_default();
        let model_value = resolve_model(model, models::huggingface::DEFAULT_MODEL);
        Self::with_model_internal(api_key_value, model_value, base_url, timeouts)
    }

    /// Behavior 10: Normalize and validate HuggingFace model IDs
    ///
    /// **Provider Selection Suffixes**:
    /// - `:fastest` - Selects provider with highest throughput
    /// - `:cheapest` - Selects provider with lowest price per token
    /// - `:provider-name` - Forces specific provider (e.g., `:novita`, `:together`, `:groq`, `:zai-org`)
    ///
    /// All suffixes are preserved during validation and passed to the router.
    ///
    /// **Reference**: https://huggingface.co/docs/inference-providers/en/guide
    fn normalize_model_id(&self, model: &str) -> Result<String, LLMError> {
        let lower = model.to_ascii_lowercase();

        // Validate MiniMax-M2 requires provider suffix
        if lower.contains("minimax-m2") && !model.contains(':') {
            return Err(LLMError::Provider {
                message: format_llm_error(
                    PROVIDER_NAME,
                    "MiniMax models require explicit provider selection (:novita suffix). \
                    Use 'MiniMaxAI/MiniMax-M2.1:novita'.\n\n\
                    Provider selection guide: https://huggingface.co/docs/inference-providers/guide#provider-selection",
                ),
                metadata: None,
            });
        }

        // Validate GLM models require provider suffix (:zai-org or :novita)
        if lower.contains("glm-4") && !model.contains(':') {
            return Err(LLMError::Provider {
                message: format_llm_error(
                    PROVIDER_NAME,
                    "GLM models require explicit provider selection on HuggingFace.\n\n\
                    Options:\n\
                    - 'zai-org/GLM-4.7:zai-org' - Use Z.AI as inference provider\n\
                    - 'zai-org/GLM-4.7:novita' - Use Novita as inference provider\n\n\
                    Provider selection guide: https://huggingface.co/docs/inference-providers/guide#provider-selection",
                ),
                metadata: None,
            });
        }

        // Preserve all provider selection suffixes (:fastest, :cheapest, :provider-name)
        Ok(model.to_string())
    }

    /// Behavior 1: Serialize tools with nested format (OpenAI-compatible)
    /// Documentation confirmed that the /v1/chat/completions endpoint
    /// expects the same nested structure as OpenAI.
    fn serialize_tools_huggingface(&self, tools: &[ToolDefinition]) -> Option<Vec<Value>> {
        crate::llm::providers::common::serialize_tools_openai_format(tools)
    }

    /// Behavior 2,4,6,7: Format Chat Completions API request with HF-specific quirks
    fn format_for_chat_completions(&self, request: &LLMRequest) -> Result<Value, LLMError> {
        let mut messages = serialize_messages_openai_format(request, PROVIDER_KEY)?;
        let is_glm = self.is_glm_model(&request.model);

        // Prepend system prompt if it exists and isn't already there
        if let Some(system) = &request.system_prompt {
            let has_system = messages
                .first()
                .and_then(|m| m.get("role"))
                .and_then(|r| r.as_str())
                == Some("system");
            if !has_system {
                messages.insert(
                    0,
                    json!({
                        "role": "system",
                        "content": system
                    }),
                );
            }
        }

        let mut payload = json!({
            "model": request.model,
            "messages": messages,
            "stream": request.stream,
        });

        // Behavior 17: Enable tool streaming for GLM models (4.6, 4.7+)
        if request.stream && request.tools.is_some() && is_glm {
            payload["tool_stream"] = json!(true);
        }

        // Behavior 6: Thinking/Reasoning parameters are currently only supported via Responses API (beta)
        // Adding them to Chat Completions API causes "Extra inputs are not permitted, field: 'thinking'" errors
        // on Hugging Face router for models like DeepSeek-V3.2.

        // Behavior 2: Use max_tokens (not max_completion_tokens)
        if let Some(max_tokens) = request.max_tokens {
            payload["max_tokens"] = json!(max_tokens);
        }

        // Behavior 1: Serialize tools with nested OpenAI format
        // Behavior 8: GLM models on HF router now support tools in Chat API
        if let Some(tools) = &request.tools {
            if let Some(serialized) = self.serialize_tools_huggingface(tools) {
                payload["tools"] = json!(serialized);

                if let Some(choice) = &request.tool_choice {
                    payload["tool_choice"] = choice.to_provider_format("openai");
                }
            }
        }

        // Behavior 6: Add sampling params (GLM models support these)
        if let Some(temperature) = request.temperature {
            payload["temperature"] = json!(temperature);
        }

        if let Some(top_p) = request.top_p {
            payload["top_p"] = json!(top_p);
        }

        if let Some(top_k) = request.top_k {
            payload["top_k"] = json!(top_k);
        }

        // Behavior 4: Skip presence_penalty and frequency_penalty
        // (Don't add these parameters at all for HF)

        // Behavior 7: Don't add stream_options
        // (Skip even for native OpenAI compatibility)

        // GLM models via Z.AI don't support response_format with json_object
        // Skip this parameter for GLM to avoid 400 errors
        if request.output_format.is_some() && !is_glm {
            payload["response_format"] = json!({ "type": "json_object" });
        }

        Ok(payload)
    }

    /// Check if model is a GLM model (Z.AI backend)
    fn is_glm_model(&self, model: &str) -> bool {
        let lower = model.to_ascii_lowercase();
        lower.contains("glm")
    }

    /// Check if model is a DeepSeek model
    fn is_deepseek_model(&self, model: &str) -> bool {
        let lower = model.to_ascii_lowercase();
        lower.contains("deepseek")
    }

    /// Check if model is a MiniMax model
    fn is_minimax_model(&self, model: &str) -> bool {
        let lower = model.to_ascii_lowercase();
        lower.contains("minimax")
    }

    /// Apply model-specific default parameters
    fn apply_model_defaults(&self, request: &mut LLMRequest) {
        if self.is_minimax_model(&request.model) {
            // MiniMax recommended parameters: temperature=1.0, top_p=0.95, top_k=40
            if request.temperature.is_none() {
                request.temperature = Some(1.0);
            }
            if request.top_p.is_none() {
                request.top_p = Some(0.95);
            }
            if request.top_k.is_none() {
                request.top_k = Some(40);
            }
        }
    }

    /// Behavior 9: Add JSON schema instruction for Responses API
    fn add_json_instruction(&self, payload: &mut Value) -> Result<(), LLMError> {
        if let Some(instructions) = payload.get_mut("instructions") {
            if let Some(text) = instructions.as_str() {
                if !text.contains("Return JSON") {
                    *instructions = json!(format!("{}\n\n{}", text, JSON_INSTRUCTION));
                }
            }
        } else {
            payload["instructions"] = json!(JSON_INSTRUCTION);
        }

        Ok(())
    }

    /// Behavior 3,5,6,9: Format Responses API request with HF-specific quirks
    fn format_for_responses_api(&self, request: &LLMRequest) -> Result<Value, LLMError> {
        let mut input = Vec::new();

        // Convert messages
        for msg in &request.messages {
            // Helper to handle ContentPart conversion
            let convert_parts = |parts: &[crate::llm::provider::ContentPart]| -> Value {
                let parts_json: Vec<Value> = parts
                    .iter()
                    .map(|part| match part {
                        crate::llm::provider::ContentPart::Text { text } => {
                            json!({ "type": "input_text", "text": text })
                        }
                        crate::llm::provider::ContentPart::Image {
                            data, mime_type, ..
                        } => {
                            json!({
                                "type": "input_image",
                                "image_url": format!("data:{};base64,{}", mime_type, data)
                            })
                        }
                    })
                    .collect();
                json!(parts_json)
            };

            match msg.role {
                MessageRole::System | MessageRole::User => {
                    // Skip if first system message matches global instructions
                    if msg.role == MessageRole::System && request.system_prompt.is_some() {
                        // Check if we should skip this - we usually skip it if it's the first message
                        // and matches request.system_prompt exactly.
                        if let crate::llm::provider::MessageContent::Text(text) = &msg.content {
                            if Some(text) == request.system_prompt.as_ref() {
                                continue;
                            }
                        }
                    }

                    let role = if msg.role == MessageRole::System {
                        "system"
                    } else {
                        "user"
                    };

                    let mut message_obj = json!({
                        "type": "message",
                        "role": role,
                    });

                    match &msg.content {
                        crate::llm::provider::MessageContent::Text(text) => {
                            message_obj["content"] = json!(text);
                        }
                        crate::llm::provider::MessageContent::Parts(parts) => {
                            message_obj["content"] = convert_parts(parts);
                        }
                    }

                    input.push(message_obj);
                }
                MessageRole::Assistant => {
                    // 1. First, handle any text/multimodal content in a 'message' item
                    let has_content = match &msg.content {
                        crate::llm::provider::MessageContent::Text(text) => !text.is_empty(),
                        crate::llm::provider::MessageContent::Parts(parts) => !parts.is_empty(),
                    };

                    if has_content {
                        let mut message_obj = json!({
                            "type": "message",
                            "role": "assistant",
                        });

                        match &msg.content {
                            crate::llm::provider::MessageContent::Text(text) => {
                                message_obj["content"] = json!(text);
                            }
                            crate::llm::provider::MessageContent::Parts(parts) => {
                                message_obj["content"] = convert_parts(parts);
                            }
                        }

                        input.push(message_obj);
                    }

                    // 2. Then, handle each tool call as a separate 'function_call' item
                    // This is required by the Responses API schema (verified via Zod errors)
                    if let Some(tool_calls) = &msg.tool_calls {
                        for tc in tool_calls {
                            if let Some(func) = &tc.function {
                                input.push(json!({
                                    "type": "function_call",
                                    "call_id": tc.id,
                                    "name": func.name,
                                    "arguments": func.arguments
                                }));
                            }
                        }
                    }
                }
                MessageRole::Tool => {
                    // Tool results must use 'function_call_output' type
                    // Field name is 'call_id' (not 'tool_call_id') and 'output'
                    input.push(json!({
                        "type": "function_call_output",
                        "call_id": msg.tool_call_id.clone().unwrap_or_default(),
                        "output": match &msg.content {
                            crate::llm::provider::MessageContent::Text(text) => text.clone(),
                            crate::llm::provider::MessageContent::Parts(parts) => {
                                // For tool output, we usually expect a string (JSON result).
                                // If it's multipart, we join text parts.
                                parts.iter().filter_map(|p| match p {
                                    crate::llm::provider::ContentPart::Text { text } => Some(text.as_str()),
                                    _ => None
                                }).collect::<Vec<_>>().join("")
                            }
                        }
                    }));
                }
            }
        }

        let mut payload = json!({
            "model": request.model,
            "input": input,
            "stream": request.stream,
        });

        // Behavior 9: Use 'instructions' for system prompt
        if let Some(system_prompt) = &request.system_prompt {
            payload["instructions"] = json!(system_prompt);
        }

        // Add reasoning effort if set
        if let Some(effort) = request.reasoning_effort {
            use crate::config::types::ReasoningEffortLevel;
            if effort != ReasoningEffortLevel::None {
                payload["reasoning"] = json!({ "effort": effort.as_str() });
            }
        }

        // Behavior 3: Skip output_types (causes 400 errors on HF)
        // Don't add: payload["output_types"] = json!(["message", "tool_call"]);

        // Behavior 5: Flatten sampling parameters to top level
        if let Some(max_tokens) = request.max_tokens {
            payload["max_tokens"] = json!(max_tokens);
        }
        if let Some(temperature) = request.temperature {
            payload["temperature"] = json!(temperature);
        }
        if let Some(top_p) = request.top_p {
            payload["top_p"] = json!(top_p);
        }
        if let Some(top_k) = request.top_k {
            payload["top_k"] = json!(top_k);
        }

        if let Some(tools) = &request.tools {
            if let Some(serialized) = self.serialize_tools_huggingface(tools) {
                payload["tools"] = json!(serialized);

                if let Some(choice) = &request.tool_choice {
                    // Responses API often takes string choice or specific format
                    payload["tool_choice"] = choice.to_provider_format("openai");
                }
            }
        }

        // Behavior 9: Add JSON instruction if using schemas/tools
        if request.output_format.is_some() || request.tools.is_some() {
            self.add_json_instruction(&mut payload)?;
        }

        // Behavior: GLM models on HF (via Z.AI) don't support response_format with json_object
        if request.output_format.is_some() && !self.is_glm_model(&request.model) {
            payload["response_format"] = json!({ "type": "json_object" });
        }

        Ok(payload)
    }

    /// Behavior 11,16: Determine whether to use Responses API or Chat Completions API
    ///
    /// **IMPORTANT**: Responses API is DISABLED by default because:
    /// 1. It's beta/unstable on HuggingFace
    /// 2. Many backend providers (Novita, Together, Groq) don't support it
    /// 3. Chat Completions API handles tools correctly for most models
    ///
    /// To enable Responses API, you would need explicit opt-in configuration.
    fn should_use_responses_api(&self, _request: &LLMRequest) -> bool {
        // Behavior 11: Responses API is beta/unstable on HuggingFace.
        // It has caused issues with tool calling on many providers (Novita, Together)
        // and specifically DeepSeek and GLM models on the HF router.
        //
        // MANDATORY: Use Chat Completions API (/v1/chat/completions) which is more stable.
        false
    }

    /// Behavior 15: Format HF-specific error messages
    /// Behavior 15: HF-specific error messages with documentation links
    fn format_error(&self, status: StatusCode, body: &str) -> LLMError {
        let message = if status == StatusCode::NOT_FOUND {
            "Model not found on HuggingFace Inference Providers.\n\n\
                Ensure the model is:\n\
                1. Correctly spelled (e.g., 'deepseek-ai/DeepSeek-R1')\n\
                2. Available for text generation/chat completion\n\
                3. Enabled for at least one inference provider\n\n\
                Browse models: https://huggingface.co/models?pipeline_tag=text-generation&inference_provider=all\n\
                Docs: https://huggingface.co/docs/inference-providers/tasks/chat-completion".to_string()
        } else if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            format!(
                "Authentication failed ({}). \n\n\
                Check your HuggingFace token:\n\
                1. Get a token: https://huggingface.co/settings/tokens\n\
                2. Ensure 'Make calls to Inference Providers' permission is enabled\n\
                3. Set HF_TOKEN environment variable or pass to constructor\n\n\
                Error details: {}",
                status, body
            )
        } else if status == StatusCode::BAD_REQUEST {
            if body.contains("Invalid API parameter") || body.contains("not supported") {
                format!(
                    "Invalid request parameters ({}).\n\n\
                    HuggingFace has strict API validation. Common issues:\n\
                    - `stream_options` not supported (causes 400)\n\
                    - `output_types` not supported in some contexts\n\
                    - Provider-specific parameter limitations\n\n\
                    Error: {}\n\
                    API Docs: https://huggingface.co/docs/inference-providers/hub-api",
                    status, body
                )
            } else {
                format!(
                    "Bad request ({}).\n\n\
                    Error details: {}\n\
                    API Docs: https://huggingface.co/docs/inference-providers",
                    status, body
                )
            }
        } else if status == StatusCode::TOO_MANY_REQUESTS {
            format!(
                "Rate limit exceeded (429).\n\n\
                Consider:\n\
                1. Using a PRO account for higher limits: https://hf.co/subscribe/pro\n\
                2. Implementing exponential backoff retry logic\n\
                3. Spreading requests across multiple providers using :fastest or :cheapest suffixes\n\n\
                Error: {}",
                body
            )
        } else {
            format!(
                "HuggingFace API error ({}).\n\n\
                Error details: {}\n\
                Support: https://huggingface.co/docs/inference-providers",
                status, body
            )
        };

        LLMError::Provider {
                    message: format_llm_error(PROVIDER_NAME, &message),
            metadata: Some(LLMErrorMetadata::new(
                PROVIDER_NAME,
                Some(status.as_u16()),
                None, // code
                None, // request_id
                None, // organization_id
                None, // retry_after
                Some(body.to_string()), // message
            )),
        }
    }

    /// Parse Responses API format (output[] array with type-tagged items)
    fn parse_responses_api_format(json: &Value) -> Result<LLMResponse, LLMError> {
        // Check for convenience output_text helper first
        let convenience_text = json.get("output_text").and_then(|t| t.as_str());

        // Extract the full response object if nested
        let json_obj = if json.get("response").is_some() {
            json.get("response").unwrap()
        } else {
            json
        };

        let output = json_obj.get("output").and_then(|v| v.as_array());

        let output_arr = match output {
            Some(arr) => arr,
            None => {
                // If we have convenience text but no output array, return the text
                if let Some(text) = convenience_text {
                    return Ok(LLMResponse {
                        content: Some(text.to_string()),
                        tool_calls: None,
                        usage: None,
                        finish_reason: crate::llm::provider::FinishReason::Stop,
                        reasoning: None,
                        reasoning_details: None,
                        tool_references: Vec::new(),
                        request_id: None,
                        organization_id: None,
                    });
                }

                // Not a Responses API format, fall back to Chat Completions
                return Err(LLMError::Provider {
                    message: format_llm_error(PROVIDER_NAME, "Not a Responses API format"),
                    metadata: None,
                });
            }
        };

        let mut content_fragments: Vec<String> = Vec::new();
        let mut reasoning_fragments: Vec<String> = Vec::new();
        let mut tool_calls: Vec<crate::llm::provider::ToolCall> = Vec::new();

        for item in output_arr {
            let item_type = item.get("type").and_then(|t| t.as_str()).unwrap_or("");

            match item_type {
                "message" => {
                    // Extract content from message.content array
                    if let Some(content_arr) = item.get("content").and_then(|c| c.as_array()) {
                        for entry in content_arr {
                            let entry_type =
                                entry.get("type").and_then(|t| t.as_str()).unwrap_or("");
                            match entry_type {
                                "text" | "output_text" => {
                                    if let Some(text) = entry.get("text").and_then(|t| t.as_str()) {
                                        if !text.is_empty() {
                                            content_fragments.push(text.to_string());
                                        }
                                    }
                                }
                                "reasoning" => {
                                    if let Some(text) = entry.get("text").and_then(|t| t.as_str()) {
                                        if !text.is_empty() {
                                            reasoning_fragments.push(text.to_string());
                                        }
                                    }
                                }
                                "function_call" | "tool_call" => {
                                    if let Some(call) = Self::parse_responses_tool_call(entry) {
                                        tool_calls.push(call);
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
                "function_call" | "tool_call" => {
                    if let Some(call) = Self::parse_responses_tool_call(item) {
                        tool_calls.push(call);
                    }
                }
                "reasoning" => {
                    // Top-level reasoning item
                    if let Some(summary_arr) = item.get("summary").and_then(|s| s.as_array()) {
                        for summary in summary_arr {
                            if let Some(text) = summary.get("text").and_then(|t| t.as_str()) {
                                if !text.is_empty() {
                                    reasoning_fragments.push(text.to_string());
                                }
                            }
                        }
                    } else if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                        // Direct reasoning text
                        reasoning_fragments.push(text.to_string());
                    }
                }
                _ => {}
            }
        }

        // Use convenience text if fragments are empty
        let content = if content_fragments.is_empty() {
            convenience_text.map(|t| t.to_string())
        } else {
            Some(content_fragments.join(""))
        };

        let reasoning = if reasoning_fragments.is_empty() {
            None
        } else {
            Some(reasoning_fragments.join("\n\n"))
        };

        let finish_reason = if !tool_calls.is_empty() {
            crate::llm::provider::FinishReason::ToolCalls
        } else {
            crate::llm::provider::FinishReason::Stop
        };

        // Parse usage from response (checking both top level and response object)
        let usage_value = json.get("usage").or_else(|| json_obj.get("usage"));
        let usage = usage_value.map(|usage_value| crate::llm::provider::Usage {
            prompt_tokens: usage_value
                .get("input_tokens")
                .or_else(|| usage_value.get("prompt_tokens"))
                .and_then(|pt| pt.as_u64())
                .unwrap_or(0) as u32,
            completion_tokens: usage_value
                .get("output_tokens")
                .or_else(|| usage_value.get("completion_tokens"))
                .and_then(|ct| ct.as_u64())
                .unwrap_or(0) as u32,
            total_tokens: usage_value
                .get("total_tokens")
                .and_then(|tt| tt.as_u64())
                .unwrap_or(0) as u32,
            cached_prompt_tokens: None,
            cache_creation_tokens: None,
            cache_read_tokens: None,
        });

        Ok(LLMResponse {
            content,
            tool_calls: if tool_calls.is_empty() {
                None
            } else {
                Some(tool_calls)
            },
            usage,
            finish_reason,
            reasoning,
            reasoning_details: None,
            tool_references: Vec::new(),
            request_id: None,
            organization_id: None,
        })
    }

    /// Parse a tool call from Responses API format
    fn parse_responses_tool_call(item: &Value) -> Option<crate::llm::provider::ToolCall> {
        let call_id = item.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let function_obj = item.get("function").and_then(|v| v.as_object());
        let name = function_obj.and_then(|f| f.get("name").and_then(|n| n.as_str()))?;
        let arguments = function_obj.and_then(|f| f.get("arguments"));

        let serialized = arguments.map_or("{}".to_owned(), |args| {
            if args.is_string() {
                args.as_str().unwrap_or("{}").to_string()
            } else {
                args.to_string()
            }
        });

        Some(crate::llm::provider::ToolCall::function(
            call_id.to_string(),
            name.to_string(),
            serialized,
        ))
    }

    async fn parse_response(
        &self,
        response: Response,
        use_responses_api: bool,
    ) -> Result<LLMResponse, LLMError> {
        let status = response.status();

        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(self.format_error(status, &body));
        }

        let json: Value = response
            .json()
            .await
            .map_err(|err| format_parse_error(PROVIDER_NAME, &err))?;

        // Log response structure for debugging
        tracing::debug!(
            target: "vtcode::llm::huggingface",
            use_responses_api = use_responses_api,
            has_output = json.get("output").is_some(),
            has_choices = json.get("choices").is_some(),
            "Parsing HuggingFace response"
        );

        // Try Responses API format first if we used that endpoint
        if use_responses_api {
            if json.get("output").is_some() {
                return Self::parse_responses_api_format(&json);
            }
            // Fall through to Chat Completions format if output is missing
            tracing::warn!(
                target: "vtcode::llm::huggingface",
                "Responses API returned Chat Completions format, falling back to Chat parser"
            );
        }

        // Parse Chat Completions format
        parse_response_openai_format::<fn(&Value, &Value) -> Option<String>>(
            json,
            PROVIDER_NAME,
            false, // No prompt cache metrics for HF
            None,  // No custom reasoning extractor
        )
    }

    pub fn available_models() -> Vec<String> {
        models::huggingface::SUPPORTED_MODELS
            .iter()
            .map(|s| s.to_string())
            .collect()
    }
    /// Behavior 14: Resolve API endpoint based on selected mode
    fn get_endpoint(&self, use_responses_api: bool) -> String {
        let base = self.base_url.trim_end_matches('/');
        if use_responses_api {
            format!("{}/responses", base)
        } else {
            format!("{}/chat/completions", base)
        }
    }
}

#[async_trait]
impl LLMProvider for HuggingFaceProvider {
    fn name(&self) -> &str {
        PROVIDER_KEY
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_reasoning(&self, model: &str) -> bool {
        models::huggingface::REASONING_MODELS.contains(&model)
    }

    fn supports_reasoning_effort(&self, model: &str) -> bool {
        // Behavior 6: HF router supports 'thinking' parameter for GLM and some other models
        self.is_glm_model(model) || self.is_deepseek_model(model)
    }

    fn supports_tools(&self, _model: &str) -> bool {
        true // Most HF models support tools
    }

    fn supports_parallel_tool_config(&self, _model: &str) -> bool {
        false
    }

    fn supports_structured_output(&self, _model: &str) -> bool {
        true
    }

    fn supports_context_caching(&self, _model: &str) -> bool {
        false // HF doesn't currently expose prompt caching
    }

    fn effective_context_size(&self, _model: &str) -> usize {
        128_000 // Default context window
    }

    async fn generate(&self, mut request: LLMRequest) -> Result<LLMResponse, LLMError> {
        if request.model.trim().is_empty() {
            request.model = self.model.clone();
        }

        // Apply model-specific defaults before validation
        self.apply_model_defaults(&mut request);

        self.validate_request(&request)?;

        // Behavior 10: Normalize and validate model
        let model = self.normalize_model_id(&request.model)?;
        request.model = model;

        // Behavior 16: Prefer Responses API even with tools
        let use_responses_api = self.should_use_responses_api(&request);

        // Format request with HF-specific quirks
        let payload = if use_responses_api {
            self.format_for_responses_api(&request)?
        } else {
            self.format_for_chat_completions(&request)?
        };

        // Make HTTP request
        let endpoint = self.get_endpoint(use_responses_api);

        let response = self
            .http_client
            .post(&endpoint)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&payload)
            .send()
            .await
            .map_err(|err| format_network_error(PROVIDER_NAME, &err))?;

        self.parse_response(response, use_responses_api).await
    }

    async fn stream(&self, mut request: LLMRequest) -> Result<LLMStream, LLMError> {
        if request.model.trim().is_empty() {
            request.model = self.model.clone();
        }

        // Apply model-specific defaults before validation
        self.apply_model_defaults(&mut request);

        self.validate_request(&request)?;
        request.stream = true;

        // Behavior 10: Normalize and validate model
        let model = self.normalize_model_id(&request.model)?;
        request.model = model;

        // Behavior 16: Prefer Responses API even with tools
        let use_responses_api = self.should_use_responses_api(&request);

        // Format request with HF-specific quirks
        let payload = if use_responses_api {
            self.format_for_responses_api(&request)?
        } else {
            self.format_for_chat_completions(&request)?
        };

        // Make HTTP request
        let endpoint = self.get_endpoint(use_responses_api);

        let response = self
            .http_client
            .post(&endpoint)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&payload)
            .send()
            .await
            .map_err(|err| format_network_error(PROVIDER_NAME, &err))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(self.format_error(status, &body));
        }

        self.create_stream(response, use_responses_api).await
    }

    fn supported_models(&self) -> Vec<String> {
        Self::available_models()
    }

    fn validate_request(&self, request: &LLMRequest) -> Result<(), LLMError> {
        if request.messages.is_empty() {
            return Err(LLMError::InvalidRequest {
                message: format_llm_error(PROVIDER_NAME, "Messages cannot be empty"),
                metadata: None,
            });
        }

        if request.model.trim().is_empty() {
            return Err(LLMError::InvalidRequest {
                message: format_llm_error(PROVIDER_NAME, "Model identifier cannot be empty"),
                metadata: None,
            });
        }

        Ok(())
    }
}

// Streaming implementation
impl HuggingFaceProvider {
    async fn create_stream(
        &self,
        response: Response,
        use_responses_api: bool,
    ) -> Result<LLMStream, LLMError> {
        let mut bytes_stream = response.bytes_stream();
        let mut buffer = String::with_capacity(4096);
        let mut tool_calls: Vec<ToolCallBuilder> = Vec::new();
        let mut content_buffer = String::new();
        let mut reasoning_buffer = String::new();
        let telemetry = NoopStreamTelemetry;
        let mut sanitizer = TagStreamSanitizer::new();
        let mut completed = false;

        let stream = try_stream! {
            while let Some(chunk_result) = bytes_stream.next().await {
                let chunk = chunk_result.map_err(|err| format_network_error(PROVIDER_NAME, &err))?;
                let text = String::from_utf8_lossy(&chunk);
                buffer.push_str(&text);

                // Behavior 15: Enforce maximum buffer size to prevent memory exhaustion
                // from malformed streams without newlines.
                if buffer.len() > 128_000 {
                    Err(LLMError::Provider {
                        message: format_llm_error(PROVIDER_NAME, "Stream buffer exceeded maximum size (128KB)"),
                        metadata: None,
                    })?;
                }

                // Process complete SSE events
                while let Some(newline_pos) = buffer.find('\n') {
                    let line = buffer[..newline_pos].trim().to_string();
                    buffer.drain(..=newline_pos);

                    if line.is_empty() || line.starts_with(':') {
                        continue;
                    }

                    let data = if let Some(stripped) = line.strip_prefix("data: ") {
                        stripped
                    } else {
                        continue;
                    };

                    if data == "[DONE]" {
                        // Stream ended with [DONE] - emit completion if we haven't already
                        if !completed && !content_buffer.is_empty() {
                            completed = true;
                            let final_tool_calls = finalize_tool_calls(tool_calls.clone());
                            let finish_reason = if final_tool_calls.is_some() {
                                crate::llm::provider::FinishReason::ToolCalls
                            } else {
                                crate::llm::provider::FinishReason::Stop
                            };
                            let response = LLMResponse {
                                content: Some(content_buffer.clone()),
                                tool_calls: final_tool_calls,
                                usage: None,
                                finish_reason,
                                reasoning: if reasoning_buffer.is_empty() { None } else { Some(reasoning_buffer.clone()) },
                                reasoning_details: None,
                                tool_references: Vec::new(),
                                request_id: None,
                                organization_id: None,
                            };
                            yield LLMStreamEvent::Completed { response };
                        }
                        break;
                    }

                    let event: Value = match serde_json::from_str(data) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };

                    // Try Responses API format first if we're using it
                    if use_responses_api {
                        let event_type = event.get("type").and_then(|t| t.as_str()).unwrap_or("");

                        match event_type {
                            // Text content delta (unified across prefixed and unprefixed)
                            "response.output_text.delta" | "output_text.delta" => {
                                if let Some(delta) = event.get("delta").and_then(|d| d.as_str()) {
                                    for ev in sanitizer.process_chunk(delta) {
                                        match &ev {
                                            LLMStreamEvent::Token { delta } => {
                                                telemetry.on_content_delta(delta);
                                                content_buffer.push_str(delta);
                                                yield ev;
                                            }
                                            _ => yield ev,
                                        }
                                    }
                                }
                                continue;
                            }
                            // Reasoning delta
                            "response.reasoning.delta" | "reasoning.delta" => {
                                if let Some(delta) = event.get("delta").and_then(|d| d.as_str()) {
                                    telemetry.on_reasoning_delta(delta);
                                    reasoning_buffer.push_str(delta);
                                }
                                continue;
                            }
                            // Function call arguments delta
                            "response.function_call_arguments.delta" | "tool_call.delta" => {
                                telemetry.on_tool_call_delta();
                                continue;
                            }
                            // Response completed
                            "response.completed" => {
                                if let Some(response_obj) = event.get("response") {
                                    // Parse the completed response using our Responses API parser
                                    match Self::parse_responses_api_format(response_obj) {
                                        Ok(mut response) => {
                                            // Merge with any buffered content
                                            if response.content.is_none() && !content_buffer.is_empty() {
                                                response.content = Some(content_buffer.clone());
                                            }
                                            if response.reasoning.is_none() && !reasoning_buffer.is_empty() {
                                                response.reasoning = Some(reasoning_buffer.clone());
                                            }
                                            completed = true;
                                            yield LLMStreamEvent::Completed { response };
                                        }
                                        Err(_) => {
                                            // Fallback: create response from buffers
                                            let finish_reason = crate::llm::provider::FinishReason::Stop;
                                            let response = LLMResponse {
                                                content: if content_buffer.is_empty() { None } else { Some(content_buffer.clone()) },
                                                tool_calls: None,
                                                usage: None,
                                                finish_reason,
                                                reasoning: if reasoning_buffer.is_empty() { None } else { Some(reasoning_buffer.clone()) },
                                                reasoning_details: None,
                                                tool_references: Vec::new(),
                                                request_id: None,
                                                organization_id: None,
                                            };
                                            completed = true;
                                            yield LLMStreamEvent::Completed { response };
                                        }
                                    }
                                }
                                break;
                            }
                            // Response done
                            "response.done" => {
                                let finish_reason = crate::llm::provider::FinishReason::Stop;
                                let response = LLMResponse {
                                    content: if content_buffer.is_empty() { None } else { Some(content_buffer.clone()) },
                                    tool_calls: finalize_tool_calls(tool_calls.clone()),
                                    usage: None,
                                    finish_reason,
                                    reasoning: if reasoning_buffer.is_empty() { None } else { Some(reasoning_buffer.clone()) },
                                    reasoning_details: None,
                                    tool_references: Vec::new(),
                                    request_id: None,
                                    organization_id: None,
                                };
                                completed = true;
                                yield LLMStreamEvent::Completed { response };
                                break;
                            }
                            _ => {}
                        }
                    }

                    // Parse Chat Completions format (delta from stream chunk)
                    let choices = event.get("choices").and_then(|c| c.as_array());
                    if let Some(choices_arr) = choices {
                        if let Some(choice) = choices_arr.first() {
                            let delta = choice.get("delta");
                            if let Some(delta_obj) = delta {
                                // Content delta
                                if let Some(content) = delta_obj.get("content").and_then(|c| c.as_str()) {
                                    for event in sanitizer.process_chunk(content) {
                                        match &event {
                                            LLMStreamEvent::Token { delta } => {
                                                telemetry.on_content_delta(delta);
                                                content_buffer.push_str(delta);
                                                yield event;
                                            }
                                            _ => yield event,
                                        }
                                    }
                                }

                                // Reasoning delta
                                if let Some(reasoning) = delta_obj.get("reasoning_content").and_then(|r| r.as_str()) {
                                    telemetry.on_reasoning_delta(reasoning);
                                    reasoning_buffer.push_str(reasoning);
                                }

                                // Tool calls delta
                                if let Some(tool_calls_arr) = delta_obj.get("tool_calls").and_then(|tc| tc.as_array()) {
                                    update_tool_calls(&mut tool_calls, tool_calls_arr);
                                    telemetry.on_tool_call_delta();
                                }
                            }

                            // Check for finish_reason
                            if let Some(finish_reason_str) = choice.get("finish_reason").and_then(|fr| fr.as_str()) {
                                let finish_reason = map_finish_reason_common(finish_reason_str);
                                let final_tool_calls = finalize_tool_calls(tool_calls.clone());

                                // Parse usage if present
                                let usage = event.get("usage").map(|usage_value| {
                                    crate::llm::provider::Usage {
                                        prompt_tokens: usage_value
                                            .get("prompt_tokens")
                                            .and_then(|pt| pt.as_u64())
                                            .unwrap_or(0) as u32,
                                        completion_tokens: usage_value
                                            .get("completion_tokens")
                                            .and_then(|ct| ct.as_u64())
                                            .unwrap_or(0) as u32,
                                        total_tokens: usage_value
                                            .get("total_tokens")
                                            .and_then(|tt| tt.as_u64())
                                            .unwrap_or(0) as u32,
                                        cached_prompt_tokens: None,
                                        cache_creation_tokens: None,
                                        cache_read_tokens: None,
                                    }
                                });

                                let response = LLMResponse {
                                    content: if content_buffer.is_empty() { None } else { Some(content_buffer.clone()) },
                                    tool_calls: final_tool_calls,
                                    usage,
                                    finish_reason,
                                    reasoning: if reasoning_buffer.is_empty() { None } else { Some(reasoning_buffer.clone()) },
                                    reasoning_details: None,
                                    tool_references: Vec::new(),
                                    request_id: None,
                                    organization_id: None,
                                };

                                completed = true;
                                yield LLMStreamEvent::Completed { response };
                                break;
                            }
                        }
                    }
                }
            }

            // If stream ended without a completion event, emit one now
            if !completed {
                let final_tool_calls = finalize_tool_calls(tool_calls.clone());
                let finish_reason = if final_tool_calls.is_some() {
                    crate::llm::provider::FinishReason::ToolCalls
                } else {
                    crate::llm::provider::FinishReason::Stop
                };
                let response = LLMResponse {
                    content: if content_buffer.is_empty() { None } else { Some(content_buffer.clone()) },
                    tool_calls: final_tool_calls,
                    usage: None,
                    finish_reason,
                    reasoning: if reasoning_buffer.is_empty() { None } else { Some(reasoning_buffer.clone()) },
                    reasoning_details: None,
                    tool_references: Vec::new(),
                    request_id: None,
                    organization_id: None,
                };
                yield LLMStreamEvent::Completed { response };
            }
        };

        Ok(Box::pin(stream))
    }
}

#[async_trait]
impl LLMClient for HuggingFaceProvider {
    async fn generate(&mut self, prompt: &str) -> Result<llm_types::LLMResponse, LLMError> {
        use super::common::convert_usage_to_llm_types;

        let request = LLMRequest {
            messages: vec![crate::llm::provider::Message::user(prompt.to_string())],
            model: self.model.clone(),
            ..Default::default()
        };
        let model = request.model.clone();
        let response = LLMProvider::generate(self, request).await?;

        Ok(llm_types::LLMResponse {
            content: response.content.unwrap_or_default(),
            model,
            usage: response.usage.map(convert_usage_to_llm_types),
            reasoning: response.reasoning,
            request_id: response.request_id,
            organization_id: response.organization_id,
        })
    }

    fn backend_kind(&self) -> llm_types::BackendKind {
        llm_types::BackendKind::HuggingFace
    }

    fn model_id(&self) -> &str {
        &self.model
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::provider::{FunctionCall, Message, MessageContent, ToolCall};

    #[test]
    fn test_format_for_responses_api_tools() {
        let provider = HuggingFaceProvider::new("test-key".to_string());
        let mut request = LLMRequest::default();
        request.model = "test-model".to_string();
        request.messages = vec![
            Message::user("What's the weather?".to_string()),
            Message {
                role: MessageRole::Assistant,
                content: MessageContent::Text("Let me check.".to_string()),
                tool_calls: Some(vec![ToolCall {
                    id: "call_123".to_string(),
                    call_type: "function".to_string(),
                    function: Some(FunctionCall {
                        name: "get_weather".to_string(),
                        arguments: "{\"location\":\"London\"}".to_string(),
                    }),
                    text: None,
                    thought_signature: None,
                }]),
                ..Default::default()
            },
            Message::tool_response("call_123".to_string(), "Sunny".to_string()),
        ];

        let result = provider
            .format_for_responses_api(&request)
            .expect("Failed to format for Responses API");
        let input = result["input"]
            .as_array()
            .expect("Input should be an array");

        assert_eq!(
            input.len(),
            4,
            "Should have 4 items: user request, assistant text, assistant tool call, tool output"
        );

        // User message
        assert_eq!(input[0]["type"], "message");
        assert_eq!(input[0]["role"], "user");
        assert_eq!(input[0]["content"], "What's the weather?");

        // Assistant text part
        assert_eq!(input[1]["type"], "message");
        assert_eq!(input[1]["role"], "assistant");
        assert_eq!(input[1]["content"], "Let me check.");

        // Assistant tool call part
        assert_eq!(input[2]["type"], "function_call");
        assert_eq!(input[2]["call_id"], "call_123");
        assert_eq!(input[2]["name"], "get_weather");
        assert_eq!(input[2]["arguments"], "{\"location\":\"London\"}");

        // Tool output part
        assert_eq!(input[3]["type"], "function_call_output");
        assert_eq!(input[3]["call_id"], "call_123");
        assert_eq!(input[3]["output"], "Sunny");
    }

    #[test]
    fn test_should_use_responses_api_is_always_false() {
        let provider = HuggingFaceProvider::new("test-key".to_string());
        let mut request = LLMRequest::default();

        // All models should use Chat Completion API by default for stability
        request.model = "meta-llama/Llama-3.3-70B-Instruct".to_string();
        assert!(!provider.should_use_responses_api(&request));

        request.model = "deepseek-ai/DeepSeek-V3.2".to_string();
        assert!(!provider.should_use_responses_api(&request));

        request.model = "zai-org/GLM-4.7:novita".to_string();
        assert!(!provider.should_use_responses_api(&request));
    }

    #[test]
    fn test_format_chat_completions_glm_tool_stream() {
        let provider = HuggingFaceProvider::new("test-key".to_string());
        let mut request = LLMRequest::default();
        request.model = "zai-org/GLM-4.7:novita".to_string();
        request.messages = vec![Message::user("Hello".to_string())];
        request.stream = true;
        request.tools = Some(vec![ToolDefinition::function(
            "test_tool".to_string(),
            "test".to_string(),
            json!({}),
        )]);

        let result = provider
            .format_for_chat_completions(&request)
            .expect("Failed to format");
        assert_eq!(result["tool_stream"], true);

        // Non-GLM should not have tool_stream
        request.model = "meta-llama/Llama-3.3-70B-Instruct".to_string();
        let result = provider
            .format_for_chat_completions(&request)
            .expect("Failed to format");
        assert!(result.get("tool_stream").is_none());
    }

    #[test]
    fn test_minimax_model_defaults() {
        let provider = HuggingFaceProvider::new("test-key".to_string());
        let mut request = LLMRequest::default();

        // Test MiniMax-M2.1:novita
        request.model = "MiniMaxAI/MiniMax-M2.1:novita".to_string();
        provider.apply_model_defaults(&mut request);

        assert_eq!(request.temperature, Some(1.0));
        assert_eq!(request.top_p, Some(0.95));
        assert_eq!(request.top_k, Some(40));

        // Test MiniMax-M2.1:novita
        let mut request2 = LLMRequest::default();
        request2.model = "MiniMaxAI/MiniMax-M2.1:novita".to_string();
        provider.apply_model_defaults(&mut request2);

        assert_eq!(request2.temperature, Some(1.0));
        assert_eq!(request2.top_p, Some(0.95));
        assert_eq!(request2.top_k, Some(40));

        // Test that existing values are not overridden
        let mut request3 = LLMRequest::default();
        request3.model = "MiniMaxAI/MiniMax-M2.1:novita".to_string();
        request3.temperature = Some(0.5);
        request3.top_p = Some(0.9);
        request3.top_k = Some(20);
        provider.apply_model_defaults(&mut request3);

        assert_eq!(request3.temperature, Some(0.5)); // Should not change
        assert_eq!(request3.top_p, Some(0.9)); // Should not change
        assert_eq!(request3.top_k, Some(20)); // Should not change

        // Test non-MiniMax model
        let mut request4 = LLMRequest::default();
        request4.model = "deepseek-ai/DeepSeek-V3.2:novita".to_string();
        provider.apply_model_defaults(&mut request4);

        assert_eq!(request4.temperature, None); // Should remain None
        assert_eq!(request4.top_p, None); // Should remain None
        assert_eq!(request4.top_k, None); // Should remain None
    }
}
