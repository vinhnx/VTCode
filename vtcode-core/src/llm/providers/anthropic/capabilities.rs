//! Model capability detection for Anthropic Claude models
//!
//! Provides methods to determine what features each Claude model supports:
//! - Reasoning/extended thinking
//! - Vision (image inputs)
//! - Structured outputs
//! - Parallel tool configuration
//! - Context window sizes

use crate::config::constants::models;
use crate::llm::providers::anthropic_types::ThinkingDisplay;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClaudeThinkingMode {
    ManualBudget,
    Adaptive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ClaudeThinkingProfile {
    pub mode: ClaudeThinkingMode,
    pub adaptive_only: bool,
    pub default_thinking_enabled: bool,
    pub manual_interleaved_beta: bool,
    pub supports_effort: bool,
    pub supports_task_budget: bool,
    pub default_display: ThinkingDisplay,
    pub default_effort: &'static str,
    pub supports_xhigh_effort: bool,
    pub supports_max_effort: bool,
}

pub(crate) fn resolve_model_name<'a>(model: &'a str, default_model: &'a str) -> &'a str {
    if model.trim().is_empty() {
        default_model
    } else {
        model
    }
}

fn matches_model(model: &str, candidate: &str) -> bool {
    model == candidate || model.contains(candidate)
}

pub(crate) fn claude_thinking_profile(
    model: &str,
    default_model: &str,
) -> Option<ClaudeThinkingProfile> {
    let requested = resolve_model_name(model, default_model);

    if matches_model(requested, models::anthropic::CLAUDE_MYTHOS_PREVIEW) {
        return Some(ClaudeThinkingProfile {
            mode: ClaudeThinkingMode::Adaptive,
            adaptive_only: true,
            default_thinking_enabled: true,
            manual_interleaved_beta: false,
            supports_effort: true,
            supports_task_budget: false,
            default_display: ThinkingDisplay::Omitted,
            default_effort: "high",
            supports_xhigh_effort: false,
            supports_max_effort: true,
        });
    }

    if matches_model(requested, models::anthropic::CLAUDE_OPUS_4_7) {
        return Some(ClaudeThinkingProfile {
            mode: ClaudeThinkingMode::Adaptive,
            adaptive_only: true,
            default_thinking_enabled: false,
            manual_interleaved_beta: false,
            supports_effort: true,
            supports_task_budget: true,
            default_display: ThinkingDisplay::Omitted,
            default_effort: "high",
            supports_xhigh_effort: true,
            supports_max_effort: true,
        });
    }

    if matches_model(requested, models::anthropic::CLAUDE_OPUS_4_6) {
        return Some(ClaudeThinkingProfile {
            mode: ClaudeThinkingMode::ManualBudget,
            adaptive_only: false,
            default_thinking_enabled: false,
            manual_interleaved_beta: false,
            supports_effort: false,
            supports_task_budget: false,
            default_display: ThinkingDisplay::Summarized,
            default_effort: "high",
            supports_xhigh_effort: false,
            supports_max_effort: false,
        });
    }

    if matches_model(requested, models::anthropic::CLAUDE_SONNET_4_6) {
        return Some(ClaudeThinkingProfile {
            mode: ClaudeThinkingMode::ManualBudget,
            adaptive_only: false,
            default_thinking_enabled: false,
            manual_interleaved_beta: true,
            supports_effort: false,
            supports_task_budget: false,
            default_display: ThinkingDisplay::Summarized,
            default_effort: "high",
            supports_xhigh_effort: false,
            supports_max_effort: false,
        });
    }

    if matches_model(requested, models::anthropic::CLAUDE_HAIKU_4_5) {
        return Some(ClaudeThinkingProfile {
            mode: ClaudeThinkingMode::ManualBudget,
            adaptive_only: false,
            default_thinking_enabled: false,
            manual_interleaved_beta: true,
            supports_effort: false,
            supports_task_budget: false,
            default_display: ThinkingDisplay::Summarized,
            default_effort: "high",
            supports_xhigh_effort: false,
            supports_max_effort: false,
        });
    }

    None
}

fn supports_native_1m_context(model: &str) -> bool {
    matches_model(model, models::anthropic::CLAUDE_SONNET_4_6)
        || matches_model(model, models::anthropic::CLAUDE_OPUS_4_6)
        || matches_model(model, models::anthropic::CLAUDE_OPUS_4_7)
        || matches_model(model, models::anthropic::CLAUDE_MYTHOS_PREVIEW)
}

pub fn supports_reasoning(model: &str, default_model: &str) -> bool {
    let requested = resolve_model_name(model, default_model);
    if claude_thinking_profile(requested, default_model).is_some() {
        return true;
    }

    models::minimax::SUPPORTED_MODELS.contains(&requested)
}

pub fn supports_reasoning_effort(model: &str, default_model: &str) -> bool {
    let requested = resolve_model_name(model, default_model);

    if claude_thinking_profile(requested, default_model).is_some() {
        return true;
    }

    if models::minimax::SUPPORTED_MODELS.contains(&requested) {
        return true;
    }

    models::anthropic::REASONING_MODELS.contains(&requested)
}

pub fn supports_effort(model: &str, default_model: &str) -> bool {
    claude_thinking_profile(model, default_model).is_some_and(|profile| profile.supports_effort)
}

pub fn supports_task_budget(model: &str, default_model: &str) -> bool {
    claude_thinking_profile(model, default_model)
        .is_some_and(|profile| profile.supports_task_budget)
}

pub(crate) fn supports_manual_thinking_budget(model: &str, default_model: &str) -> bool {
    claude_thinking_profile(model, default_model)
        .is_some_and(|profile| matches!(profile.mode, ClaudeThinkingMode::ManualBudget))
}

pub(crate) fn supports_adaptive_thinking(model: &str, default_model: &str) -> bool {
    claude_thinking_profile(model, default_model)
        .is_some_and(|profile| matches!(profile.mode, ClaudeThinkingMode::Adaptive))
}

pub(crate) fn supports_manual_interleaved_beta(model: &str, default_model: &str) -> bool {
    claude_thinking_profile(model, default_model)
        .is_some_and(|profile| profile.manual_interleaved_beta)
}

pub(crate) fn adaptive_thinking_is_default(model: &str, default_model: &str) -> bool {
    claude_thinking_profile(model, default_model)
        .is_some_and(|profile| profile.default_thinking_enabled)
}

pub(crate) fn default_effort_for_model(model: &str, default_model: &str) -> Option<&'static str> {
    claude_thinking_profile(model, default_model)
        .filter(|profile| profile.supports_effort)
        .map(|profile| profile.default_effort)
}

pub(crate) fn allowed_efforts_for_model(
    model: &str,
    default_model: &str,
) -> Option<&'static [&'static str]> {
    let profile = claude_thinking_profile(model, default_model)?;
    if !profile.supports_effort {
        return None;
    }

    if profile.supports_xhigh_effort {
        Some(&["low", "medium", "high", "xhigh", "max"])
    } else if profile.supports_max_effort {
        Some(&["low", "medium", "high", "max"])
    } else {
        Some(&["low", "medium", "high"])
    }
}

pub(crate) fn effort_allowed_for_model(model: &str, default_model: &str, effort: &str) -> bool {
    let normalized = effort.trim().to_ascii_lowercase();
    allowed_efforts_for_model(model, default_model)
        .is_some_and(|allowed| allowed.contains(&normalized.as_str()))
}

pub fn supports_parallel_tool_config(_model: &str) -> bool {
    true
}

pub fn effective_context_size(model: &str) -> usize {
    if supports_native_1m_context(model) {
        1_000_000
    } else {
        200_000
    }
}

pub fn supports_structured_output(model: &str, default_model: &str) -> bool {
    let requested = resolve_model_name(model, default_model);

    if claude_thinking_profile(requested, default_model).is_some() {
        return true;
    }

    requested.contains("claude-sonnet-4-5")
        || requested.contains("claude-opus-4-5")
        || requested.contains("claude-haiku-4-5")
}

pub fn supports_vision(model: &str, default_model: &str) -> bool {
    let requested = resolve_model_name(model, default_model);

    if claude_thinking_profile(requested, default_model).is_some() {
        return true;
    }

    requested.contains("claude-3")
        || requested.contains("claude-4-sonnet")
        || requested.contains("claude-4-6")
        || requested.contains("claude-4-7")
        || requested == models::anthropic::CLAUDE_HAIKU_4_5
        || requested == models::anthropic::CLAUDE_HAIKU_4_5_20251001
}

#[allow(dead_code)]
pub fn is_claude_model(model: &str, default_model: &str) -> bool {
    claude_thinking_profile(model, default_model).is_some()
}

pub fn supported_models() -> Vec<String> {
    let mut supported: Vec<String> = models::anthropic::SUPPORTED_MODELS
        .iter()
        .map(|s| s.to_string())
        .collect();

    supported.extend(
        models::minimax::SUPPORTED_MODELS
            .iter()
            .map(|s| s.to_string()),
    );

    supported.sort();
    supported.dedup();
    supported
}
