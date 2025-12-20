use crate::config::constants::{env_vars, models, urls};
use crate::config::core::{AnthropicConfig, PromptCachingConfig};
use crate::config::TimeoutsConfig;
use crate::llm::error_display;
use crate::llm::provider::{LLMError, LLMProvider, LLMRequest, LLMResponse, LLMStream};
use async_trait::async_trait;

use super::common::override_base_url;
use super::openai::OpenAIProvider;

/// Hugging Face Inference Providers (OpenAI-compatible router)
///
/// This provider reuses the OpenAI client but points to the Hugging Face router
/// (`https://router.huggingface.co/v1`) and validates against the HF model list.
pub struct HuggingFaceProvider {
	inner: OpenAIProvider,
}

impl HuggingFaceProvider {
	#[allow(clippy::too_many_arguments)]
	pub fn from_config(
		api_key: Option<String>,
		model: Option<String>,
		base_url: Option<String>,
		prompt_cache: Option<PromptCachingConfig>,
		timeouts: Option<TimeoutsConfig>,
		anthropic: Option<AnthropicConfig>,
	) -> Self {
		let resolved_base_url = override_base_url(
			urls::HUGGINGFACE_API_BASE,
			base_url,
			Some(env_vars::HUGGINGFACE_BASE_URL),
		);

		let inner = OpenAIProvider::from_config(
			api_key,
			model,
			Some(resolved_base_url),
			prompt_cache,
			timeouts,
			anthropic,
		);

		Self { inner }
	}

	fn validate_model(&self, model: &str) -> Result<(), LLMError> {
			if model.trim().is_empty() {
				let formatted_error =
					error_display::format_llm_error("HuggingFace", "Model identifier cannot be empty");
				return Err(LLMError::InvalidRequest {
					message: formatted_error,
					metadata: None,
				});
			}

			// Allow any HF router model ID; the curated list is used only for display and capability hints.
		Ok(())
	}
}

#[async_trait]
impl LLMProvider for HuggingFaceProvider {
	fn name(&self) -> &str {
		"huggingface"
	}

	fn supports_streaming(&self) -> bool {
		self.inner.supports_streaming()
	}

	fn supports_reasoning(&self, model: &str) -> bool {
		models::huggingface::REASONING_MODELS.contains(&model)
	}

	fn supports_reasoning_effort(&self, _model: &str) -> bool {
		models::huggingface::REASONING_MODELS.contains(&_model)
	}

	fn supports_tools(&self, model: &str) -> bool {
		self.inner.supports_tools(model)
	}

	fn supports_parallel_tool_config(&self, model: &str) -> bool {
		self.inner.supports_parallel_tool_config(model)
	}

	fn supports_structured_output(&self, model: &str) -> bool {
		self.inner.supports_structured_output(model)
	}

	fn supports_context_caching(&self, model: &str) -> bool {
		self.inner.supports_context_caching(model)
	}

	fn effective_context_size(&self, model: &str) -> usize {
		self.inner.effective_context_size(model)
	}

	async fn generate(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
		self.validate_model(&request.model)?;
		self.inner.generate(request).await
	}

	async fn stream(&self, request: LLMRequest) -> Result<LLMStream, LLMError> {
		self.validate_model(&request.model)?;
		self.inner.stream(request).await
	}

	fn supported_models(&self) -> Vec<String> {
		models::huggingface::SUPPORTED_MODELS
			.iter()
			.map(|s| s.to_string())
			.collect()
	}

	fn validate_request(&self, request: &LLMRequest) -> Result<(), LLMError> {
		if request.messages.is_empty() {
			let formatted_error =
				error_display::format_llm_error("HuggingFace", "Messages cannot be empty");
			return Err(LLMError::InvalidRequest {
				message: formatted_error,
				metadata: None,
			});
		}

		self.validate_model(&request.model)
	}
}
