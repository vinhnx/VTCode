use async_stream::try_stream;
use async_trait::async_trait;
use once_cell::sync::Lazy;
use rustc_hash::FxHashMap;
use std::sync::RwLock;

use super::{LLMNormalizedStream, LLMRequest, LLMResponse, LLMStream, LLMStreamEvent, Message};
pub use vtcode_commons::llm::{LLMError, LLMErrorMetadata};

/// Cached provider capabilities to reduce repeated trait method calls
#[derive(Debug, Clone)]
pub struct ProviderCapabilities {
    pub provider_name: String,
    pub model: String,
    pub streaming: bool,
    pub reasoning: bool,
    pub reasoning_effort: bool,
    pub tools: bool,
    pub parallel_tool_config: bool,
    pub structured_output: bool,
    pub context_caching: bool,
    pub responses_compaction: bool,
    pub context_awareness: bool,
    pub vision: bool,
    pub context_size: usize,
}

impl ProviderCapabilities {
    pub fn detect(provider: &dyn LLMProvider, model: &str) -> Self {
        Self {
            provider_name: provider.name().to_string(),
            model: model.to_string(),
            streaming: provider.supports_streaming(),
            reasoning: provider.supports_reasoning(model),
            reasoning_effort: provider.supports_reasoning_effort(model),
            tools: provider.supports_tools(model),
            parallel_tool_config: provider.supports_parallel_tool_config(model),
            structured_output: provider.supports_structured_output(model),
            context_caching: provider.supports_context_caching(model),
            responses_compaction: provider.supports_responses_compaction(model),
            context_awareness: provider.supports_context_awareness(model),
            vision: provider.supports_vision(model),
            context_size: provider.effective_context_size(model),
        }
    }

    pub fn has_advanced_features(&self) -> bool {
        self.reasoning || self.structured_output || self.context_caching || self.reasoning_effort
    }

    pub fn summary(&self) -> String {
        let mut features = Vec::new();

        if self.streaming {
            features.push("streaming");
        }
        if self.reasoning {
            features.push("advanced-reasoning");
        }
        if self.reasoning_effort {
            features.push("reasoning-effort");
        }
        if self.structured_output {
            features.push("structured-output");
        }
        if self.context_caching {
            features.push("context-caching");
        }
        if self.parallel_tool_config {
            features.push("parallel-tools");
        }
        if self.responses_compaction {
            features.push("responses-compaction");
        }

        let features_str = if features.is_empty() {
            "basic".to_string()
        } else {
            features.join(", ")
        };

        format!(
            "{} ({} tokens): {}",
            self.model, self.context_size, features_str
        )
    }
}

/// Global cache for provider capabilities (provider_name::model -> capabilities)
static CAPABILITY_CACHE: Lazy<RwLock<FxHashMap<String, ProviderCapabilities>>> =
    Lazy::new(|| RwLock::new(FxHashMap::default()));

/// Extract and cache provider capabilities for a given provider and model
pub fn get_cached_capabilities(provider: &dyn LLMProvider, model: &str) -> ProviderCapabilities {
    let cache_key = format!("{}::{}", provider.name(), model);

    // Check if already cached
    if let Ok(cache) = CAPABILITY_CACHE.read()
        && let Some(caps) = cache.get(&cache_key)
    {
        return caps.clone();
    }

    // Compute capabilities
    let caps = ProviderCapabilities::detect(provider, model);

    // Cache for future use
    if let Ok(mut cache) = CAPABILITY_CACHE.write() {
        cache.insert(cache_key, caps.clone());
    }

    caps
}

/// Universal LLM provider trait
#[async_trait]
pub trait LLMProvider: Send + Sync {
    /// Provider name (e.g., "gemini", "openai", "anthropic")
    fn name(&self) -> &str;

    /// Whether the provider has native streaming support
    fn supports_streaming(&self) -> bool {
        false
    }

    /// Whether the provider can service non-streaming generation requests for the model.
    fn supports_non_streaming(&self, _model: &str) -> bool {
        true
    }

    /// Whether the provider surfaces structured reasoning traces for the given model
    fn supports_reasoning(&self, _model: &str) -> bool {
        false
    }

    /// Whether the provider accepts configurable reasoning effort for the model
    fn supports_reasoning_effort(&self, _model: &str) -> bool {
        false
    }

    /// Whether the provider supports structured tool calling for the given model
    fn supports_tools(&self, _model: &str) -> bool {
        true
    }

    /// Whether the provider understands parallel tool configuration payloads
    fn supports_parallel_tool_config(&self, _model: &str) -> bool {
        false
    }

    /// Whether the provider supports structured output (JSON schema guarantees)
    fn supports_structured_output(&self, _model: &str) -> bool {
        false
    }

    /// Whether the provider supports prompt/context caching
    fn supports_context_caching(&self, _model: &str) -> bool {
        false
    }

    /// Whether the provider supports vision (image analysis) for given model
    fn supports_vision(&self, _model: &str) -> bool {
        false
    }

    /// Whether the provider supports Responses API server-side compaction.
    fn supports_responses_compaction(&self, _model: &str) -> bool {
        false
    }

    /// Whether the provider exposes native context-awareness / token-budget prompts.
    fn supports_context_awareness(&self, _model: &str) -> bool {
        false
    }

    /// Get the effective context window size for a model
    fn effective_context_size(&self, _model: &str) -> usize {
        // Default to 128k context window (common baseline)
        128_000
    }

    /// Compact conversation history using provider-native Responses `/compact`
    /// support when available.
    async fn compact_history(
        &self,
        _model: &str,
        _history: &[Message],
    ) -> Result<Vec<Message>, LLMError> {
        Err(LLMError::Provider {
            message: "Conversation compaction is not supported by this provider".to_string(),
            metadata: None,
        })
    }

    /// Generate completion
    async fn generate(&self, request: LLMRequest) -> Result<LLMResponse, LLMError>;

    /// Stream completion (optional)
    async fn stream(&self, request: LLMRequest) -> Result<LLMStream, LLMError> {
        // Default implementation falls back to non-streaming
        let response = self.generate(request).await?;
        let stream = try_stream! {
            yield LLMStreamEvent::Completed { response: Box::new(response) };
        };
        Ok(Box::pin(stream))
    }

    /// Normalized streaming contract layered on top of the legacy provider stream.
    async fn stream_normalized(
        &self,
        request: LLMRequest,
    ) -> Result<LLMNormalizedStream, LLMError> {
        let mut legacy_stream = self.stream(request).await?;
        let stream = try_stream! {
            while let Some(event) = futures::StreamExt::next(&mut legacy_stream).await {
                for normalized in event?.into_normalized() {
                    yield normalized;
                }
            }
        };
        Ok(Box::pin(stream))
    }

    /// Provider-specific streaming path that can service interactive runtime
    /// requests while the stream is active. Copilot uses this to bridge ACP
    /// tool calls and permission prompts back into VT Code's turn runtime.
    fn start_copilot_prompt_session<'a>(
        &'a self,
        _request: LLMRequest,
        _tools: &'a [super::ToolDefinition],
    ) -> Option<crate::copilot::CopilotPromptSessionFuture<'a>> {
        None
    }

    /// Get supported models
    fn supported_models(&self) -> Vec<String>;

    /// Validate request for this provider
    #[allow(clippy::result_large_err)]
    fn validate_request(&self, request: &LLMRequest) -> Result<(), LLMError>;
}
