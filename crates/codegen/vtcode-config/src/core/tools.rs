use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::constants::{defaults, tools};
use crate::core::plugins::PluginRuntimeConfig;

/// Model-facing tool profile for a session.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolProfile {
    /// VT Code standard baseline: exec_command, write_stdin, and apply_patch.
    #[default]
    #[serde(rename = "vt_code")]
    #[serde(alias = "codex_default")]
    #[cfg_attr(feature = "schema", schemars(rename = "vt_code"))]
    VtCode,
    /// VTCode specialised tools, including code_search and eligible dynamic tools.
    #[serde(rename = "advanced_vtcode")]
    #[cfg_attr(feature = "schema", schemars(rename = "advanced_vtcode"))]
    AdvancedVtCode,
}

/// Tools configuration
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ToolsConfig {
    /// Model-facing tool profile.
    #[serde(default)]
    pub profile: ToolProfile,

    /// Default policy for tools not explicitly listed
    #[serde(default = "default_tool_policy")]
    pub default_policy: ToolPolicy,

    /// Specific tool policies
    #[serde(default)]
    #[cfg_attr(
        feature = "schema",
        schemars(with = "std::collections::BTreeMap<String, ToolPolicy>")
    )]
    pub policies: IndexMap<String, ToolPolicy>,

    /// Maximum inner tool-call loops per user turn. Set to `0` to disable the limit.
    ///
    /// Prevents infinite tool-calling cycles in interactive chat. This limits how
    /// many back-and-forths the agent will perform executing tools and
    /// re-asking the model before returning a final answer.
    ///
    #[serde(default = "default_max_tool_loops")]
    pub max_tool_loops: usize,

    /// Maximum number of times the same tool invocation can be retried with the
    /// identical arguments within a single turn.
    #[serde(default = "default_max_repeated_tool_calls")]
    pub max_repeated_tool_calls: usize,

    /// Maximum consecutive blocked tool calls allowed per turn before forcing a
    /// turn break. This prevents long blocked-call churn from consuming CPU.
    #[serde(default = "default_max_consecutive_blocked_tool_calls_per_turn")]
    pub max_consecutive_blocked_tool_calls_per_turn: usize,

    /// Optional per-second rate limit for tool calls to smooth bursty retries.
    /// When unset, the runtime defaults apply.
    #[serde(default = "default_max_tool_rate_per_second")]
    pub max_tool_rate_per_second: Option<usize>,

    /// Maximum sequential spool-chunk `read_file` calls allowed per turn before
    /// nudging the agent to switch to targeted extraction/summarization.
    #[serde(default = "default_max_sequential_spool_chunk_reads")]
    pub max_sequential_spool_chunk_reads: usize,

    /// Web Fetch tool security configuration
    #[serde(default)]
    pub web_fetch: WebFetchConfig,

    /// Web Search tool configuration (provider selection, result caps, timeouts).
    #[serde(default)]
    pub web_search: WebSearchConfig,

    /// Dynamic plugin runtime configuration
    #[serde(default)]
    pub plugins: PluginRuntimeConfig,

    /// External editor integration settings used by `/edit` and keyboard shortcuts
    #[serde(default)]
    pub editor: EditorToolConfig,

    /// Tool-specific loop thresholds (Adaptive Loop Detection)
    /// Allows setting higher loop limits for read-only tools (e.g., ls, grep)
    /// and lower limits for mutating tools.
    #[serde(default)]
    pub loop_thresholds: IndexMap<String, usize>,

    /// Enables client-local deferred tool loading for providers without a
    /// hosted tool search (e.g. Gemini). When enabled, tools flagged
    /// `defer_loading: true` are omitted from the request payload instead
    /// of being sent eagerly, and a compact summary of what is discoverable
    /// is appended to the system prompt; the model loads them via the
    /// local MCP discovery tools. Enabled by default because
    /// eager MCP schemas are the dominant source of token inflation.
    #[serde(default = "default_client_tool_search")]
    pub client_tool_search: bool,
}

/// External editor integration configuration
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EditorToolConfig {
    /// Enable external editor support for `/edit` and keyboard shortcuts
    #[serde(default = "default_editor_enabled")]
    pub enabled: bool,

    /// Preferred editor command override (supports arguments, e.g. "code --wait")
    #[serde(default)]
    pub preferred_editor: String,

    /// Suspend the TUI event loop while editor is running
    #[serde(default = "default_editor_suspend_tui")]
    pub suspend_tui: bool,
}

/// Web fetch security mode
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WebFetchMode {
    /// Blocklist mode: allow by default, block listed domains (default)
    #[default]
    Restricted,
    /// Allowlist mode: block by default, allow only listed domains
    Whitelist,
}

/// Web search provider identifier.
///
/// VT Code only targets the keyless DuckDuckGo HTML endpoint for web
/// search, so the provider enum is intentionally minimal. `Auto` defers to
/// the DDG default and is the recommended choice.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum WebSearchProvider {
    /// Default; same as `Duckduckgo` for now (kept for back-compat).
    #[default]
    Auto,
    /// Keyless DuckDuckGo HTML scraping (`https://html.duckduckgo.com/html/`).
    Duckduckgo,
}

/// Web Search tool configuration.
///
/// VT Code only uses the keyless DuckDuckGo HTML endpoint. The defaults
/// below are tuned to be polite to that endpoint and to keep the agent
/// responsive under low-quota conditions:
/// - `cooldown_ms`: minimum gap between consecutive live requests on the
///   same tool instance (avoids bursty hammering that triggers the DDG
///   anti-bot challenge).
/// - `cache_ttl_secs`: how long successful results are served from memory
///   before a fresh request is made.
/// - `session_max_requests`: hard cap on outbound requests per tool
///   instance (defends against runaway loops).
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct WebSearchConfig {
    /// Provider selection. Currently the only supported backend is
    /// DuckDuckGo; this field is kept for future extension.
    #[serde(default)]
    pub provider: WebSearchProvider,

    /// Default cap on the number of results returned per call. Hard-capped
    /// at 20 by the runtime to keep responses inline-friendly.
    #[serde(default = "default_web_search_max_results")]
    pub max_results: usize,

    /// Per-request timeout in seconds. Capped at 60s by the runtime.
    #[serde(default = "default_web_search_timeout_secs")]
    pub timeout_secs: u64,

    /// Minimum gap between consecutive live requests, in milliseconds.
    /// Defaults to 3000ms (3s).
    #[serde(default = "default_web_search_cooldown_ms")]
    pub cooldown_ms: u64,

    /// How long successful search results are cached before a fresh
    /// request is made, in seconds. Defaults to 300s (5 min).
    #[serde(default = "default_web_search_cache_ttl_secs")]
    pub cache_ttl_secs: u64,

    /// Hard cap on outbound network requests per tool instance. Defaults
    /// to 12 to stay well below DDG's soft session quotas.
    #[serde(default = "default_web_search_session_max_requests")]
    pub session_max_requests: u32,
}

/// Web Fetch tool security configuration
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct WebFetchConfig {
    /// Security mode: restricted (blocklist) or whitelist (allowlist)
    #[serde(default = "default_web_fetch_mode")]
    pub mode: WebFetchMode,

    /// Inline blocklist - Additional domains to block
    #[serde(default)]
    pub blocked_domains: Vec<String>,

    /// Inline whitelist - Domains to allow in restricted mode
    #[serde(default)]
    pub allowed_domains: Vec<String>,

    /// Additional blocked patterns
    #[serde(default)]
    pub blocked_patterns: Vec<String>,

    /// Strict HTTPS-only mode
    #[serde(default = "default_strict_https")]
    pub strict_https_only: bool,
}

impl Default for ToolsConfig {
    fn default() -> Self {
        let policies = DEFAULT_TOOL_POLICIES
            .iter()
            .map(|(tool, policy)| ((*tool).into(), policy.clone()))
            .collect::<IndexMap<_, _>>();
        Self {
            profile: ToolProfile::default(),
            default_policy: default_tool_policy(),
            policies,
            max_tool_loops: default_max_tool_loops(),
            max_repeated_tool_calls: default_max_repeated_tool_calls(),
            max_consecutive_blocked_tool_calls_per_turn: default_max_consecutive_blocked_tool_calls_per_turn(),
            max_tool_rate_per_second: default_max_tool_rate_per_second(),
            max_sequential_spool_chunk_reads: default_max_sequential_spool_chunk_reads(),
            web_fetch: WebFetchConfig::default(),
            web_search: WebSearchConfig::default(),
            plugins: PluginRuntimeConfig::default(),
            editor: EditorToolConfig::default(),
            loop_thresholds: IndexMap::new(),
            client_tool_search: default_client_tool_search(),
        }
    }
}

impl ToolsConfig {
    #[inline]
    fn tool_loop_limit_reached(&self, completed_tool_loops: usize) -> bool {
        tool_loop_limit_reached(completed_tool_loops, self.max_tool_loops)
    }

    #[inline]
    pub fn tool_call_delay(&self) -> Option<Duration> {
        tool_call_delay_for_rate(self.max_tool_rate_per_second)
    }
}

#[inline]
pub const fn tool_loop_limit_reached(completed_tool_loops: usize, max_tool_loops: usize) -> bool {
    max_tool_loops > 0 && completed_tool_loops >= max_tool_loops
}

#[inline]
pub fn tool_call_delay_for_rate(max_per_second: Option<usize>) -> Option<Duration> {
    let rate = max_per_second?;
    if rate == 0 {
        return None;
    }

    let nanos = 1_000_000_000u64.saturating_div(rate as u64).max(1);
    Some(Duration::from_nanos(nanos))
}

impl Default for WebFetchConfig {
    fn default() -> Self {
        Self {
            mode: default_web_fetch_mode(),
            blocked_domains: Vec::new(),
            allowed_domains: default_web_fetch_allowed_domains(),
            blocked_patterns: Vec::new(),
            strict_https_only: true,
        }
    }
}

/// Default `WebFetchConfig::allowed_domains` value.
///
/// Sourced from the curated TOML allowlist (shipped with the crate at
/// `data/network_allowlist.toml`) and filtered down to the categories
/// that make sense as `web_fetch` targets: search engines, specialized
/// knowledge bases, package registries, code-hosting platforms, and
/// web-crawl relays other than `defuddle.md`. AI provider endpoints,
/// OAuth flows, dev infrastructure, and OS-update mirrors are excluded
/// (those need auth or aren't useful as fetch targets).
///
/// The result is cached in a process-global `OnceLock` so the TOML parse
/// happens exactly once per process even though `WebFetchConfig::default`
/// is called from serde paths, the tool registry, and tests.
fn default_web_fetch_allowed_domains() -> Vec<String> {
    use std::sync::OnceLock;
    static CACHE: OnceLock<Vec<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| crate::network_allowlist::NetworkAllowlist::load_default().web_fetch_relevant_domains())
        .clone()
}

impl Default for WebSearchConfig {
    fn default() -> Self {
        Self {
            provider: WebSearchProvider::default(),
            max_results: default_web_search_max_results(),
            timeout_secs: default_web_search_timeout_secs(),
            cooldown_ms: default_web_search_cooldown_ms(),
            cache_ttl_secs: default_web_search_cache_ttl_secs(),
            session_max_requests: default_web_search_session_max_requests(),
        }
    }
}

impl Default for EditorToolConfig {
    fn default() -> Self {
        Self {
            enabled: default_editor_enabled(),
            preferred_editor: String::new(),
            suspend_tui: default_editor_suspend_tui(),
        }
    }
}

/// Tool execution policy
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ToolPolicy {
    /// Allow execution without confirmation
    Allow,
    /// Prompt user for confirmation
    #[default]
    Prompt,
    /// Deny execution
    Deny,
}

#[inline]
const fn default_tool_policy() -> ToolPolicy {
    ToolPolicy::Prompt
}

#[inline]
const fn default_max_tool_loops() -> usize {
    defaults::DEFAULT_MAX_TOOL_LOOPS
}

#[inline]
const fn default_max_repeated_tool_calls() -> usize {
    defaults::DEFAULT_MAX_REPEATED_TOOL_CALLS
}

#[inline]
const fn default_max_consecutive_blocked_tool_calls_per_turn() -> usize {
    defaults::DEFAULT_MAX_CONSECUTIVE_BLOCKED_TOOL_CALLS_PER_TURN
}

#[inline]
const fn default_max_tool_rate_per_second() -> Option<usize> {
    None
}

#[inline]
const fn default_max_sequential_spool_chunk_reads() -> usize {
    defaults::DEFAULT_MAX_SEQUENTIAL_SPOOL_CHUNK_READS_PER_TURN
}

#[inline]
fn default_web_fetch_mode() -> WebFetchMode {
    WebFetchMode::Restricted
}

fn default_strict_https() -> bool {
    true
}

#[inline]
const fn default_web_search_max_results() -> usize {
    8
}

#[inline]
const fn default_web_search_timeout_secs() -> u64 {
    20
}

#[inline]
const fn default_web_search_cooldown_ms() -> u64 {
    3_000
}

#[inline]
const fn default_web_search_cache_ttl_secs() -> u64 {
    300
}

#[inline]
const fn default_web_search_session_max_requests() -> u32 {
    12
}

#[inline]
const fn default_editor_enabled() -> bool {
    true
}

#[inline]
const fn default_editor_suspend_tui() -> bool {
    true
}

#[inline]
const fn default_client_tool_search() -> bool {
    true
}

const DEFAULT_TOOL_POLICIES: &[(&str, ToolPolicy)] = &[
    // Core workflow tools (non-destructive)
    (tools::START_PLANNING, ToolPolicy::Allow),
    (tools::TASK_TRACKER, ToolPolicy::Allow),
    // Public model-facing tools.
    (tools::CODE_SEARCH, ToolPolicy::Allow),
    (tools::EXEC_COMMAND, ToolPolicy::Allow),
    (tools::WRITE_STDIN, ToolPolicy::Allow),
    (tools::APPLY_PATCH, ToolPolicy::Prompt),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tools_config_defaults_to_vt_code_profile() {
        assert_eq!(ToolsConfig::default().profile, ToolProfile::VtCode);
    }

    #[test]
    fn vt_code_profile_round_trips_through_toml() {
        let config: ToolsConfig = toml::from_str("profile = \"vt_code\"").expect("vt_code tool profile should parse");
        assert_eq!(config.profile, ToolProfile::VtCode);

        let serialised = toml::to_string(&config).expect("tools config should serialise");
        assert!(serialised.contains("profile = \"vt_code\""));

        let round_tripped: ToolsConfig = toml::from_str(&serialised).expect("serialised tools config should parse");
        assert_eq!(round_tripped.profile, ToolProfile::VtCode);
    }

    #[test]
    fn tool_profile_round_trips_through_toml() {
        let config: ToolsConfig =
            toml::from_str("profile = \"advanced_vtcode\"").expect("advanced tool profile should parse");
        assert_eq!(config.profile, ToolProfile::AdvancedVtCode);

        let serialised = toml::to_string(&config).expect("tools config should serialise");
        assert!(serialised.contains("profile = \"advanced_vtcode\""));

        let round_tripped: ToolsConfig = toml::from_str(&serialised).expect("serialised tools config should parse");
        assert_eq!(round_tripped.profile, ToolProfile::AdvancedVtCode);
    }

    #[test]
    fn tool_profile_rejects_unintended_derived_spelling() {
        let error = toml::from_str::<ToolsConfig>("profile = \"advanced_vt_code\"")
            .expect_err("unintended spelling should fail")
            .to_string();

        assert!(error.contains("unknown variant `advanced_vt_code`"), "{error}");
        assert!(error.contains("`advanced_vtcode`"), "{error}");
    }

    #[test]
    fn invalid_tool_profile_reports_allowed_values() {
        let error = toml::from_str::<ToolsConfig>("profile = \"experimental\"")
            .expect_err("unknown tool profile should fail")
            .to_string();

        assert!(error.contains("unknown variant `experimental`"), "{error}");
        assert!(error.contains("`vt_code`"), "{error}");
        assert!(error.contains("`advanced_vtcode`"), "{error}");
    }

    #[cfg(feature = "schema")]
    #[test]
    fn tools_config_schema_includes_profile_values() {
        let schema = schemars::schema_for!(ToolsConfig);
        let schema_json = serde_json::to_value(schema).expect("schema should serialise");
        let schema_text = serde_json::to_string(&schema_json).expect("schema should stringify");

        assert!(schema_json["properties"].get("profile").is_some());
        assert!(schema_text.contains("vt_code"));
        assert!(schema_text.contains("advanced_vtcode"));
    }

    #[test]
    fn editor_config_defaults_are_enabled() {
        let config = ToolsConfig::default();
        assert!(config.editor.enabled);
        assert!(config.editor.preferred_editor.is_empty());
        assert!(config.editor.suspend_tui);
    }

    #[test]
    fn disabled_tool_loop_limit_never_trips() {
        assert!(!tool_loop_limit_reached(1, 0));
        assert!(!tool_loop_limit_reached(32, 0));
        assert!(tool_loop_limit_reached(2, 2));
    }

    #[test]
    fn tools_config_reports_tool_loop_limit() {
        let config = ToolsConfig { max_tool_loops: 2, ..Default::default() };

        assert!(!config.tool_loop_limit_reached(1));
        assert!(config.tool_loop_limit_reached(2));
    }

    #[test]
    fn tool_call_delay_for_rate_ignores_unset_or_zero_limits() {
        assert_eq!(tool_call_delay_for_rate(None), None);
        assert_eq!(tool_call_delay_for_rate(Some(0)), None);
    }

    #[test]
    fn tool_call_delay_for_rate_uses_per_second_interval() {
        assert_eq!(tool_call_delay_for_rate(Some(4)), Some(Duration::from_millis(250)));
    }

    #[test]
    fn default_tool_policies_only_seed_current_public_surface() {
        let config = ToolsConfig::default();

        assert_eq!(config.policies.get(tools::EXEC_COMMAND), Some(&ToolPolicy::Allow));
        assert_eq!(config.policies.get(tools::WRITE_STDIN), Some(&ToolPolicy::Allow));
        assert_eq!(config.policies.get(tools::CODE_SEARCH), Some(&ToolPolicy::Allow));
        assert_eq!(config.policies.get(tools::APPLY_PATCH), Some(&ToolPolicy::Prompt));
        for legacy_tool in [
            tools::UNIFIED_EXEC,
            tools::UNIFIED_SEARCH,
            tools::UNIFIED_FILE,
            tools::READ_FILE,
            tools::WRITE_FILE,
            tools::EDIT_FILE,
            tools::RUN_PTY_CMD,
            tools::READ_PTY_SESSION,
            tools::LIST_PTY_SESSIONS,
            tools::SEND_PTY_INPUT,
            tools::CLOSE_PTY_SESSION,
            tools::EXECUTE_CODE,
        ] {
            assert!(!config.policies.contains_key(legacy_tool));
        }
    }

    #[test]
    fn client_tool_search_defaults_to_enabled() {
        let config = ToolsConfig::default();
        assert!(config.client_tool_search);

        let deserialized: ToolsConfig = toml::from_str("default_policy = \"prompt\"\n")
            .expect("tools config should parse without client_tool_search");
        assert!(deserialized.client_tool_search);

        let disabled: ToolsConfig = toml::from_str(
            r#"
default_policy = "prompt"
client_tool_search = false
"#,
        )
        .expect("tools config should parse with client_tool_search disabled");
        assert!(!disabled.client_tool_search);
    }

    #[test]
    fn editor_config_deserializes_from_toml() {
        let config: ToolsConfig = toml::from_str(
            r#"
default_policy = "prompt"

[editor]
enabled = false
preferred_editor = "code --wait"
suspend_tui = false
"#,
        )
        .expect("tools config should parse");

        assert!(!config.editor.enabled);
        assert_eq!(config.editor.preferred_editor, "code --wait");
        assert!(!config.editor.suspend_tui);
    }

    #[test]
    fn web_search_config_deserializes_from_toml() {
        let config: ToolsConfig = toml::from_str(
            r#"
default_policy = "prompt"

[web_search]
provider = "duckduckgo"
max_results = 12
timeout_secs = 25
cooldown_ms = 1500
cache_ttl_secs = 120
session_max_requests = 5
"#,
        )
        .expect("tools config should parse");

        assert_eq!(config.web_search.provider, WebSearchProvider::Duckduckgo);
        assert_eq!(config.web_search.max_results, 12);
        assert_eq!(config.web_search.timeout_secs, 25);
        assert_eq!(config.web_search.cooldown_ms, 1500);
        assert_eq!(config.web_search.cache_ttl_secs, 120);
        assert_eq!(config.web_search.session_max_requests, 5);
    }

    #[test]
    fn web_search_config_defaults_are_polite() {
        let config = WebSearchConfig::default();
        assert_eq!(config.provider, WebSearchProvider::Auto);
        assert_eq!(config.max_results, 8);
        assert_eq!(config.timeout_secs, 20);
        assert!(config.cooldown_ms >= 1_000);
        assert!(config.cache_ttl_secs >= 60);
        assert!(config.session_max_requests > 0);
    }

    #[test]
    fn web_search_provider_serializes_lowercase() {
        // Serde rename_all = "snake_case" means enum variants serialize
        // lowercase, matching the LLM-facing schema strings.
        let json = serde_json::to_value(WebSearchProvider::Duckduckgo).unwrap();
        assert_eq!(json, serde_json::json!("duckduckgo"));
    }

    #[test]
    fn web_fetch_default_allowed_domains_seed_common_dev_sites() {
        // The defaults should include the sites the agent most commonly
        // needs (looking up a user, fetching a crate's README, etc.).
        // This is a regression test for the "who is vinhnx?" case where
        // the default `Restricted` mode blocked github.com / npmjs.com /
        // crates.io even though none of them are on the blocklist.
        //
        // After H1: the inline list was removed in favour of the TOML
        // allowlist, filtered to web-fetch-relevant categories. Hosts
        // that aren't in the TOML (`npmjs.com`, `www.npmjs.com`,
        // `docs.rs`, etc.) are no longer in the defaults — that's
        // intentional, see the allowlist policy in
        // `NetworkAllowlist::web_fetch_relevant_domains`.
        let allowed = WebFetchConfig::default().allowed_domains;
        for host in [
            "github.com",
            "api.github.com",
            "raw.githubusercontent.com",
            "crates.io",
            "index.crates.io",
            "registry.npmjs.org",
            "pypi.org",
        ] {
            assert!(
                allowed.iter().any(|d| d == host),
                "default allowed_domains should include {host}; got {allowed:?}"
            );
        }
    }

    #[test]
    fn web_fetch_default_allowed_domains_include_relevant_categories() {
        // The TOML allowlist is filtered to web-friendly categories for the
        // web_fetch defaults: search engines, package registries, code
        // hosting, web-crawl relays (minus defuddle.md), MCP servers, and
        // specialized knowledge bases. AI provider endpoints and dev
        // infrastructure are explicitly excluded (those need auth or
        // aren't useful as fetch targets).
        let allowed = WebFetchConfig::default().allowed_domains;
        for host in [
            "github.com",
            "crates.io",
            "registry.npmjs.org",
            "pypi.org",
            "en.wikipedia.org",
            "r.jina.ai",
            "api.tavily.com",
        ] {
            assert!(
                allowed.iter().any(|d| d == host),
                "default allowed_domains should include {host}; got {allowed:?}"
            );
        }
    }

    #[test]
    fn web_fetch_default_allowed_domains_exclude_ai_and_dev_infra() {
        // H1 (review): the old default merged every category. The new
        // default must not include AI provider endpoints (LLM inference
        // APIs that need auth), OAuth flows, dev infrastructure, OS-update
        // mirrors, or `defuddle.md` (which is a relay, not a fetch
        // target).
        //
        // Search APIs like `api.tavily.com` ARE allowed — those are
        // legitimate fetch targets the agent might query directly.
        let allowed = WebFetchConfig::default().allowed_domains;
        for host in [
            "api.anthropic.com",
            "api.openai.com",
            "api.fireworks.ai",
            "api.deepseek.com",
            "defuddle.md",
            "*.auth0.com",
            "*.workers.dev",
            "*.vercel.app",
            "us.i.posthog.com",
            "security.ubuntu.com",
            "archive.ubuntu.com",
        ] {
            assert!(
                !allowed.iter().any(|d| d == host),
                "default allowed_domains must NOT include {host}; got {allowed:?}"
            );
        }
    }

    #[test]
    fn web_fetch_default_allowed_domains_preserves_wildcards_in_relevant_categories() {
        // The TOML has `*.vercel.app` in dev_infra and `*.clerk.accounts.dev`
        // in auth. After filtering for web_fetch, neither should remain.
        // A future TOML change that moves a wildcard into a web-relevant
        // category will be picked up here.
        let allowed = WebFetchConfig::default().allowed_domains;
        let wildcards: Vec<&str> = allowed.iter().map(|s| s.as_str()).filter(|s| s.starts_with("*.")).collect();
        assert!(
            wildcards.is_empty(),
            "expected no wildcards in web_fetch defaults (dev_infra/auth are excluded); got {wildcards:?}"
        );
    }

    #[test]
    fn web_fetch_default_allowed_domains_returns_fresh_vec() {
        // `default_web_fetch_allowed_domains` is cached in a OnceLock, but
        // each call must return a fresh `Vec` so callers can mutate the
        // returned value without affecting the cached snapshot.
        let mut a = WebFetchConfig::default().allowed_domains;
        a.push("evil.example".to_string());
        let b = WebFetchConfig::default().allowed_domains;
        assert!(!b.iter().any(|d| d == "evil.example"));
    }
}
