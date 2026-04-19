#![allow(clippy::result_large_err)]

use crate::config::TimeoutsConfig;
use crate::config::constants::{env_vars, models, urls};
use crate::config::core::{AnthropicConfig, ModelConfig, PromptCachingConfig};
use crate::config::models::model_catalog_entry;
use crate::llm::client::LLMClient;
use crate::llm::provider::{LLMError, LLMProvider, LLMRequest, LLMResponse, LLMStream};
use crate::llm::types as llm_types;
use async_trait::async_trait;
use reqwest::Client as HttpClient;

use super::AnthropicProvider;
use super::common::{override_base_url, resolve_model};
use super::opencode_shared::OpenCodeCompatibleProvider;

const PROVIDER_NAME: &str = "OpenCode Go";
const PROVIDER_KEY: &str = "opencode-go";
const API_KEY_ENV: &str = "OPENCODE_GO_API_KEY";

enum GoProtocol {
    MessagesApi,
    OpenAICompatible,
}

pub struct OpenCodeGoProvider {
    api_key: String,
    http_client: HttpClient,
    base_url: String,
    model: String,
}

impl OpenCodeGoProvider {
    fn normalize_model(model: &str) -> &str {
        model
            .trim()
            .strip_prefix("opencode-go/")
            .unwrap_or(model.trim())
    }

    pub fn new(api_key: String) -> Self {
        Self::with_model_internal(
            api_key,
            models::opencode_go::DEFAULT_MODEL.to_string(),
            None,
            None,
            None,
        )
    }

    pub fn with_model(api_key: String, model: String) -> Self {
        Self::with_model_internal(api_key, model, None, None, None)
    }

    pub fn new_with_client(
        api_key: String,
        model: String,
        http_client: reqwest::Client,
        base_url: String,
        _timeouts: TimeoutsConfig,
    ) -> Self {
        Self {
            api_key,
            http_client,
            base_url,
            model: Self::normalize_model(&model).to_string(),
        }
    }

    pub fn from_config(
        api_key: Option<String>,
        model: Option<String>,
        base_url: Option<String>,
        _prompt_cache: Option<PromptCachingConfig>,
        timeouts: Option<TimeoutsConfig>,
        _anthropic: Option<AnthropicConfig>,
        _model_behavior: Option<ModelConfig>,
    ) -> Self {
        let api_key_value = api_key.unwrap_or_default();
        let model_value = resolve_model(model, models::opencode_go::DEFAULT_MODEL);

        Self::with_model_internal(
            api_key_value,
            model_value,
            base_url,
            timeouts,
            _model_behavior,
        )
    }

    fn with_model_internal(
        api_key: String,
        model: String,
        base_url: Option<String>,
        timeouts: Option<TimeoutsConfig>,
        _model_behavior: Option<ModelConfig>,
    ) -> Self {
        use crate::llm::http_client::HttpClientFactory;

        let timeouts = timeouts.unwrap_or_default();

        Self {
            api_key,
            http_client: HttpClientFactory::for_llm(&timeouts),
            base_url: override_base_url(
                urls::OPENCODE_GO_API_BASE,
                base_url,
                Some(env_vars::OPENCODE_GO_BASE_URL),
            ),
            model: Self::normalize_model(&model).to_string(),
        }
    }

    fn requested_model<'a>(&'a self, model: &'a str) -> &'a str {
        if model.trim().is_empty() {
            self.model.as_str()
        } else {
            Self::normalize_model(model)
        }
    }

    fn catalog_entry(&self, model: &str) -> Option<vtcode_config::models::ModelCatalogEntry> {
        model_catalog_entry(PROVIDER_KEY, self.requested_model(model))
    }

    fn protocol_for_model(model: &str) -> GoProtocol {
        if models::opencode_go::MESSAGES_API_MODELS.contains(&model) {
            GoProtocol::MessagesApi
        } else {
            GoProtocol::OpenAICompatible
        }
    }

    fn delegate_for_model(&self, model: &str) -> Box<dyn LLMProvider> {
        let requested = self.requested_model(model).to_string();
        match Self::protocol_for_model(requested.as_str()) {
            GoProtocol::MessagesApi => Box::new(AnthropicProvider::new_with_client(
                self.api_key.clone(),
                requested,
                self.http_client.clone(),
                self.base_url.clone(),
                TimeoutsConfig::default(),
            )),
            GoProtocol::OpenAICompatible => Box::new(OpenCodeCompatibleProvider::new(
                PROVIDER_NAME,
                PROVIDER_KEY,
                API_KEY_ENV,
                self.api_key.clone(),
                self.http_client.clone(),
                self.base_url.clone(),
                requested,
                models::opencode_go::SUPPORTED_MODELS,
            )),
        }
    }
}

#[async_trait]
impl LLMProvider for OpenCodeGoProvider {
    fn name(&self) -> &str {
        PROVIDER_KEY
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_reasoning(&self, model: &str) -> bool {
        self.catalog_entry(model)
            .map(|entry| entry.reasoning)
            .unwrap_or(false)
    }

    fn supports_tools(&self, model: &str) -> bool {
        self.catalog_entry(model)
            .map(|entry| entry.tool_call)
            .unwrap_or(true)
    }

    fn supports_structured_output(&self, model: &str) -> bool {
        self.catalog_entry(model)
            .map(|entry| entry.structured_output)
            .unwrap_or(false)
    }

    fn supports_context_caching(&self, model: &str) -> bool {
        self.catalog_entry(model)
            .map(|entry| entry.caching)
            .unwrap_or(false)
    }

    fn supports_vision(&self, model: &str) -> bool {
        self.catalog_entry(model)
            .map(|entry| entry.vision)
            .unwrap_or(false)
    }

    fn effective_context_size(&self, model: &str) -> usize {
        self.catalog_entry(model)
            .map(|entry| entry.context_window)
            .filter(|value| *value > 0)
            .unwrap_or(128_000)
    }

    async fn generate(&self, mut request: LLMRequest) -> Result<LLMResponse, LLMError> {
        if request.model.trim().is_empty() {
            request.model = self.model.clone();
        } else {
            request.model = self.requested_model(&request.model).to_string();
        }
        self.validate_request(&request)?;
        self.delegate_for_model(&request.model)
            .generate(request)
            .await
    }

    async fn stream(&self, mut request: LLMRequest) -> Result<LLMStream, LLMError> {
        if request.model.trim().is_empty() {
            request.model = self.model.clone();
        } else {
            request.model = self.requested_model(&request.model).to_string();
        }
        self.validate_request(&request)?;
        self.delegate_for_model(&request.model)
            .stream(request)
            .await
    }

    fn supported_models(&self) -> Vec<String> {
        models::opencode_go::SUPPORTED_MODELS
            .iter()
            .map(|model| model.to_string())
            .collect()
    }

    fn validate_request(&self, request: &LLMRequest) -> Result<(), LLMError> {
        let mut normalized = request.clone();
        if !normalized.model.trim().is_empty() {
            normalized.model = self.requested_model(&normalized.model).to_string();
        }

        let supported_models = models::opencode_go::SUPPORTED_MODELS
            .iter()
            .map(|model| model.to_string())
            .collect::<Vec<_>>();

        super::common::validate_request_common(
            &normalized,
            PROVIDER_NAME,
            PROVIDER_KEY,
            Some(&supported_models),
        )
    }
}

#[async_trait]
impl LLMClient for OpenCodeGoProvider {
    async fn generate(&mut self, prompt: &str) -> Result<LLMResponse, LLMError> {
        let request = LLMRequest {
            messages: vec![crate::llm::provider::Message::user(prompt.to_string())],
            model: self.model.clone(),
            ..Default::default()
        };
        LLMProvider::generate(self, request).await
    }

    fn backend_kind(&self) -> llm_types::BackendKind {
        llm_types::BackendKind::OpenCodeGo
    }

    fn model_id(&self) -> &str {
        &self.model
    }
}
