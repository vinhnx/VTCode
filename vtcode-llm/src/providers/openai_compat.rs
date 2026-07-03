//! Shared shell for OpenAI-compatible chat-completions providers.
//!
//! Most third-party providers in this crate speak the OpenAI chat-completions
//! dialect and differ only in a handful of quirk axes: authentication style,
//! system-prompt placement, the max-tokens key, sampling suppression under
//! reasoning, stream options, reasoning encoding, and validation strategy.
//!
//! [`OpenAiCompatSpec`] encodes those axes as associated consts and default
//! methods, [`OpenAiCompatCore`] implements the request/response shell once,
//! and [`impl_openai_compat_provider!`] generates the public provider newtype
//! (preserving its type name and the 7-argument `from_config` constructor the
//! registration layer depends on).

use std::marker::PhantomData;

use reqwest::{Client as HttpClient, RequestBuilder};
use serde_json::{Map, Value};
use vtcode_config::core::{ModelConfig, PromptCachingConfig};

use crate::provider::{LLMError, LLMRequest, LLMResponse, LLMStream};

use super::common::{
    chat_completions_url, ensure_model, extract_prompt_cache_settings_default,
    float_to_json_number, override_base_url, parse_json_response, parse_response_openai_format,
    resolve_model, send_chat_completions, serialize_messages_openai_format,
    serialize_tools_openai_format, spawn_openai_compatible_stream, validate_request_common,
    validate_supported_models,
};
use super::error_handling::handle_openai_http_error;
use super::shared::OpenAiDeltaOrder;

/// Extractor pulling reasoning text out of a chat-completions response
/// (`message`, `choice`) pair.
pub(crate) type ReasoningExtractor = fn(&Value, &Value) -> Option<String>;

/// Where the request's system prompt lands in the wire payload.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SystemPromptPlacement {
    /// The system prompt is not sent (history serialization handles it).
    Omitted,
    /// Inserted as a `{"role": "system"}` message at index 0.
    FirstMessage,
    /// Sent as a top-level `"system"` string field.
    TopLevelField,
}

/// Static per-provider description of an OpenAI-compatible wire dialect.
pub(crate) trait OpenAiCompatSpec: Sized + Send + Sync + 'static {
    /// Human-readable provider name used in error messages.
    const NAME: &'static str;
    /// Provider key used for message/tool-choice serialization and validation.
    const KEY: &'static str;
    /// Environment variable named in authentication error hints.
    const API_KEY_ENV: &'static str;
    /// Model used when none is configured.
    const DEFAULT_MODEL: &'static str;
    /// Default API base URL.
    const DEFAULT_BASE_URL: &'static str;
    /// Environment variable that may override the base URL.
    const BASE_URL_ENV: Option<&'static str>;
    /// Models advertised through `supported_models()`.
    const LISTED_MODELS: &'static [&'static str];
    /// When `Some`, requests are validated against this allowlist; when
    /// `None`, only request shape is validated.
    const VALIDATION_ALLOWLIST: Option<&'static [&'static str]>;

    /// JSON key carrying the max-tokens budget.
    const MAX_TOKENS_KEY: &'static str = "max_tokens";
    /// System prompt placement in the payload.
    const SYSTEM_PROMPT: SystemPromptPlacement = SystemPromptPlacement::FirstMessage;
    /// Whether `top_p` is forwarded when present.
    const INCLUDE_TOP_P: bool = true;
    /// Whether `temperature`/`top_p` are dropped while reasoning is enabled.
    const SUPPRESS_SAMPLING_WHEN_REASONING: bool = true;
    /// Whether streaming requests include `stream_options.include_usage`.
    const STREAM_OPTIONS_INCLUDE_USAGE: bool = false;
    /// Whether a `user_id` metadata entry is forwarded.
    const INCLUDE_USER_ID: bool = false;
    /// Whether `generate` validates the request before dispatch (`stream`
    /// always validates).
    const VALIDATE_ON_GENERATE: bool = false;
    /// Delta fields scanned for streamed reasoning text.
    const STREAM_REASONING_FIELDS: &'static [&'static str] = &["reasoning_content"];
    /// Ordering of reasoning vs. content deltas in the stream.
    const DELTA_ORDER: OpenAiDeltaOrder = OpenAiDeltaOrder::ReasoningFirst;
    /// Extractor for reasoning text on non-streaming responses.
    const RESPONSE_REASONING_EXTRACTOR: Option<ReasoningExtractor> = None;

    /// Resolves the API key from configuration (env fallbacks live here).
    fn resolve_api_key(api_key: Option<String>) -> String {
        api_key.unwrap_or_default()
    }

    /// Resolves the effective base URL from configuration.
    fn resolve_base_url(base_url: Option<String>) -> String {
        override_base_url(Self::DEFAULT_BASE_URL, base_url, Self::BASE_URL_ENV)
    }

    /// Normalizes a model identifier before it is stored or sent.
    fn normalize_model(model: String) -> String {
        model
    }

    /// Whether prompt-cache metrics should be surfaced for this provider.
    fn prompt_cache_enabled(prompt_cache: Option<&PromptCachingConfig>) -> bool {
        extract_prompt_cache_settings_default(prompt_cache.cloned(), Self::KEY).0
    }

    /// Whether non-streaming responses report cache metrics.
    fn response_cache_metrics(_core: &OpenAiCompatCore<Self>) -> bool {
        false
    }

    /// Whether streamed responses report cache metrics.
    fn stream_cache_metrics(_core: &OpenAiCompatCore<Self>) -> bool {
        false
    }

    /// Whether the request has reasoning enabled (drives sampling
    /// suppression when [`SUPPRESS_SAMPLING_WHEN_REASONING`] is set).
    fn reasoning_enabled(_core: &OpenAiCompatCore<Self>, request: &LLMRequest) -> bool {
        request
            .reasoning_effort
            .is_some_and(|effort| effort != vtcode_config::types::ReasoningEffortLevel::None)
    }

    /// Encodes a float payload parameter, allowing providers to keep their
    /// historical error messages.
    fn float_number(value: f32) -> Result<serde_json::Number, LLMError> {
        float_to_json_number(value)
    }

    /// Inserts the provider's `tool_choice` encoding.
    fn insert_tool_choice(
        _core: &OpenAiCompatCore<Self>,
        request: &LLMRequest,
        payload: &mut Map<String, Value>,
    ) {
        if let Some(choice) = &request.tool_choice {
            payload.insert(
                "tool_choice".to_owned(),
                choice.to_provider_format(Self::KEY),
            );
        }
    }

    /// Inserts the provider's reasoning/thinking encoding. No-op by default.
    fn insert_reasoning(
        _core: &OpenAiCompatCore<Self>,
        _request: &LLMRequest,
        _payload: &mut Map<String, Value>,
    ) -> Result<(), LLMError> {
        Ok(())
    }

    /// Final hook for provider-specific payload fields.
    fn finish_payload(
        _core: &OpenAiCompatCore<Self>,
        _request: &LLMRequest,
        _payload: &mut Map<String, Value>,
    ) -> Result<(), LLMError> {
        Ok(())
    }

    /// Applies authentication to the outgoing request.
    fn apply_auth(core: &OpenAiCompatCore<Self>, builder: RequestBuilder) -> RequestBuilder {
        builder.bearer_auth(&core.api_key)
    }

    /// Environment variable named in HTTP auth error hints.
    fn api_key_env(_core: &OpenAiCompatCore<Self>) -> &'static str {
        Self::API_KEY_ENV
    }

    /// Validates a request before dispatch.
    fn validate(_core: &OpenAiCompatCore<Self>, request: &LLMRequest) -> Result<(), LLMError> {
        match Self::VALIDATION_ALLOWLIST {
            Some(models) => validate_supported_models(request, Self::NAME, Self::KEY, models),
            None => validate_request_common(request, Self::NAME, Self::KEY, None),
        }
    }
}

/// Shared state and behavior for an OpenAI-compatible provider instance.
pub(crate) struct OpenAiCompatCore<S: OpenAiCompatSpec> {
    pub(crate) api_key: String,
    pub(crate) http_client: HttpClient,
    pub(crate) base_url: String,
    pub(crate) model: String,
    pub(crate) prompt_cache_enabled: bool,
    // Read by specs whose capability gates depend on configured model behavior.
    #[allow(dead_code)]
    pub(crate) model_behavior: Option<ModelConfig>,
    spec: PhantomData<S>,
}

impl<S: OpenAiCompatSpec> OpenAiCompatCore<S> {
    /// Constructor backing the provider's `with_model`/`new` shorthands.
    pub(crate) fn direct(api_key: String, model: String) -> Self {
        Self::assemble(api_key, model, None, None, None)
    }

    /// Constructor backing the provider's 7-argument `from_config`.
    pub(crate) fn from_config(
        api_key: Option<String>,
        model: Option<String>,
        base_url: Option<String>,
        prompt_cache: Option<PromptCachingConfig>,
        timeouts: Option<vtcode_config::TimeoutsConfig>,
        model_behavior: Option<ModelConfig>,
    ) -> Self {
        let api_key = S::resolve_api_key(api_key);
        let model = resolve_model(model, S::DEFAULT_MODEL);
        let mut core = Self::assemble(api_key, model, base_url, timeouts, model_behavior);
        core.prompt_cache_enabled = S::prompt_cache_enabled(prompt_cache.as_ref());
        core
    }

    fn assemble(
        api_key: String,
        model: String,
        base_url: Option<String>,
        timeouts: Option<vtcode_config::TimeoutsConfig>,
        model_behavior: Option<ModelConfig>,
    ) -> Self {
        use crate::http_client::HttpClientFactory;

        let timeouts = timeouts.unwrap_or_default();
        Self {
            api_key,
            http_client: HttpClientFactory::for_llm(&timeouts),
            base_url: S::resolve_base_url(base_url),
            model: S::normalize_model(model),
            prompt_cache_enabled: false,
            model_behavior,
            spec: PhantomData,
        }
    }

    /// Constructor backing the provider's `new_with_client`.
    pub(crate) fn from_parts(
        api_key: String,
        model: String,
        http_client: HttpClient,
        base_url: String,
    ) -> Self {
        Self {
            api_key,
            http_client,
            base_url,
            model: S::normalize_model(model),
            prompt_cache_enabled: false,
            model_behavior: None,
            spec: PhantomData,
        }
    }

    /// Fills in the default model and normalizes the request's model id.
    pub(crate) fn prepare(&self, request: &mut LLMRequest) {
        ensure_model(request, &self.model);
        request.model = S::normalize_model(std::mem::take(&mut request.model));
    }

    /// Builds the chat-completions payload for the request.
    pub(crate) fn convert_request(&self, request: &LLMRequest) -> Result<Value, LLMError> {
        let mut payload = Map::new();

        payload.insert("model".to_owned(), Value::String(request.model.clone()));

        let mut messages = serialize_messages_openai_format(request, S::KEY)?;
        if S::SYSTEM_PROMPT == SystemPromptPlacement::FirstMessage
            && let Some(system) = &request.system_prompt
        {
            let trimmed = system.trim();
            if !trimmed.is_empty() {
                messages.insert(0, serde_json::json!({"role": "system", "content": trimmed}));
            }
        }
        payload.insert("messages".to_owned(), Value::Array(messages));

        if S::SYSTEM_PROMPT == SystemPromptPlacement::TopLevelField
            && let Some(system) = &request.system_prompt
        {
            let trimmed = system.trim();
            if !trimmed.is_empty() {
                payload.insert("system".to_owned(), Value::String(trimmed.to_owned()));
            }
        }

        if let Some(max_tokens) = request.max_tokens {
            payload.insert(
                S::MAX_TOKENS_KEY.to_owned(),
                Value::Number(serde_json::Number::from(u64::from(max_tokens))),
            );
        }

        let suppress_sampling =
            S::SUPPRESS_SAMPLING_WHEN_REASONING && S::reasoning_enabled(self, request);
        if !suppress_sampling {
            if let Some(temperature) = request.temperature {
                payload.insert(
                    "temperature".to_owned(),
                    Value::Number(S::float_number(temperature)?),
                );
            }
            if S::INCLUDE_TOP_P
                && let Some(top_p) = request.top_p
            {
                payload.insert("top_p".to_owned(), Value::Number(S::float_number(top_p)?));
            }
        }

        if request.stream {
            payload.insert("stream".to_owned(), Value::Bool(true));
            if S::STREAM_OPTIONS_INCLUDE_USAGE {
                payload.insert(
                    "stream_options".to_owned(),
                    serde_json::json!({"include_usage": true}),
                );
            }
        }

        if let Some(tools) = &request.tools
            && let Some(serialized_tools) = serialize_tools_openai_format(tools)
        {
            payload.insert("tools".to_owned(), Value::Array(serialized_tools));
        }

        S::insert_tool_choice(self, request, &mut payload);
        S::insert_reasoning(self, request, &mut payload)?;

        if S::INCLUDE_USER_ID
            && let Some(user_id) = request
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("user_id"))
                .and_then(Value::as_str)
        {
            payload.insert("user_id".to_owned(), Value::String(user_id.to_owned()));
        }

        S::finish_payload(self, request, &mut payload)?;

        Ok(Value::Object(payload))
    }

    async fn dispatch(&self, request: &LLMRequest) -> Result<reqwest::Response, LLMError> {
        let payload = self.convert_request(request)?;
        let url = chat_completions_url(&self.base_url);
        let builder = S::apply_auth(self, self.http_client.post(&url));
        let response = send_chat_completions(builder, &payload, S::NAME).await?;
        handle_openai_http_error(response, S::NAME, S::api_key_env(self)).await
    }

    /// Executes a prepared (model-resolved, validated as needed) generate call.
    pub(crate) async fn generate_prepared(
        &self,
        request: LLMRequest,
    ) -> Result<LLMResponse, LLMError> {
        let model = request.model.clone();
        let response = self.dispatch(&request).await?;
        let response_json = parse_json_response(response, S::NAME).await?;
        parse_response_openai_format::<ReasoningExtractor>(
            response_json,
            S::NAME,
            model,
            S::response_cache_metrics(self),
            S::RESPONSE_REASONING_EXTRACTOR,
        )
    }

    /// Executes a prepared streaming call (`request.stream` must be set).
    pub(crate) async fn stream_prepared(&self, request: LLMRequest) -> Result<LLMStream, LLMError> {
        let model = request.model.clone();
        let response = self.dispatch(&request).await?;
        Ok(spawn_openai_compatible_stream(
            response,
            S::NAME,
            model,
            S::STREAM_REASONING_FIELDS,
            S::DELTA_ORDER,
            S::stream_cache_metrics(self),
        ))
    }

    pub(crate) fn supported_models(&self) -> Vec<String> {
        S::LISTED_MODELS
            .iter()
            .map(|model| model.to_string())
            .collect()
    }

    pub(crate) fn validate(&self, request: &LLMRequest) -> Result<(), LLMError> {
        S::validate(self, request)
    }
}

/// Generates the public provider newtype over [`OpenAiCompatCore`]: the
/// constructor quartet (`new`, `with_model`, `new_with_client`, `from_config`),
/// the delegating [`LLMProvider`](crate::provider::LLMProvider) impl, and the
/// [`LLMClient`](crate::client::LLMClient) impl. Extra `LLMProvider` methods
/// (capabilities, `get_balance`, ...) go in the optional trailing block.
macro_rules! impl_openai_compat_provider {
    ($provider:ident, $spec:ty $(, { $($extra:item)* })?) => {
        pub struct $provider {
            core: crate::providers::openai_compat::OpenAiCompatCore<$spec>,
        }

        impl $provider {
            pub fn new(api_key: String) -> Self {
                Self::with_model(
                    api_key,
                    <$spec as crate::providers::openai_compat::OpenAiCompatSpec>::DEFAULT_MODEL
                        .to_string(),
                )
            }

            pub fn with_model(api_key: String, model: String) -> Self {
                Self {
                    core: crate::providers::openai_compat::OpenAiCompatCore::direct(
                        api_key, model,
                    ),
                }
            }

            pub fn new_with_client(
                api_key: String,
                model: String,
                http_client: reqwest::Client,
                base_url: String,
                _timeouts: vtcode_config::TimeoutsConfig,
            ) -> Self {
                Self {
                    core: crate::providers::openai_compat::OpenAiCompatCore::from_parts(
                        api_key,
                        model,
                        http_client,
                        base_url,
                    ),
                }
            }

            pub fn from_config(
                api_key: Option<String>,
                model: Option<String>,
                base_url: Option<String>,
                prompt_cache: Option<vtcode_config::core::PromptCachingConfig>,
                timeouts: Option<vtcode_config::TimeoutsConfig>,
                _anthropic: Option<vtcode_config::core::AnthropicConfig>,
                model_behavior: Option<vtcode_config::core::ModelConfig>,
            ) -> Self {
                Self {
                    core: crate::providers::openai_compat::OpenAiCompatCore::from_config(
                        api_key,
                        model,
                        base_url,
                        prompt_cache,
                        timeouts,
                        model_behavior,
                    ),
                }
            }
        }

        #[async_trait::async_trait]
        impl crate::provider::LLMProvider for $provider {
            fn name(&self) -> &str {
                <$spec as crate::providers::openai_compat::OpenAiCompatSpec>::KEY
            }

            async fn generate(
                &self,
                mut request: crate::provider::LLMRequest,
            ) -> Result<crate::provider::LLMResponse, crate::provider::LLMError> {
                self.core.prepare(&mut request);
                if <$spec as crate::providers::openai_compat::OpenAiCompatSpec>::VALIDATE_ON_GENERATE
                {
                    crate::provider::LLMProvider::validate_request(self, &request)?;
                }
                self.core.generate_prepared(request).await
            }

            async fn stream(
                &self,
                mut request: crate::provider::LLMRequest,
            ) -> Result<crate::provider::LLMStream, crate::provider::LLMError> {
                self.core.prepare(&mut request);
                crate::provider::LLMProvider::validate_request(self, &request)?;
                request.stream = true;
                self.core.stream_prepared(request).await
            }

            fn supported_models(&self) -> Vec<String> {
                self.core.supported_models()
            }

            fn validate_request(
                &self,
                request: &crate::provider::LLMRequest,
            ) -> Result<(), crate::provider::LLMError> {
                self.core.validate(request)
            }

            $($($extra)*)?
        }

        #[async_trait::async_trait]
        impl crate::client::LLMClient for $provider {
            async fn generate(
                &mut self,
                prompt: &str,
            ) -> Result<crate::provider::LLMResponse, crate::provider::LLMError> {
                let request =
                    crate::providers::common::make_default_request(prompt, &self.core.model);
                Ok(<$provider as crate::provider::LLMProvider>::generate(self, request).await?)
            }

            fn model_id(&self) -> &str {
                &self.core.model
            }
        }
    };
}

pub(crate) use impl_openai_compat_provider;
