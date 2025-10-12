use crate::config::models::Provider;
use crate::config::types::ReasoningEffortLevel;
use anyhow::Result;
use rig::client::CompletionClient;
use rig::providers::gemini::completion::gemini_api_types::ThinkingConfig;
use rig::providers::{anthropic, deepseek, gemini, openai, openrouter, xai};
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
        Provider::DeepSeek => {
            let client = deepseek::Client::new(api_key);
            let _ = client.completion_model(model);
        }
        Provider::OpenRouter => {
            let client = openrouter::Client::new(api_key);
            let _ = client.completion_model(model);
        }
        Provider::Ollama => {
            // Rig does not provide an Ollama integration; validation is skipped.
        }
        Provider::XAI => {
            let client = xai::Client::new(api_key);
            let _ = client.completion_model(model);
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
                ReasoningEffortLevel::Low => openai::responses_api::ReasoningEffort::Low,
                ReasoningEffortLevel::Medium => openai::responses_api::ReasoningEffort::Medium,
                ReasoningEffortLevel::High => openai::responses_api::ReasoningEffort::High,
            };
            reasoning = reasoning.with_effort(mapped);
            serde_json::to_value(reasoning).ok()
        }
        Provider::Gemini => {
            let include_thoughts = matches!(effort, ReasoningEffortLevel::High);
            let budget = match effort {
                ReasoningEffortLevel::Low => 64,
                ReasoningEffortLevel::Medium => 128,
                ReasoningEffortLevel::High => 256,
            };
            let config = ThinkingConfig {
                thinking_budget: budget,
                include_thoughts: Some(include_thoughts),
            };
            serde_json::to_value(config)
                .ok()
                .map(|value| json!({ "thinking_config": value }))
        }
        Provider::Ollama => None,
        Provider::ZAI => None,
        _ => None,
    }
}
