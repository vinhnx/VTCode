use anyhow::{Context, Result};
use rig::client::CompletionClient;
use rig::providers::gemini::completion::gemini_api_types::ThinkingConfig;
use rig::providers::{anthropic, deepseek, gemini, openai, openrouter};
use serde_json::{Value, json};
use vtcode_config::models::Provider;
use vtcode_config::types::ReasoningEffortLevel;

/// Result of validating a provider/model combination through rig-core.
#[derive(Debug, Clone)]
pub struct RigValidationSummary {
    pub provider: Provider,
    pub model: String,
}

/// Internal bridge for Rig-backed provider/model capability checks.
#[derive(Debug, Clone)]
pub struct RigProviderCapabilities {
    provider: Provider,
    model: String,
}

impl RigProviderCapabilities {
    #[must_use]
    pub fn new(provider: Provider, model: impl Into<String>) -> Self {
        Self {
            provider,
            model: model.into(),
        }
    }

    /// Attempt to construct a rig-core client for the given provider and
    /// instantiate the requested model. This performs a lightweight validation
    /// without issuing a network request, ensuring that downstream calls can
    /// reuse the rig client configuration paths.
    pub fn validate_model(&self, api_key: &str) -> Result<RigValidationSummary> {
        match self.provider {
            Provider::Gemini => {
                let client = gemini::Client::new(api_key)
                    .context("failed to initialise Rig Gemini client")?;
                let _ = client.completion_model(&self.model);
            }
            Provider::OpenAI => {
                let client = openai::Client::new(api_key)
                    .context("failed to initialise Rig OpenAI client")?;
                let _ = client.completion_model(&self.model);
            }
            Provider::Anthropic => {
                let client = anthropic::Client::new(api_key)
                    .context("failed to initialise Rig Anthropic client")?;
                let _ = client.completion_model(&self.model);
            }
            Provider::Copilot => {
                // Copilot is authenticated through the official CLI, not rig.
            }
            Provider::Minimax => {
                // MiniMax uses an Anthropic-compatible API; rig has no direct client.
            }
            Provider::DeepSeek => {
                let client = deepseek::Client::new(api_key)
                    .context("failed to initialise Rig DeepSeek client")?;
                let _ = client.completion_model(&self.model);
            }
            Provider::HuggingFace => {
                // Hugging Face exposes an OpenAI-compatible router; rig does not ship a dedicated client.
            }
            Provider::OpenRouter => {
                let client = openrouter::Client::new(api_key)
                    .context("failed to initialise Rig OpenRouter client")?;
                let _ = client.completion_model(&self.model);
            }
            Provider::Ollama => {
                // Rig does not provide an Ollama integration; validation is skipped.
            }
            Provider::LlamaCpp => {
                // llama.cpp provides an OpenAI-compatible API; rig has no direct client.
            }
            Provider::LmStudio => {
                // LM Studio provides an OpenAI-compatible API; rig has no direct client.
            }
            Provider::Moonshot => {
                // Moonshot does not have a rig client integration yet.
            }
            Provider::Mistral => {
                // Mistral exposes an OpenAI-compatible API; rig has no dedicated client.
            }
            Provider::ZAI => {
                // The rig crate does not yet expose a dedicated Z.AI client.
                // Skip instantiation while still marking the provider as verified.
            }
            Provider::OpenCodeZen | Provider::OpenCodeGo => {
                // OpenCode Zen/Go are API gateways; rig has no direct client.
            }
            Provider::MiMo => {
                // Xiaomi MiMo is an OpenAI-compatible API; rig has no dedicated client.
            }
            Provider::Qwen => {
                // Alibaba Cloud Qwen uses an OpenAI-compatible API; rig has no dedicated client.
            }
            Provider::StepFun => {
                // StepFun uses an OpenAI-compatible API; rig has no dedicated client.
            }
            Provider::Evolink => {
                // Evolink is an OpenAI-compatible gateway; rig has no dedicated client.
            }
            Provider::Poolside => {
                // Poolside uses an OpenAI-compatible API; rig has no dedicated client.
            }
        }

        Ok(RigValidationSummary {
            provider: self.provider,
            model: self.model.clone(),
        })
    }

    /// Convert a VT Code reasoning effort level to provider-specific parameters
    /// using rig-core data structures. The resulting JSON payload can be merged
    /// into provider requests when supported.
    #[must_use]
    pub fn reasoning_parameters(&self, effort: ReasoningEffortLevel) -> Option<Value> {
        match self.provider {
            Provider::OpenAI => {
                let mut reasoning = openai::responses_api::Reasoning::new();
                let mapped = match effort {
                    ReasoningEffortLevel::None | ReasoningEffortLevel::Unknown => return None,
                    ReasoningEffortLevel::Minimal => {
                        let effort = if is_gpt5_codex_model(&self.model) {
                            "low"
                        } else {
                            "minimal"
                        };
                        return Some(json!({ "effort": effort }));
                    }
                    ReasoningEffortLevel::Low => openai::responses_api::ReasoningEffort::Low,
                    ReasoningEffortLevel::Medium => openai::responses_api::ReasoningEffort::Medium,
                    ReasoningEffortLevel::High => openai::responses_api::ReasoningEffort::High,
                    ReasoningEffortLevel::XHigh => return Some(json!({ "effort": "xhigh" })),
                    ReasoningEffortLevel::Max => return Some(json!({ "effort": "max" })),
                };
                reasoning = reasoning.with_effort(mapped);
                serde_json::to_value(reasoning).ok()
            }
            Provider::Gemini => {
                let include_thoughts = matches!(
                    effort,
                    ReasoningEffortLevel::High
                        | ReasoningEffortLevel::XHigh
                        | ReasoningEffortLevel::Max
                );
                let budget = match effort {
                    ReasoningEffortLevel::None | ReasoningEffortLevel::Unknown => return None,
                    ReasoningEffortLevel::Minimal => 16,
                    ReasoningEffortLevel::Low => 64,
                    ReasoningEffortLevel::Medium => 128,
                    ReasoningEffortLevel::High
                    | ReasoningEffortLevel::XHigh
                    | ReasoningEffortLevel::Max => 256,
                };
                let config = ThinkingConfig {
                    thinking_budget: Some(budget),
                    thinking_level: None,
                    include_thoughts: Some(include_thoughts),
                };
                serde_json::to_value(config)
                    .ok()
                    .map(|value| json!({ "thinking_config": value }))
            }
            Provider::HuggingFace => match effort {
                ReasoningEffortLevel::None | ReasoningEffortLevel::Unknown => None,
                ReasoningEffortLevel::Minimal => Some(json!({ "reasoning_effort": "minimal" })),
                ReasoningEffortLevel::Low => Some(json!({ "reasoning_effort": "low" })),
                ReasoningEffortLevel::Medium => Some(json!({ "reasoning_effort": "medium" })),
                ReasoningEffortLevel::High
                | ReasoningEffortLevel::XHigh
                | ReasoningEffortLevel::Max => Some(json!({ "reasoning_effort": "high" })),
            },
            // DeepSeek only accepts `high` and `max` for reasoning_effort.
            // Per DeepSeek docs: low/medium → high, xhigh → max.
            Provider::DeepSeek => match effort {
                ReasoningEffortLevel::None | ReasoningEffortLevel::Unknown => None,
                ReasoningEffortLevel::Minimal
                | ReasoningEffortLevel::Low
                | ReasoningEffortLevel::Medium
                | ReasoningEffortLevel::High => {
                    Some(json!({"thinking": {"type": "enabled"}, "reasoning_effort": "high"}))
                }
                ReasoningEffortLevel::XHigh | ReasoningEffortLevel::Max => {
                    Some(json!({"thinking": {"type": "enabled"}, "reasoning_effort": "max"}))
                }
            },
            Provider::Minimax => None,
            Provider::Ollama => None,
            Provider::LlamaCpp => None,
            Provider::ZAI => match effort {
                ReasoningEffortLevel::None | ReasoningEffortLevel::Unknown => None,
                ReasoningEffortLevel::Minimal => Some(json!({
                    "thinking": { "type": "enabled" },
                    "thinking_effort": "minimal"
                })),
                ReasoningEffortLevel::Low => Some(json!({
                    "thinking": { "type": "enabled" },
                    "thinking_effort": "low"
                })),
                ReasoningEffortLevel::Medium => Some(json!({
                    "thinking": { "type": "enabled" },
                    "thinking_effort": "medium"
                })),
                ReasoningEffortLevel::High
                | ReasoningEffortLevel::XHigh
                | ReasoningEffortLevel::Max => Some(json!({
                    "thinking": { "type": "enabled" },
                    "thinking_effort": "high"
                })),
            },
            Provider::StepFun => match effort {
                ReasoningEffortLevel::None | ReasoningEffortLevel::Unknown => None,
                ReasoningEffortLevel::Minimal | ReasoningEffortLevel::Low => {
                    Some(json!({ "reasoning_effort": "low" }))
                }
                ReasoningEffortLevel::Medium => Some(json!({ "reasoning_effort": "medium" })),
                ReasoningEffortLevel::High
                | ReasoningEffortLevel::XHigh
                | ReasoningEffortLevel::Max => Some(json!({ "reasoning_effort": "high" })),
            },
            Provider::Evolink => match effort {
                ReasoningEffortLevel::None | ReasoningEffortLevel::Unknown => None,
                ReasoningEffortLevel::Minimal | ReasoningEffortLevel::Low => {
                    Some(json!({ "reasoning_effort": "low" }))
                }
                ReasoningEffortLevel::Medium => Some(json!({ "reasoning_effort": "medium" })),
                ReasoningEffortLevel::High
                | ReasoningEffortLevel::XHigh
                | ReasoningEffortLevel::Max => Some(json!({ "reasoning_effort": "high" })),
            },
            _ => None,
        }
    }
}

fn is_gpt5_codex_model(model: &str) -> bool {
    model == "gpt-5-codex" || (model.starts_with("gpt-5.") && model.contains("codex"))
}

#[cfg(test)]
mod tests {
    use super::RigProviderCapabilities;
    use vtcode_config::models::Provider;
    use vtcode_config::types::ReasoningEffortLevel;

    #[test]
    fn rig_capabilities_validate_rig_backed_and_noop_providers() {
        let openai = RigProviderCapabilities::new(Provider::OpenAI, "gpt-5")
            .validate_model("test-key")
            .expect("openai validation");
        assert_eq!(openai.provider, Provider::OpenAI);
        assert_eq!(openai.model, "gpt-5");

        let deepseek = RigProviderCapabilities::new(Provider::DeepSeek, "deepseek-chat")
            .validate_model("test-key")
            .expect("no-op validation");
        assert_eq!(deepseek.provider, Provider::DeepSeek);
        assert_eq!(deepseek.model, "deepseek-chat");
    }

    #[test]
    fn rig_capabilities_generate_reasoning_payload_for_supported_provider() {
        let payload = RigProviderCapabilities::new(Provider::ZAI, "glm-5")
            .reasoning_parameters(ReasoningEffortLevel::Medium)
            .expect("reasoning payload");

        assert_eq!(payload["thinking"]["type"], "enabled");
        assert_eq!(payload["thinking_effort"], "medium");
    }

    #[test]
    fn rig_capabilities_preserve_reasoning_payload_for_openai() {
        let payload = RigProviderCapabilities::new(Provider::OpenAI, "gpt-5")
            .reasoning_parameters(ReasoningEffortLevel::Medium)
            .expect("reasoning payload");

        assert_eq!(payload["effort"], "medium");

        let codex_payload = RigProviderCapabilities::new(Provider::OpenAI, "gpt-5-codex")
            .reasoning_parameters(ReasoningEffortLevel::Minimal)
            .expect("reasoning payload");

        assert_eq!(codex_payload["effort"], "low");
    }

    #[test]
    fn rig_capabilities_preserve_reasoning_payload_for_gemini() {
        let payload = RigProviderCapabilities::new(Provider::Gemini, "gemini-2.5-pro")
            .reasoning_parameters(ReasoningEffortLevel::High)
            .expect("reasoning payload");

        assert_eq!(payload["thinking_config"]["thinkingBudget"], 256);
        assert_eq!(payload["thinking_config"]["includeThoughts"], true);
    }

    #[test]
    fn rig_capabilities_preserve_reasoning_payload_for_anthropic() {
        assert!(
            RigProviderCapabilities::new(Provider::Anthropic, "claude-sonnet-4-5")
                .reasoning_parameters(ReasoningEffortLevel::High)
                .is_none()
        );
    }

    #[test]
    fn rig_capabilities_preserve_reasoning_payload_for_deepseek() {
        let high_payload = RigProviderCapabilities::new(Provider::DeepSeek, "deepseek-chat")
            .reasoning_parameters(ReasoningEffortLevel::Medium)
            .expect("reasoning payload");

        assert_eq!(high_payload["thinking"]["type"], "enabled");
        assert_eq!(high_payload["reasoning_effort"], "high");

        let max_payload = RigProviderCapabilities::new(Provider::DeepSeek, "deepseek-chat")
            .reasoning_parameters(ReasoningEffortLevel::XHigh)
            .expect("reasoning payload");

        assert_eq!(max_payload["thinking"]["type"], "enabled");
        assert_eq!(max_payload["reasoning_effort"], "max");
    }

    #[test]
    fn rig_capabilities_preserve_reasoning_payload_for_openrouter() {
        assert!(
            RigProviderCapabilities::new(Provider::OpenRouter, "openai/gpt-5")
                .reasoning_parameters(ReasoningEffortLevel::High)
                .is_none()
        );
    }

    #[test]
    fn rig_capabilities_skip_reasoning_payload_for_unsupported_provider() {
        assert!(
            RigProviderCapabilities::new(Provider::Ollama, "qwen")
                .reasoning_parameters(ReasoningEffortLevel::High)
                .is_none()
        );
    }
}
