use crate::client::LLMClient;
use crate::provider::{LLMError, LLMProvider, LLMRequest, LLMResponse, LLMStream};
use async_trait::async_trait;
use reqwest::Client as HttpClient;
use vtcode_config::TimeoutsConfig;
use vtcode_config::constants::{env_vars, models, urls};
use vtcode_config::core::{AnthropicConfig, ModelConfig, PromptCachingConfig};
use vtcode_config::models::model_catalog_entry;

use super::AnthropicProvider;
use super::common::{override_base_url, resolve_model};
use super::opencode_shared::{
    OPENCODE_GO_SESSION_HEADER, OpenCodeCompatibleProvider, OpenCodeGoInnerSpec, validate_normalized_request,
};

const PROVIDER_NAME: &str = "OpenCode Go";
const PROVIDER_KEY: &str = "opencode-go";

enum GoProtocol {
    MessagesApi,
    OpenAICompatible,
}

pub struct OpenCodeGoProvider {
    api_key: String,
    http_client: HttpClient,
    base_url: String,
    model: String,
    /// Instance-stable fallback for `x-opencode-session` when the request
    /// carries no session lineage. The Go endpoint rejects headerless
    /// requests, so every dispatch must send a value.
    session_id: String,
}

impl OpenCodeGoProvider {
    fn normalize_model(model: &str) -> &str {
        model.trim().strip_prefix("opencode-go/").unwrap_or(model.trim())
    }

    pub fn new(api_key: String) -> Self {
        Self::with_model_internal(api_key, models::opencode_go::DEFAULT_MODEL.to_string(), None, None, None)
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
            session_id: uuid::Uuid::new_v4().to_string(),
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

        Self::with_model_internal(api_key_value, model_value, base_url, timeouts, _model_behavior)
    }

    fn with_model_internal(
        api_key: String,
        model: String,
        base_url: Option<String>,
        timeouts: Option<TimeoutsConfig>,
        _model_behavior: Option<ModelConfig>,
    ) -> Self {
        use crate::http_client::HttpClientFactory;

        let timeouts = timeouts.unwrap_or_default();

        Self {
            api_key,
            http_client: HttpClientFactory::for_llm(&timeouts),
            base_url: override_base_url(urls::OPENCODE_GO_API_BASE, base_url, Some(env_vars::OPENCODE_GO_BASE_URL)),
            model: Self::normalize_model(&model).to_string(),
            session_id: uuid::Uuid::new_v4().to_string(),
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

    /// Guarantee the request carries session lineage so both delegates can
    /// attach `x-opencode-session`. Lineage from the runloop wins; otherwise
    /// the provider's instance-stable id keeps the value constant across
    /// turns of this process instead of failing with `MissingSessionID`.
    fn ensure_session_key(&self, request: &mut LLMRequest) {
        if crate::providers::shared::session_lineage_from_prompt_cache_key(request.prompt_cache_key.as_deref())
            .is_none()
        {
            request.prompt_cache_key = Some(self.session_id.clone());
        }
    }

    fn delegate_for_model(&self, model: &str) -> Box<dyn LLMProvider> {
        let requested = self.requested_model(model).to_string();
        match Self::protocol_for_model(requested.as_str()) {
            GoProtocol::MessagesApi => Box::new(
                AnthropicProvider::new_with_client(
                    self.api_key.clone(),
                    requested,
                    self.http_client.clone(),
                    self.base_url.clone(),
                    TimeoutsConfig::default(),
                )
                .with_session_affinity_header(OPENCODE_GO_SESSION_HEADER),
            ),
            GoProtocol::OpenAICompatible => Box::new(OpenCodeCompatibleProvider::<OpenCodeGoInnerSpec>::new(
                self.api_key.clone(),
                self.http_client.clone(),
                self.base_url.clone(),
                requested,
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

    fn supports_non_streaming(&self, _model: &str) -> bool {
        // Pinned so the stream-timeout fallback cannot silently regress.
        true
    }

    fn supports_reasoning(&self, model: &str) -> bool {
        self.catalog_entry(model).map(|entry| entry.reasoning).unwrap_or(false)
    }

    fn supports_tools(&self, model: &str) -> bool {
        self.catalog_entry(model).map(|entry| entry.tool_call).unwrap_or(true)
    }

    fn supports_structured_output(&self, model: &str) -> bool {
        self.catalog_entry(model).map(|entry| entry.structured_output).unwrap_or(false)
    }

    fn supports_context_caching(&self, model: &str) -> bool {
        self.catalog_entry(model).map(|entry| entry.caching).unwrap_or(false)
    }

    fn supports_vision(&self, model: &str) -> bool {
        self.catalog_entry(model).map(|entry| entry.vision).unwrap_or(false)
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
        self.ensure_session_key(&mut request);
        self.validate_request(&request)?;
        self.delegate_for_model(&request.model).generate(request).await
    }

    async fn stream(&self, mut request: LLMRequest) -> Result<LLMStream, LLMError> {
        if request.model.trim().is_empty() {
            request.model = self.model.clone();
        } else {
            request.model = self.requested_model(&request.model).to_string();
        }
        self.ensure_session_key(&mut request);
        self.validate_request(&request)?;
        self.delegate_for_model(&request.model).stream(request).await
    }

    fn supported_models(&self) -> Vec<String> {
        models::opencode_go::SUPPORTED_MODELS
            .iter()
            .map(|model| model.to_string())
            .collect()
    }

    fn validate_request(&self, request: &LLMRequest) -> Result<(), LLMError> {
        validate_normalized_request(
            request,
            self.requested_model(&request.model),
            PROVIDER_NAME,
            PROVIDER_KEY,
            models::opencode_go::SUPPORTED_MODELS,
        )
    }
}

#[async_trait]
impl LLMClient for OpenCodeGoProvider {
    async fn generate(&mut self, prompt: &str) -> Result<LLMResponse, LLMError> {
        let request = LLMRequest {
            messages: std::sync::Arc::new(vec![crate::provider::Message::user(prompt.to_string())]),
            model: self.model.clone(),
            ..Default::default()
        };
        LLMProvider::generate(self, request).await
    }

    fn model_id(&self) -> &str {
        &self.model
    }
}

#[cfg(test)]
mod tests {
    use super::OpenCodeGoProvider;
    use crate::provider::{LLMProvider, LLMRequest, Message};
    use crate::providers::test_support::start_mock_server_or_skip;
    use serde_json::json;
    use vtcode_config::TimeoutsConfig;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, ResponseTemplate};

    const SESSION_HEADER: &str = "x-opencode-session";

    fn test_provider(base_url: &str, model: &str) -> OpenCodeGoProvider {
        let http_client = reqwest::Client::builder().no_proxy().build().expect("test client should build");
        OpenCodeGoProvider::new_with_client(
            "test-key".to_string(),
            model.to_string(),
            http_client,
            base_url.to_string(),
            TimeoutsConfig::default(),
        )
    }

    fn chat_request(model: &str, prompt_cache_key: Option<String>) -> LLMRequest {
        LLMRequest {
            model: model.to_string(),
            messages: vec![Message::user("hi".to_string())].into(),
            prompt_cache_key,
            ..Default::default()
        }
    }

    fn session_header_value(received: &wiremock::Request) -> String {
        received
            .headers
            .get(SESSION_HEADER)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string()
    }

    #[test]
    fn go_spec_advertises_session_affinity_header() {
        use crate::providers::openai_compat::OpenAiCompatSpec;
        use crate::providers::opencode_shared::OpenCodeGoInnerSpec;
        assert_eq!(OpenCodeGoInnerSpec::session_affinity_header(), Some(SESSION_HEADER));
    }

    #[tokio::test]
    async fn chat_completions_path_sends_lineage_session_header() {
        let model = vtcode_config::constants::models::opencode_go::GLM_5_3;
        let Some(server) = start_mock_server_or_skip().await else {
            return;
        };
        let provider = test_provider(&server.uri(), model);

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(header(SESSION_HEADER, "session-lineage-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{"finish_reason": "stop", "message": {"content": "ok"}}],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
            })))
            .expect(1)
            .mount(&server)
            .await;

        let response = provider
            .generate(chat_request(model, Some("vtcode:opencode-go:session-lineage-1".to_string())))
            .await
            .expect("generate with session lineage");
        assert_eq!(response.content.as_deref(), Some("ok"));
    }

    #[tokio::test]
    async fn chat_completions_path_sends_fallback_session_header_without_lineage() {
        let model = vtcode_config::constants::models::opencode_go::GLM_5_3;
        let Some(server) = start_mock_server_or_skip().await else {
            return;
        };
        let provider = test_provider(&server.uri(), model);

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{"finish_reason": "stop", "message": {"content": "ok"}}],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
            })))
            .expect(1)
            .mount(&server)
            .await;

        provider
            .generate(chat_request(model, None))
            .await
            .expect("generate without lineage");

        let requests = server.received_requests().await.expect("received requests");
        assert_eq!(requests.len(), 1);
        let session = session_header_value(&requests[0]);
        assert!(!session.trim().is_empty(), "x-opencode-session must always be sent");
    }

    #[tokio::test]
    async fn fallback_session_id_is_stable_across_requests() {
        let model = vtcode_config::constants::models::opencode_go::GLM_5_3;
        let Some(server) = start_mock_server_or_skip().await else {
            return;
        };
        let provider = test_provider(&server.uri(), model);

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{"finish_reason": "stop", "message": {"content": "ok"}}],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
            })))
            .expect(2)
            .mount(&server)
            .await;

        for _ in 0..2 {
            provider
                .generate(chat_request(model, None))
                .await
                .expect("generate without lineage");
        }

        let requests = server.received_requests().await.expect("received requests");
        assert_eq!(requests.len(), 2);
        let sessions: Vec<String> = requests.iter().map(session_header_value).collect();
        assert!(!sessions[0].trim().is_empty(), "first request must carry a session id");
        assert_eq!(sessions[0], sessions[1], "fallback session id must be stable per provider");
    }

    #[tokio::test]
    async fn messages_api_path_sends_lineage_session_header() {
        let model = vtcode_config::constants::models::opencode_go::MINIMAX_M3;
        let Some(server) = start_mock_server_or_skip().await else {
            return;
        };
        let provider = test_provider(&server.uri(), model);

        Mock::given(method("POST"))
            .and(path("/messages"))
            .and(header(SESSION_HEADER, "session-lineage-2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "content": [{"type": "text", "text": "ok"}],
                "stop_reason": "end_turn"
            })))
            .expect(1)
            .mount(&server)
            .await;

        let response = provider
            .generate(chat_request(model, Some("vtcode:opencode-go:session-lineage-2".to_string())))
            .await
            .expect("messages-api generate with session lineage");
        assert_eq!(response.content.as_deref(), Some("ok"));
    }

    #[tokio::test]
    async fn chat_completions_stream_sends_lineage_session_header() {
        use futures::StreamExt;

        use crate::provider::LLMStreamEvent;

        let model = vtcode_config::constants::models::opencode_go::GLM_5_3;
        let Some(server) = start_mock_server_or_skip().await else {
            return;
        };
        let provider = test_provider(&server.uri(), model);

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(header(SESSION_HEADER, "session-lineage-7"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n",
            ))
            .expect(1)
            .mount(&server)
            .await;

        let mut stream = provider
            .stream(chat_request(model, Some("vtcode:opencode-go:session-lineage-7".to_string())))
            .await
            .expect("stream with session lineage");
        let mut completed = None;
        while let Some(event) = stream.next().await {
            if let LLMStreamEvent::Completed { response } = event.expect("stream event") {
                completed = Some(response);
            }
        }
        assert_eq!(completed.expect("completed response").content.as_deref(), Some("ok"));
    }

    #[tokio::test]
    async fn messages_api_path_sends_fallback_session_header_without_lineage() {
        let model = vtcode_config::constants::models::opencode_go::MINIMAX_M3;
        let Some(server) = start_mock_server_or_skip().await else {
            return;
        };
        let provider = test_provider(&server.uri(), model);

        Mock::given(method("POST"))
            .and(path("/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "content": [{"type": "text", "text": "ok"}],
                "stop_reason": "end_turn"
            })))
            .expect(1)
            .mount(&server)
            .await;

        // Blank keys carry no identity and must fall back exactly like a missing key
        // (the chat-path test covers `None`; this one covers whitespace).
        provider
            .generate(chat_request(model, Some("   ".to_string())))
            .await
            .expect("messages-api generate without lineage");

        let requests = server.received_requests().await.expect("received requests");
        assert_eq!(requests.len(), 1);
        let session = session_header_value(&requests[0]);
        assert!(!session.trim().is_empty(), "x-opencode-session must always be sent");
    }
}
