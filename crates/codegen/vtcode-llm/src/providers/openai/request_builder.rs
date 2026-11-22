//! Chat Completions and Responses request builders for OpenAI-compatible APIs.
//!
//! Model policy and Responses history shaping live in private child modules.

use serde_json::{Value, json};
use vtcode_config::core::OpenAIHostedShellConfig;
use vtcode_config::types::ReasoningEffortLevel;

mod chat;
mod history;
mod model_policy;
mod responses;

pub(crate) use chat::build_chat_request;
pub(crate) use responses::build_responses_request;

pub(super) fn supports_prompt_cache_options(model: &str) -> bool {
    vtcode_config::core::prompt_cache::supports_openai_explicit_prompt_cache(model)
}

pub(crate) struct ChatRequestContext<'a> {
    pub model: &'a str,
    pub is_native_openai: bool,
    pub supports_tools: bool,
    pub supports_parallel_tool_config: bool,
    pub supports_temperature: bool,
    pub prompt_cache_key: Option<&'a str>,
    pub default_service_tier: Option<&'a str>,
    pub include_structured_output: bool,
}

pub(crate) struct ResponsesRequestContext<'a> {
    pub supports_tools: bool,
    pub supports_allowed_tools: bool,
    pub supports_parallel_tool_config: bool,
    pub supports_temperature: bool,
    pub supports_reasoning_effort: bool,
    pub supported_reasoning_efforts: &'a [&'a str],
    pub supports_reasoning: bool,
    pub is_responses_api_model: bool,
    pub include_max_output_tokens: bool,
    pub include_output_types: bool,
    pub include_sampling_parameters: bool,
    pub force_response_store_false: bool,
    pub include_assistant_phase: bool,
    pub prompt_cache_key: Option<&'a str>,
    pub include_prompt_cache_retention: bool,
    pub prompt_cache_retention: Option<&'a str>,
    pub supports_prompt_cache_options: bool,
    pub prompt_cache_ttl: Option<vtcode_config::core::prompt_cache::PromptCacheTtl>,
    /// Optional separate lifetime for the stable developer-instruction breakpoint.
    /// Emitted only on native GPT-5.6+ Responses routes that already support
    /// `prompt_cache_options`; other routes inherit the request-level TTL.
    pub stable_prefix_ttl: Option<vtcode_config::core::prompt_cache::PromptCacheTtl>,
    /// Emit explicit `prompt_cache_breakpoint` markers on the stable prefix.
    /// Only native api.openai.com honors them: third-party
    /// Responses-compatible endpoints (e.g. Merge Gateway via a base-URL
    /// override) reject the field with 400 `invalid_parameter`, and the
    /// ChatGPT subscription backend rejects it with 400
    /// `prompt_cache_breakpoint is not supported on this model` (observed
    /// live on `gpt-6.1-sol`). Implicit breakpoints still cover the latest
    /// message on those backends.
    pub include_explicit_cache_breakpoints: bool,
    pub default_service_tier: Option<&'a str>,
    pub default_response_store: Option<bool>,
    pub default_responses_include: Option<&'a [String]>,
    pub include_encrypted_reasoning: bool,
    pub hosted_shell: Option<&'a OpenAIHostedShellConfig>,
    pub include_structured_history_in_input: bool,
    pub preserve_structured_history_on_replay: bool,
    pub preserve_assistant_phase_on_replay: bool,
    pub reasoning_context: Option<&'a str>,
    pub safety_identifier: Option<&'a str>,
    pub include_structured_output: bool,
}

pub(super) fn trimmed_non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

/// Normalized JSON Schema descriptor extracted from VT Code's provider-agnostic
/// `output_format`. Both OpenAI request shapes accept a name, schema, strictness,
/// and optional description; only their nesting differs.
struct JsonSchemaFormat<'a> {
    name: &'a str,
    schema: &'a Value,
    strict: bool,
    description: Option<&'a str>,
}

fn json_schema_format(output_format: &Value) -> Option<JsonSchemaFormat<'_>> {
    if output_format.get("type").and_then(Value::as_str) != Some("json_schema") {
        return None;
    }
    // Chat Completions style: `{type: "json_schema", json_schema: {...}}`.
    if let Some(nested) = output_format.get("json_schema").and_then(Value::as_object) {
        return Some(JsonSchemaFormat {
            name: nested.get("name").and_then(Value::as_str).unwrap_or("response"),
            schema: nested.get("schema")?,
            strict: nested.get("strict").and_then(Value::as_bool).unwrap_or(true),
            description: nested.get("description").and_then(Value::as_str),
        });
    }
    // Responses style: `{type: "json_schema", name, schema, strict}`.
    Some(JsonSchemaFormat {
        name: output_format.get("name").and_then(Value::as_str).unwrap_or("response"),
        schema: output_format.get("schema")?,
        strict: output_format.get("strict").and_then(Value::as_bool).unwrap_or(true),
        description: output_format.get("description").and_then(Value::as_str),
    })
}

fn json_object_format(output_format: &Value) -> bool {
    output_format.get("type").and_then(Value::as_str) == Some("json_object")
}

/// Translate `LLMRequest::output_format` into the Chat Completions
/// `response_format` field. Unknown shapes are dropped rather than forwarded so
/// a malformed descriptor cannot turn into an OpenAI 400.
pub(super) fn chat_response_format(output_format: Option<&Value>) -> Option<Value> {
    let format = output_format?;
    if json_object_format(format) {
        return Some(json!({"type": "json_object"}));
    }
    let schema = json_schema_format(format)?;
    let mut nested = json!({
        "name": schema.name,
        "schema": schema.schema,
        "strict": schema.strict,
    });
    if let Some(description) = schema.description {
        nested["description"] = json!(description);
    }
    Some(json!({"type": "json_schema", "json_schema": nested}))
}

/// Translate `LLMRequest::output_format` into the Responses `text.format` field.
pub(super) fn responses_text_format(output_format: Option<&Value>) -> Option<Value> {
    let format = output_format?;
    if json_object_format(format) {
        return Some(json!({"type": "json_object"}));
    }
    let schema = json_schema_format(format)?;
    let mut value = json!({
        "type": "json_schema",
        "name": schema.name,
        "schema": schema.schema,
        "strict": schema.strict,
    });
    if let Some(description) = schema.description {
        value["description"] = json!(description);
    }
    Some(value)
}

fn resolved_service_tier<'a>(model: &str, explicit: Option<&'a str>, default: Option<&'a str>) -> Option<&'a str> {
    use vtcode_config::core::OpenAIServiceTier;
    use vtcode_config::models::{Provider, ProviderModelSupport};

    let value = trimmed_non_empty(explicit.or(default))?;
    if !Provider::OpenAI.supports_service_tier(model) {
        tracing::debug!(model, tier = value, "dropping service tier: model supports no service tier");
        return None;
    }
    if let Some(tier) = OpenAIServiceTier::parse(value)
        && !Provider::OpenAI.supports_service_tier_value(model, tier)
    {
        tracing::debug!(model, tier = value, "dropping service tier: tier unsupported for model");
        return None;
    }
    Some(value)
}

#[cfg(test)]
mod tests;
