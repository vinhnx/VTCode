use crate::constants::prompt_cache;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Global prompt caching configuration loaded from vtcode.toml
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

    /// Provider specific overrides
    #[serde(default)]
    pub providers: ProviderPromptCachingConfig,
}

impl Default for PromptCachingConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            cache_dir: default_cache_dir(),
            max_entries: default_max_entries(),
            max_age_days: default_max_age_days(),
            enable_auto_cleanup: default_auto_cleanup(),
            min_quality_threshold: default_min_quality_threshold(),
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
}

/// Per-provider configuration overrides
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
    pub moonshot: MoonshotPromptCacheSettings,

    #[serde(default = "XAIPromptCacheSettings::default")]
    pub xai: XAIPromptCacheSettings,

    #[serde(default = "DeepSeekPromptCacheSettings::default")]
    pub deepseek: DeepSeekPromptCacheSettings,

    #[serde(default = "ZaiPromptCacheSettings::default")]
    pub zai: ZaiPromptCacheSettings,
}

/// OpenAI prompt caching controls (automatic with metrics)
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OpenAIPromptCacheSettings {
    #[serde(default = "default_true")]
    pub enabled: bool,

    #[serde(default = "default_openai_min_prefix_tokens")]
    pub min_prefix_tokens: u32,

    #[serde(default = "default_openai_idle_expiration")]
    pub idle_expiration_seconds: u64,

    #[serde(default = "default_true")]
    pub surface_metrics: bool,
}

impl Default for OpenAIPromptCacheSettings {
    fn default() -> Self {
        Self {
            enabled: default_true(),
            min_prefix_tokens: default_openai_min_prefix_tokens(),
            idle_expiration_seconds: default_openai_idle_expiration(),
            surface_metrics: default_true(),
        }
    }
}

/// Anthropic Claude cache control settings
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AnthropicPromptCacheSettings {
    #[serde(default = "default_true")]
    pub enabled: bool,

    #[serde(default = "default_anthropic_default_ttl")]
    pub default_ttl_seconds: u64,

    /// Optional extended TTL (1 hour) for long-lived caches
    #[serde(default = "default_anthropic_extended_ttl")]
    pub extended_ttl_seconds: Option<u64>,

    #[serde(default = "default_anthropic_max_breakpoints")]
    pub max_breakpoints: u8,

    /// Apply cache control to system prompts by default
    #[serde(default = "default_true")]
    pub cache_system_messages: bool,

    /// Apply cache control to user messages exceeding threshold
    #[serde(default = "default_true")]
    pub cache_user_messages: bool,
}

impl Default for AnthropicPromptCacheSettings {
    fn default() -> Self {
        Self {
            enabled: default_true(),
            default_ttl_seconds: default_anthropic_default_ttl(),
            extended_ttl_seconds: default_anthropic_extended_ttl(),
            max_breakpoints: default_anthropic_max_breakpoints(),
            cache_system_messages: default_true(),
            cache_user_messages: default_true(),
        }
    }
}

/// Gemini API caching preferences
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GeminiPromptCacheSettings {
    #[serde(default = "default_true")]
    pub enabled: bool,

    #[serde(default = "default_gemini_mode")]
    pub mode: GeminiPromptCacheMode,

    #[serde(default = "default_gemini_min_prefix_tokens")]
    pub min_prefix_tokens: u32,

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
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OpenRouterPromptCacheSettings {
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Propagate provider cache instructions automatically
    #[serde(default = "default_true")]
    pub propagate_provider_capabilities: bool,

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
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MoonshotPromptCacheSettings {
    #[serde(default = "default_moonshot_enabled")]
    pub enabled: bool,
}

impl Default for MoonshotPromptCacheSettings {
    fn default() -> Self {
        Self {
            enabled: default_moonshot_enabled(),
        }
    }
}

/// xAI prompt caching configuration (automatic platform-level cache)
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct XAIPromptCacheSettings {
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl Default for XAIPromptCacheSettings {
    fn default() -> Self {
        Self {
            enabled: default_true(),
        }
    }
}

/// DeepSeek prompt caching configuration (automatic KV cache reuse)
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
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ZaiPromptCacheSettings {
    #[serde(default = "default_zai_enabled")]
    pub enabled: bool,
}

impl Default for ZaiPromptCacheSettings {
    fn default() -> Self {
        Self {
            enabled: default_zai_enabled(),
        }
    }
}

fn default_enabled() -> bool {
    prompt_cache::DEFAULT_ENABLED
}

fn default_cache_dir() -> String {
    format!("~/{path}", path = prompt_cache::DEFAULT_CACHE_DIR)
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

fn default_true() -> bool {
    true
}

fn default_openai_min_prefix_tokens() -> u32 {
    prompt_cache::OPENAI_MIN_PREFIX_TOKENS
}

fn default_openai_idle_expiration() -> u64 {
    prompt_cache::OPENAI_IDLE_EXPIRATION_SECONDS
}

fn default_anthropic_default_ttl() -> u64 {
    prompt_cache::ANTHROPIC_DEFAULT_TTL_SECONDS
}

fn default_anthropic_extended_ttl() -> Option<u64> {
    Some(prompt_cache::ANTHROPIC_EXTENDED_TTL_SECONDS)
}

fn default_anthropic_max_breakpoints() -> u8 {
    prompt_cache::ANTHROPIC_MAX_BREAKPOINTS
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

    if let Some(stripped) = trimmed
        .strip_prefix("~/")
        .or_else(|| trimmed.strip_prefix("~\\"))
    {
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
    if let Some(home) = dirs::home_dir() {
        return home.join(prompt_cache::DEFAULT_CACHE_DIR);
    }
    PathBuf::from(prompt_cache::DEFAULT_CACHE_DIR)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn prompt_caching_defaults_align_with_constants() {
        let cfg = PromptCachingConfig::default();
        assert!(cfg.enabled);
        assert_eq!(cfg.max_entries, prompt_cache::DEFAULT_MAX_ENTRIES);
        assert_eq!(cfg.max_age_days, prompt_cache::DEFAULT_MAX_AGE_DAYS);
        assert!(
            (cfg.min_quality_threshold - prompt_cache::DEFAULT_MIN_QUALITY_THRESHOLD).abs()
                < f64::EPSILON
        );
        assert!(cfg.providers.openai.enabled);
        assert_eq!(
            cfg.providers.openai.min_prefix_tokens,
            prompt_cache::OPENAI_MIN_PREFIX_TOKENS
        );
        assert_eq!(
            cfg.providers.anthropic.extended_ttl_seconds,
            Some(prompt_cache::ANTHROPIC_EXTENDED_TTL_SECONDS)
        );
        assert_eq!(cfg.providers.gemini.mode, GeminiPromptCacheMode::Implicit);
        assert!(cfg.providers.moonshot.enabled);
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
        let temp = tempdir().unwrap();
        let workspace = temp.path();
        let cfg = PromptCachingConfig {
            cache_dir: "relative/cache".to_string(),
            ..PromptCachingConfig::default()
        };
        let resolved = cfg.resolve_cache_dir(Some(workspace));
        assert_eq!(resolved, workspace.join("relative/cache"));
    }
}
