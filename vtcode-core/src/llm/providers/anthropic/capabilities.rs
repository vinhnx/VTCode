//! Model capability detection for Anthropic Claude models
//!
//! Provides methods to determine what features each Claude model supports:
//! - Reasoning/extended thinking
//! - Vision (image inputs)
//! - Structured outputs
//! - Parallel tool configuration
//! - Context window sizes

use crate::config::constants::models;

pub(crate) fn resolve_model_name<'a>(model: &'a str, default_model: &'a str) -> &'a str {
    if model.trim().is_empty() {
        default_model
    } else {
        model
    }
}

fn is_claude_opus_47(model: &str) -> bool {
    model == models::anthropic::CLAUDE_OPUS_4_7 || model.contains("claude-opus-4-7")
}

fn supports_native_1m_context(model: &str) -> bool {
    model.contains("claude-sonnet-4-6") || is_claude_opus_47(model)
}

pub fn supports_reasoning(model: &str, default_model: &str) -> bool {
    let requested = resolve_model_name(model, default_model);
    models::minimax::SUPPORTED_MODELS.contains(&requested)
}

pub fn supports_reasoning_effort(model: &str, default_model: &str) -> bool {
    let requested = resolve_model_name(model, default_model);

    if models::minimax::SUPPORTED_MODELS.contains(&requested) {
        return true;
    }

    models::anthropic::REASONING_MODELS.contains(&requested)
}

pub fn supports_effort(model: &str, default_model: &str) -> bool {
    let requested = resolve_model_name(model, default_model);

    is_claude_opus_47(requested)
}

pub fn supports_task_budget(model: &str, default_model: &str) -> bool {
    let requested = resolve_model_name(model, default_model);

    is_claude_opus_47(requested)
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

    requested.contains("claude-sonnet-4-6")
        || requested.contains("claude-opus-4-7")
        || requested.contains("claude-sonnet-4-5")
        || requested.contains("claude-opus-4-5")
        || requested.contains("claude-haiku-4-5")
}

pub fn supports_vision(model: &str, default_model: &str) -> bool {
    let requested = resolve_model_name(model, default_model);

    requested.contains("claude-3")
        || requested.contains("claude-4-sonnet")
        || requested.contains("claude-4-6")
        || requested.contains("claude-4-7")
        || requested == models::anthropic::CLAUDE_SONNET_4_6
        || requested == models::anthropic::CLAUDE_OPUS_4_7
        || requested == models::anthropic::CLAUDE_HAIKU_4_5
        || requested == models::anthropic::CLAUDE_HAIKU_4_5_20251001
}

#[allow(dead_code)]
pub fn is_claude_model(model: &str, default_model: &str) -> bool {
    let requested = resolve_model_name(model, default_model);
    models::anthropic::SUPPORTED_MODELS.contains(&requested)
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
