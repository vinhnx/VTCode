use super::OpenAIProvider;
use crate::config::constants::models;
use crate::llm::client::LLMClient;
use crate::llm::error_display;
use crate::llm::provider;
use crate::llm::providers::common::{
    execute_token_count_request, parse_prompt_tokens_from_count_response,
    strip_generation_controls_for_token_count,
};
use crate::llm::types as llm_types;
use async_trait::async_trait;

#[async_trait]
impl provider::LLMProvider for OpenAIProvider {
    fn name(&self) -> &str {
        "openai"
    }

    fn supports_streaming(&self) -> bool {
        // OpenAI requires ID verification for GPT-5 models, so we must disable streaming
        if matches!(
            self.model.as_ref(),
            models::openai::GPT_5 | models::openai::GPT_5_MINI | models::openai::GPT_5_NANO
        ) {
            return false;
        }

        // Even if Responses API is disabled (e.g., Hugging Face router), we can stream via Chat Completions.
        true
    }

    fn supports_reasoning(&self, model: &str) -> bool {
        let requested = if model.trim().is_empty() {
            self.model.as_ref()
        } else {
            model
        };

        // Codex-inspired robustness: Setting model_supports_reasoning to false
        // does NOT disable it for known reasoning models.
        models::openai::REASONING_MODELS.contains(&requested)
            || self
                .model_behavior
                .as_ref()
                .and_then(|b| b.model_supports_reasoning)
                .unwrap_or(false)
    }

    fn supports_reasoning_effort(&self, model: &str) -> bool {
        let requested = if model.trim().is_empty() {
            self.model.as_ref()
        } else {
            model
        };

        // Same robustness logic for reasoning effort
        models::openai::REASONING_MODELS
            .iter()
            .any(|candidate| *candidate == requested)
            || self
                .model_behavior
                .as_ref()
                .and_then(|b| b.model_supports_reasoning_effort)
                .unwrap_or(false)
    }

    fn supports_tools(&self, model: &str) -> bool {
        let requested = if model.trim().is_empty() {
            self.model.as_ref()
        } else {
            model
        };

        !models::openai::TOOL_UNAVAILABLE_MODELS.contains(&requested)
    }

    async fn stream(
        &self,
        request: provider::LLMRequest,
    ) -> Result<provider::LLMStream, provider::LLMError> {
        self.stream_request(request).await
    }

    async fn generate(
        &self,
        request: provider::LLMRequest,
    ) -> Result<provider::LLMResponse, provider::LLMError> {
        self.generate_request(request).await
    }

    async fn count_prompt_tokens_exact(
        &self,
        request: &provider::LLMRequest,
    ) -> Result<Option<u32>, provider::LLMError> {
        if !self.base_url.contains("api.openai.com") {
            return Ok(None);
        }

        let mut payload = self.convert_to_openai_responses_format(request)?;
        strip_generation_controls_for_token_count(&mut payload);

        let count_url = format!(
            "{}/responses/input_tokens",
            self.base_url.trim_end_matches('/')
        );
        let value = execute_token_count_request(
            self.authorize(
                self.http_client
                    .post(&count_url)
                    .header("Content-Type", "application/json"),
            ),
            &payload,
            "OpenAI",
        )
        .await?;

        let Some(value) = value else {
            return Ok(None);
        };

        Ok(parse_prompt_tokens_from_count_response(&value))
    }

    fn supported_models(&self) -> Vec<String> {
        models::openai::SUPPORTED_MODELS
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    fn validate_request(&self, request: &provider::LLMRequest) -> Result<(), provider::LLMError> {
        let supported_models = self.supported_models();

        super::super::super::common::validate_request_common(
            request,
            "OpenAI",
            "openai",
            Some(&supported_models),
        )
    }
}

#[async_trait]
impl LLMClient for OpenAIProvider {
    async fn generate(
        &mut self,
        prompt: &str,
    ) -> Result<llm_types::LLMResponse, provider::LLMError> {
        let request = super::super::super::common::make_default_request(prompt, &self.model);
        let request_model = request.model.to_string();
        let response = provider::LLMProvider::generate(self, request).await?;

        Ok(llm_types::LLMResponse {
            content: Some(response.content.unwrap_or_default()),
            model: request_model,
            usage: response
                .usage
                .map(super::super::super::common::convert_usage_to_llm_types),
            reasoning: response.reasoning,
            reasoning_details: response.reasoning_details,
            request_id: response.request_id,
            organization_id: response.organization_id,
            finish_reason: response.finish_reason,
            tool_calls: response.tool_calls,
            tool_references: response.tool_references,
        })
    }

    fn backend_kind(&self) -> llm_types::BackendKind {
        llm_types::BackendKind::OpenAI
    }

    fn model_id(&self) -> &str {
        &self.model
    }
}
