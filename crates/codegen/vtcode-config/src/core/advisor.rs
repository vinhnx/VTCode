//! Configuration for the Claude Advisor server-side tool.
//!
//! The advisor pairs a faster executor model with a higher-intelligence advisor
//! model that provides strategic guidance mid-generation. The advisor runs as an
//! Anthropic server-side tool and is therefore only honored for Anthropic models
//! and providers.

use serde::{Deserialize, Serialize};

/// Cache lifetime for the advisor's own transcript across calls in a conversation.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AdvisorCacheTtl {
    /// 5 minute cache lifetime.
    #[serde(rename = "5m")]
    FiveMinutes,
    /// 1 hour cache lifetime.
    #[serde(rename = "1h")]
    OneHour,
}

impl AdvisorCacheTtl {
    /// Returns the wire string for the `ttl` field.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FiveMinutes => "5m",
            Self::OneHour => "1h",
        }
    }
}

/// Enables prompt caching for the advisor's own transcript across calls within a
/// conversation. This is an on/off switch, not a cache-control breakpoint.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdvisorCachingConfig {
    /// Whether advisor-side prompt caching is enabled.
    #[serde(default = "default_advisor_caching_enabled")]
    pub enabled: bool,
    /// Cache lifetime for the advisor transcript.
    #[serde(default = "default_advisor_cache_ttl")]
    pub ttl: AdvisorCacheTtl,
}

impl Default for AdvisorCachingConfig {
    fn default() -> Self {
        Self {
            enabled: default_advisor_caching_enabled(),
            ttl: default_advisor_cache_ttl(),
        }
    }
}

#[inline]
const fn default_advisor_caching_enabled() -> bool {
    false
}

#[inline]
fn default_advisor_cache_ttl() -> AdvisorCacheTtl {
    AdvisorCacheTtl::FiveMinutes
}

/// Configuration for the Claude Advisor server-side tool.
///
/// The advisor tool is only valid for Anthropic providers and models. When
/// `enabled` is `true`, vtcode injects an `advisor_20260301` tool into the
/// Anthropic request and sends the `advisor-tool-2026-03-01` beta header. The
/// executor model must form a valid pair with the configured advisor model
/// (see `vtcode_config::constants::models::anthropic::advisor_compatibility`).
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdvisorConfig {
    /// Master toggle for the Anthropic server-side advisor tool.
    #[serde(default = "default_advisor_enabled")]
    pub enabled: bool,

    /// Advisor model id (must be an Anthropic/Claude model at least as capable
    /// as the executor). Empty or omitted falls back to a sensible default
    /// advisor for the executor model.
    #[serde(default)]
    pub model: String,

    /// Maximum number of advisor invocations per request. `None` means
    /// unlimited (the API default). Once the executor reaches this cap, further
    /// advisor calls return an `advisor_tool_result_error`.
    #[serde(default)]
    pub max_uses: Option<u32>,

    /// Caps the advisor's total output (thinking plus text) per call. Minimum
    /// 1024. `None` lets the advisor model choose its own output cap.
    #[serde(default)]
    pub max_tokens: Option<u32>,

    /// Enables prompt caching for the advisor's own transcript across calls
    /// within a conversation. Only worthwhile for long agent loops (three or
    /// more expected advisor calls).
    #[serde(default)]
    pub caching: Option<AdvisorCachingConfig>,
}

impl Default for AdvisorConfig {
    fn default() -> Self {
        Self {
            enabled: default_advisor_enabled(),
            model: String::new(),
            max_uses: None,
            max_tokens: None,
            caching: None,
        }
    }
}

#[inline]
const fn default_advisor_enabled() -> bool {
    false
}
