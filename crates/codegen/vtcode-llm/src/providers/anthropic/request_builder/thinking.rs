use crate::provider::{AnthropicThinkingDisplayOverride, AnthropicThinkingModeOverride, LLMRequest};
use crate::providers::anthropic_types::{ThinkingConfig, ThinkingDisplay};
use crate::rig_adapter::RigProviderCapabilities;
use serde_json::{Value, json};
use std::env;
use tracing::warn;
use vtcode_config::constants::env_vars;
use vtcode_config::core::AnthropicConfig;
use vtcode_config::models::Provider;
use vtcode_config::types::ReasoningEffortLevel;

use vtcode_config::constants::models::anthropic;

use super::super::capabilities::{
    ClaudeThinkingProfile, claude_thinking_profile, default_max_tokens_for_model, default_thinking_display,
    effort_is_at_most_high, effort_str_is_at_most_high, matches_model, resolve_model_name, supports_reasoning_effort,
    supports_thinking_display_updates,
};

fn resolve_configured_thinking_display(anthropic_config: &AnthropicConfig) -> Option<ThinkingDisplay> {
    anthropic_config.thinking_display.and_then(|d| match d {
        vtcode_config::ThinkingDisplayMode::Summarized => Some(ThinkingDisplay::Summarized),
        vtcode_config::ThinkingDisplayMode::Omitted => Some(ThinkingDisplay::Omitted),
        vtcode_config::ThinkingDisplayMode::Updates => Some(ThinkingDisplay::Updates),
        vtcode_config::ThinkingDisplayMode::Unknown => None,
    })
}

/// Resolves `thinking.display` for the request.
///
/// Per-request overrides (the Anthropic-compatible endpoint) take precedence
/// over the `[provider.anthropic]` setting; with neither set, the model's VT
/// Code default applies (`updates` on Claude Opus 5.5). Either source may name
/// `updates`, which only some models accept: the override comes from a caller
/// that may name any model, and the setting applies to every Claude model the
/// session switches to. On models that reject it the field is omitted, so the
/// model default display applies instead of the request failing.
fn resolve_thinking_display(
    request: &LLMRequest,
    anthropic_config: &AnthropicConfig,
    resolved_model: &str,
    default_model: &str,
) -> Option<ThinkingDisplay> {
    let (display, source) = match request.anthropic_request_overrides.as_ref() {
        Some(overrides) => match overrides.thinking_display {
            AnthropicThinkingDisplayOverride::Inherit => return None,
            AnthropicThinkingDisplayOverride::Summarized => (ThinkingDisplay::Summarized, "request"),
            AnthropicThinkingDisplayOverride::Omitted => (ThinkingDisplay::Omitted, "request"),
            AnthropicThinkingDisplayOverride::Updates => (ThinkingDisplay::Updates, "request"),
        },
        None => match resolve_configured_thinking_display(anthropic_config) {
            Some(display) => (display, "provider.anthropic.thinking_display"),
            None => return default_thinking_display(resolved_model, default_model),
        },
    };

    if display == ThinkingDisplay::Updates && !supports_thinking_display_updates(resolved_model, default_model) {
        tracing::debug!(
            model = %resolved_model,
            source,
            "thinking display \"updates\" is not supported by this model; using the model default display"
        );
        return None;
    }
    Some(display)
}

/// Builds a manual `budget_tokens` config clamped below `max_tokens`, which
/// must be the value the request will actually send.
fn manual_thinking_config(budget: u32, max_tokens: u32, display: Option<ThinkingDisplay>) -> Option<ThinkingConfig> {
    if budget < 1024 {
        return None;
    }

    let effective_budget = budget.min(max_tokens.saturating_sub(100)).max(1024);
    Some(ThinkingConfig::Enabled { budget_tokens: effective_budget, display })
}

/// The `thinking` value that expresses "no up-front thinking" as a direct
/// request to `model`, or `None` when the model has no such setting.
///
/// Adaptive-only models (Opus 5.5, Fable 5.x) reject every "thinking off"
/// request. Claude Sonnet 5 accepts `disabled`; Claude Opus 5 accepts it only
/// at effort `high` or below; Claude Sonnet 5.5 rejects `disabled` and takes
/// `between_tools` instead, which is itself rejected at `xhigh`/`max` effort.
/// The 5-generation ids are substrings of their 5.5 counterparts, so the
/// `between_tools` check must run before the Opus 5 effort gate.
fn lowest_thinking_config(
    profile: &ClaudeThinkingProfile,
    model: &str,
    effort_is_at_most_high: impl FnOnce() -> bool,
) -> Option<ThinkingConfig> {
    if profile.adaptive_only {
        return None;
    }
    if profile.rejects_disabled_thinking {
        return effort_is_at_most_high().then_some(ThinkingConfig::BetweenTools);
    }
    if matches_model(model, anthropic::CLAUDE_OPUS_5) {
        return effort_is_at_most_high().then_some(ThinkingConfig::Disabled);
    }
    Some(ThinkingConfig::Disabled)
}

/// Rewrites a "thinking off" config into the lowest setting `model` accepts,
/// falling back to adaptive thinking when it has none. `None` means the config
/// is already valid as a direct request to `model`.
fn rewrite_lowest_thinking(
    thinking: &ThinkingConfig,
    profile: &ClaudeThinkingProfile,
    model: &str,
    effort: Option<&str>,
) -> Option<ThinkingConfig> {
    let lowest =
        lowest_thinking_config(profile, model, || effort_str_is_at_most_high(effort.unwrap_or(profile.default_effort)));
    let already_valid = matches!(
        (thinking, &lowest),
        (ThinkingConfig::Disabled, Some(ThinkingConfig::Disabled))
            | (ThinkingConfig::BetweenTools, Some(ThinkingConfig::BetweenTools))
    );
    if already_valid {
        return None;
    }
    Some(lowest.unwrap_or(ThinkingConfig::Adaptive { display: None }))
}

/// Rewrites `thinking` into a config that is valid as a direct request to
/// `model`, or returns `None` when it is already valid (or the model has no
/// capability profile to check against).
///
/// Server-side fallback entries are merged into the primary request, so the
/// merged request must satisfy the fallback model's own rules: models without
/// manual-budget support get adaptive thinking instead of `budget_tokens`, and
/// a "thinking off" config the model rejects becomes its own lowest setting.
/// `effort` is the request's `output_config.effort`, which fallback entries
/// inherit; `None` means the model's default effort applies.
pub(crate) fn rewrite_thinking_for_model(
    thinking: &ThinkingConfig,
    model: &str,
    default_model: &str,
    effort: Option<&str>,
) -> Option<ThinkingConfig> {
    let resolved_model = resolve_model_name(model, default_model);
    let rewritten = claude_thinking_profile(resolved_model, default_model).and_then(|profile| match thinking {
        ThinkingConfig::Enabled { display, .. } if !profile.supports_manual_budget => {
            Some(ThinkingConfig::Adaptive { display: *display })
        }
        ThinkingConfig::Disabled | ThinkingConfig::BetweenTools => {
            rewrite_lowest_thinking(thinking, &profile, resolved_model, effort)
        }
        _ => None,
    });

    // `display: "updates"` is rejected by models without progress updates,
    // so a config carrying it (for example inherited from an Opus 5.5
    // primary) falls back to that model's default display.
    let candidate = rewritten.as_ref().unwrap_or(thinking);
    if thinking_display(candidate) == Some(ThinkingDisplay::Updates)
        && !supports_thinking_display_updates(resolved_model, default_model)
    {
        return Some(with_thinking_display(candidate, None));
    }
    rewritten
}

fn thinking_display(thinking: &ThinkingConfig) -> Option<ThinkingDisplay> {
    match thinking {
        ThinkingConfig::Adaptive { display } | ThinkingConfig::Enabled { display, .. } => *display,
        // `between_tools` takes no other field, so it never carries a display.
        ThinkingConfig::Disabled | ThinkingConfig::BetweenTools | ThinkingConfig::Unknown => None,
    }
}

fn with_thinking_display(thinking: &ThinkingConfig, display: Option<ThinkingDisplay>) -> ThinkingConfig {
    match thinking {
        ThinkingConfig::Adaptive { .. } => ThinkingConfig::Adaptive { display },
        ThinkingConfig::Enabled { budget_tokens, .. } => {
            ThinkingConfig::Enabled { budget_tokens: *budget_tokens, display }
        }
        other => other.clone(),
    }
}

pub(crate) fn build_thinking_config(
    request: &LLMRequest,
    anthropic_config: &AnthropicConfig,
    default_model: &str,
) -> Result<(Option<ThinkingConfig>, Option<Value>), crate::provider::LLMError> {
    let resolved_model = resolve_model_name(&request.model, default_model);
    let profile = claude_thinking_profile(resolved_model, default_model);
    let display = resolve_thinking_display(request, anthropic_config, resolved_model, default_model);
    let default_thinking = profile.is_some_and(|p| p.default_thinking_enabled);
    // The request builder sends this same default whenever thinking is on, so
    // manual budgets are clamped against the real `max_tokens`.
    let thinking_max_tokens = request
        .max_tokens
        .unwrap_or_else(|| default_max_tokens_for_model(resolved_model, default_model, true));

    if let Some(overrides) = request.anthropic_request_overrides.as_ref() {
        match overrides.thinking_mode {
            AnthropicThinkingModeOverride::Disabled => {
                // Models that think by default need an explicit "thinking off"
                // setting when they accept one; otherwise the field is omitted
                // and the model runs its default thinking mode.
                if let Some(profile) = profile.filter(|p| p.default_thinking_enabled)
                    && let Some(thinking) = lowest_thinking_config(&profile, resolved_model, || {
                        effort_is_at_most_high(request, anthropic_config)
                    })
                {
                    return Ok((Some(thinking), None));
                }
                return Ok((None, None));
            }
            AnthropicThinkingModeOverride::Adaptive => {
                return Ok((Some(ThinkingConfig::Adaptive { display }), None));
            }
            AnthropicThinkingModeOverride::ManualBudget(budget) => {
                // Profiled models without manual-budget support (every
                // current Claude 5.x model) reject `budget_tokens` with a
                // 400, so an explicit budget is served as adaptive thinking.
                if profile.is_some_and(|p| !p.supports_manual_budget) {
                    return Ok((Some(ThinkingConfig::Adaptive { display }), None));
                }
                return Ok((manual_thinking_config(budget, thinking_max_tokens, display), None));
            }
            AnthropicThinkingModeOverride::Inherit => {}
        }
    }

    let thinking_enabled = if default_thinking {
        if !anthropic_config.extended_thinking_enabled {
            // Omitting the field would run the model's own default, which is
            // adaptive thinking, so an opt-out has to name the model's lowest
            // setting explicitly. Adaptive-only models have none: the field
            // stays absent and they keep thinking regardless.
            if let Some(off) = profile.and_then(|p| {
                lowest_thinking_config(&p, resolved_model, || effort_is_at_most_high(request, anthropic_config))
            }) {
                return Ok((Some(off), None));
            }
            tracing::warn!(
                model = %request.model,
                "extended_thinking_enabled=false overridden by model default thinking profile; thinking will be enabled"
            );
        }
        true
    } else {
        anthropic_config.extended_thinking_enabled && supports_reasoning_effort(resolved_model, default_model)
    };

    if thinking_enabled {
        if profile.is_some_and(|p| matches!(p.mode, super::super::capabilities::ClaudeThinkingMode::Adaptive)) {
            if profile.is_some_and(|p| p.supports_manual_budget)
                && let Some(explicit_budget) = request.thinking_budget
            {
                return Ok((manual_thinking_config(explicit_budget, thinking_max_tokens, display), None));
            }
            return Ok((Some(ThinkingConfig::Adaptive { display }), None));
        }

        let max_thinking_tokens: Option<u32> =
            env::var(env_vars::MAX_THINKING_TOKENS).ok().and_then(|v| v.parse().ok());

        let budget = if let Some(explicit_budget) = request.thinking_budget {
            explicit_budget
        } else if let Some(env_budget) = max_thinking_tokens {
            env_budget
        } else if let Some(effort) = request.reasoning_effort {
            match effort {
                ReasoningEffortLevel::None | ReasoningEffortLevel::Unknown => 0,
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

        if let Some(thinking) = manual_thinking_config(budget, thinking_max_tokens, display) {
            return Ok((Some(thinking), None));
        }
    } else if let Some(effort) = request.reasoning_effort {
        if profile.is_some_and(|p| matches!(p.mode, super::super::capabilities::ClaudeThinkingMode::Adaptive)) {
            return Ok((None, None));
        }

        if let Some(payload) =
            RigProviderCapabilities::new(Provider::Anthropic, &request.model).reasoning_parameters(effort)?
        {
            return Ok((None, Some(payload)));
        } else {
            return Ok((None, Some(json!({ "effort": effort.as_str() }))));
        }
    }

    Ok((None, None))
}

#[cfg(test)]
mod tests {
    use super::*;
    use vtcode_config::constants::models::anthropic;

    #[test]
    fn sonnet_5_5_disabled_thinking_override_becomes_between_tools() {
        let request = LLMRequest {
            model: anthropic::CLAUDE_SONNET_5_5.to_string(),
            anthropic_request_overrides: Some(crate::provider::AnthropicRequestOverrides {
                thinking_mode: AnthropicThinkingModeOverride::Disabled,
                ..Default::default()
            }),
            ..Default::default()
        };
        let config = AnthropicConfig::default();
        let (thinking, reasoning) =
            build_thinking_config(&request, &config, anthropic::DEFAULT_MODEL).expect("thinking config");

        // `disabled` is a 400 on Sonnet 5.5; `between_tools` is its lowest setting.
        assert!(matches!(thinking, Some(ThinkingConfig::BetweenTools)), "got {thinking:?}");
        assert!(reasoning.is_none());
    }

    #[test]
    fn sonnet_5_5_falls_back_to_adaptive_above_high_effort() {
        // `between_tools` is rejected at `xhigh`/`max`, so the config must be
        // omitted and let the model run its own adaptive default.
        let request = LLMRequest {
            model: anthropic::CLAUDE_SONNET_5_5.to_string(),
            effort: Some("xhigh".to_string()),
            anthropic_request_overrides: Some(crate::provider::AnthropicRequestOverrides {
                thinking_mode: AnthropicThinkingModeOverride::Disabled,
                ..Default::default()
            }),
            ..Default::default()
        };
        let config = AnthropicConfig::default();
        let (thinking, _) =
            build_thinking_config(&request, &config, anthropic::DEFAULT_MODEL).expect("thinking config");

        assert!(thinking.is_none(), "got {thinking:?}");
    }

    #[test]
    fn extended_thinking_disabled_uses_the_model_lowest_setting() {
        let config = AnthropicConfig {
            extended_thinking_enabled: false,
            ..AnthropicConfig::default()
        };
        for (model, expected) in [
            (anthropic::CLAUDE_SONNET_5_5, "between_tools"),
            (anthropic::CLAUDE_SONNET_5, "disabled"),
            (anthropic::CLAUDE_OPUS_5, "disabled"),
            // Adaptive-only models have no "thinking off" setting.
            (anthropic::CLAUDE_OPUS_5_5, "adaptive"),
            (anthropic::CLAUDE_FABLE_5_1, "adaptive"),
        ] {
            let request = LLMRequest { model: model.to_string(), ..Default::default() };
            let (thinking, _) = build_thinking_config(&request, &config, anthropic::DEFAULT_MODEL).expect("config");
            let serialized = thinking
                .as_ref()
                .map(|thinking| serde_json::to_value(thinking).expect("serialize"))
                .and_then(|value| value.get("type").cloned())
                .unwrap_or_else(|| json!("omitted"));
            assert_eq!(serialized, json!(expected), "{model}");
        }
    }

    #[test]
    fn between_tools_serializes_without_extra_fields() {
        let thinking = ThinkingConfig::BetweenTools;
        let value = serde_json::to_value(thinking).expect("serialize thinking");
        assert_eq!(value, json!({ "type": "between_tools" }));
    }

    #[test]
    fn disabled_thinking_is_rewritten_to_between_tools_for_sonnet_5_5() {
        let rewritten =
            rewrite_thinking_for_model(&ThinkingConfig::Disabled, anthropic::CLAUDE_SONNET_5_5, "", Some("medium"));
        assert!(matches!(rewritten, Some(ThinkingConfig::BetweenTools)), "got {rewritten:?}");

        // Above `high` effort Sonnet 5.5 falls back to adaptive thinking.
        let rewritten =
            rewrite_thinking_for_model(&ThinkingConfig::Disabled, anthropic::CLAUDE_SONNET_5_5, "", Some("xhigh"));
        assert!(matches!(rewritten, Some(ThinkingConfig::Adaptive { display: None })), "got {rewritten:?}");
    }

    #[test]
    fn between_tools_is_rewritten_to_disabled_for_sonnet_5() {
        let rewritten =
            rewrite_thinking_for_model(&ThinkingConfig::BetweenTools, anthropic::CLAUDE_SONNET_5, "", Some("medium"));
        assert!(matches!(rewritten, Some(ThinkingConfig::Disabled)), "got {rewritten:?}");

        // Already valid for the model that produced it.
        let rewritten =
            rewrite_thinking_for_model(&ThinkingConfig::BetweenTools, anthropic::CLAUDE_SONNET_5_5, "", Some("low"));
        assert!(rewritten.is_none(), "got {rewritten:?}");
        let rewritten =
            rewrite_thinking_for_model(&ThinkingConfig::Disabled, anthropic::CLAUDE_SONNET_5, "", Some("low"));
        assert!(rewritten.is_none(), "got {rewritten:?}");
    }

    #[test]
    fn sonnet_5_5_defaults_to_updates_display_like_opus_5_5() {
        let request = LLMRequest {
            model: anthropic::CLAUDE_SONNET_5_5.to_string(),
            ..Default::default()
        };
        let config = AnthropicConfig::default();
        let (thinking, _) =
            build_thinking_config(&request, &config, anthropic::DEFAULT_MODEL).expect("thinking config");

        match thinking {
            Some(ThinkingConfig::Adaptive { display: Some(ThinkingDisplay::Updates) }) => {}
            other => panic!("expected Adaptive with Updates display, got {other:?}"),
        }
    }

    #[test]
    fn ignores_explicit_budget_for_opus_5() {
        let request = LLMRequest {
            model: anthropic::CLAUDE_OPUS_5.to_string(),
            thinking_budget: Some(2048),
            ..Default::default()
        };
        let config = AnthropicConfig::default();
        let (thinking, _) =
            build_thinking_config(&request, &config, anthropic::DEFAULT_MODEL).expect("thinking config");

        assert!(matches!(thinking, Some(ThinkingConfig::Adaptive { .. })));
    }

    fn manual_budget_override_request(model: &str, budget: u32) -> LLMRequest {
        LLMRequest {
            model: model.to_string(),
            anthropic_request_overrides: Some(crate::provider::AnthropicRequestOverrides {
                thinking_mode: AnthropicThinkingModeOverride::ManualBudget(budget),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn manual_budget_override_becomes_adaptive_for_opus_5_5() {
        let request = manual_budget_override_request(anthropic::CLAUDE_OPUS_5_5, 4096);
        let config = AnthropicConfig::default();
        let (thinking, reasoning) =
            build_thinking_config(&request, &config, anthropic::DEFAULT_MODEL).expect("thinking config");

        assert!(matches!(thinking, Some(ThinkingConfig::Adaptive { display: None })), "got {thinking:?}");
        assert!(reasoning.is_none());
    }

    #[test]
    fn manual_budget_override_becomes_adaptive_for_every_profiled_model() {
        let config = AnthropicConfig::default();
        for model in [
            anthropic::CLAUDE_SONNET_5_5,
            anthropic::CLAUDE_OPUS_5,
            anthropic::CLAUDE_OPUS_5_5,
            anthropic::CLAUDE_FABLE_5,
            anthropic::CLAUDE_FABLE_5_1,
        ] {
            let request = manual_budget_override_request(model, 8192);
            let (thinking, _) =
                build_thinking_config(&request, &config, anthropic::DEFAULT_MODEL).expect("thinking config");
            assert!(
                matches!(thinking, Some(ThinkingConfig::Adaptive { .. })),
                "{model} must not receive budget_tokens, got {thinking:?}"
            );
        }
    }

    #[test]
    fn manual_budget_override_keeps_display_when_downgraded_to_adaptive() {
        let mut request = manual_budget_override_request(anthropic::CLAUDE_OPUS_5_5, 4096);
        if let Some(overrides) = request.anthropic_request_overrides.as_mut() {
            overrides.thinking_display = AnthropicThinkingDisplayOverride::Summarized;
        }
        let config = AnthropicConfig::default();
        let (thinking, _) =
            build_thinking_config(&request, &config, anthropic::DEFAULT_MODEL).expect("thinking config");

        assert!(
            matches!(thinking, Some(ThinkingConfig::Adaptive { display: Some(ThinkingDisplay::Summarized) })),
            "got {thinking:?}"
        );
    }

    #[test]
    fn manual_budget_override_is_kept_for_unprofiled_models() {
        let request = manual_budget_override_request("claude-unlisted-model", 4096);
        let config = AnthropicConfig::default();
        let (thinking, _) =
            build_thinking_config(&request, &config, anthropic::DEFAULT_MODEL).expect("thinking config");

        assert!(matches!(thinking, Some(ThinkingConfig::Enabled { budget_tokens: 4096, .. })), "got {thinking:?}");
    }

    #[test]
    fn adaptive_thinking_includes_summarized_display() {
        let request = LLMRequest {
            model: anthropic::CLAUDE_SONNET_5.to_string(),
            ..Default::default()
        };
        let config = AnthropicConfig {
            thinking_display: Some(vtcode_config::ThinkingDisplayMode::Summarized),
            ..AnthropicConfig::default()
        };
        let (thinking, _) =
            build_thinking_config(&request, &config, anthropic::DEFAULT_MODEL).expect("thinking config");

        match thinking {
            Some(ThinkingConfig::Adaptive { display: Some(ThinkingDisplay::Summarized) }) => {}
            other => panic!("expected Adaptive with Summarized display, got {other:?}"),
        }
    }

    #[test]
    fn adaptive_thinking_includes_omitted_display() {
        let request = LLMRequest {
            model: anthropic::CLAUDE_SONNET_5.to_string(),
            ..Default::default()
        };
        let config = AnthropicConfig {
            thinking_display: Some(vtcode_config::ThinkingDisplayMode::Omitted),
            ..AnthropicConfig::default()
        };
        let (thinking, _) =
            build_thinking_config(&request, &config, anthropic::DEFAULT_MODEL).expect("thinking config");

        match thinking {
            Some(ThinkingConfig::Adaptive { display: Some(ThinkingDisplay::Omitted) }) => {}
            other => panic!("expected Adaptive with Omitted display, got {other:?}"),
        }
    }

    #[test]
    fn adaptive_thinking_includes_display_for_sonnet_5_when_configured() {
        let request = LLMRequest {
            model: anthropic::CLAUDE_SONNET_5.to_string(),
            ..Default::default()
        };
        let config = AnthropicConfig {
            thinking_display: Some(vtcode_config::ThinkingDisplayMode::Summarized),
            ..AnthropicConfig::default()
        };
        let (thinking, _) =
            build_thinking_config(&request, &config, anthropic::DEFAULT_MODEL).expect("thinking config");

        match thinking {
            Some(ThinkingConfig::Adaptive { display: Some(ThinkingDisplay::Summarized) }) => {}
            other => panic!("expected Adaptive with Summarized display, got {other:?}"),
        }
    }

    #[test]
    fn opus_5_5_defaults_to_updates_display() {
        let request = LLMRequest {
            model: anthropic::CLAUDE_OPUS_5_5.to_string(),
            ..Default::default()
        };
        let config = AnthropicConfig::default();
        let (thinking, _) =
            build_thinking_config(&request, &config, anthropic::DEFAULT_MODEL).expect("thinking config");

        match thinking {
            Some(ThinkingConfig::Adaptive { display: Some(ThinkingDisplay::Updates) }) => {}
            other => panic!("expected Adaptive with Updates display, got {other:?}"),
        }
    }

    #[test]
    fn opus_5_5_respects_configured_display() {
        let request = LLMRequest {
            model: anthropic::CLAUDE_OPUS_5_5.to_string(),
            ..Default::default()
        };
        let config = AnthropicConfig {
            thinking_display: Some(vtcode_config::ThinkingDisplayMode::Omitted),
            ..AnthropicConfig::default()
        };
        let (thinking, _) =
            build_thinking_config(&request, &config, anthropic::DEFAULT_MODEL).expect("thinking config");

        match thinking {
            Some(ThinkingConfig::Adaptive { display: Some(ThinkingDisplay::Omitted) }) => {}
            other => panic!("expected Adaptive with Omitted display, got {other:?}"),
        }
    }

    #[test]
    fn configured_updates_display_applies_on_fable_5() {
        let request = LLMRequest {
            model: anthropic::CLAUDE_FABLE_5.to_string(),
            ..Default::default()
        };
        let config = AnthropicConfig {
            thinking_display: Some(vtcode_config::ThinkingDisplayMode::Updates),
            ..AnthropicConfig::default()
        };
        let (thinking, _) =
            build_thinking_config(&request, &config, anthropic::DEFAULT_MODEL).expect("thinking config");

        match thinking {
            Some(ThinkingConfig::Adaptive { display: Some(ThinkingDisplay::Updates) }) => {}
            other => panic!("expected Adaptive with Updates display, got {other:?}"),
        }
    }

    #[test]
    fn configured_updates_display_falls_back_on_unsupported_models() {
        let config = AnthropicConfig {
            thinking_display: Some(vtcode_config::ThinkingDisplayMode::Updates),
            ..AnthropicConfig::default()
        };
        for model in [anthropic::CLAUDE_SONNET_5, anthropic::CLAUDE_OPUS_5] {
            let request = LLMRequest { model: model.to_string(), ..Default::default() };
            let (thinking, _) =
                build_thinking_config(&request, &config, anthropic::DEFAULT_MODEL).expect("thinking config");

            match thinking {
                Some(ThinkingConfig::Adaptive { display: None }) => {}
                other => panic!("{model}: expected Adaptive with no display, got {other:?}"),
            }
        }
    }

    fn display_override_request(model: &str, display: AnthropicThinkingDisplayOverride) -> LLMRequest {
        LLMRequest {
            model: model.to_string(),
            anthropic_request_overrides: Some(crate::provider::AnthropicRequestOverrides {
                thinking_mode: AnthropicThinkingModeOverride::Adaptive,
                thinking_display: display,
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn requested_updates_display_is_kept_on_supporting_models() {
        let config = AnthropicConfig::default();
        for model in [
            anthropic::CLAUDE_OPUS_5_5,
            anthropic::CLAUDE_SONNET_5_5,
            anthropic::CLAUDE_FABLE_5,
            anthropic::CLAUDE_FABLE_5_1,
        ] {
            let request = display_override_request(model, AnthropicThinkingDisplayOverride::Updates);
            let (thinking, _) =
                build_thinking_config(&request, &config, anthropic::DEFAULT_MODEL).expect("thinking config");

            match thinking {
                Some(ThinkingConfig::Adaptive { display: Some(ThinkingDisplay::Updates) }) => {}
                other => panic!("{model}: expected Adaptive with Updates display, got {other:?}"),
            }
        }
    }

    #[test]
    fn requested_updates_display_is_omitted_on_unsupported_models() {
        let config = AnthropicConfig::default();
        for model in [
            anthropic::CLAUDE_SONNET_5,
            anthropic::CLAUDE_OPUS_5,
            "claude-opus-4-8",
            vtcode_config::constants::models::minimax::MINIMAX_M3,
        ] {
            let request = display_override_request(model, AnthropicThinkingDisplayOverride::Updates);
            let (thinking, _) =
                build_thinking_config(&request, &config, anthropic::DEFAULT_MODEL).expect("thinking config");

            match thinking {
                Some(ThinkingConfig::Adaptive { display: None }) => {}
                other => panic!("{model}: expected Adaptive with no display, got {other:?}"),
            }
        }
    }

    #[test]
    fn requested_summarized_display_is_kept_on_every_model() {
        let config = AnthropicConfig::default();
        for model in [
            anthropic::CLAUDE_OPUS_5_5,
            anthropic::CLAUDE_SONNET_5,
            "claude-opus-4-8",
        ] {
            let request = display_override_request(model, AnthropicThinkingDisplayOverride::Summarized);
            let (thinking, _) =
                build_thinking_config(&request, &config, anthropic::DEFAULT_MODEL).expect("thinking config");

            match thinking {
                Some(ThinkingConfig::Adaptive { display: Some(ThinkingDisplay::Summarized) }) => {}
                other => panic!("{model}: expected Adaptive with Summarized display, got {other:?}"),
            }
        }
    }

    #[test]
    fn updates_display_serializes_as_updates() {
        let thinking = ThinkingConfig::Adaptive { display: Some(ThinkingDisplay::Updates) };
        let value = serde_json::to_value(thinking).expect("serialize thinking");
        assert_eq!(value, json!({ "type": "adaptive", "display": "updates" }));
    }

    #[test]
    fn thinking_display_defaults_to_none() {
        let request = LLMRequest {
            model: anthropic::CLAUDE_SONNET_5.to_string(),
            ..Default::default()
        };
        let config = AnthropicConfig::default();
        let (thinking, _) =
            build_thinking_config(&request, &config, anthropic::DEFAULT_MODEL).expect("thinking config");

        match thinking {
            Some(ThinkingConfig::Adaptive { display: None }) => {}
            other => panic!("expected Adaptive with no display, got {other:?}"),
        }
    }
}
