//! Main Anthropic Claude provider implementation
//!
//! This is the primary interface for the Anthropic provider, implementing
//! the LLMProvider and LLMClient traits. It delegates to submodules for:
//! - Request building (request_builder)
//! - Response parsing (response_parser)
//! - Stream decoding (stream_decoder)
//! - Capability detection (capabilities)
//! - Validation (validation)
//! - Header management (headers)

use crate::client::LLMClient;
use crate::provider::{LLMError, LLMProvider, LLMRequest, LLMResponse, LLMStream, LLMStreamEvent, ToolDefinition};
use vtcode_config::TimeoutsConfig;
use vtcode_config::constants::{env_vars, models, urls};
use vtcode_config::core::{AnthropicConfig, AnthropicPromptCacheSettings, ModelConfig, PromptCachingConfig};

use super::capabilities;
use super::fallback_retry::{self, RefusalRetryPlan};
use super::headers;
use super::request_builder::{self, RequestBuilderContext};
use super::response_parser;
use super::stream_decoder;
use super::validation;

use crate::providers::common::{extract_prompt_cache_settings, override_base_url, resolve_model};
use crate::providers::error_handling::{format_network_error, format_parse_error, handle_anthropic_http_error};
use crate::providers::openai::CustomProviderAuthHandle;

use async_trait::async_trait;
use futures::StreamExt;
use reqwest::Client as HttpClient;
use reqwest::StatusCode;
use serde_json::Value;

const ANTHROPIC_COMPACT_BETA: &str = "compact-2026-01-12";
const ANTHROPIC_CONTEXT_MANAGEMENT_BETA: &str = "context-management-2025-06-27";
const ANTHROPIC_ADVISOR_BETA: &str = "advisor-tool-2026-03-01";

/// Whether `base_url` points at the first-party Claude API, the only endpoint
/// VT Code talks to that accepts server-side refusal fallbacks.
fn is_first_party_anthropic_endpoint(base_url: &str) -> bool {
    url::Url::parse(base_url)
        .ok()
        .and_then(|url| url.host_str().map(|host| host.eq_ignore_ascii_case("api.anthropic.com")))
        .unwrap_or(false)
}

#[derive(Clone)]
pub struct AnthropicProvider {
    api_key: String,
    http_client: HttpClient,
    base_url: String,
    model: String,
    prompt_cache_enabled: bool,
    prompt_cache_settings: AnthropicPromptCacheSettings,
    anthropic_config: AnthropicConfig,
    custom_provider_auth: Option<CustomProviderAuthHandle>,
    model_behavior: Option<ModelConfig>,
}

impl AnthropicProvider {
    pub fn new(api_key: String) -> Self {
        Self::with_model_internal(
            api_key,
            models::anthropic::DEFAULT_MODEL.to_string(),
            None,
            None,
            AnthropicConfig::default(),
            TimeoutsConfig::default(),
            None,
        )
    }

    fn with_model(api_key: String, model: String) -> Self {
        Self::with_model_internal(
            api_key,
            model,
            None,
            None,
            AnthropicConfig::default(),
            TimeoutsConfig::default(),
            None,
        )
    }

    pub(crate) fn new_with_client(
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
            model,
            prompt_cache_enabled: false,
            prompt_cache_settings: AnthropicPromptCacheSettings::default(),
            anthropic_config: AnthropicConfig::default(),
            custom_provider_auth: None,
            model_behavior: None,
        }
    }

    pub fn from_config(
        api_key: Option<String>,
        model: Option<String>,
        base_url: Option<String>,
        prompt_cache: Option<PromptCachingConfig>,
        timeouts: Option<TimeoutsConfig>,
        anthropic_config: Option<AnthropicConfig>,
        model_behavior: Option<ModelConfig>,
    ) -> Self {
        let api_key_value = api_key.unwrap_or_default();
        let model_value = resolve_model(model, models::anthropic::DEFAULT_MODEL);
        let anthropic_cfg = anthropic_config.unwrap_or_default();

        Self::with_model_internal(
            api_key_value,
            model_value,
            prompt_cache,
            base_url,
            anthropic_cfg,
            timeouts.unwrap_or_default(),
            model_behavior,
        )
    }

    fn with_model_internal(
        api_key: String,
        model: String,
        prompt_cache: Option<PromptCachingConfig>,
        base_url: Option<String>,
        anthropic_config: AnthropicConfig,
        timeouts: TimeoutsConfig,
        model_behavior: Option<ModelConfig>,
    ) -> Self {
        use crate::http_client::HttpClientFactory;

        let (prompt_cache_enabled, prompt_cache_settings) = extract_prompt_cache_settings(
            prompt_cache,
            |providers| &providers.anthropic,
            |cfg, provider_settings| cfg.enabled && provider_settings.enabled,
        );

        let base_url_value = if models::minimax::SUPPORTED_MODELS.contains(&model.as_str()) {
            crate::providers::minimax::resolve_minimax_base_url(base_url)
        } else {
            override_base_url(urls::ANTHROPIC_API_BASE, base_url, Some(env_vars::ANTHROPIC_BASE_URL))
        };

        Self {
            api_key,
            http_client: HttpClientFactory::for_llm(&timeouts),
            base_url: base_url_value,
            model,
            prompt_cache_enabled,
            prompt_cache_settings,
            anthropic_config,
            custom_provider_auth: None,
            model_behavior,
        }
    }

    pub(crate) fn with_custom_auth(mut self, custom_provider_auth: Option<CustomProviderAuthHandle>) -> Self {
        self.custom_provider_auth = custom_provider_auth;
        self
    }

    fn requires_advanced_tool_use_beta(&self, request: &LLMRequest) -> bool {
        request.tools.as_ref().is_some_and(|tools| {
            tools.iter().any(|tool| {
                (tool.is_tool_search() || tool.defer_loading.unwrap_or(false))
                    || tool.allowed_callers.as_ref().is_some_and(|callers| !callers.is_empty())
                    || tool.input_examples.as_ref().is_some_and(|examples| !examples.is_empty())
            })
        })
    }

    fn code_execution_betas(&self, request: &LLMRequest) -> Vec<String> {
        request
            .tools
            .as_ref()
            .map(|tools| {
                tools
                    .iter()
                    .filter_map(|tool| {
                        tool.is_anthropic_code_execution()
                            .then(|| code_execution_beta_name(&tool.tool_type))
                            .flatten()
                    })
                    .fold(Vec::new(), |mut betas, beta| {
                        if !betas.contains(&beta) {
                            betas.push(beta);
                        }
                        betas
                    })
            })
            .unwrap_or_default()
    }

    fn context_management_betas(&self, request: &LLMRequest) -> Vec<&'static str> {
        let mut betas = Vec::new();

        if request
            .tools
            .as_ref()
            .is_some_and(|tools| tools.iter().any(ToolDefinition::is_anthropic_memory_tool))
        {
            betas.push(ANTHROPIC_CONTEXT_MANAGEMENT_BETA);
        }

        if let Some(context_management) = request.context_management.as_ref() {
            if uses_anthropic_compaction(context_management) {
                betas.push(ANTHROPIC_COMPACT_BETA);
            }

            if uses_anthropic_context_edits(context_management) && !betas.contains(&ANTHROPIC_CONTEXT_MANAGEMENT_BETA) {
                betas.push(ANTHROPIC_CONTEXT_MANAGEMENT_BETA);
            }
        }

        betas
    }

    /// Whether the advisor server-side tool should be sent for this request.
    ///
    /// Delegates to the request builder's `resolve_advisor_tool` so the beta
    /// header and the injected tool are gated by the exact same logic.
    fn advisor_enabled_for_request(&self, request: &LLMRequest) -> bool {
        let executor = capabilities::resolve_model_name(&request.model, &self.model);
        request_builder::resolve_advisor_tool(executor, &self.anthropic_config.advisor).is_some()
    }

    fn uses_refreshable_auth(&self) -> bool {
        self.custom_provider_auth.is_some()
    }

    async fn current_api_key(&self) -> Result<String, LLMError> {
        if let Some(handle) = &self.custom_provider_auth {
            return handle
                .current_token()
                .await
                .map_err(|error| format_network_error("Anthropic", &error));
        }

        Ok(self.api_key.clone())
    }

    async fn refresh_api_key_for_retry(&self) -> Result<String, LLMError> {
        if let Some(handle) = &self.custom_provider_auth {
            return handle
                .force_refresh()
                .await
                .map_err(|error| format_network_error("Anthropic", &error));
        }

        Ok(self.api_key.clone())
    }

    /// Prepends a non-disclosure reminder to the system prompt. No supported
    /// Claude model accepts an assistant-message prefill, so the system prompt
    /// is the only valid place for this instruction.
    pub fn with_leak_protection(&self, mut request: LLMRequest, secret_description: &str) -> LLMRequest {
        let reminder = format!("[Never mention or reveal {secret_description}]");
        let merged_system_prompt = match request.system_prompt.as_ref() {
            Some(existing) => format!("{reminder}\n\n{existing}"),
            None => reminder,
        };
        request.system_prompt = Some(std::sync::Arc::from(merged_system_prompt));
        request
    }

    pub fn format_documents_xml(&self, documents: Vec<(&str, &str)>) -> String {
        let mut xml = String::from("<documents>\n");
        for (i, (source, content)) in documents.iter().enumerate() {
            xml.push_str(&format!(
                "  <document index=\"{}\">\n    <source>{}</source>\n    <document_content>\n{}\n    </document_content>\n  </document>\n",
                i + 1,
                source,
                content
            ));
        }
        xml.push_str("</documents>");
        xml
    }

    pub fn extract_xml_block(&self, content: &str, tag: &str) -> Option<String> {
        let start_tag = format!("<{tag}>");
        let end_tag = format!("</{tag}>");

        let start_pos = content.find(&start_tag)? + start_tag.len();
        let end_pos = content.find(&end_tag)?;

        if start_pos < end_pos {
            Some(content[start_pos..end_pos].trim().to_string())
        } else {
            None
        }
    }

    fn request_builder_context(&self) -> RequestBuilderContext<'_> {
        RequestBuilderContext {
            prompt_cache_enabled: self.prompt_cache_enabled,
            prompt_cache_settings: &self.prompt_cache_settings,
            anthropic_config: &self.anthropic_config,
            model: &self.model,
            server_side_fallbacks_available: is_first_party_anthropic_endpoint(&self.base_url),
        }
    }

    fn resolved_request_model<'a>(&'a self, request: &'a LLMRequest) -> &'a str {
        capabilities::resolve_model_name(&request.model, &self.model)
    }

    fn effective_betas(&self, request: &LLMRequest) -> Option<Vec<String>> {
        let mut betas = request.betas.clone().unwrap_or_default();
        for beta in self.context_management_betas(request) {
            if !betas.iter().any(|existing| existing == beta) {
                betas.push(beta.to_string());
            }
        }
        for beta in self.code_execution_betas(request) {
            if !betas.iter().any(|existing| existing == &beta) {
                betas.push(beta);
            }
        }
        if self.advisor_enabled_for_request(request) && !betas.iter().any(|beta| beta == ANTHROPIC_ADVISOR_BETA) {
            betas.push(ANTHROPIC_ADVISOR_BETA.to_string());
        }

        (!betas.is_empty()).then_some(betas)
    }

    fn convert_to_anthropic_format(&self, request: &LLMRequest) -> Result<Value, LLMError> {
        request_builder::convert_to_anthropic_format(request, &self.request_builder_context())
    }

    fn beta_header_for_request(
        &self,
        request: &LLMRequest,
        anthropic_request: &Value,
        include_advanced_tool_use: bool,
        request_betas: Option<&[String]>,
    ) -> Option<String> {
        let server_side_fallback = headers::ServerSideFallbackForm::of_request(anthropic_request);
        let beta_config = headers::BetaHeaderConfig {
            config: &self.anthropic_config,
            model: self.resolved_request_model(request),
            include_advanced_tool_use,
            include_manual_interleaved_beta: anthropic_request
                .get("thinking")
                .and_then(|value| value.get("type"))
                .and_then(Value::as_str)
                == Some("enabled"),
            request_betas,
            include_task_budget: anthropic_request
                .get("output_config")
                .and_then(|value| value.get("task_budget"))
                .is_some(),
            server_side_fallback,
            // The credit beta must accompany the original request for a
            // refusal to carry `fallback_credit_token`. The default-form
            // fallback beta already grants those fields; the list form does
            // not, so it needs the credit beta alongside it.
            include_fallback_credit: request.fallback_credit_token.is_some()
                || server_side_fallback == Some(headers::ServerSideFallbackForm::List),
            include_mid_conversation_tool_changes: false,
            include_mid_conversation_system_clear_at: capabilities::supports_turn_scoped_system_messages(
                self.resolved_request_model(request),
                &self.model,
            ) && anthropic_request
                .get("messages")
                .and_then(Value::as_array)
                .is_some_and(|messages| {
                    messages
                        .iter()
                        .any(|message| message.get("clear_at").and_then(Value::as_str) == Some("next_user_message"))
                }),
            // The primary config or any server-side fallback entry may carry
            // `display: "updates"`; either needs the beta.
            include_thinking_display_updates: std::iter::once(anthropic_request.get("thinking"))
                .chain(
                    anthropic_request
                        .get("fallbacks")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .map(|fallback| fallback.get("thinking")),
                )
                .flatten()
                .any(|thinking| thinking.get("display").and_then(Value::as_str) == Some("updates")),
            // A budget-continuation profile promotes the messages breakpoint to
            // the profile TTL, so the beta decision must see the same profile
            // the request builder used.
            prompt_cache_profile: request.prompt_cache_profile,
        };

        headers::combined_beta_header_value(self.prompt_cache_enabled, &self.prompt_cache_settings, &beta_config)
    }

    async fn send_request(
        &self,
        request: &LLMRequest,
        anthropic_request: &Value,
    ) -> Result<AnthropicHttpResponse, LLMError> {
        let include_advanced_tool_use = self.requires_advanced_tool_use_beta(request);
        let betas = self.effective_betas(request);
        let url = format!("{}/messages", self.base_url);

        let beta_header =
            self.beta_header_for_request(request, anthropic_request, include_advanced_tool_use, betas.as_deref());
        let metadata = request.metadata.clone();

        let send_once = |api_key: String| {
            let mut request_builder = self
                .http_client
                .post(&url)
                .header("x-api-key", api_key)
                .header("anthropic-version", urls::ANTHROPIC_API_VERSION);

            if let Some(beta_header) = beta_header.clone() {
                request_builder = request_builder.header("anthropic-beta", beta_header);
            }

            if let Some(metadata) = metadata.as_ref()
                && let Ok(metadata_str) = serde_json::to_string(metadata)
            {
                request_builder = request_builder.header("X-Turn-Metadata", metadata_str);
            }

            request_builder.json(anthropic_request)
        };

        let response = send_once(self.current_api_key().await?)
            .send()
            .await
            .map_err(|e| format_network_error("Anthropic", &e))?;

        let response = if self.uses_refreshable_auth()
            && matches!(response.status(), StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN)
        {
            send_once(self.refresh_api_key_for_retry().await?)
                .send()
                .await
                .map_err(|e| format_network_error("Anthropic", &e))?
        } else {
            response
        };

        let response = handle_anthropic_http_error(response).await?;

        let request_id = response
            .headers()
            .get("request-id")
            .and_then(|h| h.to_str().ok().map(|s| s.to_string()));
        let organization_id = response
            .headers()
            .get("anthropic-organization-id")
            .and_then(|h| h.to_str().ok().map(|s| s.to_string()));

        Ok(AnthropicHttpResponse { response, request_id, organization_id })
    }
}

fn code_execution_beta_name(tool_type: &str) -> Option<String> {
    let suffix = tool_type.strip_prefix("code_execution_")?;
    if suffix.len() != 8 || !suffix.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }

    Some(format!("code-execution-{}-{}-{}", &suffix[0..4], &suffix[4..6], &suffix[6..8]))
}

fn uses_anthropic_compaction(context_management: &Value) -> bool {
    context_management
        .as_array()
        .is_some_and(|items| items.iter().any(is_compaction_item))
        || context_management
            .get("edits")
            .and_then(Value::as_array)
            .is_some_and(|edits| edits.iter().any(is_compaction_edit_item))
}

fn is_compaction_item(item: &Value) -> bool {
    item.get("type").and_then(Value::as_str) == Some("compaction")
}

fn is_compaction_edit_item(item: &Value) -> bool {
    item.get("type")
        .and_then(Value::as_str)
        .is_some_and(|edit_type| edit_type.starts_with("compact_"))
}

fn uses_anthropic_context_edits(context_management: &Value) -> bool {
    context_management
        .get("edits")
        .and_then(Value::as_array)
        .is_some_and(|edits| edits.iter().any(is_context_edit_item))
}

fn is_context_edit_item(item: &Value) -> bool {
    item.get("type")
        .and_then(Value::as_str)
        .is_some_and(|edit_type| edit_type.starts_with("clear_tool_uses_") || edit_type.starts_with("clear_thinking_"))
}

struct AnthropicHttpResponse {
    response: reqwest::Response,
    request_id: Option<String>,
    organization_id: Option<String>,
}

impl AnthropicProvider {
    async fn generate_with_body(
        &self,
        request: &LLMRequest,
        anthropic_request: &Value,
        model: String,
    ) -> Result<LLMResponse, LLMError> {
        let AnthropicHttpResponse { response, request_id, organization_id } =
            self.send_request(request, anthropic_request).await?;

        let anthropic_response: Value = response.json().await.map_err(|e| format_parse_error("Anthropic", &e))?;

        let mut llm_response = response_parser::parse_response(anthropic_response, model)?;
        llm_response.request_id = request_id;
        llm_response.organization_id = organization_id;
        Ok(llm_response)
    }

    /// Sends the one bounded refusal retry described in `fallback_retry`. A
    /// 400 naming the credit token is resent once without it; any other
    /// failure is logged and reported as `None` so the caller keeps the
    /// original refusal.
    async fn send_refusal_retry(
        &self,
        request: &LLMRequest,
        original_body: &Value,
        plan: &RefusalRetryPlan,
    ) -> Option<AnthropicHttpResponse> {
        let fallback_retry::RefusalRetryBody { mut body, with_token } = plan.retry_body(original_body, &self.model);
        tracing::info!(
            provider = "anthropic",
            recommended_model = %plan.model,
            with_credit_token = with_token,
            "refusal fallback could not run server-side; retrying once on the recommended model"
        );
        match self.send_request(&plan.retry_request(request, with_token), &body).await {
            Ok(response) => Some(response),
            Err(err) if with_token && fallback_retry::rejects_credit_token(&err) => {
                tracing::warn!(
                    provider = "anthropic",
                    error = %err,
                    "fallback credit token rejected; resending the refusal retry without it"
                );
                fallback_retry::strip_credit_token(&mut body);
                match self.send_request(&plan.retry_request(request, false), &body).await {
                    Ok(response) => Some(response),
                    Err(err) => {
                        tracing::warn!(provider = "anthropic", error = %err, "refusal retry failed");
                        None
                    }
                }
            }
            Err(err) => {
                tracing::warn!(provider = "anthropic", error = %err, "refusal retry failed");
                None
            }
        }
    }

    async fn retry_refused_generate(
        &self,
        request: &LLMRequest,
        original_body: &Value,
        plan: &RefusalRetryPlan,
        refused: LLMResponse,
    ) -> Result<LLMResponse, LLMError> {
        let Some(AnthropicHttpResponse { response, request_id, organization_id }) =
            self.send_refusal_retry(request, original_body, plan).await
        else {
            return Ok(refused);
        };
        let parsed = match response.json::<Value>().await {
            Ok(value) => response_parser::parse_response(value, plan.model.clone()),
            Err(err) => Err(format_parse_error("Anthropic", &err)),
        };
        match parsed {
            Ok(mut retried) => {
                retried.request_id = request_id;
                retried.organization_id = organization_id;
                Ok(retried)
            }
            Err(err) => {
                tracing::warn!(provider = "anthropic", error = %err, "refusal retry response could not be parsed");
                Ok(refused)
            }
        }
    }

    /// Wraps a primary stream so a pre-output refusal eligible for the
    /// bounded retry is replaced by the retry's stream. Every other event,
    /// including a refusal that is not retried, passes through unchanged.
    fn stream_with_refusal_retry(
        &self,
        primary: LLMStream,
        request: LLMRequest,
        original_body: Value,
        model: String,
    ) -> LLMStream {
        let provider = self.clone();
        Box::pin(async_stream::stream! {
            let mut primary = primary;
            while let Some(event) = primary.next().await {
                let response = match event {
                    Ok(LLMStreamEvent::Completed { response }) => response,
                    other => {
                        yield other;
                        continue;
                    }
                };
                let retry = match RefusalRetryPlan::for_response(&response, &model) {
                    Some(plan) => provider
                        .send_refusal_retry(&request, &original_body, &plan)
                        .await
                        .map(|http| (plan, http)),
                    None => None,
                };
                match retry {
                    Some((plan, AnthropicHttpResponse { response: http, request_id, organization_id })) => {
                        let mut retried =
                            stream_decoder::create_stream(http, plan.model, request_id, organization_id);
                        while let Some(event) = retried.next().await {
                            yield event;
                        }
                    }
                    None => yield Ok(LLMStreamEvent::Completed { response }),
                }
            }
        })
    }
}

#[async_trait]
impl LLMProvider for AnthropicProvider {
    fn name(&self) -> &str {
        "anthropic"
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_non_streaming(&self, _model: &str) -> bool {
        // Pinned so the stream-timeout fallback cannot silently regress.
        true
    }

    fn supports_reasoning(&self, model: &str) -> bool {
        // Codex-inspired robustness: Setting model_supports_reasoning to false
        // does NOT disable it for known reasoning models.
        capabilities::supports_reasoning(model, &self.model)
            || self
                .model_behavior
                .as_ref()
                .and_then(|b| b.model_supports_reasoning)
                .unwrap_or(false)
    }

    fn supported_reasoning_efforts(&self, model: &str) -> &'static [&'static str] {
        capabilities::allowed_efforts_for_model(model, &self.model).unwrap_or_else(|| {
            self.model_behavior
                .as_ref()
                .and_then(|behavior| behavior.model_supports_reasoning_effort)
                .filter(|supported| *supported)
                .map(|_| crate::provider::GENERIC_REASONING_EFFORTS)
                .unwrap_or(&[])
        })
    }

    fn supports_reasoning_effort(&self, model: &str) -> bool {
        // Same robustness logic for reasoning effort
        capabilities::supports_reasoning_effort(model, &self.model)
            || self
                .model_behavior
                .as_ref()
                .and_then(|b| b.model_supports_reasoning_effort)
                .unwrap_or(false)
    }

    fn supports_parallel_tool_config(&self, model: &str) -> bool {
        capabilities::supports_parallel_tool_config(model)
    }

    fn supports_context_edits(&self, _model: &str) -> bool {
        true
    }

    fn supports_turn_scoped_system_messages(&self, model: &str) -> bool {
        capabilities::supports_turn_scoped_system_messages(model, &self.model)
    }

    fn supports_responses_compaction(&self, model: &str) -> bool {
        // Anthropic server-side compaction is supported on Claude Opus 4.x
        // and Sonnet 4.6+ models via context_management.edits.
        capabilities::supports_compaction(model)
    }

    fn supports_native_inline_compaction(&self, model: &str) -> bool {
        // Anthropic drives compaction inline via the `compact_20260112`
        // context-management edit on a `generate` request, so it is the
        // `NativeInline` strategy provider.
        capabilities::supports_compaction(model)
    }

    fn effective_context_size(&self, model: &str) -> usize {
        capabilities::effective_context_size(model)
    }

    fn supports_structured_output(&self, model: &str) -> bool {
        capabilities::supports_structured_output(model, &self.model)
    }

    fn supports_vision(&self, model: &str) -> bool {
        capabilities::supports_vision(model, &self.model)
    }

    async fn generate(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
        let resolved_model = self.resolved_request_model(&request).to_string();
        let anthropic_request = self.convert_to_anthropic_format(&request)?;

        let response = self
            .generate_with_body(&request, &anthropic_request, resolved_model.clone())
            .await?;
        match RefusalRetryPlan::for_response(&response, &resolved_model) {
            Some(plan) => self.retry_refused_generate(&request, &anthropic_request, &plan, response).await,
            None => Ok(response),
        }
    }

    async fn stream(&self, request: LLMRequest) -> Result<LLMStream, LLMError> {
        let resolved_model = self.resolved_request_model(&request).to_string();
        let mut anthropic_request = self.convert_to_anthropic_format(&request)?;

        if let Some(obj) = anthropic_request.as_object_mut() {
            obj.insert("stream".to_string(), Value::Bool(true));
        }

        let AnthropicHttpResponse { response, request_id, organization_id } =
            self.send_request(&request, &anthropic_request).await?;

        let primary = stream_decoder::create_stream(response, resolved_model.clone(), request_id, organization_id);
        Ok(self.stream_with_refusal_retry(primary, request, anthropic_request, resolved_model))
    }

    fn supported_models(&self) -> Vec<String> {
        capabilities::supported_models()
    }

    fn validate_request(&self, request: &LLMRequest) -> Result<(), LLMError> {
        validation::validate_request(request, &self.model, &self.anthropic_config, "Anthropic")
    }
}

#[async_trait]
impl LLMClient for AnthropicProvider {
    async fn generate(&mut self, prompt: &str) -> Result<LLMResponse, LLMError> {
        let request = crate::providers::common::make_default_request(prompt, &self.model);
        let request_model = request.model.clone();
        let response = LLMProvider::generate(self, request).await?;

        Ok(LLMResponse {
            content: Some(response.content.unwrap_or_default()),
            model: request_model,
            usage: response.usage.map(crate::providers::common::convert_usage_to_llm_types),
            reasoning: response.reasoning,
            reasoning_details: response.reasoning_details,
            request_id: response.request_id,
            organization_id: response.organization_id,
            finish_reason: response.finish_reason,
            tool_calls: response.tool_calls,
            tool_references: response.tool_references,
            compaction: response.compaction,
        })
    }

    fn model_id(&self) -> &str {
        &self.model
    }
}

#[cfg(test)]
mod tests;
