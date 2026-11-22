//! Chat Completions and Responses request builders for OpenAI-compatible APIs.
//!
//! Model policy and Responses history shaping live in private child modules.

use serde_json::{Value, json};
use vtcode_config::core::OpenAIHostedShellConfig;
use vtcode_config::types::ReasoningEffortLevel;

use crate::{error_display, provider::LLMError};

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

/// Keywords outside the JSON Schema subset accepted by OpenAI Structured
/// Outputs (`allOf`, composition gates, `patternProperties`). `anyOf` and
/// `$defs` are intentionally absent: they are supported in nested positions.
const UNSUPPORTED_STRICT_KEYWORDS: &[&str] = &[
    "allOf",
    "oneOf",
    "not",
    "dependentRequired",
    "dependentSchemas",
    "if",
    "then",
    "else",
    "patternProperties",
];

fn invalid_schema(provider_name: &str, message: String) -> LLMError {
    let formatted = error_display::format_llm_error(provider_name, &message);
    LLMError::InvalidRequest { message: formatted, metadata: None }
}

/// Validate a caller-supplied `output_format` against OpenAI's strict
/// Structured Outputs schema rules before it reaches the wire.
///
/// OpenAI rejects a `strict: true` schema whose root is not an object, whose
/// object nodes omit `additionalProperties: false` or leave a property out of
/// `required`, or that uses an unsupported keyword. Checking locally turns an
/// opaque API 400 into an actionable error and avoids a wasted request. This
/// mirrors `anthropic::validation::validate_anthropic_schema`; JSON mode and
/// non-strict schemas are left untouched because those rules do not apply.
pub(super) fn validate_output_format(output_format: Option<&Value>, provider_name: &str) -> Result<(), LLMError> {
    let Some(format) = output_format else {
        return Ok(());
    };
    if json_object_format(format) {
        return Ok(());
    }
    let Some(schema) = json_schema_format(format) else {
        // The translation layer drops unknown descriptor shapes on purpose.
        return Ok(());
    };
    if !schema.strict {
        return Ok(());
    }
    validate_strict_schema(schema.schema, "root", provider_name)
}

/// `items` and `prefixItems` accept either a single schema or a tuple of
/// schemas; both forms must satisfy the strict subset.
fn validate_schema_list(value: &Value, path: &str, provider_name: &str) -> Result<(), LLMError> {
    match value {
        Value::Array(schemas) => {
            for (index, child) in schemas.iter().enumerate() {
                validate_strict_schema(child, &format!("{path}[{index}]"), provider_name)?;
            }
        }
        _ => validate_strict_schema(value, path, provider_name)?,
    }
    Ok(())
}

fn validate_strict_schema(schema: &Value, path: &str, provider_name: &str) -> Result<(), LLMError> {
    let Value::Object(map) = schema else {
        return Err(invalid_schema(
            provider_name,
            format!("Structured output schema at '{path}' must be a JSON object"),
        ));
    };

    if path == "root" {
        if map.contains_key("anyOf") {
            return Err(invalid_schema(
                provider_name,
                "Structured output root cannot use anyOf; it must be an object".to_string(),
            ));
        }
        if map.get("type").and_then(Value::as_str) != Some("object") {
            return Err(invalid_schema(provider_name, "Structured output root must have type 'object'".to_string()));
        }
    }

    for keyword in UNSUPPORTED_STRICT_KEYWORDS {
        if map.contains_key(*keyword) {
            return Err(invalid_schema(
                provider_name,
                format!("Keyword '{keyword}' is not supported in strict structured output schemas (path: {path})"),
            ));
        }
    }

    let object_type = match map.get("type") {
        Some(Value::String(schema_type)) => schema_type == "object",
        Some(Value::Array(types)) => types.iter().any(|schema_type| schema_type.as_str() == Some("object")),
        _ => false,
    };
    if object_type || map.contains_key("properties") || map.contains_key("required") {
        if map.get("additionalProperties") != Some(&Value::Bool(false)) {
            return Err(invalid_schema(
                provider_name,
                format!("Object at '{path}' must set additionalProperties to false for strict structured output"),
            ));
        }

        let required: Vec<&str> = map
            .get("required")
            .and_then(Value::as_array)
            .map(|names| names.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let properties = map.get("properties").and_then(Value::as_object);

        if let Some(properties) = properties {
            for name in properties.keys() {
                if !required.contains(&name.as_str()) {
                    return Err(invalid_schema(
                        provider_name,
                        format!(
                            "Property '{name}' at '{path}' must be listed in required for strict structured output"
                        ),
                    ));
                }
            }
            for name in &required {
                if !properties.contains_key(*name) {
                    return Err(invalid_schema(
                        provider_name,
                        format!("Required property '{name}' at '{path}' is not declared in properties"),
                    ));
                }
            }
        } else if !required.is_empty() {
            return Err(invalid_schema(
                provider_name,
                format!("Object at '{path}' declares required properties without a properties map"),
            ));
        }
    }

    if let Some(properties) = map.get("properties").and_then(Value::as_object) {
        for (name, child) in properties {
            validate_strict_schema(child, &format!("{path}.properties.{name}"), provider_name)?;
        }
    }
    if let Some(items) = map.get("items") {
        validate_schema_list(items, &format!("{path}.items"), provider_name)?;
    }
    if let Some(prefix_items) = map.get("prefixItems") {
        validate_schema_list(prefix_items, &format!("{path}.prefixItems"), provider_name)?;
    }
    if let Some(variants) = map.get("anyOf").and_then(Value::as_array) {
        for (index, child) in variants.iter().enumerate() {
            validate_strict_schema(child, &format!("{path}.anyOf[{index}]"), provider_name)?;
        }
    }
    for key in ["$defs", "definitions"] {
        if let Some(definitions) = map.get(key).and_then(Value::as_object) {
            for (name, child) in definitions {
                validate_strict_schema(child, &format!("{path}.{key}.{name}"), provider_name)?;
            }
        }
    }

    Ok(())
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
