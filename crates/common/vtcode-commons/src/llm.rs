//! Core LLM types shared across the project

use serde::{Deserialize, Serialize, ser::SerializeStruct};
use std::fmt;

use crate::sanitizer::sanitize_provider_diagnostic;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackendKind {
    Gemini,
    OpenAI,
    Anthropic,
    DeepSeek,
    Meta,
    Mistral,
    OpenRouter,
    Ollama,
    LlamaCpp,
    ZAI,
    Moonshot,
    HuggingFace,
    Minimax,
    MiMo,
    OpenCodeZen,
    OpenCodeGo,
    Qwen,
    StepFun,
    Evolink,
    Poolside,
    Xai,
    Nvidia,
    MergeGateway,
    Vercel,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    pub cached_prompt_tokens: Option<u32>,
    pub cache_creation_tokens: Option<u32>,
    pub cache_read_tokens: Option<u32>,
    /// Per-iteration token usage for Anthropic server-side fallback, advisor,
    /// and compaction passes. Each entry represents one sampling pass.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iterations: Option<Vec<serde_json::Value>>,
}

/// Usage totals across every provider sampling iteration.
///
/// Anthropic's compaction response reports only the final message iteration in
/// the top-level fields. Keeping this as a named shape avoids making callers
/// reconstruct the billing totals from an untyped JSON array.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UsageTotals {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    pub cache_read_tokens: u32,
    pub cache_creation_tokens: u32,
}

impl Usage {
    #[inline]
    pub fn billable_totals(&self) -> UsageTotals {
        let mut totals = UsageTotals::default();
        let mut recognized_iteration = false;
        let mut saw_iteration_cache_read = false;
        let mut saw_iteration_cache_creation = false;

        if let Some(iterations) = &self.iterations {
            for iteration in iterations {
                let Some(object) = iteration.as_object() else {
                    continue;
                };
                let Some(iteration_type) = object.get("type").and_then(serde_json::Value::as_str) else {
                    continue;
                };
                if !matches!(iteration_type, "message" | "fallback_message" | "advisor_message" | "compaction") {
                    continue;
                }

                recognized_iteration = true;
                totals.prompt_tokens = totals.prompt_tokens.saturating_add(json_u32(object.get("input_tokens")));
                totals.completion_tokens =
                    totals.completion_tokens.saturating_add(json_u32(object.get("output_tokens")));

                if let Some(cache_read) = object.get("cache_read_input_tokens").and_then(serde_json::Value::as_u64) {
                    saw_iteration_cache_read = true;
                    totals.cache_read_tokens = totals.cache_read_tokens.saturating_add(saturating_u32(cache_read));
                }
                if let Some(cache_creation) =
                    object.get("cache_creation_input_tokens").and_then(serde_json::Value::as_u64)
                {
                    saw_iteration_cache_creation = true;
                    totals.cache_creation_tokens =
                        totals.cache_creation_tokens.saturating_add(saturating_u32(cache_creation));
                }
            }
        }

        if !recognized_iteration {
            totals.prompt_tokens = self.prompt_tokens;
            totals.completion_tokens = self.completion_tokens;
            totals.cache_read_tokens = self.cache_read_tokens_or_fallback();
            totals.cache_creation_tokens = self.cache_creation_tokens_or_zero();
        } else {
            // Some Anthropic streaming events carry iteration token counts but
            // leave cache metrics only at the top level. Use those fields when
            // no iteration supplied the corresponding metric, without
            // double-counting a metric that was present in an iteration.
            if !saw_iteration_cache_read {
                totals.cache_read_tokens = self.cache_read_tokens_or_fallback();
            }
            if !saw_iteration_cache_creation {
                totals.cache_creation_tokens = self.cache_creation_tokens_or_zero();
            }
        }

        totals.total_tokens = totals.prompt_tokens.saturating_add(totals.completion_tokens);
        totals
    }

    #[inline]
    fn has_cache_read_metric(&self) -> bool {
        self.cache_read_tokens.is_some() || self.cached_prompt_tokens.is_some()
    }

    #[inline]
    fn has_any_cache_metrics(&self) -> bool {
        self.has_cache_read_metric() || self.cache_creation_tokens.is_some()
    }

    #[inline]
    pub fn cache_read_tokens_or_fallback(&self) -> u32 {
        self.cache_read_tokens.or(self.cached_prompt_tokens).unwrap_or(0)
    }

    #[inline]
    pub fn cache_creation_tokens_or_zero(&self) -> u32 {
        self.cache_creation_tokens.unwrap_or(0)
    }

    #[inline]
    pub fn cache_hit_rate(&self) -> Option<f64> {
        if !self.has_any_cache_metrics() {
            return None;
        }
        let read = self.cache_read_tokens_or_fallback() as f64;
        let creation = self.cache_creation_tokens_or_zero() as f64;
        let total = read + creation;
        if total > 0.0 {
            Some((read / total) * 100.0)
        } else {
            None
        }
    }

    #[inline]
    fn is_cache_hit(&self) -> Option<bool> {
        self.has_any_cache_metrics().then(|| self.cache_read_tokens_or_fallback() > 0)
    }

    #[inline]
    fn is_cache_miss(&self) -> Option<bool> {
        self.has_any_cache_metrics()
            .then(|| self.cache_creation_tokens_or_zero() > 0 && self.cache_read_tokens_or_fallback() == 0)
    }

    #[inline]
    fn total_cache_tokens(&self) -> u32 {
        let read = self.cache_read_tokens_or_fallback();
        let creation = self.cache_creation_tokens_or_zero();
        read + creation
    }

    #[inline]
    fn cache_savings_ratio(&self) -> Option<f64> {
        if !self.has_cache_read_metric() {
            return None;
        }
        let read = self.cache_read_tokens_or_fallback() as f64;
        let prompt = self.prompt_tokens as f64;
        if prompt > 0.0 { Some(read / prompt) } else { None }
    }
}

/// Provider-agnostic balance information for account status display.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BalanceInfo {
    /// Human-readable balance string (e.g. "100.00¥", "$50.00").
    pub display: String,
    /// Whether the account has sufficient balance for API calls.
    pub is_available: bool,
}

/// DeepSeek-specific balance info from GET /user/balance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeepSeekBalanceResponse {
    is_available: bool,
    balance_infos: Vec<DeepSeekCurrencyBalance>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeepSeekCurrencyBalance {
    currency: String,
    total_balance: String,
    #[serde(default)]
    granted_balance: String,
    #[serde(default)]
    topped_up_balance: String,
}

impl From<DeepSeekBalanceResponse> for BalanceInfo {
    fn from(resp: DeepSeekBalanceResponse) -> Self {
        let display = resp
            .balance_infos
            .first()
            .map(|b| {
                let symbol = match b.currency.as_str() {
                    "CNY" => "¥",
                    "USD" => "$",
                    _ => &b.currency,
                };
                format!("{}{}", b.total_balance, symbol)
            })
            .unwrap_or_else(|| "N/A".to_string());
        BalanceInfo { display, is_available: resp.is_available }
    }
}

#[inline]
fn saturating_u32(value: u64) -> u32 {
    u32::try_from(value.min(u64::from(u32::MAX))).unwrap_or(u32::MAX)
}

#[inline]
fn json_u32(value: Option<&serde_json::Value>) -> u32 {
    value.and_then(serde_json::Value::as_u64).map_or(0, saturating_u32)
}

#[cfg(test)]
mod usage_tests {
    use super::Usage;
    use serde_json::json;

    #[test]
    fn cache_helpers_fall_back_to_cached_prompt_tokens() {
        let usage = Usage {
            prompt_tokens: 1_000,
            completion_tokens: 200,
            total_tokens: 1_200,
            cached_prompt_tokens: Some(600),
            cache_creation_tokens: Some(150),
            cache_read_tokens: None,
            iterations: None,
        };

        assert_eq!(usage.cache_read_tokens_or_fallback(), 600);
        assert_eq!(usage.cache_creation_tokens_or_zero(), 150);
        assert_eq!(usage.total_cache_tokens(), 750);
        assert_eq!(usage.is_cache_hit(), Some(true));
        assert_eq!(usage.is_cache_miss(), Some(false));
        assert_eq!(usage.cache_savings_ratio(), Some(0.6));
        assert_eq!(usage.cache_hit_rate(), Some(80.0));
    }

    #[test]
    fn cache_helpers_preserve_unknown_without_metrics() {
        let usage = Usage {
            prompt_tokens: 1_000,
            completion_tokens: 200,
            total_tokens: 1_200,
            cached_prompt_tokens: None,
            cache_creation_tokens: None,
            cache_read_tokens: None,
            iterations: None,
        };

        assert_eq!(usage.total_cache_tokens(), 0);
        assert_eq!(usage.is_cache_hit(), None);
        assert_eq!(usage.is_cache_miss(), None);
        assert_eq!(usage.cache_savings_ratio(), None);
        assert_eq!(usage.cache_hit_rate(), None);
    }

    #[test]
    fn billable_totals_aggregate_recognized_iterations() {
        let usage = Usage {
            prompt_tokens: 10,
            completion_tokens: 1,
            total_tokens: 11,
            cached_prompt_tokens: None,
            cache_creation_tokens: Some(2),
            cache_read_tokens: Some(3),
            iterations: Some(vec![
                json!({
                    "type": "compaction",
                    "input_tokens": 50,
                    "output_tokens": 5,
                    "cache_creation_input_tokens": 4,
                    "cache_read_input_tokens": 6,
                }),
                json!({
                    "type": "message",
                    "input_tokens": 10,
                    "output_tokens": 2,
                    "cache_creation_input_tokens": 1,
                    "cache_read_input_tokens": 2,
                }),
            ]),
        };

        assert_eq!(
            usage.billable_totals(),
            super::UsageTotals {
                prompt_tokens: 60,
                completion_tokens: 7,
                total_tokens: 67,
                cache_read_tokens: 8,
                cache_creation_tokens: 5,
            }
        );
    }

    #[test]
    fn billable_totals_fall_back_to_top_level_without_known_iterations() {
        let usage = Usage {
            prompt_tokens: 10,
            completion_tokens: 1,
            total_tokens: 11,
            cached_prompt_tokens: Some(4),
            cache_creation_tokens: Some(2),
            cache_read_tokens: None,
            iterations: Some(vec![json!({"type": "future_iteration"})]),
        };

        assert_eq!(
            usage.billable_totals(),
            super::UsageTotals {
                prompt_tokens: 10,
                completion_tokens: 1,
                total_tokens: 11,
                cache_read_tokens: 4,
                cache_creation_tokens: 2,
            }
        );
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum FinishReason {
    #[default]
    Stop,
    Length,
    ToolCalls,
    ContentFilter,
    Pause,
    Refusal,
    Error(String),
}

/// Universal tool call that matches OpenAI/Anthropic/Gemini specifications
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    /// Unique identifier for this tool call (e.g., "call_123")
    pub id: String,

    /// The type of tool call: "function", "custom" (GPT-5 freeform), or other
    #[serde(rename = "type")]
    pub call_type: String,

    /// Function call details (for function-type tools)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function: Option<FunctionCall>,

    /// Raw text payload (for custom freeform tools in GPT-5)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,

    /// Gemini-specific thought signature for maintaining reasoning context
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thought_signature: Option<String>,
}

/// Function call within a tool call
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionCall {
    /// Optional namespace for grouped or deferred tools.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,

    /// The name of the function to call
    pub name: String,

    /// The arguments to pass to the function, as a JSON string
    pub arguments: String,
}

impl ToolCall {
    /// Create a new function tool call
    pub fn function(id: String, name: String, arguments: String) -> Self {
        Self::function_with_namespace(id, None, name, arguments)
    }

    /// Create a new function tool call with an optional namespace.
    pub fn function_with_namespace(id: String, namespace: Option<String>, name: String, arguments: String) -> Self {
        Self {
            id,
            call_type: "function".to_owned(),
            function: Some(FunctionCall { namespace, name, arguments }),
            text: None,
            thought_signature: None,
        }
    }

    /// Create a new custom tool call with raw text payload (GPT-5 freeform)
    pub fn custom(id: String, name: String, text: String) -> Self {
        Self {
            id,
            call_type: "custom".to_owned(),
            function: Some(FunctionCall { namespace: None, name, arguments: text.clone() }),
            text: Some(text),
            thought_signature: None,
        }
    }

    /// Returns true when this tool call uses GPT-5 custom/freeform semantics.
    pub fn is_custom(&self) -> bool {
        self.call_type == "custom"
    }

    /// Returns the tool name when the call includes function details.
    pub fn tool_name(&self) -> Option<&str> {
        self.function.as_ref().map(|function| function.name.as_str())
    }

    /// Returns the raw payload text exactly as emitted by the model.
    pub fn raw_input(&self) -> Option<&str> {
        self.text
            .as_deref()
            .or_else(|| self.function.as_ref().map(|function| function.arguments.as_str()))
    }

    /// Parse the arguments as JSON Value (for function-type tools)
    pub fn parsed_arguments(&self) -> Result<serde_json::Value, serde_json::Error> {
        if let Some(ref func) = self.function {
            parse_tool_arguments(&func.arguments)
        } else {
            // Return an error by trying to parse invalid JSON
            serde_json::from_str("")
        }
    }

    /// Returns the execution payload for this tool call.
    ///
    /// Function tools keep their JSON semantics. Custom tools execute with their
    /// raw text payload wrapped as a JSON string value so freeform inputs can
    /// flow through the existing tool pipeline.
    pub fn execution_arguments(&self) -> Result<serde_json::Value, serde_json::Error> {
        if self.is_custom() {
            return Ok(serde_json::Value::String(self.raw_input().unwrap_or_default().to_string()));
        }

        self.parsed_arguments()
    }

    /// Validate that this tool call is properly formed
    pub fn validate(&self) -> Result<(), String> {
        if self.id.is_empty() {
            return Err("Tool call ID cannot be empty".to_owned());
        }

        match self.call_type.as_str() {
            "function" => {
                if let Some(func) = &self.function {
                    if func.name.is_empty() {
                        return Err("Function name cannot be empty".to_owned());
                    }
                    // Validate that arguments is valid JSON for function tools
                    if let Err(e) = self.parsed_arguments() {
                        return Err(format!("Invalid JSON in function arguments: {e}"));
                    }
                } else {
                    return Err("Function tool call missing function details".to_owned());
                }
            }
            "custom" => {
                // For custom tools, we allow raw text payload without JSON validation
                if let Some(func) = &self.function {
                    if func.name.is_empty() {
                        return Err("Custom tool name cannot be empty".to_owned());
                    }
                } else {
                    return Err("Custom tool call missing function details".to_owned());
                }
            }
            _ => return Err(format!("Unsupported tool call type: {}", self.call_type)),
        }

        Ok(())
    }
}

fn parse_tool_arguments(raw_arguments: &str) -> Result<serde_json::Value, serde_json::Error> {
    let trimmed = raw_arguments.trim();
    match serde_json::from_str(trimmed) {
        Ok(parsed) => Ok(parsed),
        Err(primary_error) => {
            if let Some(candidate) = extract_balanced_json(trimmed)
                && let Ok(parsed) = serde_json::from_str(candidate)
            {
                return Ok(parsed);
            }
            if let Some(candidate) = repair_tag_polluted_json(trimmed)
                && let Ok(parsed) = serde_json::from_str(&candidate)
            {
                return Ok(parsed);
            }
            if let Some(repaired) = close_incomplete_json_prefix(trimmed)
                && let Ok(parsed) = serde_json::from_str(&repaired)
            {
                return Ok(parsed);
            }
            Err(primary_error)
        }
    }
}

fn extract_balanced_json(input: &str) -> Option<&str> {
    let start = input.find(['{', '['])?;
    let opening = input.as_bytes().get(start).copied()?;
    let closing = match opening {
        b'{' => b'}',
        b'[' => b']',
        _ => return None,
    };

    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for (offset, ch) in input.get(start..)?.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
                continue;
            }
            if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            _ if ch as u32 == opening as u32 => depth += 1,
            _ if ch as u32 == closing as u32 => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    let end = start + offset + ch.len_utf8();
                    return input.get(start..end);
                }
            }
            _ => {}
        }
    }

    None
}

fn repair_tag_polluted_json(input: &str) -> Option<String> {
    let start = input.find(['{', '['])?;
    let candidate = input.get(start..)?;
    let boundary = find_provider_markup_boundary(candidate)?;
    if boundary == 0 {
        return None;
    }

    close_incomplete_json_prefix(candidate.get(..boundary)?.trim_end())
}

fn find_provider_markup_boundary(input: &str) -> Option<usize> {
    const PROVIDER_MARKERS: &[&str] = &[
        "<</",
        "</parameter>",
        "</invoke>",
        "</minimax:tool_call>",
        "<minimax:tool_call>",
        "<parameter name=\"",
        "<invoke name=\"",
        "<tool_call>",
        "</tool_call>",
    ];

    input.char_indices().find_map(|(offset, _)| {
        let rest = input.get(offset..)?;
        PROVIDER_MARKERS.iter().any(|marker| rest.starts_with(marker)).then_some(offset)
    })
}

fn close_incomplete_json_prefix(prefix: &str) -> Option<String> {
    if prefix.is_empty() {
        return None;
    }

    let mut repaired = String::with_capacity(prefix.len() + 8);
    let mut expected_closers = Vec::new();
    let mut in_string = false;
    let mut escaped = false;

    for ch in prefix.chars() {
        repaired.push(ch);

        if in_string {
            if escaped {
                escaped = false;
                continue;
            }

            match ch {
                '\\' => escaped = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '{' => expected_closers.push('}'),
            '[' => expected_closers.push(']'),
            '}' | ']' if expected_closers.pop() != Some(ch) => return None,
            '}' | ']' => {}
            _ => {}
        }
    }

    if in_string {
        repaired.push('"');
    }
    for closer in expected_closers.drain(..) {
        repaired.push(closer);
    }

    Some(repaired)
}

/// Universal LLM response structure
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct LLMResponse {
    /// The response content text
    pub content: Option<String>,

    /// Tool calls made by the model
    pub tool_calls: Option<Vec<ToolCall>>,

    /// The model that generated this response
    pub model: String,

    /// Token usage statistics
    pub usage: Option<Usage>,

    /// Why the response finished
    pub finish_reason: FinishReason,

    /// Reasoning content (for models that support it)
    pub reasoning: Option<String>,

    /// Detailed reasoning traces (for models that support it)
    pub reasoning_details: Option<Vec<String>>,

    /// Tool references for context
    pub tool_references: Vec<String>,

    /// Request ID from the provider
    pub request_id: Option<String>,

    /// Organization ID from the provider
    pub organization_id: Option<String>,

    /// Compaction summary content from Anthropic's server-side compaction.
    /// Populated when `stop_reason` is `Pause` (from `"compaction"`).
    /// The caller should pass this back in subsequent requests so the API
    /// can drop prior messages before the compaction block.
    pub compaction: Option<String>,
}

impl LLMResponse {
    /// Create a new LLM response with mandatory fields
    pub fn new(model: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            content: Some(content.into()),
            tool_calls: None,
            model: model.into(),
            usage: None,
            finish_reason: FinishReason::Stop,
            reasoning: None,
            reasoning_details: None,
            tool_references: Vec::new(),
            request_id: None,
            organization_id: None,
            compaction: None,
        }
    }

    /// Get content or empty string
    pub fn content_text(&self) -> &str {
        self.content.as_deref().unwrap_or("")
    }

    /// Get content as String (clone)
    pub fn content_string(&self) -> String {
        self.content.clone().unwrap_or_default()
    }
}

#[derive(Clone, Deserialize, PartialEq, Eq)]
pub struct LLMErrorMetadata {
    provider: Option<String>,
    pub status: Option<u16>,
    pub code: Option<String>,
    request_id: Option<String>,
    organization_id: Option<String>,
    pub retry_after: Option<String>,
    pub message: Option<String>,
}

impl fmt::Debug for LLMErrorMetadata {
    // Derived `Debug` implies `#[inline]`; this metadata is nested inside every
    // `LLMError` and formatted on failure paths, so keep it outlined rather than
    // letting it inline into each `?err` call site.
    #[inline(never)]
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LLMErrorMetadata")
            .field("provider", &self.provider)
            .field("status", &self.status)
            .field("code", &self.code)
            .field("request_id", &self.request_id)
            .field("organization_id", &self.organization_id)
            .field("retry_after", &self.retry_after)
            .field(
                "message",
                &self
                    .message
                    .as_deref()
                    .map(|message| sanitize_provider_diagnostic(message.as_bytes())),
            )
            .finish()
    }
}

impl Serialize for LLMErrorMetadata {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_struct("LLMErrorMetadata", 7)?;
        state.serialize_field("provider", &self.provider)?;
        state.serialize_field("status", &self.status)?;
        state.serialize_field("code", &self.code)?;
        state.serialize_field("request_id", &self.request_id)?;
        state.serialize_field("organization_id", &self.organization_id)?;
        state.serialize_field("retry_after", &self.retry_after)?;
        let message = self
            .message
            .as_deref()
            .map(|message| sanitize_provider_diagnostic(message.as_bytes()));
        state.serialize_field("message", &message)?;
        state.end()
    }
}

impl LLMErrorMetadata {
    /// Boxed constructor because metadata is always stored inside `Option<Box<LLMErrorMetadata>>`
    /// in the LLMError enum variants.
    #[must_use]
    pub fn new(
        provider: impl Into<String>,
        status: Option<u16>,
        code: Option<String>,
        request_id: Option<String>,
        organization_id: Option<String>,
        retry_after: Option<String>,
        message: Option<String>,
    ) -> Box<Self> {
        Box::new(Self {
            provider: Some(provider.into()),
            status,
            code,
            request_id,
            organization_id,
            retry_after,
            message: message.map(|message| sanitize_provider_diagnostic(message.as_bytes())),
        })
    }
}

/// LLM error types with optional provider metadata
#[derive(Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LLMError {
    Authentication {
        message: String,
        metadata: Option<Box<LLMErrorMetadata>>,
    },
    RateLimit {
        metadata: Option<Box<LLMErrorMetadata>>,
    },
    InvalidRequest {
        message: String,
        metadata: Option<Box<LLMErrorMetadata>>,
    },
    Network {
        message: String,
        metadata: Option<Box<LLMErrorMetadata>>,
    },
    Provider {
        message: String,
        metadata: Option<Box<LLMErrorMetadata>>,
    },
}

impl fmt::Debug for LLMError {
    // See `LLMErrorMetadata::fmt`: keep the nested error `Debug` outlined so it
    // does not inline into every `{:?}` / `?err` call site.
    #[inline(never)]
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Authentication { message, metadata } => formatter
                .debug_struct("Authentication")
                .field("message", &sanitize_provider_diagnostic(message.as_bytes()))
                .field("metadata", metadata)
                .finish(),
            Self::RateLimit { metadata } => formatter.debug_struct("RateLimit").field("metadata", metadata).finish(),
            Self::InvalidRequest { message, metadata } => formatter
                .debug_struct("InvalidRequest")
                .field("message", &sanitize_provider_diagnostic(message.as_bytes()))
                .field("metadata", metadata)
                .finish(),
            Self::Network { message, metadata } => formatter
                .debug_struct("Network")
                .field("message", &sanitize_provider_diagnostic(message.as_bytes()))
                .field("metadata", metadata)
                .finish(),
            Self::Provider { message, metadata } => formatter
                .debug_struct("Provider")
                .field("message", &sanitize_provider_diagnostic(message.as_bytes()))
                .field("metadata", metadata)
                .finish(),
        }
    }
}

impl fmt::Display for LLMError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Authentication { message, .. } => {
                write!(formatter, "Authentication failed: {}", sanitize_provider_diagnostic(message.as_bytes()))
            }
            Self::RateLimit { .. } => formatter.write_str("Rate limit exceeded"),
            Self::InvalidRequest { message, .. } => {
                write!(formatter, "Invalid request: {}", sanitize_provider_diagnostic(message.as_bytes()))
            }
            Self::Network { message, .. } => {
                write!(formatter, "Network error: {}", sanitize_provider_diagnostic(message.as_bytes()))
            }
            Self::Provider { message, .. } => {
                write!(formatter, "Provider error: {}", sanitize_provider_diagnostic(message.as_bytes()))
            }
        }
    }
}

impl std::error::Error for LLMError {}

impl LLMError {
    /// Borrow the provider metadata carried by this error, if any.
    ///
    /// Every variant stores an `Option<Box<LLMErrorMetadata>>`. This is the
    /// single accessor so callers that only need the metadata (retry-after,
    /// provider code, raw body, misconfiguration detection) never re-spell the
    /// five-variant match — a match that is easy to drift when a variant is
    /// added.
    #[inline]
    #[must_use]
    pub fn metadata(&self) -> Option<&LLMErrorMetadata> {
        match self {
            Self::Authentication { metadata, .. }
            | Self::RateLimit { metadata }
            | Self::InvalidRequest { metadata, .. }
            | Self::Network { metadata, .. }
            | Self::Provider { metadata, .. } => metadata.as_deref(),
        }
    }
}

impl Serialize for LLMError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::Authentication { message, metadata } => {
                let mut state = serializer.serialize_struct("LLMError", 3)?;
                state.serialize_field("type", "authentication")?;
                state.serialize_field("message", &sanitize_provider_diagnostic(message.as_bytes()))?;
                state.serialize_field("metadata", metadata)?;
                state.end()
            }
            Self::RateLimit { metadata } => {
                let mut state = serializer.serialize_struct("LLMError", 2)?;
                state.serialize_field("type", "rate_limit")?;
                state.serialize_field("metadata", metadata)?;
                state.end()
            }
            Self::InvalidRequest { message, metadata } => {
                let mut state = serializer.serialize_struct("LLMError", 3)?;
                state.serialize_field("type", "invalid_request")?;
                state.serialize_field("message", &sanitize_provider_diagnostic(message.as_bytes()))?;
                state.serialize_field("metadata", metadata)?;
                state.end()
            }
            Self::Network { message, metadata } => {
                let mut state = serializer.serialize_struct("LLMError", 3)?;
                state.serialize_field("type", "network")?;
                state.serialize_field("message", &sanitize_provider_diagnostic(message.as_bytes()))?;
                state.serialize_field("metadata", metadata)?;
                state.end()
            }
            Self::Provider { message, metadata } => {
                let mut state = serializer.serialize_struct("LLMError", 3)?;
                state.serialize_field("type", "provider")?;
                state.serialize_field("message", &sanitize_provider_diagnostic(message.as_bytes()))?;
                state.serialize_field("metadata", metadata)?;
                state.end()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{LLMError, LLMErrorMetadata, ToolCall};
    use serde_json::json;

    #[test]
    fn parsed_arguments_accepts_trailing_characters() {
        let call = ToolCall::function(
            "call_read".to_string(),
            "exec_command".to_string(),
            r#"{"path":"src/main.rs"} trailing text"#.to_string(),
        );

        let parsed = call.parsed_arguments().expect("arguments with trailing text should recover");
        assert_eq!(parsed, json!({"path":"src/main.rs"}));
    }

    #[test]
    fn parsed_arguments_accepts_code_fenced_json() {
        let call = ToolCall::function(
            "call_read".to_string(),
            "exec_command".to_string(),
            "```json\n{\"path\":\"src/lib.rs\",\"limit\":25}\n```".to_string(),
        );

        let parsed = call.parsed_arguments().expect("code-fenced arguments should recover");
        assert_eq!(parsed, json!({"path":"src/lib.rs","limit":25}));
    }

    #[test]
    fn parsed_arguments_recovers_truncated_json_missing_closing_brace() {
        let call = ToolCall::function(
            "call_search".to_string(),
            "code_search".to_string(),
            r#"{"query":"context","path":".","file_types":["rust"],"result_types":["definition"],"max_results":20"#
                .to_string(),
        );

        let parsed = call
            .parsed_arguments()
            .expect("truncated JSON missing closing brace should recover");
        assert_eq!(
            parsed,
            json!({
                "query": "context",
                "path": ".",
                "file_types": ["rust"],
                "result_types": ["definition"],
                "max_results": 20
            })
        );
    }

    #[test]
    fn parsed_arguments_rejects_incomplete_json() {
        let call = ToolCall::function(
            "call_read".to_string(),
            "exec_command".to_string(),
            r#"{"path":"src/main.rs","limit""#.to_string(),
        );

        assert!(call.parsed_arguments().is_err());
    }

    #[test]
    fn llm_error_debug_and_json_redact_provider_secrets() {
        let secret = concat!("sk-", "test1234567890abcdefghij");
        let error = LLMError::Provider {
            message: format!("response body api_key={secret} bearer Bearer abcdefghijklmnop"),
            metadata: Some(LLMErrorMetadata::new(
                "OpenAI",
                Some(401),
                Some("invalid_api_key".to_owned()),
                Some("req-123".to_owned()),
                None,
                None,
                Some(format!("{}={}", "AWS_SECRET_ACCESS_KEY", "cloud-secret-value")),
            )),
        };

        let debug = format!("{error:?}");
        let json = serde_json::to_string(&error).expect("LLM errors should serialize");

        assert!(!debug.contains(secret));
        assert!(!debug.contains("cloud-secret-value"));
        assert!(!json.contains(secret));
        assert!(!json.contains("cloud-secret-value"));
        assert!(json.contains("req-123"));
        assert!(json.contains("401"));
    }

    #[test]
    fn parsed_arguments_recovers_truncated_minimax_markup() {
        let call = ToolCall::function(
            "call_search".to_string(),
            "code_search".to_string(),
            "{\"query\":\"persistent_memory\",\"file_types\":[\"rust\"],\"result_types\":[\"text\"],\"max_results\":20,\"path\":\"crates/codegen/vtcode-core/src</parameter>\n<</invoke>\n</minimax:tool_call>".to_string(),
        );

        let parsed = call.parsed_arguments().expect("minimax markup spillover should recover");
        assert_eq!(
            parsed,
            json!({
                "query": "persistent_memory",
                "path": "crates/codegen/vtcode-core/src",
                "file_types": ["rust"],
                "result_types": ["text"],
                "max_results": 20
            })
        );
    }

    #[test]
    fn function_call_serializes_optional_namespace() {
        let call = ToolCall::function_with_namespace(
            "call_read".to_string(),
            Some("workspace".to_string()),
            "exec_command".to_string(),
            r#"{"path":"src/main.rs"}"#.to_string(),
        );

        let json = serde_json::to_value(&call).expect("tool call should serialize");
        assert_eq!(json["function"]["namespace"], "workspace");
        assert_eq!(json["function"]["name"], "exec_command");
    }

    #[test]
    fn custom_tool_call_exposes_raw_execution_arguments() {
        let patch = "*** Begin Patch\n*** End Patch\n".to_string();
        let call = ToolCall::custom("call_patch".to_string(), "apply_patch".to_string(), patch.clone());

        assert!(call.is_custom());
        assert_eq!(call.tool_name(), Some("apply_patch"));
        assert_eq!(call.raw_input(), Some(patch.as_str()));
        assert_eq!(call.execution_arguments().expect("custom arguments"), json!(patch));
        assert!(call.parsed_arguments().is_err(), "custom tool payload should stay freeform rather than JSON");
    }

    #[test]
    fn metadata_accessor_covers_every_variant() {
        // Every `LLMError` variant exposes its metadata through one accessor so
        // callers never re-spell the five-variant match.
        let meta = LLMErrorMetadata::new(
            "OpenAI",
            Some(429),
            Some("rate_limit_error".to_string()),
            None,
            None,
            Some("3".to_string()),
            Some("too many requests".to_string()),
        );

        assert!(
            LLMError::Authentication {
                message: "a".to_string(),
                metadata: Some(meta.clone())
            }
            .metadata()
            .is_some()
        );
        assert!(LLMError::RateLimit { metadata: Some(meta.clone()) }.metadata().is_some());
        assert!(
            LLMError::InvalidRequest {
                message: "b".to_string(),
                metadata: Some(meta.clone())
            }
            .metadata()
            .is_some()
        );
        assert!(
            LLMError::Network {
                message: "c".to_string(),
                metadata: Some(meta.clone())
            }
            .metadata()
            .is_some()
        );
        assert!(
            LLMError::Provider { message: "d".to_string(), metadata: Some(meta) }
                .metadata()
                .is_some()
        );

        // Absent metadata returns `None` rather than panicking.
        assert!(LLMError::RateLimit { metadata: None }.metadata().is_none());
        assert!(
            LLMError::Network { message: "no meta".to_string(), metadata: None }
                .metadata()
                .is_none()
        );
    }
}
