//! Legacy OpenAI-compatible route spec for Merge Gateway.

use super::*;

pub struct MergeGatewaySpec;

fn no_reasoning(_message: &Value, _choice: &Value) -> Option<String> {
    None
}

impl OpenAiCompatSpec for MergeGatewaySpec {
    const NAME: &'static str = "Merge Gateway";
    const KEY: &'static str = "merge-gateway";
    const API_KEY_ENV: &'static str = env_vars::MERGE_GATEWAY_API_KEY;
    const DEFAULT_MODEL: &'static str = models::merge_gateway::DEFAULT_MODEL;
    const DEFAULT_BASE_URL: &'static str = urls::MERGE_GATEWAY_NATIVE_API_BASE;
    const BASE_URL_ENV: Option<&'static str> = Some(env_vars::MERGE_GATEWAY_BASE_URL);
    const LISTED_MODELS: &'static [&'static str] = models::merge_gateway::SUPPORTED_MODELS;
    const VALIDATION_ALLOWLIST: Option<&'static [&'static str]> = None;
    const STREAM_OPTIONS_INCLUDE_USAGE: bool = true;
    const SUPPRESS_SAMPLING_WHEN_REASONING: bool = false;
    const STREAM_REASONING_FIELDS: &'static [&'static str] = &[];
    const DELTA_ORDER: crate::providers::shared::OpenAiDeltaOrder =
        crate::providers::shared::OpenAiDeltaOrder::ContentFirst;
    const RESPONSE_REASONING_EXTRACTOR: Option<crate::providers::openai_compat::ReasoningExtractor> =
        Some(no_reasoning);

    fn resolve_api_key(api_key: Option<String>) -> String {
        api_key
            .or_else(|| std::env::var(Self::API_KEY_ENV).ok().filter(|key| !key.trim().is_empty()))
            .unwrap_or_default()
    }

    fn insert_tool_choice(_core: &OpenAiCompatCore<Self>, request: &LLMRequest, payload: &mut Map<String, Value>) {
        // Merge routes that terminate at Anthropic Bedrock reject
        // `tool_choice: "none"`. Keep the serialized tool definitions on the
        // wire so the provider prefix stays cache-stable across recovery turns
        // (OpenAI guidance: disable tool use with `tool_choice: "none"` rather
        // than removing definitions). Only the choice field is omitted; the
        // harness rejects tool calls during tool-free recovery.
        if matches!(request.tool_choice, Some(ToolChoice::None)) {
            static OMITTED_CHOICE_ADVISORY: std::sync::Once = std::sync::Once::new();
            OMITTED_CHOICE_ADVISORY.call_once(|| {
                tracing::debug!(
                    "Merge Gateway omits tool_choice for ToolChoice::None (Bedrock rejects none); \
                     tool definitions stay on the wire for prompt-cache stability"
                );
            });
            return;
        }

        if let Some(choice) = &request.tool_choice {
            payload.insert("tool_choice".to_owned(), choice.to_provider_format(Self::KEY));
        }
    }
}
