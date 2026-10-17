//! Merge Gateway provider.

use crate::error_display;
use crate::http_client::HttpClientFactory;
use crate::provider::{
    FinishReason, LLMError, LLMNormalizedStream, LLMProvider, LLMRequest, LLMResponse, LLMStream, LLMStreamEvent,
    Message, NormalizedStreamEvent, ToolCall, ToolChoice, ToolDefinition, Usage,
};
use crate::providers::common::{
    override_base_url, resolve_model, serialize_message_content_openai_for_model, validate_request_common,
};
use crate::providers::error_handling::{format_network_error, format_parse_error};
use crate::providers::gemini::sanitize_function_parameters;
use crate::providers::openai_compat::{OpenAiCompatCore, OpenAiCompatSpec};
use crate::providers::shared::{
    RESPONSES_COMPLETION_TOKEN_KEYS, RESPONSES_PROMPT_TOKEN_KEYS, Utf8StreamDecoder, extract_data_payload,
    function_output_value_from_message_content, generate_tool_call_id, next_sse_event,
    parse_cache_write_tokens_from_usage, parse_cached_prompt_tokens_from_usage, usage_u32_from_keys,
};
use async_stream::try_stream;
use async_trait::async_trait;
use futures::StreamExt;
use reqwest::{Client as HttpClient, StatusCode};
use serde_json::{Map, Value, json};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use vtcode_config::TimeoutsConfig;
use vtcode_config::constants::{env_vars, models, urls};
use vtcode_config::core::{AnthropicConfig, ModelConfig, PromptCachingConfig};

/// Static provider metadata shared by native and explicit legacy routes.

#[derive(Debug)]
struct NativeMergeGatewayCore {
    api_key: String,
    http_client: HttpClient,
    base_url: String,
    model: String,
    model_behavior: Option<ModelConfig>,
}

pub struct MergeGatewayProvider {
    native: NativeMergeGatewayCore,
    legacy_core: Option<OpenAiCompatCore<MergeGatewaySpec>>,
    /// Models that already failed a non-streaming tool request with
    /// `capability_unavailable`: no vendor serves tools for them, so further
    /// tool turns fail fast without burning calls. Session-scoped (like the
    /// OpenAI tier-unsupported cache): a fresh session re-probes in case
    /// vendors onboarded, and tool-free requests always bypass the cache.
    no_tool_vendor_cache: Mutex<HashSet<String>>,
}

use classification::*;
use spec::*;
use stream_state::*;

mod classification;
mod provider_impl;
mod spec;
mod stream_state;

#[cfg(test)]
mod tests;
