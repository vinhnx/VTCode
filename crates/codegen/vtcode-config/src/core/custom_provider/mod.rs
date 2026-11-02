use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::types::ReasoningEffortLevel;

fn default_auth_timeout_ms() -> u64 {
    5_000
}

fn default_auth_refresh_interval_ms() -> u64 {
    300_000
}

fn skip_serializing_custom_provider_api_format(api_format: &CustomProviderApiFormat) -> bool {
    api_format.is_auto()
}

/// Typed API format used by custom providers.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
pub enum CustomProviderApiFormat {
    #[default]
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "openai-chat")]
    OpenAIChat,
    #[serde(rename = "openai-responses")]
    OpenAIResponses,
    #[serde(rename = "anthropic-messages")]
    AnthropicMessages,
}

impl CustomProviderApiFormat {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::OpenAIChat => "openai-chat",
            Self::OpenAIResponses => "openai-responses",
            Self::AnthropicMessages => "anthropic-messages",
        }
    }

    pub const fn is_auto(self) -> bool {
        matches!(self, Self::Auto)
    }

    pub const fn resolved(self) -> Option<Self> {
        match self {
            Self::Auto => None,
            other => Some(other),
        }
    }
}

/// Sparse per-provider or per-model capability/profile settings.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct CustomProviderProfileConfig {
    /// Typed API format for this provider/profile.
    #[serde(default, skip_serializing_if = "skip_serializing_custom_provider_api_format")]
    pub api_format: CustomProviderApiFormat,

    /// Optional context window size in tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<usize>,

    /// Optional sampling temperature override (0.0-2.0) sent with requests.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,

    /// Optional nucleus-sampling override (0.0-1.0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,

    /// Optional top-k override (>= 0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_k: Option<i32>,

    /// Optional presence penalty override (-2.0-2.0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f32>,

    /// Optional frequency penalty override (-2.0-2.0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f32>,

    /// Optional max output tokens override (> 0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,

    /// Optional reasoning effort override sent with requests for this model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<ReasoningEffortLevel>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_tools: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_reasoning: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_reasoning_effort: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_vision: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_structured_output: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_parallel_tool_calls: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_context_caching: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_responses_compaction: Option<bool>,
    /// Explicit compatibility opt-in for Responses prompt_cache_options.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_prompt_cache_options: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_context_edits: Option<bool>,
}

impl CustomProviderProfileConfig {
    fn validate(&self, provider_name: &str, profile_key: &str) -> Result<(), String> {
        if self.context_window == Some(0) {
            return Err(format!(
                "custom_providers[{provider_name}].profiles[{profile_key}]: `context_window` must be greater than 0"
            ));
        }

        if let Some(temperature) = self.temperature
            && !(0.0..=2.0).contains(&temperature)
        {
            return Err(format!(
                "custom_providers[{provider_name}].profiles[{profile_key}]: `temperature` must be between 0.0 and 2.0"
            ));
        }

        if let Some(top_p) = self.top_p
            && !(0.0..=1.0).contains(&top_p)
        {
            return Err(format!(
                "custom_providers[{provider_name}].profiles[{profile_key}]: `top_p` must be between 0.0 and 1.0"
            ));
        }

        if let Some(top_k) = self.top_k
            && top_k < 0
        {
            return Err(format!("custom_providers[{provider_name}].profiles[{profile_key}]: `top_k` must be >= 0"));
        }

        for (field, value) in [
            ("`presence_penalty`", self.presence_penalty),
            ("`frequency_penalty`", self.frequency_penalty),
        ] {
            if let Some(value) = value
                && !(-2.0..=2.0).contains(&value)
            {
                return Err(format!(
                    "custom_providers[{provider_name}].profiles[{profile_key}]: {field} must be between -2.0 and 2.0"
                ));
            }
        }

        if self.max_tokens == Some(0) {
            return Err(format!(
                "custom_providers[{provider_name}].profiles[{profile_key}]: `max_tokens` must be greater than 0"
            ));
        }

        if self.reasoning_effort == Some(ReasoningEffortLevel::Unknown) {
            return Err(format!(
                "custom_providers[{provider_name}].profiles[{profile_key}]: `reasoning_effort` is not a recognized level (use none, minimal, low, medium, high, xhigh, or max)"
            ));
        }

        Ok(())
    }
}

/// Resolved capability/profile settings after applying provider defaults and
/// exact model-specific overrides.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ResolvedCustomProviderProfile {
    pub api_format: Option<CustomProviderApiFormat>,
    pub context_window: Option<usize>,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub top_k: Option<i32>,
    pub presence_penalty: Option<f32>,
    pub frequency_penalty: Option<f32>,
    pub max_tokens: Option<u32>,
    pub reasoning_effort: Option<ReasoningEffortLevel>,
    pub supports_tools: Option<bool>,
    pub supports_reasoning: Option<bool>,
    pub supports_reasoning_effort: Option<bool>,
    pub supports_vision: Option<bool>,
    pub supports_structured_output: Option<bool>,
    pub supports_parallel_tool_calls: Option<bool>,
    pub supports_context_caching: Option<bool>,
    pub supports_responses_compaction: Option<bool>,
    /// Explicit compatibility opt-in for Responses prompt_cache_options.
    pub supports_prompt_cache_options: Option<bool>,
    pub supports_context_edits: Option<bool>,
}

impl ResolvedCustomProviderProfile {
    fn from_layers(defaults: &CustomProviderProfileConfig, profile: Option<&CustomProviderProfileConfig>) -> Self {
        let fallback_profile;
        let profile = match profile {
            Some(profile) => profile,
            None => {
                fallback_profile = CustomProviderProfileConfig::default();
                &fallback_profile
            }
        };

        Self {
            api_format: profile.api_format.resolved().or(defaults.api_format.resolved()),
            context_window: profile.context_window.or(defaults.context_window),
            temperature: profile.temperature.or(defaults.temperature),
            top_p: profile.top_p.or(defaults.top_p),
            top_k: profile.top_k.or(defaults.top_k),
            presence_penalty: profile.presence_penalty.or(defaults.presence_penalty),
            frequency_penalty: profile.frequency_penalty.or(defaults.frequency_penalty),
            max_tokens: profile.max_tokens.or(defaults.max_tokens),
            reasoning_effort: profile.reasoning_effort.or(defaults.reasoning_effort),
            supports_tools: profile.supports_tools.or(defaults.supports_tools),
            supports_reasoning: profile.supports_reasoning.or(defaults.supports_reasoning),
            supports_reasoning_effort: profile.supports_reasoning_effort.or(defaults.supports_reasoning_effort),
            supports_vision: profile.supports_vision.or(defaults.supports_vision),
            supports_structured_output: profile.supports_structured_output.or(defaults.supports_structured_output),
            supports_parallel_tool_calls: profile
                .supports_parallel_tool_calls
                .or(defaults.supports_parallel_tool_calls),
            supports_context_caching: profile.supports_context_caching.or(defaults.supports_context_caching),
            supports_prompt_cache_options: profile
                .supports_prompt_cache_options
                .or(defaults.supports_prompt_cache_options),
            supports_responses_compaction: profile
                .supports_responses_compaction
                .or(defaults.supports_responses_compaction),
            supports_context_edits: profile.supports_context_edits.or(defaults.supports_context_edits),
        }
    }
}

/// Command-backed bearer token configuration for a custom provider.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct CustomProviderCommandAuthConfig {
    /// Command to execute. Bare names are resolved via `PATH`. Command-backed
    /// auth is accepted only from trusted system/user or explicitly selected
    /// configuration; repository-controlled workspace/project values are
    /// rejected.
    pub command: String,

    /// Optional command arguments.
    #[serde(default)]
    pub args: Vec<String>,

    /// Optional working directory for the token command.
    #[serde(default)]
    pub cwd: Option<PathBuf>,

    /// Maximum time to wait for the command to complete successfully.
    #[serde(default = "default_auth_timeout_ms")]
    pub timeout_ms: u64,

    /// Maximum age for the cached token before rerunning the command.
    #[serde(default = "default_auth_refresh_interval_ms")]
    pub refresh_interval_ms: u64,
}

impl Default for CustomProviderCommandAuthConfig {
    fn default() -> Self {
        Self {
            command: String::new(),
            args: Vec::new(),
            cwd: None,
            timeout_ms: default_auth_timeout_ms(),
            refresh_interval_ms: default_auth_refresh_interval_ms(),
        }
    }
}

impl CustomProviderCommandAuthConfig {
    fn validate(&self, provider_name: &str) -> Result<(), String> {
        if self.command.trim().is_empty() {
            return Err(format!("custom_providers[{provider_name}]: `auth.command` must not be empty"));
        }

        if self.timeout_ms == 0 {
            return Err(format!("custom_providers[{provider_name}]: `auth.timeout_ms` must be greater than 0"));
        }

        if self.refresh_interval_ms == 0 {
            return Err(format!(
                "custom_providers[{provider_name}]: `auth.refresh_interval_ms` must be greater than 0"
            ));
        }

        Ok(())
    }
}

/// Configuration for a user-defined OpenAI-compatible provider endpoint.
///
/// Allows users to define multiple named custom endpoints (e.g., corporate
/// proxies) with distinct display names, so they can toggle between them
/// and clearly see which endpoint is active.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct CustomProviderConfig {
    /// Stable provider key used for routing and persistence (e.g., "mycorp").
    /// Must be lowercase alphanumeric with optional hyphens/underscores.
    pub name: String,

    /// Human-friendly label shown in the TUI header, footer, and model picker
    /// (e.g., "MyCorporateName").
    pub display_name: String,

    /// Base URL of the OpenAI-compatible API endpoint
    /// (e.g., `<https://llm.corp.example/v1>`). Non-empty custom providers
    /// from repository-controlled workspace/project layers are rejected.
    pub base_url: String,

    /// Typed API format for the provider's default profile.
    #[serde(default, skip_serializing_if = "skip_serializing_custom_provider_api_format")]
    pub api_format: CustomProviderApiFormat,

    /// Optional context window size in tokens for models served by this endpoint.
    ///
    /// When omitted, the OpenAI-compatible provider uses its default context
    /// window size.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<usize>,

    /// Optional support for tool calling.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_tools: Option<bool>,

    /// Optional support for reasoning.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_reasoning: Option<bool>,

    /// Optional support for reasoning effort.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_reasoning_effort: Option<bool>,

    /// Optional support for vision inputs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_vision: Option<bool>,

    /// Optional support for structured output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_structured_output: Option<bool>,

    /// Optional support for parallel tool calls.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_parallel_tool_calls: Option<bool>,

    /// Optional support for context caching.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_context_caching: Option<bool>,

    /// Optional support for responses compaction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_responses_compaction: Option<bool>,
    /// Explicit compatibility opt-in for Responses prompt_cache_options.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_prompt_cache_options: Option<bool>,

    /// Optional support for context edits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_context_edits: Option<bool>,

    /// Optional sampling temperature default (0.0-2.0) for models served by
    /// this endpoint unless a profile overrides it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,

    /// Optional nucleus-sampling default (0.0-1.0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,

    /// Optional top-k default (>= 0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_k: Option<i32>,

    /// Optional presence penalty default (-2.0-2.0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f32>,

    /// Optional frequency penalty default (-2.0-2.0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f32>,

    /// Optional max output tokens default (> 0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,

    /// Optional reasoning effort default for models without a profile
    /// override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<ReasoningEffortLevel>,

    /// Environment variable name that holds the API key for this endpoint
    /// (e.g., "MYCORP_API_KEY").
    #[serde(default)]
    pub api_key_env: String,

    /// Optional command-backed bearer token configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<CustomProviderCommandAuthConfig>,

    /// Default model to use with this endpoint (e.g., "gpt-5-mini").
    ///
    /// When [`models`](Self::models) is empty, this single model is what the
    /// `/model` picker offers for this provider. When [`models`](Self::models)
    /// is non-empty, this field is used as the default selection but the
    /// picker lists every entry in [`models`](Self::models).
    #[serde(default)]
    pub model: String,

    /// Optional list of additional model identifiers offered by the provider.
    ///
    /// Useful for OpenAI-compatible aggregators such as Atlas Cloud that
    /// expose many models behind a single endpoint. When set, the `/model`
    /// picker shows one entry per model. When empty, the picker falls back to
    /// the single [`model`](Self::model) field.
    #[serde(default)]
    pub models: Vec<String>,

    /// Exact model-keyed sparse capability profiles.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub profiles: BTreeMap<String, CustomProviderProfileConfig>,
}

impl CustomProviderConfig {
    /// Resolve the API key environment variable used for this provider.
    ///
    /// Falls back to a derived `NAME_API_KEY`-style variable when the config
    /// does not set `api_key_env`.
    pub fn resolved_api_key_env(&self) -> String {
        if !self.api_key_env.trim().is_empty() {
            return self.api_key_env.clone();
        }

        crate::api_keys::api_key_env_var(&self.name)
    }

    pub fn uses_command_auth(&self) -> bool {
        self.auth.is_some()
    }

    /// Return the list of models the `/model` picker should offer for this
    /// provider.
    ///
    /// If `models` is non-empty, every entry is returned (trimmed). Otherwise
    /// the single `model` field is returned as a one-element list. An empty
    /// `model` field with no `models` list yields an empty result.
    pub fn effective_models(&self) -> Vec<String> {
        if !self.models.is_empty() {
            return self
                .models
                .iter()
                .map(|m| m.trim().to_string())
                .filter(|m| !m.is_empty())
                .collect();
        }
        let trimmed = self.model.trim();
        if trimmed.is_empty() {
            Vec::new()
        } else {
            vec![trimmed.to_string()]
        }
    }

    pub fn profile(&self, model: &str) -> Option<&CustomProviderProfileConfig> {
        self.profiles.get(model)
    }

    pub fn resolved_profile(&self, model: &str) -> ResolvedCustomProviderProfile {
        let defaults = self.provider_defaults_profile();
        ResolvedCustomProviderProfile::from_layers(&defaults, self.profile(model))
    }

    pub fn provider_defaults_profile(&self) -> CustomProviderProfileConfig {
        CustomProviderProfileConfig {
            api_format: self.api_format,
            context_window: self.context_window,
            temperature: self.temperature,
            top_p: self.top_p,
            top_k: self.top_k,
            presence_penalty: self.presence_penalty,
            frequency_penalty: self.frequency_penalty,
            max_tokens: self.max_tokens,
            reasoning_effort: self.reasoning_effort,
            supports_tools: self.supports_tools,
            supports_reasoning: self.supports_reasoning,
            supports_reasoning_effort: self.supports_reasoning_effort,
            supports_vision: self.supports_vision,
            supports_structured_output: self.supports_structured_output,
            supports_parallel_tool_calls: self.supports_parallel_tool_calls,
            supports_context_caching: self.supports_context_caching,
            supports_responses_compaction: self.supports_responses_compaction,
            supports_prompt_cache_options: self.supports_prompt_cache_options,
            supports_context_edits: self.supports_context_edits,
        }
    }

    /// Validate that required fields are present and the name doesn't collide
    /// with built-in provider keys.
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("custom_providers: `name` must not be empty".to_string());
        }

        if !is_valid_provider_name(&self.name) {
            return Err(format!(
                "custom_providers[{}]: `name` must use lowercase letters, digits, hyphens, or underscores",
                self.name
            ));
        }

        if self.display_name.trim().is_empty() {
            return Err(format!("custom_providers[{}]: `display_name` must not be empty", self.name));
        }

        if self.base_url.trim().is_empty() {
            return Err(format!("custom_providers[{}]: `base_url` must not be empty", self.name));
        }

        if self.context_window == Some(0) {
            return Err(format!("custom_providers[{}]: `context_window` must be greater than 0", self.name));
        }

        if let Some(temperature) = self.temperature
            && !(0.0..=2.0).contains(&temperature)
        {
            return Err(format!("custom_providers[{}]: `temperature` must be between 0.0 and 2.0", self.name));
        }

        if let Some(top_p) = self.top_p
            && !(0.0..=1.0).contains(&top_p)
        {
            return Err(format!("custom_providers[{}]: `top_p` must be between 0.0 and 1.0", self.name));
        }

        if let Some(top_k) = self.top_k
            && top_k < 0
        {
            return Err(format!("custom_providers[{}]: `top_k` must be >= 0", self.name));
        }

        for (field, value) in [
            ("`presence_penalty`", self.presence_penalty),
            ("`frequency_penalty`", self.frequency_penalty),
        ] {
            if let Some(value) = value
                && !(-2.0..=2.0).contains(&value)
            {
                return Err(format!("custom_providers[{}]: {field} must be between -2.0 and 2.0", self.name));
            }
        }

        if self.max_tokens == Some(0) {
            return Err(format!("custom_providers[{}]: `max_tokens` must be greater than 0", self.name));
        }

        if self.reasoning_effort == Some(ReasoningEffortLevel::Unknown) {
            return Err(format!(
                "custom_providers[{}]: `reasoning_effort` is not a recognized level (use none, minimal, low, medium, high, xhigh, or max)",
                self.name
            ));
        }

        if let Some(auth) = &self.auth {
            auth.validate(&self.name)?;
            if !self.api_key_env.trim().is_empty() {
                return Err(format!("custom_providers[{}]: `auth` cannot be combined with `api_key_env`", self.name));
            }
        }

        if !self.api_key_env.trim().is_empty()
            && let Err(err) = crate::auth::CredentialIdentity::new(&self.name, &self.api_key_env)
        {
            return Err(format!("custom_providers[{}]: invalid `api_key_env`: {err}", self.name));
        }

        if self.models.iter().any(|m| m.trim().is_empty()) {
            return Err(format!("custom_providers[{}]: `models` entries must not be empty", self.name));
        }

        for (profile_key, profile) in &self.profiles {
            if profile_key.trim().is_empty() || profile_key.trim() != profile_key {
                return Err(format!(
                    "custom_providers[{}]: profile key `{profile_key}` must not be empty or contain surrounding whitespace",
                    self.name
                ));
            }

            profile.validate(&self.name, profile_key)?;
        }

        let reserved = [
            "openai",
            "anthropic",
            "gemini",
            "copilot",
            "deepseek",
            "meta",
            "meta-ai",
            "openrouter",
            "ollama",
            "lmstudio",
            "llamacpp",
            "moonshot",
            "zai",
            "minimax",
            "huggingface",
            "openresponses",
        ];
        let lower = self.name.to_lowercase();
        if reserved.contains(&lower.as_str()) {
            return Err(format!("custom_providers[{}]: name collides with built-in provider", self.name));
        }

        Ok(())
    }
}

fn is_valid_provider_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    let Some(first) = bytes.first() else {
        return false;
    };
    let Some(last) = bytes.last() else {
        return false;
    };

    let is_valid_char = |ch: u8| matches!(ch, b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_');
    let is_alphanumeric = |ch: u8| matches!(ch, b'a'..=b'z' | b'0'..=b'9');

    is_alphanumeric(*first) && is_alphanumeric(*last) && bytes.iter().copied().all(is_valid_char)
}

#[cfg(test)]
mod prompt_cache_options_tests;
#[cfg(test)]
mod tests;
