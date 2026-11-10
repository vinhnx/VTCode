use rig::providers::gemini::completion::gemini_api_types::ThinkingConfig;
use serde_json::{Value, json};
use vtcode_config::models::Provider;
use vtcode_config::types::ReasoningEffortLevel;

/// Internal bridge for provider reasoning-parameter construction.
#[derive(Debug, Clone)]
pub struct RigProviderCapabilities {
    provider: Provider,
    model: compact_str::CompactString,
}

impl RigProviderCapabilities {
    #[must_use]
    pub fn new(provider: Provider, model: impl Into<String>) -> Self {
        Self { provider, model: model.into().into() }
    }

    /// Serialize an already mapped effort without changing its fidelity.
    /// Unsupported controls are returned as an invalid request so callers
    /// cannot silently drop or coerce the requested effort.
    pub fn reasoning_parameters(
        &self,
        effort: ReasoningEffortLevel,
    ) -> Result<Option<Value>, crate::provider::LLMError> {
        let supported = crate::provider::catalog_reasoning_efforts(self.provider.as_ref(), &self.model)
            .or_else(|| (self.provider == Provider::Anthropic).then_some(crate::provider::GENERIC_REASONING_EFFORTS))
            .unwrap_or(&[]);
        self.reasoning_parameters_for_supported_efforts(effort, supported)
    }

    /// Serialize an effort against a provider-resolved capability list.
    ///
    /// Built-in model routes should use [`Self::reasoning_parameters`], which
    /// validates against the catalog. Configured custom routes can instead
    /// pass the Provider trait's advertised levels so strict validation does
    /// not reject a capability that was explicitly supplied by the user.
    pub fn reasoning_parameters_for_supported_efforts(
        &self,
        effort: ReasoningEffortLevel,
        supported: &[&str],
    ) -> Result<Option<Value>, crate::provider::LLMError> {
        if effort == ReasoningEffortLevel::None {
            return Ok(None);
        }
        let _mapping =
            crate::reasoning_effort::ReasoningEffortMapper::map(effort, supported, false).map_err(|error| {
                crate::provider::LLMError::InvalidRequest { message: error.to_string(), metadata: None }
            })?;
        let payload = match self.provider {
            Provider::OpenAI => Some(json!({ "effort": effort.as_str() })),
            Provider::Gemini => {
                let budget = match effort {
                    ReasoningEffortLevel::Minimal => 16,
                    ReasoningEffortLevel::Low => 64,
                    ReasoningEffortLevel::Medium => 128,
                    ReasoningEffortLevel::High => 256,
                    _ => {
                        return Err(crate::provider::LLMError::InvalidRequest {
                            message: format!("Gemini does not serialize reasoning effort `{effort}`"),
                            metadata: None,
                        });
                    }
                };
                let config = ThinkingConfig {
                    thinking_budget: Some(budget),
                    thinking_level: None,
                    include_thoughts: Some(effort == ReasoningEffortLevel::High),
                };
                serde_json::to_value(config)
                    .ok()
                    .map(|value| json!({ "thinking_config": value }))
            }
            Provider::DeepSeek | Provider::ZAI => Some(json!({
                "thinking": { "type": "enabled" }, "reasoning_effort": effort.as_str()
            })),
            Provider::HuggingFace | Provider::Meta | Provider::Evolink => {
                Some(json!({ "reasoning_effort": effort.as_str() }))
            }
            // StepFun speaks the native Responses envelope (`reasoning.effort`).
            Provider::StepFun => Some(json!({ "reasoning": { "effort": effort.as_str() } })),
            // OpenRouter follows the OpenAI-compatible `reasoning.effort`
            // envelope, including for custom routes whose capabilities are
            // supplied by the Provider trait rather than the built-in catalog.
            Provider::OpenRouter => Some(json!({ "effort": effort.as_str() })),
            Provider::Anthropic => None,
            _ => None,
        };
        Ok(payload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rig_serialization_preserves_exact_catalog_effort() {
        for (provider, model, key) in [
            (Provider::OpenAI, "gpt-6-astra", "effort"),
            (Provider::ZAI, "glm-5.3", "reasoning_effort"),
        ] {
            for effort in [
                ReasoningEffortLevel::Low,
                ReasoningEffortLevel::High,
                ReasoningEffortLevel::Max,
            ] {
                let payload = RigProviderCapabilities::new(provider, model)
                    .reasoning_parameters(effort)
                    .expect("reasoning serialization should not fail")
                    .expect("catalog-supported effort");
                assert_eq!(payload[key], effort.as_str());
            }
        }
    }

    #[test]
    fn rig_serialization_rejects_unsupported_effort_without_aliasing() {
        for (provider, model, effort) in [
            (Provider::OpenAI, "gpt-6-astra", ReasoningEffortLevel::Minimal),
            (Provider::ZAI, "glm-5.3", ReasoningEffortLevel::XHigh),
            (Provider::Gemini, "gemini-3.7-flash", ReasoningEffortLevel::Max),
            (Provider::OpenAI, "unknown-model", ReasoningEffortLevel::High),
            (Provider::OpenRouter, "meta/muse-spark-1.3", ReasoningEffortLevel::High),
        ] {
            assert!(
                RigProviderCapabilities::new(provider, model)
                    .reasoning_parameters(effort)
                    .is_err()
            );
        }
    }

    #[test]
    fn rig_serialization_accepts_explicit_custom_capability_levels() {
        let payload = RigProviderCapabilities::new(Provider::OpenAI, "custom-route")
            .reasoning_parameters_for_supported_efforts(ReasoningEffortLevel::High, &["low", "medium", "high"])
            .expect("explicit provider capability should validate")
            .expect("OpenRouter should serialize reasoning effort");
        assert_eq!(payload["effort"], "high");
    }

    #[test]
    fn anthropic_custom_routes_use_generic_efforts_without_silent_drops() {
        let payload = RigProviderCapabilities::new(Provider::Anthropic, "custom-route")
            .reasoning_parameters(ReasoningEffortLevel::High)
            .expect("generic custom effort should validate");
        assert!(payload.is_none(), "Anthropic encodes this effort in output_config");
        assert!(
            RigProviderCapabilities::new(Provider::Anthropic, "custom-route")
                .reasoning_parameters(ReasoningEffortLevel::Max)
                .is_err()
        );
    }

    #[test]
    fn native_providers_do_not_fail_rig_serialization() {
        for (provider, model) in [
            (Provider::MergeGateway, "openai/gpt-5.5"),
            (Provider::OllamaCloud, "gpt-oss:120b-cloud"),
        ] {
            let payload = RigProviderCapabilities::new(provider, model)
                .reasoning_parameters_for_supported_efforts(ReasoningEffortLevel::High, &["low", "medium", "high"])
                .expect("native provider should not error on supported effort");
            assert!(payload.is_none(), "native provider handles reasoning effort on its own");
        }
    }

    #[test]
    fn all_providers_handle_reasoning_parameters_gracefully() {
        let supported_efforts = &["low", "medium", "high", "xhigh", "max"];
        for (provider, model) in [
            (Provider::OpenAI, "gpt-5.1-mini"),
            (Provider::Anthropic, "claude-sonnet-5"),
            (Provider::Gemini, "gemini-3.7-flash"),
            (Provider::DeepSeek, "deepseek-v4.1-flash"),
            (Provider::ZAI, "glm-5.3-flash"),
            (Provider::Meta, "muse-spark-1.3"),
            (Provider::OpenRouter, "openrouter/auto"),
            (Provider::Ollama, "gpt-oss:20b"),
            (Provider::OllamaCloud, "gpt-oss:20b-cloud"),
            (Provider::LmStudio, "gpt-oss-20b"),
            (Provider::LlamaCpp, "gpt-oss-20b"),
            (Provider::Moonshot, "kimi-k3"),
            (Provider::Minimax, "minimax-m2.7"),
            (Provider::MiMo, "mimo-v2.6-pro"),
            (Provider::Mistral, "ministral-3-3b-2512"),
            (Provider::HuggingFace, "openai/gpt-oss-20b:huggingface"),
            (Provider::OpenCodeZen, "gpt-5.6-sol"),
            (Provider::OpenCodeGo, "glm-5.2"),
            (Provider::Qwen, "deepseek-flash"),
            (Provider::StepFun, "step-3.7-flash"),
            (Provider::Evolink, "deepseek-flash"),
            (Provider::Poolside, "laguna-xs2"),
            (Provider::XAI, "grok-4-6"),
            (Provider::NVIDIA, "nvidia/nemotron-3-nano-30b-a3b"),
            (Provider::MergeGateway, "openai/gpt-5.5"),
            (Provider::Vercel, "claude-sonnet-5"),
        ] {
            let result = RigProviderCapabilities::new(provider, model)
                .reasoning_parameters_for_supported_efforts(ReasoningEffortLevel::High, supported_efforts);
            assert!(result.is_ok(), "provider {provider:?} with model {model} should not error on supported effort");
        }
    }
}
