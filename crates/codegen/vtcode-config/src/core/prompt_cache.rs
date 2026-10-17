use crate::constants::prompt_cache;
use crate::env_helpers::default_true;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use vtcode_commons::VtCodePaths;

/// Global prompt caching configuration loaded from vtcode.toml
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PromptCachingConfig {
    /// Enable prompt caching features globally
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// Base directory for local prompt cache storage (supports `~` expansion)
    #[serde(default = "default_cache_dir")]
    pub cache_dir: String,

    /// Maximum number of cached prompt entries to retain on disk
    #[serde(default = "default_max_entries")]
    pub max_entries: usize,

    /// Maximum age (in days) before cached entries are purged
    #[serde(default = "default_max_age_days")]
    pub max_age_days: u64,

    /// Automatically evict stale entries on startup/shutdown
    #[serde(default = "default_auto_cleanup")]
    pub enable_auto_cleanup: bool,

    /// Minimum quality score required before persisting an entry
    #[serde(default = "default_min_quality_threshold")]
    pub min_quality_threshold: f64,

    /// Enable prompt-shaping optimizations that improve provider-side cache locality.
    /// When enabled, VT Code keeps volatile runtime context at the end of prompt text.
    #[serde(default = "default_cache_friendly_prompt_shaping")]
    pub cache_friendly_prompt_shaping: bool,

    /// Keep the wire tool catalog identical across planning and execution
    /// turns (union of both modes' tools). Planning toggles then do not
    /// rewrite the tool prefix. The fail-closed execution gate still blocks
    /// mutations during planning.
    #[serde(default = "default_true")]
    pub stable_tool_catalog_across_modes: bool,

    /// Warn before a request when the pause since the previous request likely
    /// exceeded the provider prompt cache lifetime (advisory only).
    #[serde(default = "default_true")]
    pub gap_warning_enabled: bool,

    /// Override for the cache-gap warning threshold in seconds. When unset,
    /// a provider-specific default is used (Anthropic 300s, OpenAI 600s).
    #[serde(default)]
    pub gap_warning_threshold_secs: Option<u64>,

    /// Provider specific overrides
    #[serde(default)]
    pub providers: ProviderPromptCachingConfig,
}

impl Default for PromptCachingConfig {
    fn default() -> Self {
        let cache_dir = default_cache_dir();
        Self {
            enabled: default_enabled() && !cache_dir.is_empty(),
            cache_dir,
            max_entries: default_max_entries(),
            max_age_days: default_max_age_days(),
            enable_auto_cleanup: default_auto_cleanup(),
            min_quality_threshold: default_min_quality_threshold(),
            cache_friendly_prompt_shaping: default_cache_friendly_prompt_shaping(),
            stable_tool_catalog_across_modes: true,
            gap_warning_enabled: default_true(),
            gap_warning_threshold_secs: None,
            providers: ProviderPromptCachingConfig::default(),
        }
    }
}

impl PromptCachingConfig {
    /// Resolve the configured cache directory to an absolute path
    ///
    /// - `~` is expanded to the user's home directory when available
    /// - Relative paths are resolved against the provided workspace root when supplied
    /// - Falls back to the configured string when neither applies
    pub fn resolve_cache_dir(&self, workspace_root: Option<&Path>) -> PathBuf {
        resolve_path(&self.cache_dir, workspace_root)
    }

    /// Returns true when prompt caching is active for the given provider runtime name.
    pub fn is_provider_enabled(&self, provider_name: &str) -> bool {
        if !self.enabled {
            return false;
        }

        match provider_name.to_ascii_lowercase().as_str() {
            "openai" | "merge-gateway" => self.providers.openai.enabled,
            "anthropic" | "minimax" => self.providers.anthropic.enabled,
            "gemini" => {
                self.providers.gemini.enabled && !matches!(self.providers.gemini.mode, GeminiPromptCacheMode::Off)
            }
            "openrouter" => self.providers.openrouter.enabled,
            "moonshot" => self.providers.moonshot.enabled,
            "deepseek" => self.providers.deepseek.enabled,
            "zai" => self.providers.zai.enabled,
            _ => false,
        }
    }

    /// Resolve the cache-gap warning threshold in seconds for a provider.
    ///
    /// Returns `None` when the warning should not fire: gap warnings disabled,
    /// caching disabled for the provider, or OpenAI configured with 24h
    /// extended retention (where a short pause cannot expire the cache).
    pub fn gap_threshold_secs(&self, provider_name: &str) -> Option<u64> {
        if !self.gap_warning_enabled || !self.is_provider_enabled(provider_name) {
            return None;
        }

        let provider = provider_name.to_ascii_lowercase();
        if provider == "openai"
            && matches!(self.providers.openai.prompt_cache_retention, Some(PromptCacheRetention::H24))
        {
            return None;
        }

        if let Some(threshold) = self.gap_warning_threshold_secs {
            return Some(threshold);
        }

        Some(match provider.as_str() {
            "anthropic" | "minimax" => prompt_cache::ANTHROPIC_CACHE_GAP_WARNING_SECONDS,
            "openai" => prompt_cache::OPENAI_CACHE_GAP_WARNING_SECONDS,
            _ => prompt_cache::DEFAULT_CACHE_GAP_WARNING_SECONDS,
        })
    }
}

/// Per-provider configuration overrides
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct ProviderPromptCachingConfig {
    #[serde(default = "OpenAIPromptCacheSettings::default")]
    pub openai: OpenAIPromptCacheSettings,

    #[serde(default = "AnthropicPromptCacheSettings::default")]
    pub anthropic: AnthropicPromptCacheSettings,

    #[serde(default = "GeminiPromptCacheSettings::default")]
    pub gemini: GeminiPromptCacheSettings,

    #[serde(default = "OpenRouterPromptCacheSettings::default")]
    pub openrouter: OpenRouterPromptCacheSettings,

    #[serde(default = "MoonshotPromptCacheSettings::default")]
    moonshot: MoonshotPromptCacheSettings,

    #[serde(default = "DeepSeekPromptCacheSettings::default")]
    pub deepseek: DeepSeekPromptCacheSettings,

    #[serde(default = "ZaiPromptCacheSettings::default")]
    zai: ZaiPromptCacheSettings,
}

/// OpenAI prompt cache retention policy.
/// Controls the model-side prompt caching window.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PromptCacheRetention {
    /// In-memory caching (short-lived, free)
    #[serde(rename = "in_memory")]
    InMemory,
    /// 24-hour persistent caching
    #[default]
    #[serde(rename = "24h")]
    H24,
    /// Forward-compatible catch-all for unknown retention values.
    ///
    /// The explicit `rename` keeps the serialized form (`"unknown"`) aligned
    /// with [`PromptCacheRetention::as_str`]; bare `#[serde(other)]` would
    /// serialize the variant name as `"Unknown"`.
    #[serde(other, rename = "unknown")]
    Unknown,
}

impl PromptCacheRetention {
    /// Returns the string representation for the OpenAI API wire format.
    pub fn as_str(&self) -> &str {
        match self {
            Self::InMemory => "in_memory",
            Self::H24 => "24h",
            Self::Unknown => "unknown",
        }
    }
}

impl std::fmt::Display for PromptCacheRetention {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// OpenAI prompt caching controls (automatic with metrics)
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OpenAIPromptCacheSettings {
    #[serde(default = "default_true")]
    pub enabled: bool,

    #[serde(default = "default_openai_min_prefix_tokens")]
    min_prefix_tokens: u32,

    #[serde(default = "default_openai_idle_expiration")]
    idle_expiration_seconds: u64,

    #[serde(default = "default_true")]
    pub surface_metrics: bool,

    /// Strategy for generating OpenAI `prompt_cache_key`.
    /// Session-scoped keys derive one stable key per VT Code conversation.
    #[serde(default = "default_openai_prompt_cache_key_mode")]
    pub prompt_cache_key_mode: OpenAIPromptCacheKeyMode,

    /// Optional prompt cache retention policy to pass directly into OpenAI Responses API.
    /// Supported values are "in_memory" and "24h". If set, VT Code will include
    /// `prompt_cache_retention` in the request body to extend the model-side prompt
    /// caching window or request the explicit in-memory policy.
    #[serde(default)]
    pub prompt_cache_retention: Option<PromptCacheRetention>,
}

impl Default for OpenAIPromptCacheSettings {
    fn default() -> Self {
        Self {
            enabled: default_true(),
            min_prefix_tokens: default_openai_min_prefix_tokens(),
            idle_expiration_seconds: default_openai_idle_expiration(),
            surface_metrics: default_true(),
            prompt_cache_key_mode: default_openai_prompt_cache_key_mode(),
            prompt_cache_retention: None,
        }
    }
}

impl OpenAIPromptCacheSettings {
    /// Validate OpenAI provider prompt cache settings.
    /// With the typed enum, invalid values are caught at deserialization time.
    fn validate(&self) -> anyhow::Result<()> {
        if let Some(PromptCacheRetention::Unknown) = self.prompt_cache_retention {
            anyhow::bail!("prompt_cache_retention must be one of: in_memory, 24h");
        }
        Ok(())
    }
}

/// Build a stable OpenAI `prompt_cache_key` for requests that should share
/// provider-side cache routing.
#[must_use]
pub fn build_openai_prompt_cache_key(
    prompt_cache_enabled: bool,
    prompt_cache_key_mode: &OpenAIPromptCacheKeyMode,
    lineage_id: Option<&str>,
) -> Option<String> {
    if !prompt_cache_enabled {
        return None;
    }

    let lineage_id = lineage_id.map(str::trim).filter(|value| !value.is_empty());
    match prompt_cache_key_mode {
        OpenAIPromptCacheKeyMode::Session => lineage_id.map(|lineage_id| format!("vtcode:openai:{lineage_id}")),
        OpenAIPromptCacheKeyMode::Off => None,
    }
}

/// Rewrite an OpenAI-style cache key for routes that share the wire shape but
/// need distinct routing stickiness. Keeps the stable session identifier while
/// namespacing gateway/session-affinity traffic apart from native OpenAI.
#[must_use]
pub fn map_prompt_cache_key_for_provider(provider_name: &str, key: String) -> String {
    let provider = provider_name.trim().to_ascii_lowercase();
    match provider.as_str() {
        "merge-gateway" => key.replacen("vtcode:openai:", "vtcode:merge:", 1),
        "openrouter" => key.replacen("vtcode:openai:", "vtcode:openrouter:", 1),
        "xai" => key.replacen("vtcode:openai:", "vtcode:xai:", 1),
        _ => key,
    }
}

/// Providers that send lineage-stable session/cache identity on the wire.
#[must_use]
pub fn session_affinity_provider(provider_name: &str) -> bool {
    matches!(
        provider_name.trim().to_ascii_lowercase().as_str(),
        "openai" | "merge-gateway" | "openrouter" | "xai"
    )
}

/// Whether `LLMRequest.prompt_cache_key` should carry session lineage for this provider.
///
/// OpenAI/Merge keep the OpenAI prompt-cache enablement gate.
/// OpenRouter/xAI need lineage for documented session affinity even when the
/// OpenAI-specific cache block is off (global prompt-cache master switch still applies).
#[must_use]
pub fn session_affinity_key_enabled(
    provider_name: &str,
    global_prompt_cache_enabled: bool,
    openai_prompt_cache_enabled: bool,
) -> bool {
    if !global_prompt_cache_enabled {
        return false;
    }
    match provider_name.trim().to_ascii_lowercase().as_str() {
        "openai" | "merge-gateway" => openai_prompt_cache_enabled,
        "openrouter" | "xai" => true,
        _ => false,
    }
}

/// Build the namespaced session-affinity / prompt-cache key for a request.
///
/// OpenAI/Merge honor [`OpenAIPromptCacheKeyMode`]. OpenRouter/xAI always use a
/// stable session namespace when the affinity gate is on: sticky routing is
/// independent of the OpenAI prompt-cache key mode.
#[must_use]
pub fn build_session_affinity_prompt_cache_key(
    provider_name: &str,
    session_affinity_enabled: bool,
    prompt_cache_key_mode: &OpenAIPromptCacheKeyMode,
    lineage_id: Option<&str>,
) -> Option<String> {
    if !session_affinity_enabled {
        return None;
    }
    let provider = provider_name.trim().to_ascii_lowercase();
    let lineage = lineage_id.map(str::trim).filter(|value| !value.is_empty())?;
    match provider.as_str() {
        "openai" | "merge-gateway" => match prompt_cache_key_mode {
            OpenAIPromptCacheKeyMode::Session => {
                Some(map_prompt_cache_key_for_provider(&provider, format!("vtcode:openai:{lineage}")))
            }
            OpenAIPromptCacheKeyMode::Off => None,
        },
        "openrouter" | "xai" => Some(format!("vtcode:{provider}:{lineage}")),
        _ => None,
    }
}

/// OpenAI prompt cache key derivation mode.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum OpenAIPromptCacheKeyMode {
    /// Do not send `prompt_cache_key` in OpenAI requests.
    Off,
    /// Send one stable `prompt_cache_key` per VT Code session.
    #[default]
    Session,
}

/// Anthropic Claude cache control settings
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AnthropicPromptCacheSettings {
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Default TTL in seconds for the first cache breakpoint (tools/system).
    /// Anthropic only supports "5m" (300s) or "1h" (3600s) TTL formats.
    /// Set to >= 3600 for 1-hour cache on tools and system prompts.
    /// Default: 3600 (1 hour) - recommended for stable tool definitions
    #[serde(default = "default_anthropic_tools_ttl")]
    pub tools_ttl_seconds: u64,

    /// TTL for subsequent cache breakpoints (messages).
    /// Set to >= 3600 for 1-hour cache on messages.
    /// Default: 300 (5 minutes) - recommended for frequently changing messages
    #[serde(default = "default_anthropic_messages_ttl")]
    pub messages_ttl_seconds: u64,

    /// Maximum number of cache breakpoints to use (max 4 per Anthropic spec).
    /// Default: 4
    #[serde(default = "default_anthropic_max_breakpoints")]
    pub max_breakpoints: u8,

    /// Apply cache control to system prompts by default
    #[serde(default = "default_true")]
    pub cache_system_messages: bool,

    /// Apply cache control to user messages exceeding threshold
    #[serde(default = "default_true")]
    pub cache_user_messages: bool,

    /// Apply cache control to tool definitions by default
    /// Default: true (tools are typically stable and benefit from longer caching)
    #[serde(default = "default_true")]
    pub cache_tool_definitions: bool,

    /// Minimum message length (in characters) before applying cache control
    /// to avoid caching very short messages that don't benefit from caching.
    /// Default: 256 characters (~64 tokens)
    #[serde(default = "default_min_message_length")]
    pub min_message_length_for_cache: usize,

    /// Extended TTL for Anthropic prompt caching (in seconds)
    /// Set to >= 3600 for 1-hour cache on messages
    #[serde(default = "default_anthropic_extended_ttl")]
    pub extended_ttl_seconds: Option<u64>,

    /// Prefer the 1h extended TTL for tools/system/messages even when the
    /// per-breakpoint TTLs are 5m. Use when sessions idle past the 5m cache
    /// window. Opt-in; 1h writes cost 2x base input.
    #[serde(default)]
    pub prefer_extended_ttl: bool,
}

impl Default for AnthropicPromptCacheSettings {
    fn default() -> Self {
        Self {
            enabled: default_true(),
            tools_ttl_seconds: default_anthropic_tools_ttl(),
            messages_ttl_seconds: default_anthropic_messages_ttl(),
            max_breakpoints: default_anthropic_max_breakpoints(),
            cache_system_messages: default_true(),
            cache_user_messages: default_true(),
            cache_tool_definitions: default_true(),
            min_message_length_for_cache: default_min_message_length(),
            extended_ttl_seconds: default_anthropic_extended_ttl(),
            prefer_extended_ttl: false,
        }
    }
}

/// Gemini API caching preferences
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GeminiPromptCacheSettings {
    #[serde(default = "default_true")]
    pub enabled: bool,

    #[serde(default = "default_gemini_mode")]
    pub mode: GeminiPromptCacheMode,

    #[serde(default = "default_gemini_min_prefix_tokens")]
    min_prefix_tokens: u32,

    /// TTL for explicit caches (ignored in implicit mode)
    #[serde(default = "default_gemini_explicit_ttl")]
    pub explicit_ttl_seconds: Option<u64>,
}

impl Default for GeminiPromptCacheSettings {
    fn default() -> Self {
        Self {
            enabled: default_true(),
            mode: GeminiPromptCacheMode::default(),
            min_prefix_tokens: default_gemini_min_prefix_tokens(),
            explicit_ttl_seconds: default_gemini_explicit_ttl(),
        }
    }
}

/// Gemini prompt caching mode selection
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum GeminiPromptCacheMode {
    #[default]
    Implicit,
    Explicit,
    Off,
}

/// OpenRouter passthrough caching controls
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OpenRouterPromptCacheSettings {
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Propagate provider cache instructions automatically
    #[serde(default = "default_true")]
    propagate_provider_capabilities: bool,

    /// Surface cache savings reported by OpenRouter
    #[serde(default = "default_true")]
    pub report_savings: bool,
}

impl Default for OpenRouterPromptCacheSettings {
    fn default() -> Self {
        Self {
            enabled: default_true(),
            propagate_provider_capabilities: default_true(),
            report_savings: default_true(),
        }
    }
}

/// Moonshot prompt caching configuration (leverages server-side reuse)
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MoonshotPromptCacheSettings {
    #[serde(default = "default_moonshot_enabled")]
    enabled: bool,
}

impl Default for MoonshotPromptCacheSettings {
    fn default() -> Self {
        Self { enabled: default_moonshot_enabled() }
    }
}

/// DeepSeek prompt caching configuration (automatic KV cache reuse)
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DeepSeekPromptCacheSettings {
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Emit cache hit/miss metrics from responses when available
    #[serde(default = "default_true")]
    pub surface_metrics: bool,
}

impl Default for DeepSeekPromptCacheSettings {
    fn default() -> Self {
        Self {
            enabled: default_true(),
            surface_metrics: default_true(),
        }
    }
}

/// Z.AI prompt caching configuration (disabled until platform exposes metrics)
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ZaiPromptCacheSettings {
    #[serde(default = "default_zai_enabled")]
    enabled: bool,
}

impl Default for ZaiPromptCacheSettings {
    fn default() -> Self {
        Self { enabled: default_zai_enabled() }
    }
}

fn default_enabled() -> bool {
    prompt_cache::DEFAULT_ENABLED
}

fn default_cache_dir() -> String {
    VtCodePaths::resolve()
        .map(|paths| paths.cache_dir().join("prompts").display().to_string())
        .unwrap_or_default()
}

fn default_max_entries() -> usize {
    prompt_cache::DEFAULT_MAX_ENTRIES
}

fn default_max_age_days() -> u64 {
    prompt_cache::DEFAULT_MAX_AGE_DAYS
}

fn default_auto_cleanup() -> bool {
    prompt_cache::DEFAULT_AUTO_CLEANUP
}

fn default_min_quality_threshold() -> f64 {
    prompt_cache::DEFAULT_MIN_QUALITY_THRESHOLD
}

fn default_cache_friendly_prompt_shaping() -> bool {
    prompt_cache::DEFAULT_CACHE_FRIENDLY_PROMPT_SHAPING
}

fn default_openai_min_prefix_tokens() -> u32 {
    prompt_cache::OPENAI_MIN_PREFIX_TOKENS
}

fn default_openai_idle_expiration() -> u64 {
    prompt_cache::OPENAI_IDLE_EXPIRATION_SECONDS
}

fn default_openai_prompt_cache_key_mode() -> OpenAIPromptCacheKeyMode {
    OpenAIPromptCacheKeyMode::Session
}

#[allow(dead_code, reason = "Intentional compatibility, platform, or test-only suppression.")]
fn default_anthropic_extended_ttl() -> Option<u64> {
    Some(prompt_cache::ANTHROPIC_EXTENDED_TTL_SECONDS)
}

fn default_anthropic_tools_ttl() -> u64 {
    prompt_cache::ANTHROPIC_TOOLS_TTL_SECONDS
}

fn default_anthropic_messages_ttl() -> u64 {
    prompt_cache::ANTHROPIC_MESSAGES_TTL_SECONDS
}

fn default_anthropic_max_breakpoints() -> u8 {
    prompt_cache::ANTHROPIC_MAX_BREAKPOINTS
}

#[allow(dead_code, reason = "Intentional compatibility, platform, or test-only suppression.")]
fn default_min_message_length() -> usize {
    prompt_cache::ANTHROPIC_MIN_MESSAGE_LENGTH_FOR_CACHE
}

fn default_gemini_min_prefix_tokens() -> u32 {
    prompt_cache::GEMINI_MIN_PREFIX_TOKENS
}

fn default_gemini_explicit_ttl() -> Option<u64> {
    Some(prompt_cache::GEMINI_EXPLICIT_DEFAULT_TTL_SECONDS)
}

fn default_gemini_mode() -> GeminiPromptCacheMode {
    GeminiPromptCacheMode::Implicit
}

fn default_zai_enabled() -> bool {
    prompt_cache::ZAI_CACHE_ENABLED
}

fn default_moonshot_enabled() -> bool {
    prompt_cache::MOONSHOT_CACHE_ENABLED
}

fn resolve_path(input: &str, workspace_root: Option<&Path>) -> PathBuf {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return resolve_default_cache_dir();
    }

    if let Some(stripped) = trimmed.strip_prefix("~/").or_else(|| trimmed.strip_prefix("~\\")) {
        if let Some(home) = dirs::home_dir() {
            return home.join(stripped);
        }
        return PathBuf::from(stripped);
    }

    let candidate = Path::new(trimmed);
    if candidate.is_absolute() {
        return candidate.to_path_buf();
    }

    if let Some(root) = workspace_root {
        return root.join(candidate);
    }

    candidate.to_path_buf()
}

fn resolve_default_cache_dir() -> PathBuf {
    VtCodePaths::resolve()
        .map(|paths| paths.cache_dir().join("prompts"))
        .unwrap_or_default()
}

impl PromptCachingConfig {
    /// Validate prompt cache config and provider overrides
    pub(crate) fn validate(&self) -> anyhow::Result<()> {
        // Validate OpenAI provider settings
        self.providers.openai.validate()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_fs::TempDir;
    use std::fs;

    /// Walk up from `CARGO_MANIFEST_DIR` to the directory whose `Cargo.toml`
    /// contains `[workspace]`. Hardcoding `parent()` breaks when crates are
    /// nested (e.g. `crates/codegen/<crate>` — three levels deep, not one).
    fn find_workspace_root() -> PathBuf {
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let mut current = manifest.as_path();
        while let Some(parent) = current.parent() {
            let cargo_toml = parent.join("Cargo.toml");
            if cargo_toml.is_file() && fs::read_to_string(&cargo_toml).is_ok_and(|c| c.contains("[workspace]")) {
                return parent.to_path_buf();
            }
            current = parent;
        }
        panic!("could not find workspace root from CARGO_MANIFEST_DIR");
    }

    #[test]
    fn prompt_caching_defaults_align_with_constants() {
        let cfg = PromptCachingConfig::default();
        assert!(cfg.enabled);
        assert_eq!(cfg.max_entries, prompt_cache::DEFAULT_MAX_ENTRIES);
        assert_eq!(cfg.max_age_days, prompt_cache::DEFAULT_MAX_AGE_DAYS);
        assert!((cfg.min_quality_threshold - prompt_cache::DEFAULT_MIN_QUALITY_THRESHOLD).abs() < f64::EPSILON);
        assert_eq!(cfg.cache_friendly_prompt_shaping, prompt_cache::DEFAULT_CACHE_FRIENDLY_PROMPT_SHAPING);
        assert!(cfg.providers.openai.enabled);
        assert_eq!(cfg.providers.openai.min_prefix_tokens, prompt_cache::OPENAI_MIN_PREFIX_TOKENS);
        assert_eq!(cfg.providers.openai.prompt_cache_key_mode, OpenAIPromptCacheKeyMode::Session);
        assert_eq!(cfg.providers.anthropic.extended_ttl_seconds, Some(prompt_cache::ANTHROPIC_EXTENDED_TTL_SECONDS));
        assert_eq!(cfg.providers.gemini.mode, GeminiPromptCacheMode::Implicit);
        assert!(cfg.providers.moonshot.enabled);
        assert_eq!(cfg.providers.openai.prompt_cache_retention, None);
    }

    #[test]
    fn resolve_cache_dir_expands_home() {
        let cfg = PromptCachingConfig {
            cache_dir: "~/.custom/cache".to_string(),
            ..PromptCachingConfig::default()
        };
        let resolved = cfg.resolve_cache_dir(None);
        if let Some(home) = dirs::home_dir() {
            assert!(resolved.starts_with(home));
        } else {
            assert_eq!(resolved, PathBuf::from(".custom/cache"));
        }
    }

    #[test]
    fn resolve_cache_dir_uses_workspace_when_relative() {
        let temp = TempDir::new().unwrap();
        let workspace = temp.path();
        let cfg = PromptCachingConfig {
            cache_dir: "relative/cache".to_string(),
            ..PromptCachingConfig::default()
        };
        let resolved = cfg.resolve_cache_dir(Some(workspace));
        assert_eq!(resolved, workspace.join("relative/cache"));
    }

    #[test]
    fn prompt_cache_retention_deserializes_valid_values() {
        let cfg: OpenAIPromptCacheSettings = toml::from_str(
            r#"
            prompt_cache_retention = "in_memory"
            "#,
        )
        .unwrap();
        assert_eq!(cfg.prompt_cache_retention, Some(PromptCacheRetention::InMemory));

        let cfg2: OpenAIPromptCacheSettings = toml::from_str(
            r#"
            prompt_cache_retention = "24h"
            "#,
        )
        .unwrap();
        assert_eq!(cfg2.prompt_cache_retention, Some(PromptCacheRetention::H24));
    }

    #[test]
    fn prompt_cache_retention_unknown_values_deserialize_as_unknown() {
        let cfg: OpenAIPromptCacheSettings = toml::from_str(
            r#"
            prompt_cache_retention = "5m"
            "#,
        )
        .unwrap();
        assert_eq!(cfg.prompt_cache_retention, Some(PromptCacheRetention::Unknown));
    }

    /// `PromptCacheRetention` exposes the same value through `as_str()`/
    /// `Display` and the serde wire form. Both the config table and the OpenAI
    /// API wire format read this enum, so a one-sided change (e.g. a bare
    /// `#[serde(other)]` that serializes `Unknown` as `"Unknown"`) would desync
    /// them. Pin every variant.
    #[test]
    fn prompt_cache_retention_as_str_matches_serde() {
        for retention in [
            PromptCacheRetention::InMemory,
            PromptCacheRetention::H24,
            PromptCacheRetention::Unknown,
        ] {
            assert_eq!(
                serde_json::to_value(retention).unwrap(),
                serde_json::json!(retention.as_str()),
                "serde wire form drifted from as_str() for {retention:?}"
            );
        }
    }

    #[test]
    fn validate_prompt_cache_rejects_unknown_retention() {
        let mut cfg = PromptCachingConfig::default();
        cfg.providers.openai.prompt_cache_retention = Some(PromptCacheRetention::Unknown);
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn prompt_cache_key_mode_parses_from_toml() {
        let parsed: PromptCachingConfig = toml::from_str(
            r#"
[providers.openai]
prompt_cache_key_mode = "off"
"#,
        )
        .expect("prompt cache config should parse");

        assert_eq!(parsed.providers.openai.prompt_cache_key_mode, OpenAIPromptCacheKeyMode::Off);
    }

    #[test]
    fn build_openai_prompt_cache_key_uses_trimmed_lineage_id() {
        let key = build_openai_prompt_cache_key(true, &OpenAIPromptCacheKeyMode::Session, Some(" lineage-abc "));

        assert_eq!(key.as_deref(), Some("vtcode:openai:lineage-abc"));
    }

    #[test]
    fn map_prompt_cache_key_namespaces_merge_gateway() {
        assert_eq!(
            map_prompt_cache_key_for_provider("merge-gateway", "vtcode:openai:abc".to_string()),
            "vtcode:merge:abc"
        );
        assert_eq!(map_prompt_cache_key_for_provider("openai", "vtcode:openai:abc".to_string()), "vtcode:openai:abc");
        assert_eq!(
            map_prompt_cache_key_for_provider("openrouter", "vtcode:openai:abc".to_string()),
            "vtcode:openrouter:abc"
        );
        assert_eq!(map_prompt_cache_key_for_provider("xai", "vtcode:openai:abc".to_string()), "vtcode:xai:abc");
    }

    #[test]
    fn session_affinity_key_gate_covers_openrouter_and_xai() {
        assert!(session_affinity_key_enabled("openrouter", true, false));
        assert!(session_affinity_key_enabled("xai", true, false));
        assert!(session_affinity_key_enabled("openai", true, true));
        assert!(!session_affinity_key_enabled("openai", true, false));
        assert!(!session_affinity_key_enabled("openrouter", false, true));
        assert!(!session_affinity_key_enabled("anthropic", true, true));
        assert!(session_affinity_provider("openrouter"));
        assert!(session_affinity_provider("xai"));
        assert!(!session_affinity_provider("anthropic"));
    }

    #[test]
    fn build_session_affinity_prompt_cache_key_namespaces_providers() {
        let mode = OpenAIPromptCacheKeyMode::Session;
        assert_eq!(
            build_session_affinity_prompt_cache_key("openai", true, &mode, Some("lineage-1")).as_deref(),
            Some("vtcode:openai:lineage-1")
        );
        assert_eq!(
            build_session_affinity_prompt_cache_key("merge-gateway", true, &mode, Some("lineage-1")).as_deref(),
            Some("vtcode:merge:lineage-1")
        );
        assert_eq!(
            build_session_affinity_prompt_cache_key("openrouter", true, &mode, Some("lineage-1")).as_deref(),
            Some("vtcode:openrouter:lineage-1")
        );
        assert_eq!(
            build_session_affinity_prompt_cache_key("xai", true, &mode, Some("lineage-1")).as_deref(),
            Some("vtcode:xai:lineage-1")
        );
        // OpenAI/Merge honor Off mode; OpenRouter/xAI ignore it for sticky routing.
        assert_eq!(
            build_session_affinity_prompt_cache_key("openai", true, &OpenAIPromptCacheKeyMode::Off, Some("lineage-1")),
            None
        );
        assert_eq!(
            build_session_affinity_prompt_cache_key(
                "openrouter",
                true,
                &OpenAIPromptCacheKeyMode::Off,
                Some("lineage-1")
            )
            .as_deref(),
            Some("vtcode:openrouter:lineage-1")
        );
        // Disabled gate or blank lineage yields None.
        assert_eq!(build_session_affinity_prompt_cache_key("openrouter", false, &mode, Some("lineage-1")), None);
        assert_eq!(build_session_affinity_prompt_cache_key("openrouter", true, &mode, Some("  ")), None);
        assert_eq!(build_session_affinity_prompt_cache_key("anthropic", true, &mode, Some("lineage-1")), None);
    }

    #[test]
    fn build_openai_prompt_cache_key_honors_disabled_or_off_mode() {
        assert_eq!(build_openai_prompt_cache_key(false, &OpenAIPromptCacheKeyMode::Session, Some("id")), None);
        assert_eq!(build_openai_prompt_cache_key(true, &OpenAIPromptCacheKeyMode::Off, Some("id")), None);
        assert_eq!(build_openai_prompt_cache_key(true, &OpenAIPromptCacheKeyMode::Session, Some("  ")), None);
    }

    #[test]
    fn provider_enablement_respects_global_and_provider_flags() {
        let mut cfg = PromptCachingConfig { enabled: true, ..PromptCachingConfig::default() };
        cfg.providers.openai.enabled = true;
        assert!(cfg.is_provider_enabled("openai"));
        assert!(cfg.is_provider_enabled("merge-gateway"));

        cfg.enabled = false;
        assert!(!cfg.is_provider_enabled("openai"));
        assert!(!cfg.is_provider_enabled("merge-gateway"));
    }

    #[test]
    fn provider_enablement_handles_aliases_and_modes() {
        let mut cfg = PromptCachingConfig { enabled: true, ..PromptCachingConfig::default() };

        cfg.providers.anthropic.enabled = true;
        assert!(cfg.is_provider_enabled("minimax"));

        cfg.providers.gemini.enabled = true;
        cfg.providers.gemini.mode = GeminiPromptCacheMode::Off;
        assert!(!cfg.is_provider_enabled("gemini"));
    }

    #[test]
    fn gap_threshold_uses_provider_defaults() {
        let cfg = PromptCachingConfig::default();
        assert_eq!(cfg.gap_threshold_secs("anthropic"), Some(prompt_cache::ANTHROPIC_CACHE_GAP_WARNING_SECONDS));
        assert_eq!(cfg.gap_threshold_secs("minimax"), Some(prompt_cache::ANTHROPIC_CACHE_GAP_WARNING_SECONDS));
        assert_eq!(cfg.gap_threshold_secs("openai"), Some(prompt_cache::OPENAI_CACHE_GAP_WARNING_SECONDS));
        assert_eq!(cfg.gap_threshold_secs("gemini"), Some(prompt_cache::DEFAULT_CACHE_GAP_WARNING_SECONDS));
    }

    #[test]
    fn gap_threshold_respects_disable_switches() {
        let cfg = PromptCachingConfig { gap_warning_enabled: false, ..Default::default() };
        assert_eq!(cfg.gap_threshold_secs("anthropic"), None);

        let mut cfg = PromptCachingConfig::default();
        cfg.providers.anthropic.enabled = false;
        assert_eq!(cfg.gap_threshold_secs("anthropic"), None);
        assert_eq!(cfg.gap_threshold_secs("minimax"), None);

        let cfg = PromptCachingConfig { enabled: false, ..Default::default() };
        assert_eq!(cfg.gap_threshold_secs("openai"), None);
    }

    #[test]
    fn gap_threshold_skips_openai_with_extended_retention() {
        let mut cfg = PromptCachingConfig::default();
        cfg.providers.openai.prompt_cache_retention = Some(PromptCacheRetention::H24);
        assert_eq!(cfg.gap_threshold_secs("openai"), None);

        cfg.providers.openai.prompt_cache_retention = Some(PromptCacheRetention::InMemory);
        assert_eq!(cfg.gap_threshold_secs("openai"), Some(prompt_cache::OPENAI_CACHE_GAP_WARNING_SECONDS));
    }

    #[test]
    fn gap_threshold_honors_explicit_override() {
        let cfg = PromptCachingConfig {
            gap_warning_threshold_secs: Some(42),
            ..Default::default()
        };
        assert_eq!(cfg.gap_threshold_secs("anthropic"), Some(42));
        assert_eq!(cfg.gap_threshold_secs("openai"), Some(42));
    }

    #[test]
    fn gap_warning_fields_parse_from_toml() {
        let parsed: PromptCachingConfig = toml::from_str(
            r#"
gap_warning_enabled = false
gap_warning_threshold_secs = 120
"#,
        )
        .expect("prompt cache config should parse");

        assert!(!parsed.gap_warning_enabled);
        assert_eq!(parsed.gap_warning_threshold_secs, Some(120));

        let defaults: PromptCachingConfig = toml::from_str("").expect("empty config");
        assert!(defaults.gap_warning_enabled);
        assert_eq!(defaults.gap_warning_threshold_secs, None);
    }

    #[test]
    fn bundled_config_templates_match_prompt_cache_defaults() {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let bootstrap_template =
            fs::read_to_string(manifest_dir.join("data/default_config.toml")).expect("bootstrap config template");
        let config: crate::VTCodeConfig = toml::from_str(&bootstrap_template).expect("parse bootstrap template");
        assert!(config.prompt_cache.enabled);
        assert!(config.prompt_cache.cache_friendly_prompt_shaping);
        assert!(bootstrap_template.contains("# prompt_cache_retention = \"24h\""));

        let workspace_root = find_workspace_root();
        let example_config =
            fs::read_to_string(workspace_root.join("vtcode.toml.example")).expect("vtcode.toml.example");
        if example_config.contains("[prompt_cache]") {
            assert!(example_config.contains("enabled = true"));
            assert!(example_config.contains("cache_friendly_prompt_shaping = true"));
            assert!(example_config.contains("# prompt_cache_retention = \"24h\""));
        }

        let prompt_cache_guide = fs::read_to_string(workspace_root.join("docs/tools/PROMPT_CACHING_GUIDE.md"))
            .expect("prompt caching guide");
        // The guide is markdownlint-wrapped, so sentences may be reflowed across
        // lines; match against whitespace-normalized text instead of raw lines.
        let normalized_guide = vtcode_commons::formatting::collapse_whitespace(&prompt_cache_guide);
        assert!(
            normalized_guide
                .contains("VT Code enables `prompt_cache.cache_friendly_prompt_shaping = true` by default.")
        );
        // The retention doc block (guide lines 44-48) states `prompt_cache_retention` is
        // opt-in for older supported Responses models; assert that intent survives doc rewrites.
        assert!(normalized_guide.contains("`prompt_cache_retention` is opt-in for older supported Responses models"));

        let field_reference = fs::read_to_string(workspace_root.join("docs/config/CONFIG_FIELD_REFERENCE.md"))
            .expect("config field reference");
        assert!(field_reference.contains("`prompt_cache.cache_friendly_prompt_shaping`"));
        assert!(field_reference.contains("`prompt_cache.providers.openai.prompt_cache_retention`"));
    }

    #[test]
    fn bundled_example_config_parses_and_validates() {
        // Guards the tracked template against stale keys and schema drift:
        // the shipped example must load as VTCodeConfig and pass validation.
        let workspace_root = find_workspace_root();
        let example = fs::read_to_string(workspace_root.join("vtcode.toml.example")).expect("vtcode.toml.example");
        let config: crate::VTCodeConfig = toml::from_str(&example).expect("example config should parse");
        config.validate().expect("example config should validate");
    }
}
