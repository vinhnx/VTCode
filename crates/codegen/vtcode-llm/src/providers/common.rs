//! Shared provider helpers, grouped by responsibility behind stable imports.

mod chat;
mod http;
mod prompt_cache;
mod reasoning;
mod request;
mod streaming;
mod token_count;

pub(crate) use chat::{
    convert_usage_to_llm_types, map_finish_reason_common, parse_chat_request_openai_format,
    parse_response_openai_format, parse_usage_openai_format, serialize_message_content_openai,
    serialize_message_content_openai_for_model, serialize_message_content_openai_for_role,
    serialize_messages_openai_format, serialize_tools_openai_format,
};
pub(crate) use http::{
    PROVIDER_ERROR_BODY_MAX_BYTES, chat_completions_url, extract_header, override_base_url, parse_json_response,
    read_provider_error_body, send_chat_completions,
};
pub use prompt_cache::forward_prompt_cache_with_state;
pub(crate) use prompt_cache::{extract_prompt_cache_settings, extract_prompt_cache_settings_default};
pub(crate) use reasoning::{
    append_normalized_reasoning_detail_items, assistant_interleaved_history_text, is_interleaved_thinking_model,
    is_minimax_m2_model, normalize_reasoning_detail_object, normalize_reasoning_detail_objects,
    preserve_interleaved_content_in_reasoning_details, serialize_reasoning_detail_values,
};
pub use reasoning::{
    extract_reasoning_text_from_detail_values, extract_reasoning_text_from_serialized_details,
    make_anthropic_thinking_config,
};
pub(crate) use request::{
    collect_history_system_directives, ensure_model, float_to_json_number, make_default_request,
    merge_system_prompt_with_history_directives, parse_client_prompt_common, resolve_model, sampling_param_f64,
    validate_request_common, validate_supported_models,
};
pub(crate) use streaming::{TaskAbortGuard, spawn_openai_compatible_stream};
pub use token_count::{
    execute_token_count_request, parse_prompt_tokens_from_count_response, strip_generation_controls_for_token_count,
};

/// Implements the `LLMClient` trait for an OpenAI-compatible provider.
/// All providers share the same pattern: create a default request, delegate to `LLMProvider::generate`.
///
/// Helpers are named by an absolute `crate::` path (not `super::`) so the
/// macro stays correct in any invoking module; see
/// `docs/development/rust-macro-hygiene.md`.
macro_rules! impl_llm_client {
    ($provider:ty) => {
        #[async_trait::async_trait]
        impl crate::client::LLMClient for $provider {
            async fn generate(
                &mut self,
                prompt: &str,
            ) -> Result<crate::provider::LLMResponse, crate::provider::LLMError> {
                let request = crate::providers::common::make_default_request(prompt, &self.model);
                Ok(<$provider as crate::provider::LLMProvider>::generate(self, request).await?)
            }

            fn model_id(&self) -> &str {
                &self.model
            }
        }
    };
}

pub(crate) use impl_llm_client;

#[cfg(test)]
mod tests;
