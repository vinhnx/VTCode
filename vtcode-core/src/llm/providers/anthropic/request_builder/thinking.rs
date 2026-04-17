use crate::config::constants::env_vars;
use crate::config::core::AnthropicConfig;
use crate::config::models::Provider;
use crate::config::types::ReasoningEffortLevel;
use crate::llm::provider::{
    AnthropicThinkingDisplayOverride, AnthropicThinkingModeOverride, LLMRequest,
};
use crate::llm::providers::anthropic_types::{ThinkingConfig, ThinkingDisplay};
use crate::llm::rig_adapter::RigProviderCapabilities;
use serde_json::{Value, json};
use std::env;

use super::super::capabilities::{
    claude_thinking_profile, resolve_model_name, supports_reasoning_effort,
};

fn resolve_configured_thinking_display(
    anthropic_config: &AnthropicConfig,
) -> Option<ThinkingDisplay> {
    anthropic_config.thinking_display.as_deref().and_then(|d| {
        match d.to_ascii_lowercase().as_str() {
            "summarized" => Some(ThinkingDisplay::Summarized),
            "omitted" => Some(ThinkingDisplay::Omitted),
            _ => None,
        }
    })
}

fn resolve_thinking_display(
    request: &LLMRequest,
    anthropic_config: &AnthropicConfig,
) -> Option<ThinkingDisplay> {
    if let Some(overrides) = request.anthropic_request_overrides.as_ref() {
        return match overrides.thinking_display {
            AnthropicThinkingDisplayOverride::Inherit => None,
            AnthropicThinkingDisplayOverride::Summarized => Some(ThinkingDisplay::Summarized),
            AnthropicThinkingDisplayOverride::Omitted => Some(ThinkingDisplay::Omitted),
        };
    }

    resolve_configured_thinking_display(anthropic_config)
}

fn manual_thinking_config(
    budget: u32,
    max_tokens: Option<u32>,
    display: Option<ThinkingDisplay>,
) -> Option<ThinkingConfig> {
    if budget < 1024 {
        return None;
    }

    let max_tokens = max_tokens.unwrap_or(16000);
    let effective_budget = budget.min(max_tokens.saturating_sub(100)).max(1024);
    Some(ThinkingConfig::Enabled {
        budget_tokens: effective_budget,
        display,
    })
}

pub(crate) fn build_thinking_config(
    request: &LLMRequest,
    anthropic_config: &AnthropicConfig,
    default_model: &str,
) -> (Option<ThinkingConfig>, Option<Value>) {
    let resolved_model = resolve_model_name(&request.model, default_model);
    let thinking_enabled = anthropic_config.extended_thinking_enabled
        && supports_reasoning_effort(resolved_model, default_model);
    let display = resolve_thinking_display(request, anthropic_config);

    if let Some(overrides) = request.anthropic_request_overrides.as_ref() {
        match overrides.thinking_mode {
            AnthropicThinkingModeOverride::Disabled => return (None, None),
            AnthropicThinkingModeOverride::Adaptive => {
                return (Some(ThinkingConfig::Adaptive { display }), None);
            }
            AnthropicThinkingModeOverride::ManualBudget(budget) => {
                return (
                    manual_thinking_config(budget, request.max_tokens, display),
                    None,
                );
            }
            AnthropicThinkingModeOverride::Inherit => {}
        }
    }

    if thinking_enabled {
        if claude_thinking_profile(resolved_model, default_model).is_some_and(|profile| {
            matches!(
                profile.mode,
                super::super::capabilities::ClaudeThinkingMode::Adaptive
            )
        }) {
            if claude_thinking_profile(resolved_model, default_model)
                .is_some_and(|profile| profile.supports_manual_budget)
                && let Some(explicit_budget) = request.thinking_budget
            {
                return (
                    manual_thinking_config(explicit_budget, request.max_tokens, display),
                    None,
                );
            }
            return (Some(ThinkingConfig::Adaptive { display }), None);
        }

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
                ReasoningEffortLevel::Max => 32768,
            }
        } else {
            anthropic_config.interleaved_thinking_budget_tokens
        };

        if let Some(thinking) = manual_thinking_config(budget, request.max_tokens, display) {
            return (Some(thinking), None);
        }
    } else if let Some(effort) = request.reasoning_effort {
        if claude_thinking_profile(resolved_model, default_model).is_some_and(|profile| {
            matches!(
                profile.mode,
                super::super::capabilities::ClaudeThinkingMode::Adaptive
            )
        }) {
            return (None, None);
        }

        if let Some(payload) = RigProviderCapabilities::new(Provider::Anthropic, &request.model)
            .reasoning_parameters(effort)
        {
            return (None, Some(payload));
        } else {
            return (None, Some(json!({ "effort": effort.as_str() })));
        }
    }

    (None, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::constants::models;

    #[test]
    fn uses_adaptive_thinking_for_opus_4_7_by_default() {
        let request = LLMRequest {
            model: models::anthropic::CLAUDE_OPUS_4_7.to_string(),
            ..Default::default()
        };
        let config = AnthropicConfig::default();
        let (thinking, reasoning) =
            build_thinking_config(&request, &config, models::anthropic::DEFAULT_MODEL);

        assert!(matches!(thinking, Some(ThinkingConfig::Adaptive { .. })));
        assert!(reasoning.is_none());
    }

    #[test]
    fn ignores_explicit_budget_for_opus_4_7() {
        let request = LLMRequest {
            model: models::anthropic::CLAUDE_OPUS_4_7.to_string(),
            thinking_budget: Some(2048),
            ..Default::default()
        };
        let config = AnthropicConfig::default();
        let (thinking, _) =
            build_thinking_config(&request, &config, models::anthropic::DEFAULT_MODEL);

        assert!(matches!(thinking, Some(ThinkingConfig::Adaptive { .. })));
    }

    #[test]
    fn adaptive_thinking_includes_summarized_display() {
        let request = LLMRequest {
            model: models::anthropic::CLAUDE_OPUS_4_7.to_string(),
            ..Default::default()
        };
        let config = AnthropicConfig {
            thinking_display: Some("summarized".to_string()),
            ..AnthropicConfig::default()
        };
        let (thinking, _) =
            build_thinking_config(&request, &config, models::anthropic::DEFAULT_MODEL);

        match thinking {
            Some(ThinkingConfig::Adaptive {
                display: Some(ThinkingDisplay::Summarized),
            }) => {}
            other => panic!("expected Adaptive with Summarized display, got {other:?}"),
        }
    }

    #[test]
    fn adaptive_thinking_includes_omitted_display() {
        let request = LLMRequest {
            model: models::anthropic::CLAUDE_OPUS_4_7.to_string(),
            ..Default::default()
        };
        let config = AnthropicConfig {
            thinking_display: Some("omitted".to_string()),
            ..AnthropicConfig::default()
        };
        let (thinking, _) =
            build_thinking_config(&request, &config, models::anthropic::DEFAULT_MODEL);

        match thinking {
            Some(ThinkingConfig::Adaptive {
                display: Some(ThinkingDisplay::Omitted),
            }) => {}
            other => panic!("expected Adaptive with Omitted display, got {other:?}"),
        }
    }

    #[test]
    fn adaptive_thinking_includes_display_for_sonnet_4_6_when_configured() {
        let request = LLMRequest {
            model: models::anthropic::CLAUDE_SONNET_4_6.to_string(),
            ..Default::default()
        };
        let config = AnthropicConfig {
            thinking_display: Some("summarized".to_string()),
            ..AnthropicConfig::default()
        };
        let (thinking, _) =
            build_thinking_config(&request, &config, models::anthropic::DEFAULT_MODEL);

        match thinking {
            Some(ThinkingConfig::Adaptive {
                display: Some(ThinkingDisplay::Summarized),
            }) => {}
            other => panic!("expected Adaptive with Summarized display, got {other:?}"),
        }
    }

    #[test]
    fn thinking_display_defaults_to_none() {
        let request = LLMRequest {
            model: models::anthropic::CLAUDE_OPUS_4_7.to_string(),
            ..Default::default()
        };
        let config = AnthropicConfig::default();
        let (thinking, _) =
            build_thinking_config(&request, &config, models::anthropic::DEFAULT_MODEL);

        match thinking {
            Some(ThinkingConfig::Adaptive { display: None }) => {}
            other => panic!("expected Adaptive with no display, got {other:?}"),
        }
    }

    #[test]
    fn uses_adaptive_thinking_for_mythos_preview() {
        let request = LLMRequest {
            model: models::anthropic::CLAUDE_MYTHOS_PREVIEW.to_string(),
            ..Default::default()
        };
        let config = AnthropicConfig::default();
        let (thinking, reasoning) =
            build_thinking_config(&request, &config, models::anthropic::DEFAULT_MODEL);

        assert!(matches!(thinking, Some(ThinkingConfig::Adaptive { .. })));
        assert!(reasoning.is_none());
    }

    #[test]
    fn uses_adaptive_thinking_for_opus_4_6_by_default() {
        let request = LLMRequest {
            model: models::anthropic::CLAUDE_OPUS_4_6.to_string(),
            ..Default::default()
        };
        let config = AnthropicConfig::default();
        let (thinking, reasoning) =
            build_thinking_config(&request, &config, models::anthropic::DEFAULT_MODEL);

        assert!(matches!(thinking, Some(ThinkingConfig::Adaptive { .. })));
        assert!(reasoning.is_none());
    }

    #[test]
    fn uses_budgeted_thinking_for_opus_4_6_when_budget_is_explicit() {
        let request = LLMRequest {
            model: models::anthropic::CLAUDE_OPUS_4_6.to_string(),
            thinking_budget: Some(4096),
            ..Default::default()
        };
        let config = AnthropicConfig::default();
        let (thinking, reasoning) =
            build_thinking_config(&request, &config, models::anthropic::DEFAULT_MODEL);

        assert!(matches!(
            thinking,
            Some(ThinkingConfig::Enabled {
                budget_tokens: 4096,
                ..
            })
        ));
        assert!(reasoning.is_none());
    }
}
