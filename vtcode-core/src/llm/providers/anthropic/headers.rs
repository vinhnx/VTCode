//! HTTP headers and beta feature management for Anthropic API
//!
//! Manages:
//! - API version headers
//! - Beta feature headers (prompt caching, extended thinking, structured outputs)
//! - Authentication headers

use crate::config::constants::models;
use crate::config::core::{AnthropicConfig, AnthropicPromptCacheSettings};

use super::prompt_cache::requires_extended_ttl_beta;

/// Configuration for beta header generation
pub struct BetaHeaderConfig<'a> {
    pub config: &'a AnthropicConfig,
    pub model: &'a str,
    pub include_advanced_tool_use: bool,
    pub request_betas: Option<&'a Vec<String>>,
    pub include_effort: bool,
    pub include_task_budget: bool,
}

pub fn prompt_cache_beta_header_value(
    cache_enabled: bool,
    settings: &AnthropicPromptCacheSettings,
) -> Option<String> {
    if !cache_enabled {
        return None;
    }

    let mut betas = vec!["prompt-caching-2024-07-31"];

    if requires_extended_ttl_beta(settings) {
        betas.push("extended-cache-ttl-2025-04-11");
    }

    Some(betas.join(", "))
}

pub fn combined_beta_header_value(
    cache_enabled: bool,
    settings: &AnthropicPromptCacheSettings,
    config: &BetaHeaderConfig,
) -> Option<String> {
    let mut pieces: Vec<String> = Vec::new();

    if let Some(pc) = prompt_cache_beta_header_value(cache_enabled, settings) {
        for p in pc
            .split(',')
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
        {
            pieces.push(p);
        }
    }

    if config.config.extended_thinking_enabled && config.model != models::anthropic::CLAUDE_OPUS_4_7
    {
        pieces.push(config.config.interleaved_thinking_beta.clone());
    }

    if config.include_advanced_tool_use {
        pieces.push("advanced-tool-use-2025-11-20".to_owned());
    }

    if config.include_effort && config.model != models::anthropic::CLAUDE_OPUS_4_7 {
        pieces.push("effort-2025-11-24".to_owned());
    }

    if config.include_task_budget {
        pieces.push(config.config.task_budget_beta.clone());
    }

    pieces.push("output-64k-2025-02-19".to_owned());

    if let Some(betas) = config.request_betas {
        for b in betas {
            if !pieces.contains(b) {
                pieces.push(b.clone());
            }
        }
    }

    if pieces.is_empty() {
        None
    } else {
        Some(pieces.join(", "))
    }
}
