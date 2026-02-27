use crate::config::models::Provider;
use crate::config::types::ReasoningEffortLevel;
use anyhow::Result;
use rig::client::CompletionClient;
use rig::providers::gemini::completion::gemini_api_types::ThinkingConfig;
use rig::providers::{anthropic, deepseek, gemini, openai, openrouter};
use serde_json::{Value, json};

/// Result of validating a provider/model combination through rig-core.
#[derive(Debug, Clone)]
pub struct RigValidationSummary {
    pub provider: Provider,
    pub model: String,
}

/// Attempt to construct a rig-core client for the given provider and
/// instantiate the requested model. This performs a lightweight validation
/// without issuing a network request, ensuring that downstream calls can
/// reuse the rig client configuration paths.
pub fn verify_model_with_rig(
    provider: Provider,
    model: &str,
    api_key: &str,
) -> Result<RigValidationSummary> {
    match provider {
        Provider::Gemini => {
            let client = gemini::Client::new(api_key);
            let _ = client.completion_model(model);
        }
        Provider::OpenAI => {
            let client = openai::Client::new(api_key);
            let _ = client.completion_model(model);
        }
        Provider::Anthropic => {
            let client = anthropic::Client::new(api_key);
            let _ = client.completion_model(model);
        }
        Provider::Minimax => {
            // MiniMax uses an Anthropic-compatible API; rig has no direct client.
        }
        Provider::DeepSeek => {
            let client = deepseek::Client::new(api_key);
            let _ = client.completion_model(model);
        }
        Provider::HuggingFace => {
            // Hugging Face exposes an OpenAI-compatible router; rig does not ship a dedicated client.
        }
        Provider::OpenRouter => {
            let client = openrouter::Client::new(api_key);
            let _ = client.completion_model(model);
        }
        Provider::Ollama => {
            // Rig does not provide an Ollama integration; validation is skipped.
        }
        Provider::Moonshot => {
            // Moonshot does not have a rig client integration yet.
        }
        Provider::ZAI => {
            // The rig crate does not yet expose a dedicated Z.AI client.
            // Skip instantiation while still marking the provider as verified.
        }
    }

    Ok(RigValidationSummary {
        provider,
        model: model.to_string(),
    })
}

/// Convert a vtcode reasoning effort level to provider-specific parameters
/// using rig-core data structures. The resulting JSON payload can be merged
/// into provider requests when supported.
pub fn reasoning_parameters_for(provider: Provider, effort: ReasoningEffortLevel) -> Option<Value> {
    match provider {
        Provider::OpenAI => {
            let mut reasoning = openai::responses_api::Reasoning::new();
            let mapped = match effort {
                ReasoningEffortLevel::None => return None, // Don't send any reasoning parameter for "none"
                ReasoningEffortLevel::Minimal => return Some(json!({ "effort": "minimal" })), // GPT-5 minimal reasoning
                ReasoningEffortLevel::Low => openai::responses_api::ReasoningEffort::Low,
                ReasoningEffortLevel::Medium => openai::responses_api::ReasoningEffort::Medium,
                ReasoningEffortLevel::High => openai::responses_api::ReasoningEffort::High,
                ReasoningEffortLevel::XHigh => return Some(json!({ "effort": "xhigh" })), // GPT-5.2+ xhigh
            };
            reasoning = reasoning.with_effort(mapped);
            serde_json::to_value(reasoning).ok()
        }
        Provider::Gemini => {
            let include_thoughts = matches!(
                effort,
                ReasoningEffortLevel::High | ReasoningEffortLevel::XHigh
            );
            let budget = match effort {
                ReasoningEffortLevel::None => return None,
                ReasoningEffortLevel::Minimal => 16, // Low budget for minimal reasoning
                ReasoningEffortLevel::Low => 64,
                ReasoningEffortLevel::Medium => 128,
                ReasoningEffortLevel::High | ReasoningEffortLevel::XHigh => 256, // Max budget for Gemini
            };
            let config = ThinkingConfig {
                thinking_budget: budget,
                include_thoughts: Some(include_thoughts),
            };
            serde_json::to_value(config)
                .ok()
                .map(|value| json!({ "thinking_config": value }))
        }
        Provider::HuggingFace => match effort {
            ReasoningEffortLevel::None => None,
            ReasoningEffortLevel::Minimal => Some(json!({ "reasoning_effort": "minimal" })),
            ReasoningEffortLevel::Low => Some(json!({ "reasoning_effort": "low" })),
            ReasoningEffortLevel::Medium => Some(json!({ "reasoning_effort": "medium" })),
            ReasoningEffortLevel::High | ReasoningEffortLevel::XHigh => {
                Some(json!({ "reasoning_effort": "high" }))
            }
        },
        Provider::Minimax => None,
        Provider::Ollama => None,
        Provider::ZAI => match effort {
            ReasoningEffortLevel::None => None,
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
            ReasoningEffortLevel::High | ReasoningEffortLevel::XHigh => Some(json!({
                "thinking": { "type": "enabled" },
                "thinking_effort": "high"
            })),
        },
        _ => None,
    }
}
