use crate::config::TimeoutsConfig;
use crate::config::constants::{env_vars, headers, models, urls};
use crate::config::core::PromptCachingConfig;
use crate::llm::client::LLMClient;
use crate::llm::error_display;
use crate::llm::provider::{
    LLMError, LLMProvider, LLMRequest, LLMResponse, MessageRole, ToolChoice, Usage,
};
use crate::llm::types as llm_types;
use async_trait::async_trait;
use reqwest::header::RETRY_AFTER;
use reqwest::{Client as HttpClient, StatusCode};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use tracing::error;

use super::common::{
    convert_usage_to_llm_types, override_base_url, parse_chat_request_openai_format_with_extractor,
    parse_client_prompt_common, resolve_model, serialize_tools_openai_format,
    validate_request_common,
};

const PROVIDER_NAME: &str = "Z.AI";
const PROVIDER_KEY: &str = "zai";
const CHAT_COMPLETIONS_PATH: &str = "/paas/v4/chat/completions";
const LOG_BODY_MAX_CHARS: usize = 2000;

pub struct ZAIProvider {
    api_key: String,
    http_client: HttpClient,
    base_url: String,
    model: String,
    validated_api_key: AtomicBool,
}

impl ZAIProvider {
    fn with_model_internal(
        api_key: String,
        model: String,
        base_url: Option<String>,
        _prompt_cache: Option<PromptCachingConfig>,
    ) -> Self {
        Self {
            api_key,
            http_client: HttpClient::new(),
            base_url: override_base_url(
                urls::Z_AI_API_BASE,
                base_url,
                Some(env_vars::Z_AI_BASE_URL),
            ),
            model,
            validated_api_key: AtomicBool::new(false),
        }
    }

    pub fn new(api_key: String) -> Self {
        Self::with_model_internal(api_key, models::zai::DEFAULT_MODEL.to_string(), None, None)
    }

    pub fn with_model(api_key: String, model: String) -> Self {
        Self::with_model_internal(api_key, model, None, None)
    }

    pub fn from_config(
        api_key: Option<String>,
        model: Option<String>,
        base_url: Option<String>,
        prompt_cache: Option<PromptCachingConfig>,
        _timeouts: Option<TimeoutsConfig>,
    ) -> Self {
        let api_key_value = api_key.unwrap_or_default();
        let model_value = resolve_model(model, models::zai::DEFAULT_MODEL);
        Self::with_model_internal(api_key_value, model_value, base_url, prompt_cache)
    }

    fn parse_client_prompt(&self, prompt: &str) -> LLMRequest {
        parse_client_prompt_common(prompt, &self.model, |value| self.parse_chat_request(value))
    }

    fn parse_chat_request(&self, value: &Value) -> Option<LLMRequest> {
        let mut request =
            parse_chat_request_openai_format_with_extractor(value, &self.model, |c| match c {
                Value::String(text) => text.to_string(),
                other => other.to_string(),
            })?;

        // ZAI supports tool_choice parsing
        request.tool_choice = value.get("tool_choice").and_then(|choice| match choice {
            Value::String(s) => match s.as_str() {
                "auto" => Some(ToolChoice::auto()),
                "none" => Some(ToolChoice::none()),
                "any" | "required" => Some(ToolChoice::any()),
                _ => None,
            },
            _ => None,
        });

        Some(request)
    }

    fn convert_to_zai_format(&self, request: &LLMRequest) -> Result<Value, LLMError> {
        let mut messages = Vec::new();
        let mut active_tool_call_ids: HashSet<String> = HashSet::new();

        if let Some(system_prompt) = &request.system_prompt {
            messages.push(json!({
                "role": crate::config::constants::message_roles::SYSTEM,
                "content": system_prompt
            }));
        }

        for msg in &request.messages {
            let role = msg.role.as_generic_str();
            let mut message = json!({
                "role": role,
                "content": msg.content
            });
            let mut skip_message = false;

            if msg.role == MessageRole::Assistant
                && let Some(tool_calls) = &msg.tool_calls
                && !tool_calls.is_empty()
            {
                let tool_calls_json: Vec<Value> = tool_calls
                    .iter()
                    .filter_map(|tc| {
                        if let Some(ref func) = tc.function {
                            active_tool_call_ids.insert(tc.id.clone());
                            Some(json!({
                                "id": tc.id,
                                "type": "function",
                                "function": {
                                    "name": func.name,
                                    "arguments": func.arguments,
                                }
                            }))
                        } else {
                            None
                        }
                    })
                    .collect();
                message["tool_calls"] = Value::Array(tool_calls_json);
            }

            if msg.role == MessageRole::Tool {
                match &msg.tool_call_id {
                    Some(tool_call_id) if active_tool_call_ids.contains(tool_call_id) => {
                        message["tool_call_id"] = Value::String(tool_call_id.clone());
                        active_tool_call_ids.remove(tool_call_id);
                    }
                    Some(_) | None => {
                        skip_message = true;
                    }
                }
            }

            if !skip_message {
                messages.push(message);
            }
        }

        if messages.is_empty() {
            let formatted = error_display::format_llm_error(PROVIDER_NAME, "No messages provided");
            return Err(LLMError::InvalidRequest(formatted));
        }

        let mut payload = json!({
            "model": request.model,
            "messages": messages,
            "stream": request.stream,
        });

        if let Some(max_tokens) = request.max_tokens {
            payload["max_tokens"] = json!(max_tokens);
        }

        if let Some(temperature) = request.temperature {
            payload["temperature"] = json!(temperature);
        }

        if let Some(tools) = &request.tools
            && let Some(serialized) = serialize_tools_openai_format(tools)
        {
            payload["tools"] = Value::Array(serialized);
        }

        if let Some(choice) = &request.tool_choice {
            payload["tool_choice"] = choice.to_provider_format("openai");
        }

        if self.supports_reasoning(&request.model) || request.reasoning_effort.is_some() {
            payload["thinking"] = json!({ "type": "enabled" });
        }

        Ok(payload)
    }

    fn parse_zai_response(&self, response_json: Value) -> Result<LLMResponse, LLMError> {
        // Custom reasoning extractor for ZAI's array format
        let reasoning_extractor = |message: &Value, _choice: &Value| {
            message
                .get("reasoning_content")
                .and_then(|value| match value {
                    Value::String(text) => Some(text.to_string()),
                    Value::Array(parts) => {
                        let combined = parts
                            .iter()
                            .filter_map(|part| part.as_str())
                            .collect::<Vec<_>>()
                            .join("");
                        if combined.is_empty() {
                            None
                        } else {
                            Some(combined)
                        }
                    }
                    _ => None,
                })
        };

        // ZAI uses cached_tokens in prompt_tokens_details, not at top level
        // We'll parse usage manually after getting the base response
        let mut response = super::common::parse_response_openai_format(
            response_json.clone(),
            PROVIDER_NAME,
            false, // Don't use standard cache metrics
            Some(reasoning_extractor),
        )?;

        // Override usage with ZAI-specific parsing
        response.usage = response_json.get("usage").map(|usage_value| Usage {
            prompt_tokens: usage_value
                .get("prompt_tokens")
                .and_then(|pt| pt.as_u64())
                .unwrap_or(0) as u32,
            completion_tokens: usage_value
                .get("completion_tokens")
                .and_then(|ct| ct.as_u64())
                .unwrap_or(0) as u32,
            total_tokens: usage_value
                .get("total_tokens")
                .and_then(|tt| tt.as_u64())
                .unwrap_or(0) as u32,
            cached_prompt_tokens: usage_value
                .get("prompt_tokens_details")
                .and_then(|details| details.get("cached_tokens"))
                .and_then(|value| value.as_u64())
                .map(|value| value as u32),
            cache_creation_tokens: None,
            cache_read_tokens: None,
        });

        Ok(response)
    }

    fn ensure_api_key(&self) -> Result<(), LLMError> {
        if self.validated_api_key.load(Ordering::Relaxed) {
            return Ok(());
        }

        if self.api_key.trim().is_empty() {
            let formatted = error_display::format_llm_error(PROVIDER_NAME, "Missing Z.AI API key");
            return Err(LLMError::Authentication(formatted));
        }

        self.validated_api_key.store(true, Ordering::Relaxed);
        Ok(())
    }

    fn format_diagnostic(
        status: StatusCode,
        code: &str,
        message: &str,
        request_id: &str,
        retry_after: Option<&str>,
    ) -> String {
        let code_part = if code.is_empty() { "unknown" } else { code };
        let trimmed = message.trim();
        let retry_part = retry_after
            .filter(|value| !value.is_empty())
            .map(|value| format!(", retry_after={}", value))
            .unwrap_or_default();
        format!(
            "{} [status={}, code={}, request_id={}{}]",
            if trimmed.is_empty() {
                "request failed"
            } else {
                trimmed
            },
            status,
            code_part,
            request_id,
            retry_part
        )
    }

    fn classify_error(
        &self,
        status: StatusCode,
        error_code: &str,
        message: &str,
        request_id: &str,
    ) -> Option<LLMError> {
        let code_num = error_code.parse::<u16>().ok();
        let message_lower = message.to_ascii_lowercase();

        let is_auth = status == StatusCode::UNAUTHORIZED
            || matches!(code_num, Some(1000..=1004))
            || message_lower.contains("authentication")
            || message_lower.contains("unauthorized")
            || message_lower.contains("invalid api key")
            || message_lower.contains("token expired");

        let is_account_issue =
            matches!(code_num, Some(1110..=1121)) || message_lower.contains("account");

        let is_balance_or_quota = status == StatusCode::PAYMENT_REQUIRED
            || matches!(code_num, Some(1113))
            || message_lower.contains("insufficient balance")
            || message_lower.contains("no resource package")
            || message_lower.contains("recharge")
            || message_lower.contains("arrears")
            || message_lower.contains("balance");

        if is_balance_or_quota {
            let diagnostic = Self::format_diagnostic(status, error_code, message, request_id, None);
            let formatted = error_display::format_llm_error(PROVIDER_NAME, &diagnostic);
            return Some(LLMError::Provider(formatted));
        }

        if is_auth || is_account_issue {
            let diagnostic = Self::format_diagnostic(status, error_code, message, request_id, None);
            let formatted = error_display::format_llm_error(PROVIDER_NAME, &diagnostic);
            return Some(LLMError::Authentication(formatted));
        }

        let is_rate_limit = status == StatusCode::TOO_MANY_REQUESTS
            || matches!(error_code, "1302" | "1303" | "1304" | "1308" | "1309")
            || message_lower.contains("rate limit")
            || message_lower.contains("rate_limit")
            || message_lower.contains("ratelimit")
            || message_lower.contains("concurrency")
            || message_lower.contains("frequency")
            || message_lower.contains("quota")
            || message_lower.contains("usage limit")
            || message_lower.contains("too many requests")
            || message_lower.contains("daily call limit")
            || message_lower.contains("package has expired");

        if is_rate_limit {
            return Some(LLMError::RateLimit);
        }

        None
    }

    fn available_models() -> Vec<String> {
        models::zai::SUPPORTED_MODELS
            .iter()
            .map(|s| s.to_string())
            .collect()
    }
}

#[async_trait]
impl LLMProvider for ZAIProvider {
    fn name(&self) -> &str {
        PROVIDER_KEY
    }

    fn supports_streaming(&self) -> bool {
        false
    }

    fn supports_reasoning(&self, model: &str) -> bool {
        matches!(
            model,
            models::zai::GLM_4_6
                | models::zai::GLM_4_5
                | models::zai::GLM_4_5_AIR
                | models::zai::GLM_4_5_X
                | models::zai::GLM_4_5_AIRX
        )
    }

    fn supports_reasoning_effort(&self, _model: &str) -> bool {
        false
    }

    async fn generate(&self, mut request: LLMRequest) -> Result<LLMResponse, LLMError> {
        self.ensure_api_key()?;

        if request.model.trim().is_empty() {
            request.model = self.model.clone();
        }

        if !Self::available_models().contains(&request.model) {
            let formatted = error_display::format_llm_error(
                PROVIDER_NAME,
                &format!("Unsupported model: {}", request.model),
            );
            return Err(LLMError::InvalidRequest(formatted));
        }

        for message in &request.messages {
            if let Err(err) = message.validate_for_provider(PROVIDER_KEY) {
                let formatted = error_display::format_llm_error(PROVIDER_NAME, &err);
                return Err(LLMError::InvalidRequest(formatted));
            }
        }

        let payload = self.convert_to_zai_format(&request)?;
        let url = format!("{}{}", self.base_url, CHAT_COMPLETIONS_PATH);

        let response = self
            .http_client
            .post(&url)
            .bearer_auth(&self.api_key)
            .header(headers::ACCEPT_LANGUAGE, headers::ACCEPT_LANGUAGE_DEFAULT)
            .json(&payload)
            .send()
            .await
            .map_err(|err| {
                let formatted = error_display::format_llm_error(
                    PROVIDER_NAME,
                    &format!("Network error: {}", err),
                );
                LLMError::Network(formatted)
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let request_id = response
                .headers()
                .get("x-request-id")
                .and_then(|value| value.to_str().ok())
                .unwrap_or("<none>")
                .to_string();
            let retry_after = response
                .headers()
                .get(RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .map(|value| value.to_string());
            let text = response.text().await.unwrap_or_default();

            // Try to parse the error JSON structure first
            // Z.AI error format: {"error": {"code": "1302", "message": "..."}}
            let (error_code, message) = serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|value| {
                    let error_obj = value.get("error")?;
                    let code = error_obj.get("code").and_then(|c| c.as_str()).unwrap_or("");
                    let msg = error_obj
                        .get("message")
                        .and_then(|m| m.as_str())
                        .map(|s| s.to_string())
                        .or_else(|| {
                            value
                                .get("message")
                                .and_then(|m| m.as_str().map(|s| s.to_string()))
                        })
                        .unwrap_or_else(|| text.clone());
                    Some((code.to_string(), msg))
                })
                .unwrap_or_else(|| (String::new(), text.clone()));

            let diagnostic = Self::format_diagnostic(
                status,
                &error_code,
                &message,
                &request_id,
                retry_after.as_deref(),
            );
            let trimmed_body: String = text.chars().take(LOG_BODY_MAX_CHARS).collect();
            error!(
                target = "vtcode::llm::zai",
                status = %status,
                code = %error_code,
                request_id = %request_id,
                message = %message,
                body = %trimmed_body,
                retry_after = retry_after.as_deref().unwrap_or("<none>"),
                "Z.AI request failed"
            );

            if let Some(mapped) = self.classify_error(status, &error_code, &message, &request_id) {
                return Err(mapped);
            }

            let formatted = error_display::format_llm_error(PROVIDER_NAME, &diagnostic);
            return Err(LLMError::Provider(formatted));
        }

        let json: Value = response.json().await.map_err(|err| {
            let formatted = error_display::format_llm_error(
                PROVIDER_NAME,
                &format!("Failed to parse response: {}", err),
            );
            LLMError::Provider(formatted)
        })?;

        self.parse_zai_response(json)
    }

    fn supported_models(&self) -> Vec<String> {
        Self::available_models()
    }

    fn validate_request(&self, request: &LLMRequest) -> Result<(), LLMError> {
        validate_request_common(
            request,
            PROVIDER_NAME,
            PROVIDER_KEY,
            Some(&Self::available_models()),
        )
    }
}

#[async_trait]
impl LLMClient for ZAIProvider {
    async fn generate(&mut self, prompt: &str) -> Result<llm_types::LLMResponse, LLMError> {
        let request = self.parse_client_prompt(prompt);
        let request_model = request.model.clone();
        let response = LLMProvider::generate(self, request).await?;

        Ok(llm_types::LLMResponse {
            content: response.content.unwrap_or_default(),
            model: request_model,
            usage: response.usage.map(convert_usage_to_llm_types),
            reasoning: response.reasoning,
        })
    }

    fn backend_kind(&self) -> llm_types::BackendKind {
        llm_types::BackendKind::ZAI
    }

    fn model_id(&self) -> &str {
        &self.model
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limit_error_patterns() {
        // Test various rate limit error message patterns
        let test_cases = vec![
            ("rate limit exceed", true),
            ("Rate Limit Exceeded", true),
            ("RATE_LIMIT", true),
            ("ratelimit error", true),
            ("High concurrency usage", true),
            ("High frequency usage", true),
            ("concurrency exceeded", true),
            ("quota exceeded", true),
            ("usage limit reached", true),
            ("too many requests", true),
            ("daily call limit reached", true),
            ("GLM Coding Plan package has expired", true),
            ("Insufficient balance or no resource package", false),
            ("balance exhausted", false),
            ("invalid api key", false),
            ("authentication failed", false),
            ("internal server error", false),
        ];

        for (message, should_match) in test_cases {
            let text_lower = message.to_lowercase();
            let is_rate_limit = text_lower.contains("rate limit")
                || text_lower.contains("rate_limit")
                || text_lower.contains("ratelimit")
                || text_lower.contains("concurrency")
                || text_lower.contains("frequency")
                || text_lower.contains("quota")
                || text_lower.contains("usage limit")
                || text_lower.contains("too many requests")
                || text_lower.contains("daily call limit")
                || text_lower.contains("package has expired");

            assert_eq!(
                is_rate_limit,
                should_match,
                "Pattern '{}' should {} rate limit",
                message,
                if should_match { "match" } else { "not match" }
            );
        }
    }

    #[test]
    fn test_classify_error_auth_and_quota() {
        let provider = ZAIProvider::with_model("key".to_string(), models::zai::GLM_4_6.to_string());

        let auth_error = provider.classify_error(
            StatusCode::UNAUTHORIZED,
            "1000",
            "Authentication Failed",
            "<req>",
        );
        assert!(matches!(auth_error, Some(LLMError::Authentication(_))));

        let quota_error = provider.classify_error(
            StatusCode::TOO_MANY_REQUESTS,
            "1113",
            "Insufficient balance or no resource package. Please recharge.",
            "<req>",
        );
        assert!(matches!(quota_error, Some(LLMError::Provider(_))));

        let rate_limit_error = provider.classify_error(
            StatusCode::TOO_MANY_REQUESTS,
            "1302",
            "High concurrency usage",
            "<req>",
        );
        assert!(matches!(rate_limit_error, Some(LLMError::RateLimit)));
    }

    #[test]
    fn test_format_diagnostic_includes_retry_after() {
        let message = ZAIProvider::format_diagnostic(
            StatusCode::TOO_MANY_REQUESTS,
            "1302",
            "rate limited",
            "req-1",
            Some("5"),
        );
        assert!(
            message.contains("retry_after=5"),
            "retry_after not included: {}",
            message
        );
    }

    #[test]
    fn test_rate_limit_error_codes() {
        // Test Z.AI specific error codes for rate limiting
        let rate_limit_codes = vec!["1302", "1303", "1304", "1308", "1309"];
        let non_rate_limit_codes = vec!["1000", "1001", "1002", "1210", "1214"];

        for code in rate_limit_codes {
            assert!(
                matches!(code, "1302" | "1303" | "1304" | "1308" | "1309"),
                "Code {} should be recognized as rate limit error",
                code
            );
        }

        for code in non_rate_limit_codes {
            assert!(
                !matches!(code, "1302" | "1303" | "1304" | "1308" | "1309"),
                "Code {} should NOT be recognized as rate limit error",
                code
            );
        }
    }

    #[test]
    fn test_error_json_parsing() {
        // Test Z.AI error JSON structure parsing
        let error_json =
            r#"{"error":{"code":"1302","message":"High concurrency usage of this API"}}"#;
        let value: Value = serde_json::from_str(error_json).unwrap();

        let error_obj = value.get("error").unwrap();
        let code = error_obj.get("code").and_then(|c| c.as_str()).unwrap();
        let message = error_obj.get("message").and_then(|m| m.as_str()).unwrap();

        assert_eq!(code, "1302");
        assert_eq!(message, "High concurrency usage of this API");
        assert!(matches!(code, "1302" | "1303" | "1304" | "1308" | "1309"));
    }
}
