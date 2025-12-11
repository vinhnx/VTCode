use serde::{Deserialize, Serialize};

/// Trust model for third-party plugins.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PluginTrustLevel {
    /// Default sandboxed mode; restricts elevated capabilities.
    Sandbox,
    /// Explicitly trusted plugins that may request expanded capabilities.
    Trusted,
    /// Untrusted plugins run with strict isolation.
    Untrusted,
}

impl Default for PluginTrustLevel {
    fn default() -> Self {
        PluginTrustLevel::Sandbox
    }
}

/// Runtime configuration for dynamic plugin loading.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PluginRuntimeConfig {
    /// Toggle the plugin runtime. When disabled, manifests are ignored.
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// Manifest paths (files or directories) that should be scanned for plugins.
    #[serde(default)]
    pub manifests: Vec<String>,

    /// Default trust level when a manifest omits trust metadata.
    #[serde(default)]
    pub default_trust: PluginTrustLevel,

    /// Explicit allow-list of plugin identifiers permitted to load.
    #[serde(default)]
    pub allow: Vec<String>,

    /// Explicit block-list of plugin identifiers that must be rejected.
    #[serde(default)]
    pub deny: Vec<String>,

    /// Enable hot-reload polling for manifests to support rapid iteration.
    #[serde(default)]
    pub auto_reload: bool,
}

impl Default for PluginRuntimeConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            manifests: Vec::new(),
            default_trust: PluginTrustLevel::Sandbox,
            allow: Vec::new(),
            deny: Vec::new(),
            auto_reload: true,
        }
    }
}

fn default_enabled() -> bool {
    true
}
