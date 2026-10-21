use async_trait::async_trait;
use vtcode_commons::llm::BackendKind;
use vtcode_config::core::AnthropicConfig;
use vtcode_config::core::{CustomProviderApiFormat, CustomProviderConfig, ResolvedCustomProviderProfile};
use vtcode_config::{ModelConfig, OpenAIConfig, PromptCachingConfig, TimeoutsConfig};

use crate::provider::{
    LLMError, LLMProvider, LLMRequest, LLMResponse, LLMStream, Message, ResponsesCompactionOptions, SamplingOverrides,
};
use crate::providers::anthropic::{self, AnthropicProvider};
use crate::providers::common::{resolve_model, validate_request_common};
use crate::providers::openai::{CustomProviderAuthHandle, OpenAIProvider};

/// Profile-aware backend router for custom providers.
pub struct CustomProviderBackendRouter {
    provider_key: String,
    display_name: String,
    default_model: String,
    supported_models: Vec<String>,
    custom_config: CustomProviderConfig,
    anthropic_config: AnthropicConfig,
    openai_auto: OpenAIProvider,
    openai_chat: OpenAIProvider,
    openai_responses: OpenAIProvider,
    anthropic: AnthropicProvider,
}

impl CustomProviderBackendRouter {
    #[allow(
        clippy::too_many_arguments,
        reason = "Intentional compatibility, platform, or test-only suppression."
    )]
    pub fn from_config(
        custom_config: CustomProviderConfig,
        api_key: Option<String>,
        model: Option<String>,
        base_url: String,
        prompt_cache: Option<PromptCachingConfig>,
        timeouts: Option<TimeoutsConfig>,
        openai: Option<OpenAIConfig>,
        anthropic: Option<AnthropicConfig>,
        model_behavior: Option<ModelConfig>,
        custom_provider_auth: Option<CustomProviderAuthHandle>,
    ) -> Self {
        let provider_key = custom_config.name.clone();
        let display_name = custom_config.display_name.clone();
        let supported_models = custom_config.effective_models();
        let configured_default = resolve_model(model, &custom_config.model);
        let default_model = if configured_default.trim().is_empty() {
            supported_models.first().cloned().unwrap_or_default()
        } else {
            configured_default
        };
        let anthropic_config = anthropic.unwrap_or_default();

        let openai_auto = OpenAIProvider::from_custom_config(
            provider_key.clone(),
            display_name.clone(),
            api_key.clone(),
            Some(default_model.clone()),
            Some(base_url.clone()),
            prompt_cache.clone(),
            timeouts.clone(),
            openai.clone(),
            model_behavior.clone(),
            custom_provider_auth.clone(),
            Some(supported_models.clone()),
        );
        let openai_chat = OpenAIProvider::from_custom_config(
            provider_key.clone(),
            display_name.clone(),
            api_key.clone(),
            Some(default_model.clone()),
            Some(base_url.clone()),
            prompt_cache.clone(),
            timeouts.clone(),
            openai.clone(),
            model_behavior.clone(),
            custom_provider_auth.clone(),
            Some(supported_models.clone()),
        )
        .with_api_format_override(Some(CustomProviderApiFormat::OpenAIChat));
        let openai_responses = OpenAIProvider::from_custom_config(
            provider_key.clone(),
            display_name.clone(),
            api_key.clone(),
            Some(default_model.clone()),
            Some(base_url.clone()),
            prompt_cache.clone(),
            timeouts.clone(),
            openai.clone(),
            model_behavior.clone(),
            custom_provider_auth.clone(),
            Some(supported_models.clone()),
        )
        .with_api_format_override(Some(CustomProviderApiFormat::OpenAIResponses))
        .with_prompt_cache_options_models(
            supported_models
                .iter()
                .filter(|model| custom_config.resolved_profile(model).supports_prompt_cache_options == Some(true))
                .cloned()
                .collect(),
        );
        let anthropic = AnthropicProvider::from_config(
            api_key,
            Some(default_model.clone()),
            Some(base_url),
            prompt_cache,
            timeouts,
            Some(anthropic_config.clone()),
            model_behavior,
        )
        .with_custom_auth(custom_provider_auth);

        Self {
            provider_key,
            display_name,
            default_model,
            supported_models,
            custom_config,
            anthropic_config,
            openai_auto,
            openai_chat,
            openai_responses,
            anthropic,
        }
    }

    fn resolved_model<'a>(&'a self, model: &'a str) -> &'a str {
        if model.trim().is_empty() {
            &self.default_model
        } else {
            model
        }
    }

    fn profile_for_model(&self, model: &str) -> ResolvedCustomProviderProfile {
        self.custom_config.resolved_profile(model)
    }

    fn backend_for_model(&self, model: &str) -> &dyn LLMProvider {
        match self.profile_for_model(self.resolved_model(model)).api_format {
            Some(CustomProviderApiFormat::AnthropicMessages) => &self.anthropic,
            Some(CustomProviderApiFormat::OpenAIChat) => &self.openai_chat,
            Some(CustomProviderApiFormat::OpenAIResponses) => &self.openai_responses,
            Some(CustomProviderApiFormat::Auto) | None => &self.openai_auto,
        }
    }

    fn override_bool(value: Option<bool>, default: bool) -> bool {
        value.unwrap_or(default)
    }
}

#[async_trait]
impl LLMProvider for CustomProviderBackendRouter {
    fn name(&self) -> &str {
        &self.provider_key
    }

    fn backend_kind(&self) -> BackendKind {
        self.backend_for_model(&self.default_model).backend_kind()
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_non_streaming(&self, model: &str) -> bool {
        self.backend_for_model(model).supports_non_streaming(self.resolved_model(model))
    }

    fn supports_reasoning(&self, model: &str) -> bool {
        let resolved = self.resolved_model(model);
        let profile = self.profile_for_model(resolved);
        Self::override_bool(profile.supports_reasoning, self.backend_for_model(model).supports_reasoning(resolved))
    }

    fn supports_reasoning_effort(&self, model: &str) -> bool {
        let resolved = self.resolved_model(model);
        let profile = self.profile_for_model(resolved);
        // A pin in THIS model's own profile implies effort support unless the
        // same profile explicitly disables it. Provider-level defaults — both
        // the effort value and the capability flag — must not flip support for
        // every model on the endpoint.
        let own = self.custom_config.profiles.get(resolved);
        let own_pin = own.and_then(|p| p.reasoning_effort).is_some();
        if own_pin {
            return own.and_then(|p| p.supports_reasoning_effort) != Some(false);
        }
        Self::override_bool(
            profile.supports_reasoning_effort,
            self.backend_for_model(model).supports_reasoning_effort(resolved),
        )
    }

    fn sampling_overrides(&self, model: &str) -> SamplingOverrides {
        let resolved = self.resolved_model(model);
        let profile = self.profile_for_model(resolved);
        SamplingOverrides {
            temperature: profile.temperature,
            top_p: profile.top_p,
            top_k: profile.top_k,
            presence_penalty: profile.presence_penalty,
            frequency_penalty: profile.frequency_penalty,
            max_tokens: profile.max_tokens,
            reasoning_effort: profile.reasoning_effort,
            suppresses_sampling_with_reasoning: profile.api_format == Some(CustomProviderApiFormat::AnthropicMessages),
            profile_aware: true,
        }
    }

    fn supports_tools(&self, model: &str) -> bool {
        let resolved = self.resolved_model(model);
        let profile = self.profile_for_model(resolved);
        Self::override_bool(profile.supports_tools, self.backend_for_model(model).supports_tools(resolved))
    }

    fn supports_parallel_tool_config(&self, model: &str) -> bool {
        let resolved = self.resolved_model(model);
        let profile = self.profile_for_model(resolved);
        Self::override_bool(
            profile.supports_parallel_tool_calls,
            self.backend_for_model(model).supports_parallel_tool_config(resolved),
        )
    }

    fn supports_structured_output(&self, model: &str) -> bool {
        let resolved = self.resolved_model(model);
        let profile = self.profile_for_model(resolved);
        Self::override_bool(
            profile.supports_structured_output,
            self.backend_for_model(model).supports_structured_output(resolved),
        )
    }

    fn requested_prompt_cache_ttl(&self, model: &str) -> Option<vtcode_config::core::prompt_cache::PromptCacheTtl> {
        self.backend_for_model(model)
            .requested_prompt_cache_ttl(self.resolved_model(model))
    }

    fn supports_context_caching(&self, model: &str) -> bool {
        let resolved = self.resolved_model(model);
        let profile = self.profile_for_model(resolved);
        Self::override_bool(
            profile.supports_context_caching,
            self.backend_for_model(model).supports_context_caching(resolved),
        )
    }

    fn supports_vision(&self, model: &str) -> bool {
        let resolved = self.resolved_model(model);
        let profile = self.profile_for_model(resolved);
        Self::override_bool(profile.supports_vision, self.backend_for_model(model).supports_vision(resolved))
    }

    fn supports_responses_compaction(&self, model: &str) -> bool {
        let resolved = self.resolved_model(model);
        let profile = self.profile_for_model(resolved);
        let backend_default = match self.profile_for_model(resolved).api_format {
            Some(CustomProviderApiFormat::AnthropicMessages) => false,
            _ => self.backend_for_model(model).supports_responses_compaction(resolved),
        };
        Self::override_bool(profile.supports_responses_compaction, backend_default)
    }

    fn supports_native_allowed_tools(&self, model: &str) -> bool {
        self.backend_for_model(model)
            .supports_native_allowed_tools(self.resolved_model(model))
    }

    fn supports_context_edits(&self, model: &str) -> bool {
        let resolved = self.resolved_model(model);
        let profile = self.profile_for_model(resolved);
        Self::override_bool(
            profile.supports_context_edits,
            self.backend_for_model(model).supports_context_edits(resolved),
        )
    }

    fn supports_turn_scoped_system_messages(&self, model: &str) -> bool {
        let resolved = self.resolved_model(model);
        matches!(self.profile_for_model(resolved).api_format, Some(CustomProviderApiFormat::AnthropicMessages))
            && self.backend_for_model(model).supports_turn_scoped_system_messages(resolved)
    }

    fn supports_manual_openai_compaction(&self, model: &str) -> bool {
        let resolved = self.resolved_model(model);
        let profile = self.profile_for_model(resolved);
        let backend_default = match self.profile_for_model(resolved).api_format {
            Some(CustomProviderApiFormat::AnthropicMessages) => false,
            _ => self.backend_for_model(model).supports_manual_openai_compaction(resolved),
        };
        Self::override_bool(profile.supports_responses_compaction, backend_default)
    }

    fn supports_native_inline_compaction(&self, model: &str) -> bool {
        let resolved = self.resolved_model(model);
        let profile = self.profile_for_model(resolved);
        Self::override_bool(
            profile.supports_context_edits,
            self.backend_for_model(model).supports_native_inline_compaction(resolved),
        )
    }

    fn manual_openai_compaction_unavailable_message(&self, model: &str) -> String {
        self.backend_for_model(model)
            .manual_openai_compaction_unavailable_message(self.resolved_model(model))
    }

    fn effective_context_size(&self, model: &str) -> usize {
        let resolved = self.resolved_model(model);
        let profile = self.profile_for_model(resolved);
        profile
            .context_window
            .unwrap_or_else(|| self.backend_for_model(model).effective_context_size(resolved))
    }

    async fn compact_history(&self, model: &str, history: &[Message]) -> Result<Vec<Message>, LLMError> {
        self.backend_for_model(model)
            .compact_history(self.resolved_model(model), history)
            .await
    }

    async fn compact_history_with_options(
        &self,
        model: &str,
        history: &[Message],
        options: &ResponsesCompactionOptions,
    ) -> Result<Vec<Message>, LLMError> {
        self.backend_for_model(model)
            .compact_history_with_options(self.resolved_model(model), history, options)
            .await
    }

    async fn stream(&self, request: LLMRequest) -> Result<LLMStream, LLMError> {
        self.backend_for_model(&request.model).stream(request).await
    }

    async fn stream_normalized(&self, request: LLMRequest) -> Result<crate::provider::LLMNormalizedStream, LLMError> {
        self.backend_for_model(&request.model).stream_normalized(request).await
    }

    async fn generate(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
        self.backend_for_model(&request.model).generate(request).await
    }

    async fn generate_auxiliary(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
        self.backend_for_model(&request.model).generate_auxiliary(request).await
    }

    fn supported_models(&self) -> Vec<String> {
        self.supported_models.clone()
    }

    fn validate_request(&self, request: &LLMRequest) -> Result<(), LLMError> {
        let resolved_model = self.resolved_model(&request.model);
        match self.profile_for_model(resolved_model).api_format {
            Some(CustomProviderApiFormat::AnthropicMessages) => {
                // Anthropic-shaped custom profiles use the Anthropic message
                // contract, including its turn-scoped system marker. Keep the
                // custom display name for diagnostics, but validate message roles
                // against the backend contract so the marker is not rejected.
                validate_request_common(request, &self.display_name, "anthropic", Some(&self.supported_models))?;
                anthropic::validation::validate_request(
                    request,
                    &self.default_model,
                    &self.anthropic_config,
                    &self.display_name,
                )
            }
            Some(CustomProviderApiFormat::OpenAIChat)
            | Some(CustomProviderApiFormat::OpenAIResponses)
            | Some(CustomProviderApiFormat::Auto)
            | None => self.backend_for_model(resolved_model).validate_request(request),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CustomProviderBackendRouter;
    use crate::provider::{LLMProvider, LLMRequest, Message, SamplingOverrides};
    use crate::providers::openai::CustomProviderAuthHandle;
    use serde_json::json;
    use std::path::Path;
    use tempfile::TempDir;
    use vtcode_config::constants::models;
    use vtcode_config::core::{
        AnthropicConfig, CustomProviderApiFormat, CustomProviderCommandAuthConfig, CustomProviderConfig,
        CustomProviderProfileConfig,
    };
    use vtcode_config::types::ReasoningEffortLevel;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn sampling_router() -> CustomProviderBackendRouter {
        let mut profiles = std::collections::BTreeMap::new();
        profiles.insert(
            "cold-model".to_string(),
            CustomProviderProfileConfig {
                temperature: Some(0.0),
                top_p: Some(0.9),
                reasoning_effort: Some(ReasoningEffortLevel::Low),
                ..Default::default()
            },
        );

        CustomProviderBackendRouter::from_config(
            CustomProviderConfig {
                name: "sampling-test".to_string(),
                display_name: "Sampling Test".to_string(),
                base_url: "https://llm.example/v1".to_string(),
                temperature: Some(0.5),
                models: vec!["cold-model".to_string(), "warm-model".to_string()],
                profiles,
                ..Default::default()
            },
            None,
            None,
            "https://llm.example/v1".to_string(),
            None,
            None,
            None,
            None,
            None,
            None,
        )
    }

    #[tokio::test]
    async fn auxiliary_generation_forwards_isolation_to_custom_openai_backend() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/responses"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "resp_custom_auxiliary",
                "output": [{"type": "message", "content": [{"type": "output_text", "text": "SAFE"}]}]
            })))
            .expect(1)
            .mount(&server)
            .await;
        let router = CustomProviderBackendRouter::from_config(
            CustomProviderConfig {
                name: "auxiliary-test".into(),
                display_name: "Auxiliary Test".into(),
                model: "gpt-6-luna".into(),
                models: vec!["gpt-6-luna".into()],
                api_format: CustomProviderApiFormat::OpenAIResponses,
                ..Default::default()
            },
            Some("fixture-key".into()),
            None,
            server.uri(),
            None,
            None,
            Some(vtcode_config::OpenAIConfig { websocket_mode: true, ..Default::default() }),
            None,
            None,
            None,
        );
        let response = router
            .generate_auxiliary(LLMRequest {
                model: "gpt-6-luna".into(),
                messages: vec![Message::user("independent custom probe".into())].into(),
                ..Default::default()
            })
            .await
            .expect("HTTP auxiliary response");
        assert_eq!(response.content.as_deref(), Some("SAFE"));
        let requests = server.received_requests().await.expect("recorded requests");
        assert_eq!(requests.len(), 1, "auxiliary must not attempt a WebSocket upgrade or warmup");
        assert_eq!(requests[0].method.as_str(), "POST");
        assert_eq!(requests[0].url.path(), "/responses");
        let payload: serde_json::Value = requests[0].body_json().expect("request payload");
        assert_eq!(payload["model"], "gpt-6-luna");
    }

    #[test]
    fn sampling_overrides_resolve_per_model_with_provider_defaults() {
        let router = sampling_router();

        let cold = router.sampling_overrides("cold-model");
        assert_eq!(
            cold,
            SamplingOverrides {
                temperature: Some(0.0),
                top_p: Some(0.9),
                top_k: None,
                presence_penalty: None,
                frequency_penalty: None,
                max_tokens: None,
                reasoning_effort: Some(ReasoningEffortLevel::Low),
                suppresses_sampling_with_reasoning: false,
                profile_aware: true,
            }
        );
        assert!(router.supports_reasoning_effort("cold-model"));

        // Models without a profile fall back to provider-level defaults.
        let warm = router.sampling_overrides("warm-model");
        assert_eq!(warm.temperature, Some(0.5));
        assert_eq!(warm.top_p, None);
    }

    #[test]
    fn sampling_overrides_suppression_matrix_matches_native_semantics() {
        let profile_openai = SamplingOverrides { profile_aware: true, ..Default::default() };
        let profile_anthropic = SamplingOverrides {
            suppresses_sampling_with_reasoning: true,
            profile_aware: true,
            ..Default::default()
        };
        let builtin_default = SamplingOverrides::default();

        // Custom openai-shaped profile keeps pinned values during reasoning.
        assert!(!profile_openai.suppresses_sampling(false, true));

        // Custom anthropic-messages profile drops them.
        assert!(profile_anthropic.suppresses_sampling(false, true));
        assert!(!profile_anthropic.suppresses_sampling(false, false));

        // Built-in Anthropic/MiniMax shape suppresses only via native match;
        // built-in OpenAI shape never does through overrides alone.
        assert!(builtin_default.suppresses_sampling(true, true));
        assert!(!builtin_default.suppresses_sampling(true, false));
        assert!(!builtin_default.suppresses_sampling(false, true));

        // Level "none"/"unknown" is not active reasoning anywhere.
        assert!(!profile_anthropic.suppresses_sampling(false, false));
    }

    #[test]
    fn turn_scoped_system_capability_follows_selected_api_profile() {
        let mut profiles = std::collections::BTreeMap::new();
        profiles.insert(
            models::anthropic::CLAUDE_FABLE_5.to_string(),
            CustomProviderProfileConfig {
                api_format: CustomProviderApiFormat::AnthropicMessages,
                ..Default::default()
            },
        );

        let router = CustomProviderBackendRouter::from_config(
            CustomProviderConfig {
                name: "profiled-transport-test".to_string(),
                display_name: "Profiled Transport Test".to_string(),
                base_url: "https://llm.example/v1".to_string(),
                model: models::anthropic::CLAUDE_FABLE_5.to_string(),
                models: vec![
                    models::anthropic::CLAUDE_FABLE_5.to_string(),
                    "openai-shaped".to_string(),
                ],
                profiles,
                ..Default::default()
            },
            Some("test-key".to_string()),
            None,
            "https://llm.example/v1".to_string(),
            None,
            None,
            None,
            Some(AnthropicConfig::default()),
            None,
            None,
        );

        assert!(router.supports_turn_scoped_system_messages(models::anthropic::CLAUDE_FABLE_5));
        assert!(!router.supports_turn_scoped_system_messages("openai-shaped"));
    }

    #[test]
    fn anthropic_profile_accepts_turn_scoped_system_marker() {
        let mut profiles = std::collections::BTreeMap::new();
        profiles.insert(
            models::anthropic::CLAUDE_FABLE_5.to_string(),
            CustomProviderProfileConfig {
                api_format: CustomProviderApiFormat::AnthropicMessages,
                ..Default::default()
            },
        );
        let router = CustomProviderBackendRouter::from_config(
            CustomProviderConfig {
                name: "profiled-validation-test".to_string(),
                display_name: "Profiled Validation Test".to_string(),
                base_url: "https://llm.example/v1".to_string(),
                model: models::anthropic::CLAUDE_FABLE_5.to_string(),
                models: vec![models::anthropic::CLAUDE_FABLE_5.to_string()],
                profiles,
                ..Default::default()
            },
            Some("test-key".to_string()),
            None,
            "https://llm.example/v1".to_string(),
            None,
            None,
            None,
            Some(AnthropicConfig::default()),
            None,
            None,
        );
        let request = LLMRequest {
            model: models::anthropic::CLAUDE_FABLE_5.to_string(),
            messages: vec![Message::turn_scoped_system("visible-output notice".to_string())].into(),
            ..Default::default()
        };

        router
            .validate_request(&request)
            .expect("custom Anthropic profile should accept native marker");
    }

    fn write_tokens_file(dir: &Path, tokens: &[&str]) {
        std::fs::write(dir.join("tokens.txt"), tokens.join("\n")).expect("write tokens file");
    }

    #[cfg(unix)]
    fn auth_fixture(dir: &TempDir, tokens: &[&str]) -> CustomProviderCommandAuthConfig {
        use std::os::unix::fs::PermissionsExt;

        write_tokens_file(dir.path(), tokens);
        let script_path = dir.path().join("print-token.sh");
        std::fs::write(
            &script_path,
            r#"#!/bin/sh
    first_line=$(sed -n '1p' tokens.txt)
    printf '%s\n' "$first_line"
    tail -n +2 tokens.txt > tokens.next
    mv tokens.next tokens.txt
    "#,
        )
        .expect("write script");
        let mut perms = std::fs::metadata(&script_path).expect("metadata").permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script_path, perms).expect("set permissions");
        CustomProviderCommandAuthConfig {
            command: "./print-token.sh".to_string(),
            args: Vec::new(),
            cwd: Some(dir.path().to_path_buf()),
            timeout_ms: 5_000,
            refresh_interval_ms: 60_000,
        }
    }

    #[cfg(windows)]
    fn auth_fixture(dir: &TempDir, tokens: &[&str]) -> CustomProviderCommandAuthConfig {
        write_tokens_file(dir.path(), tokens);
        let script_path = dir.path().join("print-token.ps1");
        std::fs::write(
            &script_path,
            r#"$lines = Get-Content -Path tokens.txt
    if ($lines.Count -eq 0) { exit 1 }
    Write-Output $lines[0]
    $lines | Select-Object -Skip 1 | Set-Content -Path tokens.txt
    "#,
        )
        .expect("write script");
        CustomProviderCommandAuthConfig {
            command: "powershell".to_string(),
            args: vec![
                "-NoProfile".to_string(),
                "-ExecutionPolicy".to_string(),
                "Bypass".to_string(),
                "-File".to_string(),
                script_path.to_string_lossy().into_owned(),
            ],
            cwd: Some(dir.path().to_path_buf()),
            timeout_ms: 5_000,
            refresh_interval_ms: 60_000,
        }
    }

    fn anthropic_response_body(text: &str) -> serde_json::Value {
        json!({
            "content": [{
                "type": "text",
                "text": text,
            }],
            "stop_reason": "end_turn",
            "usage": {
                "input_tokens": 1,
                "output_tokens": 1,
                "cache_creation_input_tokens": 0,
                "cache_read_input_tokens": 0
            }
        })
    }

    #[tokio::test]
    async fn anthropic_command_auth_refreshes_and_uses_protocol_headers() {
        let server = MockServer::start().await;
        let tempdir = TempDir::new().expect("tempdir");
        let seen_keys = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let auth_config = auth_fixture(&tempdir, &["first-token", "second-token"]);

        let seen_for_mock = std::sync::Arc::clone(&seen_keys);
        Mock::given(method("POST"))
            .and(path("/messages"))
            .respond_with(move |request: &wiremock::Request| {
                let api_key = request
                    .headers
                    .get("x-api-key")
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or_default()
                    .to_string();
                seen_for_mock.lock().expect("mutex").push(api_key);
                let count = seen_for_mock.lock().expect("mutex").len();
                if count == 1 {
                    ResponseTemplate::new(401)
                } else {
                    ResponseTemplate::new(200).set_body_json(anthropic_response_body("anthropic command auth"))
                }
            })
            .expect(2)
            .mount(&server)
            .await;

        let config = CustomProviderConfig {
            name: "anthropic-custom".to_string(),
            display_name: "Anthropic Custom".to_string(),
            base_url: server.uri(),
            api_format: CustomProviderApiFormat::AnthropicMessages,
            context_window: None,
            temperature: None,
            top_p: None,
            top_k: None,
            presence_penalty: None,
            frequency_penalty: None,
            max_tokens: None,
            reasoning_effort: None,
            supports_tools: None,
            supports_reasoning: None,
            supports_reasoning_effort: None,
            supports_vision: None,
            supports_structured_output: None,
            supports_parallel_tool_calls: None,
            supports_context_caching: None,
            supports_responses_compaction: None,
            supports_prompt_cache_options: None,
            supports_context_edits: Some(true),
            api_key_env: "ANTHROPIC_CUSTOM_API_KEY".to_string(),
            auth: None,
            model: models::anthropic::DEFAULT_MODEL.to_string(),
            models: vec![models::anthropic::DEFAULT_MODEL.to_string()],
            profiles: std::collections::BTreeMap::new(),
        };

        let router = CustomProviderBackendRouter::from_config(
            config,
            None,
            Some(models::anthropic::DEFAULT_MODEL.to_string()),
            server.uri(),
            None,
            None,
            None,
            Some(AnthropicConfig::default()),
            None,
            Some(CustomProviderAuthHandle::new(auth_config, None)),
        );

        let response = router
            .generate(LLMRequest {
                model: models::anthropic::DEFAULT_MODEL.to_string(),
                messages: vec![Message::user("hello".to_string())].into(),
                ..Default::default()
            })
            .await
            .expect("anthropic auth should refresh and succeed");

        assert_eq!(response.content.as_deref(), Some("anthropic command auth"));
        assert_eq!(
            seen_keys.lock().expect("mutex").as_slice(),
            &["first-token".to_string(), "second-token".to_string()]
        );
    }
}
