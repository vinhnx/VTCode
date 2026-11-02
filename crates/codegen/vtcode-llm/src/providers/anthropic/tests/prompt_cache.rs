use crate::provider::PromptCacheProfile;
use crate::providers::anthropic::prompt_cache::*;
use vtcode_config::core::AnthropicPromptCacheSettings;

#[test]
fn test_cache_ttl_for_seconds() {
    assert_eq!(get_cache_ttl_for_seconds(300), "5m");
    assert_eq!(get_cache_ttl_for_seconds(3600), "1h");
    assert_eq!(get_cache_ttl_for_seconds(7200), "1h");
}

#[test]
fn test_requires_extended_ttl_beta() {
    let settings = AnthropicPromptCacheSettings {
        tools_ttl_seconds: 3600,
        messages_ttl_seconds: 300,
        ..Default::default()
    };
    assert!(requires_extended_ttl_beta(&settings, None));

    let settings = AnthropicPromptCacheSettings {
        tools_ttl_seconds: 300,
        messages_ttl_seconds: 300,
        ..Default::default()
    };
    assert!(!requires_extended_ttl_beta(&settings, None));

    // A budget-continuation profile promotes the messages breakpoint to the
    // profile TTL (1h by default), so the beta must be requested even with
    // 5m tools/messages settings.
    assert_eq!(messages_cache_ttl_for_profile(&settings, Some(PromptCacheProfile::BudgetContinuation)), "1h");
    assert!(requires_extended_ttl_beta(&settings, Some(PromptCacheProfile::BudgetContinuation)));

    // The profile TTL itself is honored when explicitly shortened.
    let short_profile = AnthropicPromptCacheSettings {
        tools_ttl_seconds: 300,
        messages_ttl_seconds: 300,
        extended_ttl_seconds: Some(300),
        ..Default::default()
    };
    assert_eq!(messages_cache_ttl_for_profile(&short_profile, Some(PromptCacheProfile::BudgetContinuation)), "5m");
    assert!(!requires_extended_ttl_beta(&short_profile, Some(PromptCacheProfile::BudgetContinuation)));
}
