use crate::config::constants::env_vars;
use crate::config::core::AnthropicConfig;
use crate::config::types::ReasoningEffortLevel;
use crate::llm::provider::LLMRequest;
use crate::llm::providers::anthropic_types::ThinkingConfig;
use crate::llm::rig_adapter::reasoning_parameters_for;
use serde_json::{json, Value};
use std::env;

use super::super::capabilities::supports_reasoning_effort;

pub(crate) fn build_thinking_config(
    request: &LLMRequest,
    anthropic_config: &AnthropicConfig,
    default_model: &str,
) -> (Option<ThinkingConfig>, Option<Value>) {
    let thinking_enabled =
        anthropic_config.extended_thinking_enabled
            && supports_reasoning_effort(&request.model, default_model);

    if thinking_enabled {
        let max_thinking_tokens: Option<u32> = env::var(env_vars::MAX_THINKING_TOKENS)
            .ok()
            .and_then(|v| v.parse().ok());

        let budget = if let Some(explicit_budget) = request.thinking_budget {
            explicit_budget
        } else if let Some(env_budget) = max_thinking_tokens {
            env_budget
        } else if let Some(effort) = request.reasoning_effort {
            match effort {
                ReasoningEffortLevel::None => 0,
                ReasoningEffortLevel::Minimal => 1024,
                ReasoningEffortLevel::Low => 4096,
                ReasoningEffortLevel::Medium => 8192,
                ReasoningEffortLevel::High => 16384,
                ReasoningEffortLevel::XHigh => 32768,
            }
        } else {
            anthropic_config.interleaved_thinking_budget_tokens
        };

        if budget >= 1024 {
            let max_tokens = request.max_tokens.unwrap_or(16000);
            let effective_budget = budget.min(max_tokens.saturating_sub(100)).max(1024);
            return (
                Some(ThinkingConfig::Enabled {
                    budget_tokens: effective_budget,
                }),
                None,
            );
        }
    } else if let Some(effort) = request.reasoning_effort {
        use crate::config::models::Provider;
        if let Some(payload) = reasoning_parameters_for(Provider::Anthropic, effort) {
            return (None, Some(payload));
        } else {
            return (None, Some(json!({ "effort": effort.as_str() })));
        }
    }

    (None, None)
}
