//! Chat Completions and Responses request builders for OpenAI-compatible APIs.
//!
//! Model policy and Responses history shaping live in private child modules.

use vtcode_config::core::OpenAIHostedShellConfig;
use vtcode_config::types::ReasoningEffortLevel;

mod chat;
mod history;
mod model_policy;
mod responses;

pub(crate) use chat::build_chat_request;
pub(crate) use responses::build_responses_request;

use model_policy::{is_gpt6_model, is_gpt56_model};

pub(super) fn supports_prompt_cache_options(model: &str) -> bool {
    is_gpt56_model(model) || is_gpt6_model(model)
}

pub(crate) struct ChatRequestContext<'a> {
    pub model: &'a str,
    pub is_native_openai: bool,
    pub supports_tools: bool,
    pub supports_parallel_tool_config: bool,
    pub supports_temperature: bool,
    pub prompt_cache_key: Option<&'a str>,
    pub default_service_tier: Option<&'a str>,
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
}

pub(super) fn trimmed_non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
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
