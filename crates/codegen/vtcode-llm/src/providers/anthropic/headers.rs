//! HTTP headers and beta feature management for Anthropic API
//!
//! Manages:
//! - API version headers
//! - Beta feature headers (extended cache TTL, interleaved thinking, server-side tools)
//! - Authentication headers

use vtcode_config::core::{AnthropicConfig, AnthropicPromptCacheSettings};

use super::capabilities::supports_manual_interleaved_beta;
use super::prompt_cache::requires_extended_ttl_beta;
use crate::provider::PromptCacheProfile;

const EXTENDED_CACHE_TTL_BETA: &str = "extended-cache-ttl-2025-04-11";
pub(crate) const MID_CONVERSATION_SYSTEM_CLEAR_AT_BETA: &str = "mid-conversation-system-clear-at-2026-08-21";
/// Required whenever a request sends `thinking.display: "updates"`.
pub(crate) const THINKING_DISPLAY_UPDATES_BETA: &str = "thinking-display-updates-2026-08-18";

/// Beta for the `fallbacks: "default"` keyword form.
pub(crate) const SERVER_SIDE_FALLBACK_DEFAULT_BETA: &str = "server-side-fallback-2026-07-01";
/// Beta for the explicit-list `fallbacks` form. The date is older than the
/// keyword form's on purpose; each header is rejected with the other form.
pub(crate) const SERVER_SIDE_FALLBACK_LIST_BETA: &str = "server-side-fallback-2026-06-01";
/// Beta that makes a refusal return a `fallback_credit_token` and lets a
/// client-side retry echo it.
pub(crate) const FALLBACK_CREDIT_BETA: &str = "fallback-credit-2026-07-01";

/// Which `fallbacks` form a request sends; each needs its own beta header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerSideFallbackForm {
    /// `fallbacks: "default"`.
    Default,
    /// `fallbacks: [{ "model": ... }, ...]`.
    List,
}

impl ServerSideFallbackForm {
    /// Detects the form from a serialized request's `fallbacks` value.
    pub(crate) fn of_request(anthropic_request: &serde_json::Value) -> Option<Self> {
        match anthropic_request.get("fallbacks")? {
            serde_json::Value::String(mode) if mode == "default" => Some(Self::Default),
            serde_json::Value::Array(entries) if !entries.is_empty() => Some(Self::List),
            _ => None,
        }
    }

    fn beta(self) -> &'static str {
        match self {
            Self::Default => SERVER_SIDE_FALLBACK_DEFAULT_BETA,
            Self::List => SERVER_SIDE_FALLBACK_LIST_BETA,
        }
    }
}

/// Configuration for beta header generation
pub struct BetaHeaderConfig<'a> {
    pub config: &'a AnthropicConfig,
    pub model: &'a str,
    pub include_advanced_tool_use: bool,
    pub include_manual_interleaved_beta: bool,
    pub request_betas: Option<&'a [String]>,
    pub include_task_budget: bool,
    /// The effective serialized request includes speed on a preview model.
    pub include_fast_mode: bool,
    pub server_side_fallback: Option<ServerSideFallbackForm>,
    pub include_fallback_credit: bool,
    pub include_mid_conversation_tool_changes: bool,
    pub include_mid_conversation_system_clear_at: bool,
    pub include_thinking_display_updates: bool,
    /// Request profile that can promote the messages breakpoint to 1h; the
    /// beta header decision must see it or a 1h breakpoint ships uncovered.
    pub prompt_cache_profile: Option<PromptCacheProfile>,
}

pub fn combined_beta_header_value(
    cache_enabled: bool,
    settings: &AnthropicPromptCacheSettings,
    config: &BetaHeaderConfig,
) -> Option<String> {
    let mut pieces: Vec<String> = Vec::new();

    // Prompt caching is GA and needs no beta; only the 1h TTL still does.
    if cache_enabled && requires_extended_ttl_beta(settings, config.prompt_cache_profile) {
        pieces.push(EXTENDED_CACHE_TTL_BETA.to_owned());
    }

    if config.include_manual_interleaved_beta && supports_manual_interleaved_beta(config.model, config.model) {
        pieces.push(config.config.interleaved_thinking_beta.clone());
    }

    if config.include_advanced_tool_use {
        pieces.push("advanced-tool-use-2025-11-20".to_owned());
    }

    if config.include_task_budget {
        pieces.push(config.config.task_budget_beta.clone());
    }

    if let Some(form) = config.server_side_fallback {
        pieces.push(form.beta().to_owned());
    }

    if config.include_fallback_credit {
        pieces.push(FALLBACK_CREDIT_BETA.to_owned());
    }

    if config.include_mid_conversation_tool_changes {
        pieces.push("mid-conversation-tool-changes-2026-07-01".to_owned());
    }

    if config.include_mid_conversation_system_clear_at {
        pieces.push(MID_CONVERSATION_SYSTEM_CLEAR_AT_BETA.to_owned());
    }

    if config.include_thinking_display_updates {
        pieces.push(THINKING_DISPLAY_UPDATES_BETA.to_owned());
    }

    if config.include_fast_mode {
        pieces.push("fast-mode-2026-02-01".to_owned());
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    fn beta_config(config: &AnthropicConfig) -> BetaHeaderConfig<'_> {
        BetaHeaderConfig {
            config,
            model: vtcode_config::constants::models::anthropic::DEFAULT_MODEL,
            include_advanced_tool_use: false,
            include_manual_interleaved_beta: false,
            request_betas: None,
            include_task_budget: false,
            include_fast_mode: false,
            server_side_fallback: None,
            include_fallback_credit: false,
            include_mid_conversation_tool_changes: false,
            include_mid_conversation_system_clear_at: false,
            include_thinking_display_updates: false,
            prompt_cache_profile: None,
        }
    }

    fn cache_settings(ttl_seconds: u64) -> AnthropicPromptCacheSettings {
        AnthropicPromptCacheSettings {
            tools_ttl_seconds: ttl_seconds,
            messages_ttl_seconds: ttl_seconds,
            ..Default::default()
        }
    }

    #[test]
    fn server_side_fallback_beta_matches_the_request_form() {
        let config = AnthropicConfig::default();
        let mut beta = beta_config(&config);

        beta.server_side_fallback = ServerSideFallbackForm::of_request(&serde_json::json!({ "fallbacks": "default" }));
        assert_eq!(
            combined_beta_header_value(false, &cache_settings(300), &beta).as_deref(),
            Some("server-side-fallback-2026-07-01")
        );

        beta.server_side_fallback =
            ServerSideFallbackForm::of_request(&serde_json::json!({ "fallbacks": [{ "model": "claude-opus-4-8" }] }));
        assert_eq!(
            combined_beta_header_value(false, &cache_settings(300), &beta).as_deref(),
            Some("server-side-fallback-2026-06-01")
        );

        assert_eq!(ServerSideFallbackForm::of_request(&serde_json::json!({ "fallbacks": [] })), None);
        assert_eq!(ServerSideFallbackForm::of_request(&serde_json::json!({ "model": "x" })), None);
    }

    #[test]
    fn fallback_credit_retry_uses_current_credit_beta() {
        let config = AnthropicConfig::default();
        let mut beta = beta_config(&config);
        beta.include_fallback_credit = true;
        assert_eq!(
            combined_beta_header_value(false, &cache_settings(300), &beta).as_deref(),
            Some("fallback-credit-2026-07-01")
        );
    }

    #[test]
    fn prompt_caching_sends_no_beta_header_with_default_ttl() {
        let config = AnthropicConfig::default();
        let header = combined_beta_header_value(true, &cache_settings(300), &beta_config(&config));

        assert_eq!(header, None);
    }

    #[test]
    fn prompt_caching_sends_only_extended_ttl_beta_for_one_hour_ttl() {
        let config = AnthropicConfig::default();
        let header = combined_beta_header_value(true, &cache_settings(3600), &beta_config(&config));

        assert_eq!(header.as_deref(), Some(EXTENDED_CACHE_TTL_BETA));
    }

    /// A budget-continuation profile emits a 1h messages breakpoint from the
    /// profile TTL even when both configured TTLs stay at 5m; the beta header
    /// must follow the effective breakpoint, not the raw settings.
    #[test]
    fn profile_ttl_promotes_messages_breakpoint_and_requires_extended_ttl_beta() {
        let config = AnthropicConfig::default();
        let settings = cache_settings(300);
        let mut beta = beta_config(&config);
        beta.prompt_cache_profile = Some(PromptCacheProfile::BudgetContinuation);

        assert_eq!(combined_beta_header_value(true, &settings, &beta).as_deref(), Some(EXTENDED_CACHE_TTL_BETA));

        // The same settings without the profile stay uncovered: the messages
        // breakpoint resolves to the configured 5m TTL.
        beta.prompt_cache_profile = None;
        assert_eq!(combined_beta_header_value(true, &settings, &beta), None);

        // Disabling prompt caching still suppresses the header.
        beta.prompt_cache_profile = Some(PromptCacheProfile::BudgetContinuation);
        assert_eq!(combined_beta_header_value(false, &settings, &beta), None);
    }

    #[test]
    fn thinking_display_updates_beta_is_sent_only_when_requested() {
        let config = AnthropicConfig::default();
        let mut beta = beta_config(&config);
        assert_eq!(combined_beta_header_value(false, &cache_settings(300), &beta), None);

        beta.include_thinking_display_updates = true;
        let header = combined_beta_header_value(true, &cache_settings(3600), &beta);
        assert_eq!(
            header.as_deref(),
            Some(format!("{EXTENDED_CACHE_TTL_BETA}, {THINKING_DISPLAY_UPDATES_BETA}").as_str())
        );
    }

    #[test]
    fn extended_ttl_beta_is_omitted_when_caching_is_disabled() {
        let config = AnthropicConfig::default();
        let header = combined_beta_header_value(false, &cache_settings(3600), &beta_config(&config));

        assert_eq!(header, None);
    }
}
