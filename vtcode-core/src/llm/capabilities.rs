//! Provider capability detection and aggregation

use super::provider::LLMProvider;

/// Aggregated capabilities of an LLM provider
#[derive(Debug, Clone)]
pub struct ProviderCapabilities {
    /// Provider name
    pub provider_name: String,

    /// Whether the provider supports streaming
    pub streaming: bool,

    /// Whether the provider supports advanced reasoning (e.g., o1, claude-3.7-opus)
    pub advanced_reasoning: bool,

    /// Whether the provider supports configurable reasoning effort
    pub reasoning_effort: bool,

    /// Whether the provider supports tool calling
    pub tools: bool,

    /// Whether the provider supports parallel tool configuration
    pub parallel_tools: bool,

    /// Whether the provider supports structured output (guaranteed JSON schema)
    pub structured_output: bool,

    /// Whether the provider supports context/prompt caching
    pub context_caching: bool,

    /// Model for which capabilities were detected
    pub model: String,

    /// Effective context window size (in tokens)
    pub context_size: usize,
}

impl ProviderCapabilities {
    /// Detect capabilities for a given provider and model
    pub fn detect(provider: &dyn LLMProvider, model: &str) -> Self {
        Self {
            provider_name: provider.name().to_string(),
            streaming: provider.supports_streaming(),
            advanced_reasoning: provider.supports_reasoning(model),
            reasoning_effort: provider.supports_reasoning_effort(model),
            tools: provider.supports_tools(model),
            parallel_tools: provider.supports_parallel_tool_config(model),
            structured_output: provider.supports_structured_output(model),
            context_caching: provider.supports_context_caching(model),
            model: model.to_string(),
            context_size: provider.effective_context_size(model),
        }
    }

    /// Check if provider has any advanced features
    pub fn has_advanced_features(&self) -> bool {
        self.advanced_reasoning
            || self.structured_output
            || self.context_caching
            || self.reasoning_effort
    }

    /// Get a human-readable summary of capabilities
    pub fn summary(&self) -> String {
        let mut features = Vec::new();

        if self.streaming {
            features.push("streaming");
        }
        if self.advanced_reasoning {
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
        if self.parallel_tools {
            features.push("parallel-tools");
        }

        format!(
            "{} ({} tokens): {}",
            self.model,
            self.context_size,
            if features.is_empty() {
                "basic".to_string()
            } else {
                features.join(", ")
            }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capability_summary() {
        let caps = ProviderCapabilities {
            provider_name: "gemini".to_string(),
            streaming: true,
            advanced_reasoning: false,
            reasoning_effort: false,
            tools: true,
            parallel_tools: false,
            structured_output: true,
            context_caching: true,
            model: "gemini-2.0-pro".to_string(),
            context_size: 2_000_000,
        };

        let summary = caps.summary();
        assert!(summary.contains("gemini-2.0-pro"));
        assert!(summary.contains("2000000"));
        assert!(summary.contains("structured-output"));
        assert!(summary.contains("context-caching"));
    }

    #[test]
    fn test_advanced_features() {
        let basic = ProviderCapabilities {
            provider_name: "basic".to_string(),
            streaming: false,
            advanced_reasoning: false,
            reasoning_effort: false,
            tools: true,
            parallel_tools: false,
            structured_output: false,
            context_caching: false,
            model: "basic-model".to_string(),
            context_size: 128_000,
        };

        assert!(!basic.has_advanced_features());

        let advanced = ProviderCapabilities {
            structured_output: true,
            ..basic.clone()
        };

        assert!(advanced.has_advanced_features());
    }
}
