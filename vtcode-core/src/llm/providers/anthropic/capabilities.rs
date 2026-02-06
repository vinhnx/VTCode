//! Model capability detection for Anthropic Claude models
//!
//! Provides methods to determine what features each Claude model supports:
//! - Reasoning/extended thinking
//! - Vision (image inputs)
//! - Structured outputs
//! - Parallel tool configuration
//! - Context window sizes

use crate::config::constants::models;

pub fn supports_reasoning(model: &str, default_model: &str) -> bool {
    let requested = if model.trim().is_empty() {
        default_model
    } else {
        model
    };
    models::minimax::SUPPORTED_MODELS.contains(&requested)
}

pub fn supports_reasoning_effort(model: &str, default_model: &str) -> bool {
    let requested = if model.trim().is_empty() {
        default_model
    } else {
        model
    };

    if models::minimax::SUPPORTED_MODELS.contains(&requested) {
        return true;
    }

    models::anthropic::REASONING_MODELS.contains(&requested)
}

pub fn supports_parallel_tool_config(_model: &str) -> bool {
    true
}

pub fn effective_context_size(model: &str) -> usize {
    match model {
        m if m.contains("claude-sonnet-4-5")
            || m.contains("claude-sonnet-4")
            || m.contains("claude-opus-4-5")
            || m.contains("claude-opus-4")
            || m.contains("claude-haiku-4-5") =>
        {
            1_000_000
        }
        _ => 200_000,
    }
}

pub fn supports_structured_output(model: &str, default_model: &str) -> bool {
    let requested = if model.trim().is_empty() {
        default_model
    } else {
        model
    };

    requested == models::anthropic::CLAUDE_SONNET_4_5
        || requested == models::anthropic::CLAUDE_SONNET_4_5_20250929
        || requested == models::anthropic::CLAUDE_OPUS_4_6
        || requested == models::anthropic::CLAUDE_OPUS_4_5
        || requested == models::anthropic::CLAUDE_OPUS_4_5_20251101
        || requested == models::anthropic::CLAUDE_OPUS_4_6
        || requested == models::anthropic::CLAUDE_OPUS_4_1
        || requested == models::anthropic::CLAUDE_OPUS_4_1_20250805
        || requested == models::anthropic::CLAUDE_HAIKU_4_5
        || requested == models::anthropic::CLAUDE_HAIKU_4_5_20251001
        || requested == models::anthropic::CLAUDE_SONNET_4_0
        || requested == models::anthropic::CLAUDE_SONNET_4_20250514
        || requested == models::anthropic::CLAUDE_OPUS_4_0
        || requested == models::anthropic::CLAUDE_OPUS_4_20250514
        || requested.contains("claude-3-7-sonnet")
        || requested.contains("claude-haiku-4-5")
}

pub fn supports_vision(model: &str, default_model: &str) -> bool {
    let requested = if model.trim().is_empty() {
        default_model
    } else {
        model
    };

    requested.contains("claude-3")
        || requested.contains("claude-4")
        || requested == models::anthropic::CLAUDE_SONNET_4_5
        || requested == models::anthropic::CLAUDE_SONNET_4_5_20250929
        || requested == models::anthropic::CLAUDE_OPUS_4_5
        || requested == models::anthropic::CLAUDE_OPUS_4_5_20251101
        || requested == models::anthropic::CLAUDE_HAIKU_4_5
        || requested == models::anthropic::CLAUDE_HAIKU_4_5_20251001
        || requested == models::anthropic::CLAUDE_SONNET_4_0
        || requested == models::anthropic::CLAUDE_SONNET_4_20250514
        || requested == models::anthropic::CLAUDE_OPUS_4_0
        || requested == models::anthropic::CLAUDE_OPUS_4_20250514
}

#[allow(dead_code)]
pub fn is_claude_model(model: &str, default_model: &str) -> bool {
    let requested = if model.trim().is_empty() {
        default_model
    } else {
        model
    };
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
