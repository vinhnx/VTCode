//! Prompt caching configuration for Anthropic Claude API
//!
//! Implements Anthropic's prompt caching feature with configurable TTLs:
//! - "5m" (5 minutes) for dynamic content like messages
//! - "1h" (1 hour) for stable content like tools and system prompts

use crate::config::core::AnthropicPromptCacheSettings;

pub fn get_cache_ttl_for_seconds(ttl_seconds: u64) -> &'static str {
    if ttl_seconds >= 3600 {
        "1h"
    } else {
        "5m"
    }
}

pub fn get_tools_cache_ttl(settings: &AnthropicPromptCacheSettings) -> &'static str {
    get_cache_ttl_for_seconds(settings.tools_ttl_seconds)
}

pub fn get_messages_cache_ttl(settings: &AnthropicPromptCacheSettings) -> &'static str {
    get_cache_ttl_for_seconds(settings.messages_ttl_seconds)
}

pub fn requires_extended_ttl_beta(settings: &AnthropicPromptCacheSettings) -> bool {
    settings.tools_ttl_seconds >= 3600 || settings.messages_ttl_seconds >= 3600
}
